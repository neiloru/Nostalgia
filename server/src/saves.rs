//! Versioned cloud saves, stored per user and per emulator:
//!   <savesDirectory>/<userId>/<emulatorId>/v<version>-<timestamp>.zip
//! The database keeps paths relative to savesDirectory, so the folder can be moved.
//! Nothing in here ever deletes a save.

use crate::db::{Db, now};
use crate::error::{HttpError, HttpResult};
use crate::util::{file_stamp, js_slice};
use axum::body::Body;
use futures_util::StreamExt;
use nostalgia_shared::SaveVersion;
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const MAX_SAVE_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB per snapshot

fn to_version(r: &Row) -> rusqlite::Result<SaveVersion> {
    Ok(SaveVersion {
        id: r.get("id")?,
        emulator_id: r.get("emulator_id")?,
        version: r.get("version")?,
        size: r.get("size")?,
        sha256: r.get("sha256")?,
        device: r.get("device")?,
        note: r.get("note")?,
        created_at: r.get("created_at")?,
    })
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Migration {
    pub moved: usize,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct StorageCheck {
    pub writable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub checked: i64,
    pub missing: i64,
}

#[derive(Debug, Serialize)]
pub struct StoreResult {
    pub version: SaveVersion,
    pub created: bool,
}

pub struct StoreMeta {
    pub device: Option<String>,
    pub note: Option<String>,
    pub expected_sha256: Option<String>,
}

#[derive(Clone)]
pub struct Saves {
    db: Db,
    pub root: PathBuf,
}

fn list_files(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let r = rel.join(e.file_name());
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            list_files(&e.path(), &r, out);
        } else if e.path().is_file() {
            out.push(r);
        }
    }
}

impl Saves {
    pub fn new(db: Db, saves_path: &Path) -> std::io::Result<Saves> {
        std::fs::create_dir_all(saves_path)?;
        Ok(Saves { db, root: saves_path.to_path_buf() })
    }

    /// One-time move from the old layout (saves inside the database folder). Files are copied
    /// (works across disks), checked by size, and the old folder is then renamed to
    /// "saves-moved-<date>" instead of being deleted. Existing files are never overwritten.
    pub fn migrate_from(&self, old_dir: &Path) -> anyhow::Result<Option<Migration>> {
        if crate::util::normalize(old_dir) == crate::util::normalize(&self.root) || !old_dir.exists() {
            return Ok(None);
        }
        let mut files = Vec::new();
        list_files(old_dir, Path::new(""), &mut files);
        if files.is_empty() {
            return Ok(None);
        }
        let mut moved = 0;
        let mut conflicts = Vec::new();
        for rel in files {
            let from = old_dir.join(&rel);
            let to = self.root.join(&rel);
            let size = std::fs::metadata(&from)?.len();
            if to.exists() {
                if std::fs::metadata(&to)?.len() != size {
                    conflicts.push(rel.to_string_lossy().into_owned());
                }
                continue;
            }
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p)?;
            }
            // Exclusive create: never replace an existing file.
            let mut src = std::fs::File::open(&from)?;
            let mut dst = std::fs::OpenOptions::new().write(true).create_new(true).open(&to)?;
            std::io::copy(&mut src, &mut dst)?;
            drop(dst);
            if std::fs::metadata(&to)?.len() != size {
                anyhow::bail!("Copying {} to {} failed (size differs)", from.display(), to.display());
            }
            moved += 1;
        }
        if conflicts.is_empty() {
            let name = format!("{}-moved-{}", old_dir.file_name().unwrap_or_default().to_string_lossy(), file_stamp());
            std::fs::rename(old_dir, old_dir.with_file_name(name))?;
        }
        Ok(Some(Migration { moved, conflicts }))
    }

    /// Startup check: is the folder writable, and are the newest save files there?
    /// Missing files usually mean an unmounted drive/share or a moved folder.
    pub fn check(&self) -> StorageCheck {
        let probe = self.root.join(format!(".write-test-{}", uuid::Uuid::new_v4()));
        let (writable, error) = match std::fs::write(&probe, "nostalgia") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                (true, None)
            }
            Err(e) => (false, Some(e.to_string())),
        };
        let files: Vec<String> = {
            let conn = self.db.conn();
            let mut st = conn
                .prepare(
                    "SELECT s.file FROM saves s JOIN (SELECT user_id, emulator_id, MAX(version) v FROM saves GROUP BY user_id, emulator_id) m
                     ON m.user_id = s.user_id AND m.emulator_id = s.emulator_id AND m.v = s.version",
                )
                .expect("sql");
            st.query_map([], |r| r.get(0)).map(|r| r.flatten().collect()).unwrap_or_default()
        };
        let missing = files.iter().filter(|f| !self.root.join(f).exists()).count() as i64;
        StorageCheck { writable, error, checked: files.len() as i64, missing }
    }

    pub fn latest_all(&self, user_id: i64) -> HttpResult<Vec<SaveVersion>> {
        let conn = self.db.conn();
        let mut st = conn.prepare(
            "SELECT s.* FROM saves s JOIN (SELECT emulator_id, MAX(version) v FROM saves WHERE user_id = ? GROUP BY emulator_id) m
             ON m.emulator_id = s.emulator_id AND m.v = s.version WHERE s.user_id = ?",
        )?;
        Ok(st.query_map(params![user_id, user_id], to_version)?.flatten().collect())
    }

    pub fn latest(&self, user_id: i64, emulator_id: i64) -> HttpResult<Option<SaveVersion>> {
        Ok(self
            .db
            .conn()
            .query_row(
                "SELECT * FROM saves WHERE user_id = ? AND emulator_id = ? ORDER BY version DESC LIMIT 1",
                params![user_id, emulator_id],
                to_version,
            )
            .optional()?)
    }

    pub fn list(&self, user_id: i64, emulator_id: i64) -> HttpResult<Vec<SaveVersion>> {
        let conn = self.db.conn();
        let mut st = conn.prepare("SELECT * FROM saves WHERE user_id = ? AND emulator_id = ? ORDER BY version DESC")?;
        Ok(st.query_map(params![user_id, emulator_id], to_version)?.flatten().collect())
    }

    pub fn get(&self, user_id: i64, id: i64) -> HttpResult<(SaveVersion, PathBuf)> {
        let row = self
            .db
            .conn()
            .query_row("SELECT * FROM saves WHERE id = ? AND user_id = ?", params![id, user_id], |r| Ok((to_version(r)?, r.get::<_, String>("file")?)))
            .optional()?;
        let (v, file) = row.ok_or_else(|| HttpError::not_found("Save not found"))?;
        Ok((v, self.root.join(file)))
    }

    pub fn count_for_user(&self, user_id: i64) -> (i64, i64) {
        self.db
            .conn()
            .query_row("SELECT COUNT(*) c, COALESCE(SUM(size),0) b FROM saves WHERE user_id = ?", [user_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((0, 0))
    }

    /// Store an uploaded snapshot (zip). If it is byte-identical to the newest version,
    /// no new version is created and the existing one is returned (`created: false`).
    pub async fn store(&self, user_id: i64, emulator_id: i64, body: Body, meta: StoreMeta) -> HttpResult<StoreResult> {
        let dir = self.root.join(user_id.to_string()).join(emulator_id.to_string());
        tokio::fs::create_dir_all(&dir).await?;
        let tmp = dir.join(format!(".upload-{}.tmp", uuid::Uuid::new_v4()));
        let result = self.receive(body, &tmp).await;
        let (sha256, size) = match result {
            Ok(r) => r,
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await; // incomplete temp upload, not a save
                return Err(e);
            }
        };
        let discard = || async {
            let _ = tokio::fs::remove_file(&tmp).await;
        };
        if let Some(exp) = &meta.expected_sha256
            && !exp.is_empty()
            && exp.to_lowercase() != sha256
        {
            discard().await;
            return Err(HttpError::new(400, "Checksum mismatch, upload corrupted"));
        }
        if size == 0 {
            discard().await;
            return Err(HttpError::new(400, "Empty save upload"));
        }
        let latest = self.latest(user_id, emulator_id)?;
        if let Some(l) = &latest
            && l.sha256 == sha256
        {
            discard().await;
            return Ok(StoreResult { version: l.clone(), created: false });
        }
        let version = latest.map(|l| l.version).unwrap_or(0) + 1;
        let rel = format!("{user_id}/{emulator_id}/v{version:05}-{}.zip", file_stamp());
        tokio::fs::rename(&tmp, self.root.join(&rel)).await?;
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO saves (user_id, emulator_id, version, file, size, sha256, device, note, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                user_id,
                emulator_id,
                version,
                rel,
                size as i64,
                sha256,
                meta.device.map(|d| js_slice(&d, 100)),
                meta.note.map(|n| js_slice(&n, 300)),
                now()
            ],
        )?;
        let id = conn.last_insert_rowid();
        let v = conn.query_row("SELECT * FROM saves WHERE id = ?", [id], to_version)?;
        Ok(StoreResult { version: v, created: true })
    }

    async fn receive(&self, body: Body, tmp: &Path) -> HttpResult<(String, u64)> {
        let mut file = tokio::fs::File::create(tmp).await?;
        let mut hash = Sha256::new();
        let mut size: u64 = 0;
        let mut stream = body.into_data_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| HttpError::new(400, e.to_string()))?;
            size += chunk.len() as u64;
            if size > MAX_SAVE_BYTES {
                return Err(HttpError::new(413, "Save too large"));
            }
            hash.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok((hex::encode(hash.finalize()), size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_from_the_old_layout_are_moved_never_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("data").join("saves");
        std::fs::create_dir_all(old.join("1").join("2")).unwrap();
        std::fs::write(old.join("1").join("2").join("v00001-x.zip"), "save one").unwrap();
        std::fs::write(old.join("1").join("2").join("v00002-x.zip"), "save two").unwrap();
        let db = Db::open(&dir.path().join("data")).unwrap();
        let saves = Saves::new(db, &dir.path().join("saves")).unwrap();
        assert_eq!(saves.migrate_from(&old).unwrap(), Some(Migration { moved: 2, conflicts: vec![] }));
        assert_eq!(std::fs::read_to_string(dir.path().join("saves").join("1").join("2").join("v00002-x.zip")).unwrap(), "save two");
        assert!(!old.exists());
        let kept = std::fs::read_dir(dir.path().join("data")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("saves-moved-"));
        assert!(kept, "old folder is kept under a new name");
        assert_eq!(saves.migrate_from(&old).unwrap(), None);
    }

    #[tokio::test]
    async fn the_startup_check_reports_missing_save_files() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("database")).unwrap();
        let saves = Saves::new(db.clone(), &dir.path().join("saves")).unwrap();
        db.conn().execute("INSERT INTO users (username, password_hash, created_at) VALUES ('u', 'x', '2026-01-01')", []).unwrap();
        db.conn()
            .execute("INSERT INTO emulators (platform, folder, rel_path, name, added_at, updated_at) VALUES ('psx', 'Emu', 'psx/Emu', 'Emu', 'x', 'x')", [])
            .unwrap();
        let meta = StoreMeta { device: None, note: None, expected_sha256: None };
        let v = saves.store(1, 1, Body::from("save data"), meta).await.unwrap();
        assert_eq!(saves.check(), StorageCheck { writable: true, error: None, checked: 1, missing: 0 });
        std::fs::remove_file(saves.get(1, v.version.id).unwrap().1).unwrap();
        assert_eq!(saves.check().missing, 1);
    }
}
