use super::*;
use anyhow::{ensure, Context};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::ffi::{c_char, c_long, CStr, CString};
unsafe extern "C" {
    fn cv_read(previous: c_long) -> *mut c_char;
    fn cv_write(json: *const c_char) -> bool;
    fn cv_paste() -> bool;
    fn cv_identity() -> *mut c_char;
    fn cv_legacy_preferences() -> *mut c_char;
    fn cv_free(value: *mut c_char);
}
fn owned_string(value: *mut c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let result = unsafe { CStr::from_ptr(value).to_string_lossy().into_owned() };
    unsafe { cv_free(value) };
    Some(result)
}
pub struct MacClipboard {
    previous: c_long,
}
impl MacClipboard {
    pub fn new() -> Self {
        Self { previous: -1 }
    }
}
impl ClipboardBackend for MacClipboard {
    fn snapshot(&mut self) -> Result<Option<Snapshot>> {
        let Some(json) = owned_string(unsafe { cv_read(self.previous) }) else {
            return Ok(None);
        };
        let mut snapshot: Snapshot = serde_json::from_str(&json)?;
        self.previous = snapshot.generation.parse()?;
        if let Some(clip) = &mut snapshot.clip {
            if clip.kind == clipvault_core::model::Kind::Text {
                clip.kind =
                    clipvault_core::model::Kind::for_text(clip.text.as_deref().unwrap_or(""));
            }
        }
        Ok(Some(snapshot))
    }
    fn write(&mut self, clip: &Clip) -> Result<()> {
        clip.validate()?;
        let json = CString::new(serde_json::to_string(clip)?)?;
        ensure!(
            unsafe { cv_write(json.as_ptr()) },
            "Could not write to the clipboard"
        );
        Ok(())
    }
    fn paste(&mut self) -> Result<()> {
        ensure!(unsafe{cv_paste()},"Copied. Enable ClipVault in System Settings → Privacy & Security → Accessibility for automatic paste.");
        Ok(())
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            backend: "macOS".into(),
            monitoring: true,
            images: true,
            app_exclusions: true,
            automatic_paste: true,
            helper_required: false,
            message: None,
        }
    }
}
pub fn identity() -> Result<Vec<u8>> {
    Ok(STANDARD.decode(
        owned_string(unsafe { cv_identity() })
            .context("Could not access the ClipVault identity in Keychain")?,
    )?)
}
pub fn legacy_preferences() -> Option<clipvault_core::model::Settings> {
    owned_string(unsafe { cv_legacy_preferences() }).and_then(|s| serde_json::from_str(&s).ok())
}
