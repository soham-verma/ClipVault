use super::*;
use zbus::blocking::{Connection, Proxy};
const DEST: &str = "org.gnome.Shell.Extensions.ClipVault";
const PATH: &str = "/org/gnome/Shell/Extensions/ClipVault";
pub struct Gnome {
    connection: Connection,
    previous: String,
}
impl Gnome {
    pub fn new() -> Result<Self> {
        let connection = zbus::blocking::connection::Builder::session()?
            .name("com.soham.ClipVault")?
            .build()?;
        let proxy = Proxy::new(
            &connection,
            DEST,
            PATH,
            "org.gnome.Shell.Extensions.ClipVault1",
        )?;
        let version: u32 = proxy.call("Version", &())?;
        ensure!(version == 1, "Update the ClipVault GNOME helper");
        Ok(Self {
            connection,
            previous: String::new(),
        })
    }
    fn proxy(&self) -> Result<Proxy<'_>> {
        Ok(Proxy::new(
            &self.connection,
            DEST,
            PATH,
            "org.gnome.Shell.Extensions.ClipVault1",
        )?)
    }
}
impl ClipboardBackend for Gnome {
    fn snapshot(&mut self) -> Result<Option<Snapshot>> {
        let json: String = self.proxy()?.call("Read", &self.previous)?;
        if json.is_empty() {
            return Ok(None);
        }
        let snapshot: Snapshot = serde_json::from_str(&json)?;
        self.previous = snapshot.generation.clone();
        Ok(Some(snapshot))
    }
    fn write(&mut self, clip: &Clip) -> Result<()> {
        clip.validate()?;
        let _: () = self.proxy()?.call("Write", &serde_json::to_string(clip)?)?;
        Ok(())
    }
    fn paste(&mut self) -> Result<()> {
        bail!("Copied. Use your application's normal paste shortcut on Wayland.")
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            backend: "GNOME helper".into(),
            monitoring: true,
            images: true,
            app_exclusions: true,
            automatic_paste: false,
            helper_required: false,
            message: None,
        }
    }
    fn configure_shortcut(&mut self, shortcut: &str) -> Result<()> {
        let binding = crate::shortcuts::linux_trigger(shortcut, true)?;
        let _: () = self.proxy()?.call("SetShortcut", &binding)?;
        Ok(())
    }
}
