use crate::model::*;
use anyhow::{ensure, Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct Store {
    connection: Mutex<Connection>,
    pub root: PathBuf,
}
impl Store {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("Blobs"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let path = root.join("clipvault.sqlite3");
        let existed = path.exists();
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(
            version <= 3,
            "This history was created by a newer ClipVault version"
        );
        if existed && version < 3 {
            let backup = root.join(format!("clipvault.pre-v3-{}.sqlite3", uuid::Uuid::new_v4()));
            let mut destination = Connection::open(backup)?;
            rusqlite::backup::Backup::new(&connection, &mut destination)?.run_to_completion(
                128,
                std::time::Duration::from_millis(10),
                None,
            )?;
        }
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let tx = connection.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS items (
          id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, preview_text TEXT NOT NULL DEFAULT '', full_text TEXT,
          blob_path TEXT, source_app TEXT, source_app_name TEXT, created_at REAL NOT NULL, pinned INTEGER NOT NULL DEFAULT 0,
          pixel_width INTEGER NOT NULL DEFAULT 0, pixel_height INTEGER NOT NULL DEFAULT 0, byte_size INTEGER NOT NULL DEFAULT 0, content_hash TEXT);
          CREATE INDEX IF NOT EXISTS idx_items_created_at ON items(created_at DESC);
          CREATE INDEX IF NOT EXISTS idx_items_pinned ON items(pinned);
          CREATE INDEX IF NOT EXISTS idx_items_hash ON items(content_hash);
          CREATE INDEX IF NOT EXISTS idx_items_blob ON items(blob_path);
          CREATE TABLE IF NOT EXISTS preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS trusted_devices (id TEXT PRIMARY KEY, name TEXT NOT NULL, public_key TEXT NOT NULL);")?;
        let has_origin = tx
            .prepare("PRAGMA table_info(items)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .contains(&"origin_device".into());
        if !has_origin {
            tx.execute("ALTER TABLE items ADD COLUMN origin_device TEXT", [])?;
        }
        if version < 3 {
            tx.execute_batch("DROP TRIGGER IF EXISTS items_ai; DROP TRIGGER IF EXISTS items_ad; DROP TRIGGER IF EXISTS items_au;
              DROP TABLE IF EXISTS items_fts;
              CREATE VIRTUAL TABLE items_fts USING fts5(full_text, preview_text, content='items', content_rowid='id', tokenize='porter unicode61');
              CREATE TRIGGER items_ai AFTER INSERT ON items BEGIN INSERT INTO items_fts(rowid,full_text,preview_text) VALUES(new.id,new.full_text,new.preview_text); END;
              CREATE TRIGGER items_ad AFTER DELETE ON items BEGIN INSERT INTO items_fts(items_fts,rowid,full_text,preview_text) VALUES('delete',old.id,old.full_text,old.preview_text); END;
              CREATE TRIGGER items_au AFTER UPDATE ON items BEGIN
                INSERT INTO items_fts(items_fts,rowid,full_text,preview_text) VALUES('delete',old.id,old.full_text,old.preview_text);
                INSERT INTO items_fts(rowid,full_text,preview_text) VALUES(new.id,new.full_text,new.preview_text); END;
              INSERT INTO items_fts(items_fts) VALUES('rebuild'); PRAGMA user_version=3;")?;
        }
        tx.commit()?;
        Ok(Self {
            connection: Mutex::new(connection),
            root,
        })
    }
    pub fn settings(&self) -> Result<Settings> {
        Ok(self.get("settings")?.unwrap_or_default())
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        settings.validate()?;
        self.set("settings", settings)
    }
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let value: Option<String> = self
            .connection
            .lock()
            .query_row("SELECT value FROM preferences WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        value
            .map(|s| serde_json::from_str(&s).context("Read saved settings"))
            .transpose()
    }
    pub fn set<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.connection.lock().execute("INSERT INTO preferences(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, serde_json::to_string(value)?])?;
        Ok(())
    }
    pub fn trusted(&self) -> Result<Vec<TrustedDevice>> {
        Ok(self
            .connection
            .lock()
            .prepare("SELECT id,name,public_key FROM trusted_devices ORDER BY name")?
            .query_map([], |r| {
                Ok(TrustedDevice {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    public_key: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn trust(&self, device: &TrustedDevice) -> Result<()> {
        ensure!(
            device.id == digest(&hex::decode(&device.public_key)?),
            "Device identity does not match its key"
        );
        self.connection.lock().execute("INSERT INTO trusted_devices(id,name,public_key) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,public_key=excluded.public_key", params![device.id, device.name, device.public_key])?;
        Ok(())
    }
    pub fn forget(&self, id: &str) -> Result<()> {
        self.connection
            .lock()
            .execute("DELETE FROM trusted_devices WHERE id=?", [id])?;
        Ok(())
    }
    pub fn insert(&self, clip: &Clip, origin_device: Option<&str>) -> Result<Option<i64>> {
        clip.validate()?;
        let hash = clip.fingerprint();
        let mut db = self.connection.lock();
        let last: Option<String> = db
            .query_row(
                "SELECT content_hash FROM items ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if last.as_deref() == Some(&hash) {
            return Ok(None);
        }
        let (w, h) = if clip.kind == Kind::Image {
            validate_png(&clip.data)?
        } else {
            (0, 0)
        };
        let blob_path = if clip.data.is_empty() {
            None
        } else {
            let blob_hash = digest(&clip.data);
            let extension = match clip.kind {
                Kind::Image => "png",
                Kind::Rtf => "rtf",
                _ => "txt",
            };
            let relative = format!("{}/{}.{}", &blob_hash[..2], blob_hash, extension);
            let path = self.root.join("Blobs").join(&relative);
            fs::create_dir_all(path.parent().unwrap())?;
            if !path.exists() {
                let tmp = path.with_extension("tmp");
                fs::write(&tmp, &clip.data)?;
                fs::rename(tmp, &path)?;
            }
            Some(relative)
        };
        let preview = if clip.kind == Kind::Image {
            format!("Image · {w} × {h}")
        } else {
            clip.text
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(400)
                .collect()
        };
        let bytes = clip.data.len() + clip.text.as_ref().map_or(0, String::len);
        let tx = db.transaction()?;
        tx.execute("INSERT INTO items(kind,preview_text,full_text,blob_path,source_app,source_app_name,created_at,pixel_width,pixel_height,byte_size,content_hash,origin_device) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)", params![clip.kind.as_str(), preview, clip.text, blob_path, clip.source_app, clip.source_name, now(), w,h,bytes,hash,origin_device])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Some(id))
    }
    pub fn image_file(&self, id: i64) -> Result<PathBuf> {
        let relative: Option<String> = self
            .connection
            .lock()
            .query_row(
                "SELECT blob_path FROM items WHERE id=? AND kind='image'",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        self.blob_file(&relative.context("Image no longer exists")?)
    }
    pub fn page(
        &self,
        query: &str,
        kind: Option<Kind>,
        offset: usize,
        limit: usize,
    ) -> Result<Page> {
        let db = self.connection.lock();
        let limit = limit.clamp(1, 200);
        let search = query
            .split_whitespace()
            .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        let filter = kind.map(|k| k.as_str().to_owned());
        let clause = if search.is_empty() {
            "WHERE (?1 IS NULL OR kind=?1 OR (?1='text' AND kind='rtf'))"
        } else {
            "WHERE (?1 IS NULL OR kind=?1 OR (?1='text' AND kind='rtf')) AND id IN (SELECT rowid FROM items_fts WHERE items_fts MATCH ?2)"
        };
        let clause = if search.is_empty() {
            format!("{clause} AND (?2 IS NOT NULL)")
        } else {
            clause.into()
        };
        let total: usize = db.query_row(
            &format!("SELECT COUNT(*) FROM items {clause}"),
            params![filter, search],
            |r| r.get(0),
        )?;
        let items = db.prepare(&format!("SELECT id,kind,preview_text,NULL AS full_text,blob_path,source_app,source_app_name,created_at,pinned,pixel_width,pixel_height,byte_size,origin_device FROM items {clause} ORDER BY pinned DESC,created_at DESC,id DESC LIMIT ?3 OFFSET ?4"))?.query_map(params![filter, search, limit, offset], Self::row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = offset + items.len() < total;
        Ok(Page {
            items,
            total,
            has_more,
        })
    }
    fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
        let kind: String = r.get("kind")?;
        Ok(Item {
            id: r.get("id")?,
            kind: Kind::parse(&kind).unwrap_or(Kind::Text),
            preview_text: r.get("preview_text")?,
            full_text: r.get("full_text")?,
            blob_path: r.get("blob_path")?,
            source_app: r.get("source_app")?,
            source_name: r.get("source_app_name")?,
            created_at: r.get("created_at")?,
            pinned: r.get("pinned")?,
            pixel_width: r.get("pixel_width")?,
            pixel_height: r.get("pixel_height")?,
            byte_size: r.get("byte_size")?,
            origin_device: r.get("origin_device")?,
        })
    }
    pub fn item_clip(&self, id: i64) -> Result<Clip> {
        let item =
            self.connection
                .lock()
                .query_row("SELECT * FROM items WHERE id=?", [id], Self::row)?;
        let data = match item.blob_path {
            Some(path) => fs::read(self.blob_file(&path)?)?,
            None => vec![],
        };
        Ok(Clip {
            kind: item.kind,
            text: item.full_text,
            data,
            source_app: item.source_app,
            source_name: item.source_name,
            sensitive: false,
        })
    }
    pub fn blob_file(&self, relative: &str) -> Result<PathBuf> {
        let path = Path::new(relative);
        ensure!(
            path.components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "Invalid blob path"
        );
        let root = self.root.join("Blobs").canonicalize()?;
        let result = root.join(path).canonicalize()?;
        ensure!(result.starts_with(root), "Blob outside storage directory");
        Ok(result)
    }
    pub fn pin(&self, id: i64, pinned: bool) -> Result<()> {
        self.connection
            .lock()
            .execute("UPDATE items SET pinned=? WHERE id=?", params![pinned, id])?;
        Ok(())
    }
    pub fn delete(&self, id: i64) -> Result<()> {
        self.remove_where("id=?", &[&id])
    }
    pub fn clear(&self, keep_pinned: bool) -> Result<()> {
        self.remove_where(if keep_pinned { "pinned=0" } else { "1=1" }, &[])
    }
    fn remove_where(&self, clause: &str, params: &[&dyn rusqlite::ToSql]) -> Result<()> {
        let mut db = self.connection.lock();
        let tx = db.transaction()?;
        // Collect all referenced paths; only delete a blob after its final reference disappears.
        let paths = tx
            .prepare(&format!(
                "SELECT DISTINCT blob_path FROM items WHERE blob_path IS NOT NULL AND ({clause})"
            ))?
            .query_map(params, |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tx.execute(&format!("DELETE FROM items WHERE {clause}"), params)?;
        let mut unused = vec![];
        for path in paths {
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM items WHERE blob_path=?",
                [&path],
                |r| r.get(0),
            )?;
            if count == 0 {
                unused.push(path);
            }
        }
        tx.commit()?;
        for path in unused {
            if let Ok(path) = self.blob_file(&path) {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
    pub fn enforce_retention(&self, settings: &Settings) -> Result<()> {
        let max = settings.max_disk_megabytes as u64 * 1_048_576;
        let ids = {
            let db = self.connection.lock();
            let mut total: u64 =
                db.query_row("SELECT COALESCE(SUM(byte_size),0) FROM items", [], |r| {
                    r.get(0)
                })?;
            let mut remaining: usize =
                db.query_row("SELECT COUNT(*) FROM items WHERE pinned=0", [], |r| {
                    r.get(0)
                })?;
            if total <= max
                && (settings.max_history_count == 0 || remaining <= settings.max_history_count)
            {
                return Ok(());
            }
            let mut statement = db.prepare(
                "SELECT id,byte_size FROM items WHERE pinned=0 ORDER BY created_at ASC,id ASC",
            )?;
            let rows =
                statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, u64>(1)?)))?;
            let mut ids = vec![];
            for row in rows {
                let (id, size) = row?;
                if (settings.max_history_count > 0 && remaining > settings.max_history_count)
                    || total > max
                {
                    ids.push(id);
                    total = total.saturating_sub(size);
                    remaining -= 1;
                } else {
                    break;
                }
            }
            ids
        };
        for id in ids {
            self.delete(id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_migration_preserves_all_kinds_pins_blobs_and_search() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let db = Connection::open(temp.path().join("clipvault.sqlite3"))?;
        db.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,preview_text TEXT NOT NULL DEFAULT '',full_text TEXT,blob_path TEXT,source_app TEXT,source_app_name TEXT,created_at REAL NOT NULL,pinned INTEGER NOT NULL DEFAULT 0,pixel_width INTEGER NOT NULL DEFAULT 0,pixel_height INTEGER NOT NULL DEFAULT 0,byte_size INTEGER NOT NULL DEFAULT 0,content_hash TEXT);")?;
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut encoded, image::ImageFormat::Png)?;
        let png = encoded.into_inner();
        let rtf = b"{\\rtf1 legacy rich text}".to_vec();
        fs::create_dir_all(temp.path().join("Blobs/aa"))?;
        fs::write(temp.path().join("Blobs/aa/image.png"), &png)?;
        fs::write(temp.path().join("Blobs/aa/rich.rtf"), &rtf)?;
        let rows = [
            ("text", Some("hello legacy"), None),
            ("url", Some("https://example.org/legacy"), None),
            ("image", None, Some("aa/image.png")),
            ("rtf", Some("legacy rich text"), Some("aa/rich.rtf")),
            ("fileList", Some("/home/person/legacy.txt"), None),
        ];
        for (kind, text, blob) in rows {
            db.execute("INSERT INTO items(kind,preview_text,full_text,blob_path,created_at,pinned) VALUES(?,'legacy preview',?,?,1,1)", params![kind, text, blob])?;
        }
        drop(db);
        let store = Store::open(temp.path())?;
        let page = store.page("legacy", None, 0, 100)?;
        assert_eq!(page.total, 5);
        assert!(page.items.iter().all(|i| i.pinned));
        for item in &page.items {
            let clip = store.item_clip(item.id)?;
            match item.kind {
                Kind::Image => assert_eq!(clip.data, png),
                Kind::Rtf => assert_eq!(clip.data, rtf),
                Kind::Files => assert_eq!(clip.text.as_deref(), Some("/home/person/legacy.txt")),
                _ => assert!(clip.text.unwrap().contains("legacy")),
            }
        }
        let prefs = Settings {
            max_history_count: 1,
            ..Settings::default()
        };
        store.save_settings(&prefs)?;
        store.enforce_retention(&prefs)?;
        assert_eq!(store.page("legacy", None, 0, 100)?.total, 5);
        assert_eq!(store.settings()?.max_history_count, 1);
        assert!(fs::read_dir(temp.path())?.any(|f| f
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("clipvault.pre-v3")));
        store.delete(page.items[0].id)?;
        assert_eq!(store.page("legacy", None, 0, 100)?.total, 4);
        Ok(())
    }
    #[test]
    fn shared_blobs_survive_deleting_one_reference_and_retention() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open(temp.path())?;
        let mut clip = Clip::text("first".into());
        clip.kind = Kind::Rtf;
        clip.data = b"{\\rtf1 example}".to_vec();
        let a = store.insert(&clip, None)?.unwrap();
        clip.text = Some("second".into());
        let b = store.insert(&clip, None)?.unwrap();
        store.pin(b, true)?;
        store.delete(a)?;
        assert_eq!(store.item_clip(b)?.data, clip.data);
        let settings = Settings {
            max_history_count: 1,
            ..Settings::default()
        };
        for n in 0..4 {
            store.insert(&Clip::text(format!("item {n}")), None)?;
        }
        store.enforce_retention(&settings)?;
        assert_eq!(store.page("", None, 0, 100)?.total, 2);
        store.clear(true)?;
        assert_eq!(store.item_clip(b)?.data, clip.data);
        store.clear(false)?;
        assert_eq!(store.page("", None, 0, 100)?.total, 0);
        Ok(())
    }
    #[test]
    fn sensitive_and_files_never_shared() -> Result<()> {
        let mut clip = Clip::text("secret".into());
        clip.sensitive = true;
        assert!(!Settings::default().permits(&clip));
        assert!(clip.shared()?.is_none());
        clip.sensitive = false;
        clip.kind = Kind::Files;
        assert!(clip.shared()?.is_none());
        assert!(Payload {
            kind: Kind::Text,
            bytes: vec![0xff]
        }
        .validate()
        .is_err());
        assert!(Payload {
            kind: Kind::Image,
            bytes: b"not a png".to_vec()
        }
        .validate()
        .is_err());
        Ok(())
    }
}
