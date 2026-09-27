use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Cursor,
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_TEXT: usize = 1_048_576;
pub const MAX_IMAGE: usize = 20 * 1_048_576;
pub const MAX_PIXELS: u64 = 32_000_000;
pub const PROTOCOL_VERSION: u32 = 1;

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub mod base64_bytes {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        bytes: &[u8],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<u8>, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() > MAX_IMAGE.div_ceil(3) * 4 {
            return Err(serde::de::Error::custom("clipboard payload too large"));
        }
        STANDARD.decode(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "url")]
    Url,
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "rtf")]
    Rtf,
    #[serde(rename = "fileList")]
    Files,
}
impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Url => "url",
            Self::Image => "image",
            Self::Rtf => "rtf",
            Self::Files => "fileList",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        serde_json::from_value(serde_json::Value::String(s.into()))
            .context("unknown clipboard type")
    }
    pub fn for_text(text: &str) -> Self {
        if !text.chars().any(char::is_whitespace)
            && (text.starts_with("https://")
                || text.starts_with("http://")
                || text.starts_with("mailto:"))
        {
            Self::Url
        } else {
            Self::Text
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub kind: Kind,
    pub text: Option<String>,
    #[serde(default, with = "base64_bytes")]
    pub data: Vec<u8>,
    #[serde(default)]
    pub source_app: Option<String>,
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub sensitive: bool,
}
impl Clip {
    pub fn text(text: String) -> Self {
        Self {
            kind: Kind::for_text(&text),
            text: Some(text),
            data: vec![],
            source_app: None,
            source_name: None,
            sensitive: false,
        }
    }
    pub fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.kind.as_str());
        hash.update([0]);
        hash.update(self.text.as_deref().unwrap_or(""));
        hash.update([0]);
        hash.update(&self.data);
        hex::encode(hash.finalize())
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.text.as_ref().map_or(0, |s| s.len()) <= MAX_TEXT,
            "Text exceeds the 1 MiB limit"
        );
        ensure!(
            self.data.len() <= MAX_IMAGE,
            "Image exceeds the 20 MiB limit"
        );
        if self.kind == Kind::Image {
            validate_png(&self.data)?;
        }
        Ok(())
    }
    pub fn shared(&self) -> Result<Option<Payload>> {
        if self.sensitive || self.kind == Kind::Files {
            return Ok(None);
        }
        let result = if self.kind == Kind::Image {
            Payload {
                kind: Kind::Image,
                bytes: self.data.clone(),
            }
        } else {
            let text = self.text.clone().unwrap_or_default();
            if text.is_empty() {
                return Ok(None);
            }
            Payload {
                kind: Kind::for_text(&text),
                bytes: text.into_bytes(),
            }
        };
        result.validate()?;
        Ok(Some(result))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Payload {
    pub kind: Kind,
    pub bytes: Vec<u8>,
}
impl Payload {
    pub fn validate(&self) -> Result<()> {
        match self.kind {
            Kind::Text | Kind::Url => {
                ensure!(self.bytes.len() <= MAX_TEXT, "Text exceeds the 1 MiB limit");
                std::str::from_utf8(&self.bytes).context("Invalid UTF-8 text")?;
            }
            Kind::Image => {
                validate_png(&self.bytes)?;
            }
            _ => bail!("This clipboard type cannot be shared"),
        }
        Ok(())
    }
    pub fn hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.kind.as_str());
        h.update([0]);
        h.update(&self.bytes);
        hex::encode(h.finalize())
    }
    pub fn to_clip(&self, device: &str) -> Clip {
        Clip {
            kind: self.kind,
            text: if self.kind == Kind::Image {
                None
            } else {
                String::from_utf8(self.bytes.clone()).ok()
            },
            data: if self.kind == Kind::Image {
                self.bytes.clone()
            } else {
                vec![]
            },
            source_app: None,
            source_name: Some(device.into()),
            sensitive: false,
        }
    }
}

pub fn validate_png(bytes: &[u8]) -> Result<(u32, u32)> {
    ensure!(bytes.len() <= MAX_IMAGE, "Image exceeds the 20 MiB limit");
    ensure!(
        image::guess_format(bytes).ok() == Some(ImageFormat::Png),
        "Expected PNG image"
    );
    let (w, h) =
        ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png).into_dimensions()?;
    ensure!(
        w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_PIXELS,
        "Image exceeds the 32 megapixel limit"
    );
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    reader.decode().context("Invalid PNG data")?;
    Ok((w, h))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub max_history_count: usize,
    pub max_disk_megabytes: usize,
    pub launch_at_login: bool,
    pub ignore_password_managers: bool,
    pub paste_on_select: bool,
    pub excluded_apps: Vec<String>,
    pub shortcut: String,
    pub device_name: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            max_history_count: 0,
            max_disk_megabytes: 2048,
            launch_at_login: false,
            ignore_password_managers: true,
            paste_on_select: cfg!(target_os = "macos"),
            excluded_apps: vec![],
            shortcut: if cfg!(target_os = "macos") {
                "Command+Shift+V"
            } else {
                "Super+Shift+V"
            }
            .into(),
            device_name: std::env::var("HOSTNAME").unwrap_or_else(|_| "My computer".into()),
        }
    }
}
impl Settings {
    pub fn permits(&self, clip: &Clip) -> bool {
        !(self.ignore_password_managers && clip.sensitive)
            && !clip
                .source_app
                .as_ref()
                .is_some_and(|app| self.excluded_apps.contains(app))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (100..=102_400).contains(&self.max_disk_megabytes),
            "Disk limit must be between 100 and 102400 MiB"
        );
        ensure!(
            self.max_history_count <= 1_000_000,
            "History limit is too large"
        );
        ensure!(
            !self.device_name.trim().is_empty() && self.device_name.len() <= 128,
            "Device name must contain 1–128 bytes"
        );
        ensure!(self.excluded_apps.len() <= 1000, "Too many excluded apps");
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub kind: Kind,
    pub preview_text: String,
    pub full_text: Option<String>,
    pub blob_path: Option<String>,
    pub source_app: Option<String>,
    pub source_name: Option<String>,
    pub created_at: f64,
    pub pinned: bool,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub byte_size: u64,
    pub origin_device: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub items: Vec<Item>,
    pub total: usize,
    pub has_more: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub id: String,
    pub name: String,
    pub public_key: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SavedSession {
    pub role: Option<String>,
    pub host_id: Option<String>,
    pub address: Option<String>,
    pub bind_address: Option<String>,
    pub paused: bool,
}
