use super::*;
use anyhow::Context;
use std::{
    collections::HashMap,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{xfixes::ConnectionExt as _, xproto::*, xtest::ConnectionExt as _, Event},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE,
};

fn atom(connection: &RustConnection, name: &str) -> Result<Atom> {
    Ok(connection
        .intern_atom(false, name.as_bytes())?
        .reply()?
        .atom)
}
fn window(connection: &RustConnection, screen: usize) -> Result<Window> {
    let id = connection.generate_id()?;
    connection.create_window(
        COPY_DEPTH_FROM_PARENT,
        id,
        connection.setup().roots[screen].root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    connection.flush()?;
    Ok(id)
}
type Formats = Vec<(String, Vec<u8>)>;
type WriteRequest = (Formats, mpsc::Sender<std::result::Result<(), String>>);
pub struct X11 {
    reader: RustConnection,
    window: Window,
    owner_window: Window,
    root: Window,
    selection: Atom,
    property: Atom,
    sender: mpsc::Sender<WriteRequest>,
    generation: u64,
    dirty: bool,
}
impl X11 {
    pub fn new() -> Result<Self> {
        let (reader, screen) = x11rb::connect(None)?;
        let root = reader.setup().roots[screen].root;
        let window = window(&reader, screen)?;
        let selection = atom(&reader, "CLIPBOARD")?;
        let property = atom(&reader, "CLIPVAULT_READ")?;
        reader.xfixes_query_version(5, 0)?.reply()?;
        reader.xfixes_select_selection_input(
            window,
            selection,
            x11rb::protocol::xfixes::SelectionEventMask::SET_SELECTION_OWNER
                | x11rb::protocol::xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                | x11rb::protocol::xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )?;
        reader.flush()?;
        let (sender, receiver) = mpsc::channel();
        let (ready_send, ready) = mpsc::channel();
        std::thread::spawn(move || {
            if let Err(error) = serve(receiver, ready_send) {
                eprintln!("ClipVault X11 ownership stopped: {error}");
            }
        });
        let owner_window = ready
            .recv_timeout(Duration::from_secs(3))?
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            reader,
            window,
            owner_window,
            root,
            selection,
            property,
            sender,
            generation: 0,
            dirty: true,
        })
    }
    fn wait(&mut self, filter: impl Fn(&Event) -> bool) -> Result<Event> {
        let start = Instant::now();
        loop {
            if let Some(event) = self.reader.poll_for_event()? {
                if matches!(event, Event::XfixesSelectionNotify(_)) {
                    self.dirty = true;
                    self.generation += 1;
                }
                if filter(&event) {
                    return Ok(event);
                }
            } else {
                ensure!(
                    start.elapsed() < Duration::from_secs(3),
                    "Clipboard owner did not respond"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
    fn read(&mut self, mime: &str, limit: usize) -> Result<Vec<u8>> {
        let target = atom(&self.reader, mime)?;
        self.reader.delete_property(self.window, self.property)?;
        self.reader.convert_selection(
            self.window,
            self.selection,
            target,
            self.property,
            CURRENT_TIME,
        )?;
        self.reader.flush()?;
        let requestor = self.window;
        let event=self.wait(|event|matches!(event,Event::SelectionNotify(e) if e.requestor==requestor && e.target==target))?;
        if let Event::SelectionNotify(event) = event {
            ensure!(event.property != NONE, "Clipboard format unavailable");
        }
        let reply = self
            .reader
            .get_property(
                true,
                self.window,
                self.property,
                AtomEnum::ANY,
                0,
                (limit / 4 + 1) as u32,
            )?
            .reply()?;
        ensure!(reply.bytes_after == 0, "Clipboard exceeds size limit");
        if reply.type_ != atom(&self.reader, "INCR")? {
            ensure!(reply.value.len() <= limit, "Clipboard exceeds size limit");
            return Ok(reply.value);
        }
        let mut bytes = vec![];
        self.reader.flush()?;
        loop {
            let property = self.property;
            self.wait(|e|matches!(e,Event::PropertyNotify(e) if e.window==requestor&&e.atom==property&&e.state==Property::NEW_VALUE))?;
            let chunk = self
                .reader
                .get_property(
                    true,
                    self.window,
                    self.property,
                    AtomEnum::ANY,
                    0,
                    (limit / 4 + 1) as u32,
                )?
                .reply()?;
            self.reader.flush()?;
            ensure!(
                chunk.bytes_after == 0 && bytes.len() + chunk.value.len() <= limit,
                "Clipboard exceeds size limit"
            );
            if chunk.value.is_empty() {
                break;
            }
            bytes.extend(chunk.value);
        }
        Ok(bytes)
    }
}
impl ClipboardBackend for X11 {
    fn snapshot(&mut self) -> Result<Option<Snapshot>> {
        while let Some(event) = self.reader.poll_for_event()? {
            if matches!(event, Event::XfixesSelectionNotify(_)) {
                self.dirty = true;
                self.generation += 1;
            }
        }
        if !self.dirty {
            return Ok(None);
        }
        self.dirty = false;
        let owner = self
            .reader
            .get_selection_owner(self.selection)?
            .reply()?
            .owner;
        if owner == NONE || owner == self.owner_window {
            return Ok(Some(Snapshot {
                generation: self.generation.to_string(),
                clip: None,
                owned: owner == self.owner_window,
                error: None,
            }));
        }
        let targets = self.read("TARGETS", 64 * 1024)?;
        let mut types = vec![];
        for bytes in targets.as_chunks::<4>().0 {
            let id = u32::from_ne_bytes(*bytes);
            if let Ok(reply) = self.reader.get_atom_name(id)?.reply() {
                types.push(String::from_utf8_lossy(&reply.name).into_owned());
            }
        }
        let owned = false;
        let mut clip = read_formats(&types, |mime, limit| self.read(mime, limit))?;
        if let Some(clip) = &mut clip {
            if let Ok(active) = self
                .reader
                .get_property(
                    false,
                    self.root,
                    atom(&self.reader, "_NET_ACTIVE_WINDOW")?,
                    AtomEnum::WINDOW,
                    0,
                    1,
                )?
                .reply()
            {
                if let Some(id) = active.value32().and_then(|mut ids| ids.next()) {
                    if let Ok(class) = self
                        .reader
                        .get_property(false, id, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)?
                        .reply()
                    {
                        let app = class
                            .value
                            .split(|b| *b == 0)
                            .rfind(|s| !s.is_empty())
                            .map(|s| String::from_utf8_lossy(s).into_owned());
                        clip.source_app = app.clone();
                        clip.source_name = app;
                    }
                }
            }
        }
        Ok(Some(Snapshot {
            generation: self.generation.to_string(),
            clip,
            owned,
            error: None,
        }))
    }
    fn write(&mut self, clip: &Clip) -> Result<()> {
        let (send, receive) = mpsc::channel();
        self.sender.send((formats(clip)?, send))?;
        receive
            .recv_timeout(Duration::from_secs(3))?
            .map_err(anyhow::Error::msg)
    }
    fn paste(&mut self) -> Result<()> {
        let setup = self.reader.setup();
        let mapping = self
            .reader
            .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)?
            .reply()?;
        let mut ctrl = None;
        let mut v = None;
        for (index, symbols) in mapping
            .keysyms
            .chunks(mapping.keysyms_per_keycode as usize)
            .enumerate()
        {
            if symbols.contains(&0xffe3) {
                ctrl = Some(setup.min_keycode + index as u8);
            }
            if symbols.contains(&0x76) {
                v = Some(setup.min_keycode + index as u8);
            }
        }
        let ctrl = ctrl.context("Control key not found")?;
        let v = v.context("V key not found")?;
        for (type_, key) in [
            (KEY_PRESS_EVENT, ctrl),
            (KEY_PRESS_EVENT, v),
            (KEY_RELEASE_EVENT, v),
            (KEY_RELEASE_EVENT, ctrl),
        ] {
            self.reader
                .xtest_fake_input(type_, key, CURRENT_TIME, self.root, 0, 0, 0)?;
        }
        self.reader.flush()?;
        Ok(())
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            backend: "X11".into(),
            monitoring: true,
            images: true,
            app_exclusions: true,
            automatic_paste: true,
            helper_required: false,
            message: None,
        }
    }
}
struct Incremental {
    bytes: Arc<Vec<u8>>,
    offset: usize,
    target: Atom,
    started: Instant,
}
fn serve(
    receiver: mpsc::Receiver<WriteRequest>,
    ready: mpsc::Sender<std::result::Result<Window, String>>,
) -> Result<()> {
    let initialized = (|| {
        let (c, s) = x11rb::connect(None)?;
        let w = window(&c, s)?;
        let selection = atom(&c, "CLIPBOARD")?;
        Ok::<_, anyhow::Error>((c, w, selection))
    })();
    let (connection, window, selection) = match initialized {
        Ok(value) => {
            let _ = ready.send(Ok(value.1));
            value
        }
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return Err(error);
        }
    };
    let targets = atom(&connection, "TARGETS")?;
    let incr = atom(&connection, "INCR")?;
    let mut formats: HashMap<Atom, Arc<Vec<u8>>> = HashMap::new();
    let mut transfers: HashMap<(Window, Atom), Incremental> = HashMap::new();
    loop {
        match receiver.try_recv() {
            Ok((new, reply)) => {
                let result = (|| {
                    let mut next = HashMap::new();
                    for (mime, data) in new {
                        next.insert(atom(&connection, &mime)?, Arc::new(data));
                    }
                    formats = next;
                    connection
                        .set_selection_owner(window, selection, CURRENT_TIME)?
                        .check()?;
                    connection.flush()?;
                    Ok::<(), anyhow::Error>(())
                })();
                let _ = reply.send(result.map_err(|e| e.to_string()));
            }
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            Err(_) => {}
        }
        transfers.retain(|_, t| t.started.elapsed() < Duration::from_secs(30));
        while let Some(event) = connection.poll_for_event()? {
            match event {
                Event::SelectionRequest(request) => {
                    let property = if request.property == NONE {
                        request.target
                    } else {
                        request.property
                    };
                    let mut accepted = true;
                    if request.target == targets {
                        let mut list = formats.keys().copied().collect::<Vec<_>>();
                        list.push(targets);
                        connection.change_property32(
                            PropMode::REPLACE,
                            request.requestor,
                            property,
                            AtomEnum::ATOM,
                            &list,
                        )?;
                    } else if let Some(bytes) = formats.get(&request.target) {
                        if bytes.len() <= CHUNK {
                            connection.change_property8(
                                PropMode::REPLACE,
                                request.requestor,
                                property,
                                request.target,
                                bytes,
                            )?;
                        } else if transfers.len() < 32 {
                            connection.change_window_attributes(
                                request.requestor,
                                &ChangeWindowAttributesAux::new()
                                    .event_mask(EventMask::PROPERTY_CHANGE),
                            )?;
                            connection.change_property32(
                                PropMode::REPLACE,
                                request.requestor,
                                property,
                                incr,
                                &[bytes.len() as u32],
                            )?;
                            transfers.insert(
                                (request.requestor, property),
                                Incremental {
                                    bytes: bytes.clone(),
                                    offset: 0,
                                    target: request.target,
                                    started: Instant::now(),
                                },
                            );
                        } else {
                            accepted = false;
                        }
                    } else {
                        accepted = false;
                    }
                    let notify = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: request.time,
                        requestor: request.requestor,
                        selection: request.selection,
                        target: request.target,
                        property: if accepted { property } else { NONE },
                    };
                    connection.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
                    connection.flush()?;
                }
                Event::PropertyNotify(event) if event.state == Property::DELETE => {
                    let key = (event.window, event.atom);
                    if let Some(transfer) = transfers.get_mut(&key) {
                        let end = (transfer.offset + CHUNK).min(transfer.bytes.len());
                        let empty = end == transfer.offset;
                        connection.change_property8(
                            PropMode::REPLACE,
                            event.window,
                            event.atom,
                            transfer.target,
                            &transfer.bytes[transfer.offset..end],
                        )?;
                        transfer.offset = end;
                        connection.flush()?;
                        if empty {
                            transfers.remove(&key);
                        }
                    }
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
const CHUNK: usize = 32 * 1024;

#[cfg(all(test, any(target_os = "linux", feature = "linux-backend-check")))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires an isolated Xvfb display; replaces the clipboard"]
    fn linux_x11_text_and_incremental_image() -> Result<()> {
        let owner = X11::new()?;
        let mut reader = X11::new()?;
        for clip in [Clip::text("Hello from an X11 client 🌍".into()), {
            let image = image::RgbaImage::from_fn(240, 240, |x, y| {
                image::Rgba([(x * y) as u8, (x * 19 + y * 71) as u8, (x ^ y) as u8, 255])
            });
            let mut png = std::io::Cursor::new(vec![]);
            image.write_to(&mut png, image::ImageFormat::Png)?;
            assert!(png.get_ref().len() > CHUNK);
            Clip {
                kind: Kind::Image,
                text: None,
                data: png.into_inner(),
                source_app: None,
                source_name: None,
                sensitive: false,
            }
        }] {
            let formats = formats(&clip)?
                .into_iter()
                .filter(|(mime, _)| mime != MARKER)
                .collect();
            let (send, receive) = mpsc::channel();
            owner.sender.send((formats, send))?;
            receive
                .recv_timeout(Duration::from_secs(3))?
                .map_err(anyhow::Error::msg)?;
            let start = Instant::now();
            loop {
                if let Some(snapshot) = reader.snapshot()? {
                    if let Some(received) = snapshot.clip {
                        if received.kind == clip.kind {
                            assert_eq!(received.text, clip.text);
                            assert_eq!(received.data, clip.data);
                            break;
                        }
                    }
                }
                ensure!(
                    start.elapsed() < Duration::from_secs(5),
                    "Clipboard event not observed"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(())
    }
}
