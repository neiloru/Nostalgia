//! Cloud saves (port of savesync.ts). Saves are per emulator; a snapshot is a zip of all save
//! folders listed in emu.yaml (entry "0/..." = first folder, "1/..." = second, ...).
//!
//! Safety rules:
//!  - before local save files are replaced they are copied to <userData>/save-backups if they
//!    are not identical to a version that already exists on the server;
//!  - nothing on the server is ever deleted, restoring an old version uploads it as a new version.

use crate::client::{ApiError, Client};
use crate::util::iso_now;
use nostalgia_shared::{ExpandContext, ItemKind, SaveVersion, expand_string, locale_compare};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LocalFile {
    /// "<index>/<path inside save folder>"
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
    pub mtime_ms: f64,
    pub sha: String,
}

fn list_dir(dir: &Path, prefix: &str, out: &mut Vec<LocalFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}/{name}");
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            list_dir(&e.path(), &rel, out);
        } else if t.is_file()
            && let Ok(m) = e.metadata()
        {
            let mtime_ms = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0);
            out.push(LocalFile { rel, abs: e.path(), size: m.len(), mtime_ms, sha: String::new() });
        }
    }
}

#[derive(Deserialize)]
struct UploadResult {
    version: SaveVersion,
    created: bool,
}

/// Small helper so `withBusy` style guards release on every path.
struct Busy {
    client: Arc<Client>,
    id: i64,
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.client.lock().busy_saves.remove(&self.id);
        self.client.emit_state();
    }
}

impl Client {
    fn env_map() -> HashMap<String, String> {
        std::env::vars().collect()
    }

    pub(crate) fn path_vars(&self, emulator_dir: &str) -> HashMap<String, String> {
        let home = self.paths.home.to_string_lossy().into_owned();
        let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| self.paths.home.join(".config").to_string_lossy().into_owned());
        let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| self.paths.home.join(".local").join("share").to_string_lossy().into_owned());
        HashMap::from([
            ("emulatorDir".into(), emulator_dir.to_string()),
            ("home".into(), home),
            ("documents".into(), self.paths.documents.to_string_lossy().into_owned()),
            ("appData".into(), self.paths.app_data.to_string_lossy().into_owned()),
            ("config".into(), config),
            ("dataHome".into(), data_home),
        ])
    }

    /// Absolute save folders for an installed emulator on this OS.
    pub fn save_paths(&self, emulator_id: i64) -> Vec<PathBuf> {
        let (emu, inst) = {
            let i = self.lock();
            (i.emulator(emulator_id), i.installed(ItemKind::Emulator, emulator_id))
        };
        let Some(sys) = emu.and_then(|e| e.config).and_then(|c| c.systems.get(self.os).cloned()) else { return vec![] };
        let Some(saves) = sys.saves.filter(|s| !s.is_empty()) else { return vec![] };
        let emu_dir = inst.as_ref().map(|i| i.path.clone()).unwrap_or_default();
        let vars = self.path_vars(&emu_dir);
        let env = Self::env_map();
        let home = self.paths.home.to_string_lossy().into_owned();
        let ctx = ExpandContext { vars: &vars, env: &env, home: &home, os: self.os };
        let base = inst.map(|i| PathBuf::from(i.path)).unwrap_or_else(|| self.paths.home.clone());
        saves.iter().map(|s| crate::util::resolve(&base, &expand_string(s, &ctx))).collect()
    }

    fn scan_saves(&self, emulator_id: i64) -> (Vec<LocalFile>, String) {
        let mut files = Vec::new();
        for (i, p) in self.save_paths(emulator_id).iter().enumerate() {
            list_dir(p, &i.to_string(), &mut files);
        }
        files.sort_by(|a, b| locale_compare(&a.rel, &b.rel));
        let mut h = Sha256::new();
        for f in &mut files {
            f.sha = std::fs::read(&f.abs).map(|b| hex::encode(Sha256::digest(&b))).unwrap_or_default();
            h.update(format!("{}\0{}\0{}\n", f.rel, f.size, f.sha).as_bytes());
        }
        let fp = if files.is_empty() { "empty".to_string() } else { hex::encode(h.finalize()) };
        (files, fp)
    }

    fn build_zip(&self, emulator_id: i64, files: &[LocalFile]) -> anyhow::Result<Vec<u8>> {
        use zip::write::SimpleFileOptions;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let dos_time = |ms: f64| -> zip::DateTime {
            // Two second precision, local time like fflate.
            let ms = (ms / 2000.0).floor() as i64 * 2000 + crate::util::local_offset_ms();
            let iso = nostalgia_shared::iso_from_millis(ms);
            let n = |r: std::ops::Range<usize>| iso[r].parse::<u16>().unwrap_or(0);
            zip::DateTime::from_date_and_time(n(0..4).max(1980), n(5..7) as u8, n(8..10) as u8, n(11..13) as u8, n(14..16) as u8, n(17..19) as u8)
                .unwrap_or_default()
        };
        for f in files {
            let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(Some(6)).last_modified_time(dos_time(f.mtime_ms));
            w.start_file(&f.rel, opts)?;
            w.write_all(&std::fs::read(&f.abs)?)?;
        }
        let manifest = json!({
            "format": 1,
            "folders": self.save_paths(emulator_id).len(),
            "files": files.iter().map(|f| json!({ "path": f.rel, "sha256": f.sha })).collect::<Vec<_>>(),
        });
        let opts = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(6))
            .last_modified_time(zip::DateTime::from_date_and_time(2000, 1, 1, 0, 0, 0).unwrap_or_default());
        w.start_file("nostalgia-save.json", opts)?;
        w.write_all(manifest.to_string().as_bytes())?;
        Ok(w.finish()?.into_inner())
    }

    fn backup_saves(&self, emulator_id: i64, files: &[LocalFile], reason: &str) -> Option<PathBuf> {
        if files.is_empty() {
            return None;
        }
        let emu = self.emulator(emulator_id);
        let stamp = iso_now().replace([':', '.'], "-");
        let name = emu.as_ref().map(|e| e.name.clone()).unwrap_or_else(|| emulator_id.to_string());
        let safe: String = {
            let re = regex::Regex::new(r"[^A-Za-z0-9_.-]+").unwrap();
            re.replace_all(&name, "_").into_owned()
        };
        let dir = self
            .paths
            .save_backup_dir()
            .join(format!("{}-{safe}", emu.as_ref().map(|e| e.platform.as_str()).unwrap_or("emu")))
            .join(format!("{stamp}-{reason}"));
        for f in files {
            let dest = f.rel.split('/').fold(dir.clone(), |p, s| p.join(s));
            if let Some(p) = dest.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::copy(&f.abs, &dest);
        }
        Some(dir)
    }

    fn take_busy(self: &Arc<Self>, emulator_id: i64) -> Result<Busy, ApiError> {
        {
            let mut i = self.lock();
            if !i.busy_saves.insert(emulator_id) {
                return Err(ApiError::new(0, "A save sync for this emulator is already running"));
            }
        }
        self.emit_state();
        Ok(Busy { client: self.clone(), id: emulator_id })
    }

    /// Install a server version into the local save folders (mirror).
    async fn apply_version(self: &Arc<Self>, emulator_id: i64, v: &SaveVersion, reason: &str, local: &[LocalFile], local_fp: &str) -> Result<Vec<u8>, ApiError> {
        let st = self.save_state(emulator_id);
        if !local.is_empty() && Some(local_fp) != st.fingerprint.as_deref()
            && let Some(dir) = self.backup_saves(emulator_id, local, reason)
        {
            let what = if reason == "restore" { "restoring" } else { "downloading" };
            self.toast(format!("Local saves backed up before {what} ({})", dir.file_name().unwrap_or_default().to_string_lossy()), false);
        }
        let path = format!("/saves/version/{}/download", v.id);
        let url = self.api_url(&path, None)?;
        let res = self.send(&path, self.request(reqwest::Method::GET, &url).timeout(Duration::from_secs(120))).await?;
        if !res.status().is_success() {
            return Err(ApiError::new(res.status().as_u16(), format!("Downloading save failed (HTTP {})", res.status().as_u16())));
        }
        let buf = res.bytes().await.map_err(|e| ApiError::new(0, e.to_string()))?.to_vec();
        if hex::encode(Sha256::digest(&buf)) != v.sha256 {
            return Err(ApiError::new(0, "Downloaded save is corrupted (checksum mismatch)"));
        }
        let paths = self.save_paths(emulator_id);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(buf.clone())).map_err(|e| ApiError::new(0, format!("Invalid save archive: {e}")))?;
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for idx in 0..archive.len() {
            let mut f = archive.by_index(idx).map_err(|e| ApiError::new(0, e.to_string()))?;
            let name = f.name().map(|n| n.into_owned()).unwrap_or_default();
            if name.ends_with('/') {
                continue;
            }
            let mut data = Vec::new();
            f.read_to_end(&mut data).map_err(|e| ApiError::new(0, e.to_string()))?;
            entries.push((name, data));
        }
        let is_save = |k: &str| k.split_once('/').is_some_and(|(n, rest)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty());
        // Remove local files that are not part of the snapshot (they are backed up or identical to a server version).
        let incoming: HashSet<&str> = entries.iter().map(|(k, _)| k.as_str()).filter(|k| is_save(k)).collect();
        for f in local {
            if !incoming.contains(f.rel.as_str()) {
                let _ = std::fs::remove_file(&f.abs);
            }
        }
        for (name, data) in &entries {
            if !is_save(name) {
                continue;
            }
            let (n, rest) = name.split_once('/').unwrap();
            let Some(root) = n.parse::<usize>().ok().and_then(|i| paths.get(i)) else { continue };
            let dest = crate::util::normalize(&rest.split('/').fold(root.clone(), |p, s| p.join(s)));
            if !dest.starts_with(crate::util::normalize(root)) || dest == crate::util::normalize(root) {
                continue; // zip-slip guard
            }
            if let Some(p) = dest.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            std::fs::write(&dest, data).map_err(|e| ApiError::new(0, e.to_string()))?;
        }
        Ok(buf)
    }

    /// Bring local saves up to date with the server:
    ///  - server has a newer version -> download it (after backing up unsynced local changes)
    ///  - local has unsynced changes and server has nothing newer -> upload
    pub async fn sync_down(self: &Arc<Self>, emulator_id: i64, reason: &str) -> Result<&'static str, ApiError> {
        if self.installed(ItemKind::Emulator, emulator_id).is_none() || self.save_paths(emulator_id).is_empty() {
            return Ok("no-save-config");
        }
        if self.lock().running.as_ref().is_some_and(|r| r.emulator_id == emulator_id) {
            return Ok("running");
        }
        if !self.is_online() {
            return Ok("offline");
        }
        let _busy = self.take_busy(emulator_id)?;
        let versions: Vec<SaveVersion> = self.get(&format!("/saves/{emulator_id}")).await?;
        let st = self.save_state(emulator_id);
        let (files, fingerprint) = self.scan_saves(emulator_id);
        let Some(latest) = versions.first() else {
            if !files.is_empty() && Some(fingerprint.as_str()) != st.fingerprint.as_deref() {
                return self.upload_saves(emulator_id, &files, &fingerprint, "first upload", None).await;
            }
            return Ok("nothing");
        };
        // Local saves vanished since the last sync (emulator reinstalled, folder deleted): download them again.
        let local_missing = files.is_empty() && st.fingerprint.is_some() && Some(fingerprint.as_str()) != st.fingerprint.as_deref();
        if Some(latest.id) == st.version_id && !local_missing {
            if Some(fingerprint.as_str()) != st.fingerprint.as_deref() && !files.is_empty() {
                return self.upload_saves(emulator_id, &files, &fingerprint, "unsynced changes", None).await;
            }
            self.set_save_state(emulator_id, |s| {
                s.last_result = Some("up to date".into());
                s.last_error = None;
                s.synced_at = Some(iso_now());
            });
            return Ok("up-to-date");
        }
        self.apply_version(emulator_id, latest, reason, &files, &fingerprint).await?;
        let (_, after_fp) = self.scan_saves(emulator_id);
        let latest = latest.clone();
        self.set_save_state(emulator_id, |s| {
            s.version_id = Some(latest.id);
            s.version = Some(latest.version);
            s.fingerprint = Some(after_fp);
            s.synced_at = Some(iso_now());
            s.pending_upload = Some(false);
            s.last_error = None;
            s.last_result = Some(format!("downloaded v{}", latest.version));
        });
        let name = self.emulator(emulator_id).map(|e| e.name).unwrap_or_else(|| "emulator".into());
        self.toast(format!("Cloud save v{} for {name} downloaded", latest.version), false);
        Ok("downloaded")
    }

    async fn upload_saves(
        self: &Arc<Self>,
        emulator_id: i64,
        files: &[LocalFile],
        fingerprint: &str,
        note: &str,
        zip: Option<Vec<u8>>,
    ) -> Result<&'static str, ApiError> {
        let body = match zip {
            Some(z) => z,
            None => self.build_zip(emulator_id, files).map_err(|e| ApiError::new(0, e.to_string()))?,
        };
        let sha = hex::encode(Sha256::digest(&body));
        let r: Result<UploadResult, ApiError> = async {
            let path = format!("/saves/{emulator_id}");
            let url = self.api_url(&path, None)?;
            let req = self
                .request(reqwest::Method::POST, &url)
                .timeout(Duration::from_secs(300))
                .header("Content-Type", "application/zip")
                .header("X-Content-Sha256", &sha)
                .header("X-Save-Note", crate::util_url::encode(note))
                .body(body);
            let res = self.send(&path, req).await?;
            let status = res.status().as_u16();
            let text = res.text().await.map_err(|e| ApiError::new(0, e.to_string()))?;
            if !(200..300).contains(&status) {
                let msg = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                    .unwrap_or_else(|| format!("HTTP {status}"));
                return Err(ApiError::new(status, msg));
            }
            serde_json::from_str(&text).map_err(|e| ApiError::new(status, e.to_string()))
        }
        .await;
        match r {
            Ok(res) => {
                let fp = fingerprint.to_string();
                let v = res.version.clone();
                self.set_save_state(emulator_id, |s| {
                    s.version_id = Some(v.id);
                    s.version = Some(v.version);
                    s.fingerprint = Some(fp);
                    s.synced_at = Some(iso_now());
                    s.pending_upload = Some(false);
                    s.last_error = None;
                    s.last_result = Some(if res.created { format!("uploaded v{}", v.version) } else { "up to date".into() });
                });
                let _ = self.refresh_latest_saves().await;
                if res.created {
                    let name = self.emulator(emulator_id).map(|e| e.name).unwrap_or_default();
                    self.toast(format!("Cloud save v{} uploaded ({name})", v.version), false);
                }
                Ok(if res.created { "uploaded" } else { "up-to-date" })
            }
            Err(e) => {
                let msg = e.message.clone();
                self.set_save_state(emulator_id, |s| {
                    s.pending_upload = Some(true);
                    s.last_error = Some(msg);
                });
                self.schedule_save_retry();
                Err(e)
            }
        }
    }

    /// Called after the emulator closed: upload if the save folder changed. Failures are retried later.
    pub async fn sync_up(self: &Arc<Self>, emulator_id: i64, note: &str) -> &'static str {
        if self.save_paths(emulator_id).is_empty() {
            return "no-save-config";
        }
        let (files, fingerprint) = self.scan_saves(emulator_id);
        let st = self.save_state(emulator_id);
        if Some(fingerprint.as_str()) == st.fingerprint.as_deref() || files.is_empty() {
            return "unchanged";
        }
        if !self.is_online() {
            self.set_save_state(emulator_id, |s| {
                s.pending_upload = Some(true);
                s.last_error = Some("offline - will upload when back online".into());
            });
            return "pending";
        }
        let busy = match self.take_busy(emulator_id) {
            Ok(b) => b,
            Err(e) => {
                self.toast(format!("Save upload failed, will retry: {e}"), true);
                return "pending";
            }
        };
        let r = self.upload_saves(emulator_id, &files, &fingerprint, note, None).await;
        drop(busy);
        match r {
            Ok(s) => s,
            Err(e) => {
                self.toast(format!("Save upload failed, will retry: {e}"), true);
                "pending"
            }
        }
    }

    pub async fn retry_pending_saves(self: &Arc<Self>) {
        if !self.is_online() {
            return;
        }
        let ids: Vec<i64> = self.lock().installed.data().emulators.keys().copied().collect();
        for id in ids {
            let running = self.lock().running.as_ref().is_some_and(|r| r.emulator_id == id);
            if self.save_state(id).pending_upload == Some(true) && !running {
                self.sync_up(id, "retry").await;
            }
        }
    }

    /// On start / reconnect: newest cloud saves for every installed emulator.
    pub async fn sync_all_saves(self: &Arc<Self>, reason: &str) {
        if !self.settings().auto_sync_saves || !self.is_online() {
            return;
        }
        let ids: Vec<i64> = self.lock().installed.data().emulators.keys().copied().collect();
        for id in ids {
            if let Err(e) = self.sync_down(id, reason).await {
                self.set_save_state(id, |s| s.last_error = Some(e.message));
            }
        }
    }

    pub(crate) fn on_remote_save(self: &Arc<Self>, emulator_id: i64) {
        let running = self.lock().running.as_ref().is_some_and(|r| r.emulator_id == emulator_id);
        if self.settings().auto_sync_saves && !running {
            let me = self.clone();
            self.rt.spawn(async move {
                let _ = me.sync_down(emulator_id, "remote").await;
            });
        }
    }

    pub async fn list_save_versions(self: &Arc<Self>, emulator_id: i64) -> Result<Vec<SaveVersion>, ApiError> {
        self.get(&format!("/saves/{emulator_id}")).await
    }

    /// Restore an older version locally, then upload it as the newest version so all devices get it.
    pub async fn restore_save(self: &Arc<Self>, emulator_id: i64, version_id: i64) -> Result<(), ApiError> {
        if self.lock().running.as_ref().is_some_and(|r| r.emulator_id == emulator_id) {
            return Err(ApiError::new(0, "Close the emulator first"));
        }
        if self.installed(ItemKind::Emulator, emulator_id).is_none() {
            return Err(ApiError::new(0, "Install the emulator first"));
        }
        let _busy = self.take_busy(emulator_id)?;
        let versions = self.list_save_versions(emulator_id).await?;
        let v = versions.iter().find(|x| x.id == version_id).cloned().ok_or_else(|| ApiError::new(0, "Version not found"))?;
        let (files, fingerprint) = self.scan_saves(emulator_id);
        let zip = self.apply_version(emulator_id, &v, "restore", &files, &fingerprint).await?;
        let (after_files, after_fp) = self.scan_saves(emulator_id);
        if versions.first().map(|x| x.id) == Some(v.id) {
            self.set_save_state(emulator_id, |s| {
                s.version_id = Some(v.id);
                s.version = Some(v.version);
                s.fingerprint = Some(after_fp);
                s.pending_upload = Some(false);
                s.last_result = Some(format!("restored v{}", v.version));
            });
        } else {
            self.upload_saves(emulator_id, &after_files, &after_fp, &format!("restored from v{}", v.version), Some(zip)).await?;
        }
        self.toast(format!("Restored cloud save v{}", v.version), false);
        Ok(())
    }

    /// Manual "sync now".
    pub async fn sync_now(self: &Arc<Self>, emulator_id: i64) -> Result<(), ApiError> {
        let r = self.sync_down(emulator_id, "manual").await?;
        if r == "up-to-date" || r == "nothing" {
            let up = self.sync_up(emulator_id, "manual sync").await;
            if up == "unchanged" {
                self.toast("Saves are up to date", false);
            }
        }
        Ok(())
    }
}
