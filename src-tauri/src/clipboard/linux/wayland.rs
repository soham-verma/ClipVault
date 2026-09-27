use super::*;
use std::{
    io::Read,
    os::fd::AsRawFd,
    time::{Duration, Instant},
};
use wl_clipboard_rs::{
    copy,
    paste::{self, ClipboardType, MimeType, Seat},
};

pub struct Wayland {
    previous: String,
    last_write: Option<String>,
}
impl Wayland {
    pub fn new() -> Result<Self> {
        match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
            Ok(_) | Err(paste::Error::ClipboardEmpty) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(Self {
            previous: String::new(),
            last_write: None,
        })
    }
}
impl ClipboardBackend for Wayland {
    fn snapshot(&mut self) -> Result<Option<Snapshot>> {
        let types = match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
            Ok(types) => types.into_iter().collect::<Vec<_>>(),
            Err(paste::Error::ClipboardEmpty) => vec![],
            Err(error) => return Err(error.into()),
        };
        let clip = read_formats(&types, |mime, limit| {
            let (mut pipe, _) = paste::get_contents(
                ClipboardType::Regular,
                Seat::Unspecified,
                MimeType::Specific(mime),
            )?;
            read_pipe(&mut pipe, limit)
        })?;
        let owned = types.iter().any(|t| t == MARKER)
            && clip
                .as_ref()
                .is_some_and(|c| self.last_write.as_deref() == Some(c.fingerprint().as_str()));
        let generation = if owned {
            "owned".into()
        } else {
            clip.as_ref()
                .map(|c| c.fingerprint())
                .unwrap_or_else(|| "empty".into())
        };
        if generation == self.previous {
            return Ok(None);
        }
        self.previous = generation.clone();
        Ok(Some(Snapshot {
            generation,
            clip,
            owned,
            error: None,
        }))
    }
    fn write(&mut self, clip: &Clip) -> Result<()> {
        let sources = formats(clip)?
            .into_iter()
            .map(|(mime, bytes)| copy::MimeSource {
                source: copy::Source::Bytes(bytes.into_boxed_slice()),
                mime_type: copy::MimeType::Specific(mime),
            })
            .collect();
        copy::Options::new().copy_multi(sources)?;
        self.last_write = Some(clip.fingerprint());
        self.previous = "owned".into();
        Ok(())
    }
    fn paste(&mut self) -> Result<()> {
        bail!("Copied. Use your application's normal paste shortcut on Wayland.")
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities{backend:"Wayland data control".into(),monitoring:true,images:true,app_exclusions:false,automatic_paste:false,helper_required:false,message:Some("Clipboard sharing is active. Per-app exclusions and automatic keystrokes are unavailable on this Wayland backend.".into())}
    }
}

fn read_pipe(pipe: &mut (impl Read + AsRawFd), limit: usize) -> Result<Vec<u8>> {
    let fd = pipe.as_raw_fd();
    // This descriptor is owned by this transfer; nonblocking reads keep a stalled source
    // from freezing the clipboard worker and its history-selection commands.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "Could not configure the clipboard pipe"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 32 * 1024];
    loop {
        ensure!(
            Instant::now() < deadline,
            "Clipboard source did not respond"
        );
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(bytes),
            Ok(n) => {
                ensure!(
                    bytes.len() + n <= limit,
                    "Clipboard content exceeds the size limit"
                );
                bytes.extend_from_slice(&buffer[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let mut wait = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // Bounded wait also handles a source that never closes its selection pipe.
                unsafe {
                    libc::poll(&mut wait, 1, 50);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
}
