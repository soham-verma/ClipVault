use anyhow::Result;
use clipvault_core::model::Clip;
use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "linux", feature = "linux-backend-check"))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub generation: String,
    pub clip: Option<Clip>,
    #[serde(default)]
    pub owned: bool,
    #[serde(default)]
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Capabilities {
    pub backend: String,
    pub monitoring: bool,
    pub images: bool,
    pub app_exclusions: bool,
    pub automatic_paste: bool,
    pub helper_required: bool,
    pub message: Option<String>,
}
pub trait ClipboardBackend: Send {
    fn snapshot(&mut self) -> Result<Option<Snapshot>>;
    fn write(&mut self, clip: &Clip) -> Result<()>;
    fn paste(&mut self) -> Result<()>;
    fn capabilities(&self) -> Capabilities;
    fn configure_shortcut(&mut self, _shortcut: &str) -> Result<()> {
        Ok(())
    }
}
pub fn create() -> Result<Box<dyn ClipboardBackend>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacClipboard::new()))
    }
    #[cfg(target_os = "linux")]
    {
        linux::create()
    }
}
pub fn identity(root: &std::path::Path) -> Result<Vec<u8>> {
    if std::env::var_os("CLIPVAULT_DATA_DIR").is_some() {
        return file_identity(root);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = root;
        macos::identity()
    }
    #[cfg(target_os = "linux")]
    {
        file_identity(root)
    }
}
fn file_identity(root: &std::path::Path) -> Result<Vec<u8>> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };
    let path = root.join("identity.key");
    if path.exists() {
        let metadata = fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Identity must be a regular private file"
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let key = fs::read(path)?;
        anyhow::ensure!(key.len() == 32, "Invalid identity key");
        return Ok(key);
    }
    let key = clipvault_core::sync::SyncService::generate_key()?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&key)?;
    file.sync_all()?;
    Ok(key)
}
pub fn legacy_preferences() -> Option<clipvault_core::model::Settings> {
    #[cfg(target_os = "macos")]
    {
        macos::legacy_preferences()
    }
    #[cfg(target_os = "linux")]
    {
        None
    }
}
