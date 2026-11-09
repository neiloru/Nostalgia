//! Sequential download queue with pause / resume (HTTP Range) that survives restarts (port of downloads.ts).

use crate::client::{ApiError, Client};
use crate::model::{DownloadJob, DownloadStatus, InstalledItem};
use crate::util::{iso_now, now_ms, safe_join};
use futures_util::StreamExt;
use nostalgia_shared::{FileListing, ItemKind, OsKey};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const PART: &str = ".nostalgia-part";
pub const MANIFEST: &str = ".nostalgia.json";

#[derive(Default)]
pub struct DownloadRuntime {
    active: Option<(String, Arc<AtomicBool>)>,
    last_emit: i64,
}

fn part_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}{PART}", dest.display()))
}

pub fn remove_empty_dirs(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                remove_empty_dirs(&e.path());
            }
        }
    }
    if std::fs::read_dir(dir).map(|mut r| r.next().is_none()).unwrap_or(false) {
        let _ = std::fs::remove_dir(dir);
    }
}

impl Client {
    pub fn download_jobs(&self) -> Vec<DownloadJob> {
        self.lock().downloads.data().jobs.clone()
    }

    pub fn find_job(&self, kind: ItemKind, id: i64) -> Option<DownloadJob> {
        self.lock().downloads.data().jobs.iter().find(|j| j.kind == kind && j.id == id && j.status != DownloadStatus::Done).cloned()
    }

    fn emit_jobs(&self, force: bool) {
        let t = now_ms();
        {
            let mut rt = self.downloads_rt.lock().unwrap();
            if !force && t - rt.last_emit < 250 {
                return;
            }
            rt.last_emit = t;
        }
        self.emit_downloads();
    }

    fn update_job(&self, key: &str, f: impl FnOnce(&mut DownloadJob)) {
        let mut i = self.lock();
        i.downloads.update(|d| {
            if let Some(j) = d.jobs.iter_mut().find(|j| j.key == key) {
                f(j);
            }
        });
    }

    fn persist_jobs(self: &Arc<Self>) {
        self.emit_jobs(true);
        self.emit_state();
    }

    pub(crate) fn on_online_downloads(self: &Arc<Self>) {
        {
            let mut i = self.lock();
            i.downloads.update(|d| {
                for j in &mut d.jobs {
                    if j.status == DownloadStatus::Error && j.error.as_deref().is_some_and(|e| e.contains("unreachable")) {
                        j.status = DownloadStatus::Queued;
                    }
                }
            });
        }
        self.pump_downloads();
    }

    pub async fn enqueue(self: &Arc<Self>, kind: ItemKind, id: i64, location: Option<String>) -> Result<(), ApiError> {
        if let Some(existing) = self.find_job(kind, id) {
            if matches!(existing.status, DownloadStatus::Paused | DownloadStatus::Error) {
                self.resume_download(&existing.key);
            }
            return Ok(());
        }
        let (online, installed, platform, name, loc) = {
            let i = self.lock();
            let (platform, name) = match kind {
                ItemKind::Game => i.game(id).map(|g| (g.platform, g.name)),
                ItemKind::Emulator => i.emulator(id).map(|e| (e.platform, e.name)),
            }
            .unzip();
            let s = i.settings.data();
            (i.online, i.installed(kind, id).is_some(), platform, name, location.or_else(|| s.default_location()))
        };
        if !online {
            return Err(ApiError::new(0, "You are offline"));
        }
        if installed {
            return Err(ApiError::new(0, "Already installed"));
        }
        let (Some(platform), Some(name)) = (platform, name) else { return Err(ApiError::new(0, "Unknown item")) };
        let Some(loc) = loc else { return Err(ApiError::new(0, "Add a download location in the settings first")) };
        let listing: FileListing = self.get(&format!("/{}/{id}/files", kind.plural())).await?;
        let target_dir = Path::new(&loc).join(kind.plural()).join(&platform).join(&listing.folder);
        let job = DownloadJob {
            key: format!("{}:{id}", kind.as_str()),
            kind,
            id,
            name,
            platform,
            folder: listing.folder,
            location: loc,
            target_dir: target_dir.to_string_lossy().into_owned(),
            files: listing.files,
            total_bytes: listing.total_bytes,
            received_bytes: 0,
            status: DownloadStatus::Queued,
            error: None,
            speed: 0.0,
            current_file: None,
            added_at: iso_now(),
            finished_at: None,
        };
        self.lock().downloads.update(|d| {
            d.jobs.retain(|j| j.key != job.key);
            d.jobs.push(job);
        });
        self.persist_jobs();
        self.pump_downloads();
        Ok(())
    }

    pub fn pause_download(self: &Arc<Self>, key: &str) {
        let paused = {
            let mut i = self.lock();
            i.downloads.update(|d| match d.jobs.iter_mut().find(|j| j.key == key) {
                Some(j) if matches!(j.status, DownloadStatus::Downloading | DownloadStatus::Queued) => {
                    j.status = DownloadStatus::Paused;
                    j.speed = 0.0;
                    true
                }
                _ => false,
            })
        };
        if !paused {
            return;
        }
        if let Some((k, flag)) = &self.downloads_rt.lock().unwrap().active
            && k == key
        {
            flag.store(true, Ordering::Release);
        }
        self.persist_jobs();
    }

    pub fn resume_download(self: &Arc<Self>, key: &str) {
        let ok = self.lock().downloads.update(|d| match d.jobs.iter_mut().find(|j| j.key == key) {
            Some(j) if matches!(j.status, DownloadStatus::Paused | DownloadStatus::Error) => {
                j.status = DownloadStatus::Queued;
                j.error = None;
                true
            }
            _ => false,
        });
        if ok {
            self.persist_jobs();
            self.pump_downloads();
        }
    }

    pub fn pause_all_downloads(self: &Arc<Self>) {
        for j in self.download_jobs() {
            if matches!(j.status, DownloadStatus::Queued | DownloadStatus::Downloading) {
                self.pause_download(&j.key);
            }
        }
    }

    pub fn resume_all_downloads(self: &Arc<Self>) {
        for j in self.download_jobs() {
            if j.status == DownloadStatus::Paused {
                self.resume_download(&j.key);
            }
        }
    }

    /// Cancel: removes the job and the partial files it created (files of a completed install are untouched).
    pub fn cancel_download(self: &Arc<Self>, key: &str) {
        let Some(job) = self.download_jobs().into_iter().find(|j| j.key == key) else { return };
        if let Some((k, flag)) = &self.downloads_rt.lock().unwrap().active
            && k == key
        {
            flag.store(true, Ordering::Release);
        }
        self.lock().downloads.update(|d| d.jobs.retain(|j| j.key != key));
        if job.status != DownloadStatus::Done && self.installed(job.kind, job.id).is_none() {
            let target = PathBuf::from(&job.target_dir);
            for f in &job.files {
                if let Ok(dest) = safe_join(&target, &f.path) {
                    let _ = std::fs::remove_file(part_path(&dest));
                    let _ = std::fs::remove_file(&dest);
                }
            }
            remove_empty_dirs(&target);
        }
        self.persist_jobs();
        self.pump_downloads();
    }

    pub fn clear_finished_downloads(self: &Arc<Self>) {
        self.lock().downloads.update(|d| d.jobs.retain(|j| j.status != DownloadStatus::Done));
        self.persist_jobs();
    }

    pub(crate) fn pump_downloads(self: &Arc<Self>) {
        let next = {
            let i = self.lock();
            if i.session.data().session.is_none() {
                return;
            }
            i.downloads.data().jobs.iter().find(|j| j.status == DownloadStatus::Queued).map(|j| j.key.clone())
        };
        let Some(key) = next else { return };
        let flag = Arc::new(AtomicBool::new(false));
        {
            let mut rt = self.downloads_rt.lock().unwrap();
            if rt.active.is_some() {
                return;
            }
            rt.active = Some((key.clone(), flag.clone()));
        }
        let me = self.clone();
        self.rt.spawn(async move { me.run_download(key, flag).await });
    }

    async fn run_download(self: Arc<Self>, key: String, cancel: Arc<AtomicBool>) {
        self.update_job(&key, |j| {
            j.status = DownloadStatus::Downloading;
            j.error = None;
        });
        self.persist_jobs();
        let result = self.download_files(&key, &cancel).await;
        let aborted = cancel.load(Ordering::Acquire);
        match result {
            Ok(()) if !aborted => {
                if let Some(job) = self.download_jobs().into_iter().find(|j| j.key == key) {
                    self.finish_install(job).await;
                }
            }
            Ok(()) => {} // paused or cancelled
            Err(e) => {
                if !aborted {
                    let name = self.download_jobs().into_iter().find(|j| j.key == key).map(|j| j.name).unwrap_or_default();
                    let msg = e.to_string();
                    self.update_job(&key, |j| {
                        j.status = DownloadStatus::Error;
                        j.error = Some(msg.clone());
                        j.speed = 0.0;
                    });
                    self.toast(format!("Download of {name} failed: {msg}"), true);
                }
            }
        }
        self.downloads_rt.lock().unwrap().active = None;
        self.persist_jobs();
        self.pump_downloads();
    }

    async fn download_files(self: &Arc<Self>, key: &str, cancel: &AtomicBool) -> anyhow::Result<()> {
        let Some(job) = self.download_jobs().into_iter().find(|j| j.key == key) else { return Ok(()) };
        let target = PathBuf::from(&job.target_dir);
        tokio::fs::create_dir_all(&target).await?;
        // Bytes already on disk (complete files and partial files).
        let mut received: u64 = 0;
        for f in &job.files {
            let dest = safe_join(&target, &f.path)?;
            match std::fs::metadata(&dest) {
                Ok(m) if m.len() == f.size => received += f.size,
                _ => received += std::fs::metadata(part_path(&dest)).map(|m| m.len().min(f.size)).unwrap_or(0),
            }
        }
        self.update_job(key, |j| j.received_bytes = received);
        let mut window_start = now_ms();
        let mut window_bytes: u64 = 0;

        for f in &job.files {
            if cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            let dest = safe_join(&target, &f.path)?;
            let part = part_path(&dest);
            if std::fs::metadata(&dest).map(|m| m.len() == f.size).unwrap_or(false) {
                continue;
            }
            if let Some(p) = dest.parent() {
                tokio::fs::create_dir_all(p).await?;
            }
            let mut start = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            if start > f.size {
                let _ = std::fs::remove_file(&part);
                start = 0;
            }
            self.update_job(key, |j| j.current_file = Some(f.path.clone()));
            if start < f.size || f.size == 0 {
                let path = format!("/{}/{}/download?path={}", job.kind.plural(), job.id, crate::util_url::encode(&f.path));
                let url = self.api_url(&path, None)?;
                let mut req = self.request(reqwest::Method::GET, &url);
                if start > 0 {
                    req = req.header("Range", format!("bytes={start}-"));
                }
                let res = self.send(&path, req).await?;
                let status = res.status().as_u16();
                if !res.status().is_success() && status != 206 {
                    anyhow::bail!("Download of {} failed: HTTP {status}", f.path);
                }
                if start > 0 && status != 206 {
                    // Server ignored the range - start over for this file.
                    self.update_job(key, |j| j.received_bytes = j.received_bytes.saturating_sub(start));
                    start = 0;
                }
                let mut out = tokio::fs::OpenOptions::new().create(true).write(true).append(start > 0).truncate(start == 0).open(&part).await?;
                let mut body = res.bytes_stream();
                while let Some(chunk) = body.next().await {
                    if cancel.load(Ordering::Acquire) {
                        out.flush().await?;
                        return Ok(());
                    }
                    let chunk = chunk.map_err(|e| anyhow::anyhow!("Server unreachable ({e})"))?;
                    out.write_all(&chunk).await?;
                    let n = chunk.len() as u64;
                    window_bytes += n;
                    let t = now_ms();
                    let speed = (t - window_start >= 1000).then(|| {
                        let s = (window_bytes as f64 * 1000.0) / (t - window_start) as f64;
                        window_start = t;
                        window_bytes = 0;
                        s
                    });
                    self.update_job(key, |j| {
                        j.received_bytes += n;
                        if let Some(s) = speed {
                            j.speed = s;
                        }
                    });
                    self.emit_jobs(false);
                }
                out.flush().await?;
            }
            if cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            if size != f.size {
                anyhow::bail!("Incomplete download of {} ({size}/{} bytes)", f.path, f.size);
            }
            std::fs::rename(&part, &dest)?;
            let mtime = std::time::UNIX_EPOCH + Duration::from_millis(f.mtime.max(0) as u64);
            if let Ok(file) = std::fs::File::options().write(true).open(&dest) {
                let _ = file.set_times(std::fs::FileTimes::new().set_modified(mtime));
            }
        }
        Ok(())
    }

    async fn finish_install(self: &Arc<Self>, job: DownloadJob) {
        let item = InstalledItem {
            kind: job.kind,
            id: job.id,
            name: job.name.clone(),
            platform: job.platform.clone(),
            folder: job.folder.clone(),
            location: job.location.clone(),
            path: job.target_dir.clone(),
            files: job.files.clone(),
            size_bytes: job.total_bytes,
            installed_at: iso_now(),
            extra: Default::default(),
        };
        if job.kind == ItemKind::Emulator && self.os != OsKey::Windows {
            self.make_executable(&item);
        }
        let server = self.lock().session.data().session.as_ref().map(|s| s.server_url.clone());
        let mut manifest = serde_json::to_value(&item).unwrap_or_default();
        if let Some(m) = manifest.as_object_mut() {
            m.remove("files");
            m.insert("server".into(), json!(server));
        }
        let _ = crate::store::write_atomic(&Path::new(&job.target_dir).join(MANIFEST), &manifest);
        self.register_installed(item.clone());
        self.update_job(&job.key, |j| {
            j.status = DownloadStatus::Done;
            j.received_bytes = j.total_bytes;
            j.finished_at = Some(iso_now());
            j.current_file = None;
            j.speed = 0.0;
        });
        self.toast(format!("{} is installed", job.name), false);
        // A freshly installed emulator gets the newest cloud save right away.
        if item.kind == ItemKind::Emulator && self.settings().auto_sync_saves {
            let me = self.clone();
            self.rt.spawn(async move {
                let _ = me.sync_down(item.id, "install").await;
            });
        }
    }

    #[allow(unused_variables)]
    fn make_executable(&self, item: &InstalledItem) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exe = self.emulator(item.id).and_then(|e| e.config).and_then(|c| c.systems.get(self.os).map(|s| s.executable.clone()));
            let exe = exe.map(|e| e.trim_start_matches("./").to_string());
            for f in &item.files {
                let lower = f.path.to_lowercase();
                // Downloads lose the executable bit: AppImages, scripts, the emu.yaml executable and
                // everything in the Contents/MacOS folder of a macOS .app bundle.
                let hit = lower.ends_with(".appimage")
                    || lower.ends_with(".sh")
                    || lower.ends_with(".x86_64")
                    || lower.contains(".app/contents/macos/")
                    || exe.as_deref() == Some(f.path.as_str());
                if hit && let Ok(p) = safe_join(Path::new(&item.path), &f.path) {
                    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
                }
            }
        }
    }

    /// Only folders created by Nostalgia inside a download location are removed.
    pub async fn uninstall(self: &Arc<Self>, kind: ItemKind, id: i64) -> Result<(), ApiError> {
        let Some(item) = self.installed(kind, id) else { return Ok(()) };
        let running = self.lock().running.clone();
        if kind == ItemKind::Emulator {
            if running.as_ref().is_some_and(|r| r.emulator_id == id) {
                return Err(ApiError::new(0, "The emulator is running"));
            }
            let _ = self.sync_up(id, "before uninstall").await;
        } else if running.as_ref().is_some_and(|r| r.game_id == id) {
            return Err(ApiError::new(0, "The game is running"));
        }
        let path = PathBuf::from(&item.path);
        let ok = path.join(MANIFEST).exists() && crate::util::is_inside(&path, Path::new(&item.location));
        if !ok {
            return Err(ApiError::new(0, format!("Refusing to delete {}", item.path)));
        }
        tokio::fs::remove_dir_all(&path).await.map_err(|e| ApiError::new(0, e.to_string()))?;
        self.unregister_installed(kind, id);
        self.toast(format!("{} was uninstalled", item.name), false);
        Ok(())
    }

    /// Install a game or emulator (adds it to the library first, optionally with its emulator).
    pub async fn install(self: &Arc<Self>, kind: ItemKind, id: i64, location: Option<String>) -> Result<(), ApiError> {
        let in_lib = self.state().in_library(kind, id);
        if !in_lib {
            let _ = self.add_to_library(kind, id).await;
        }
        self.enqueue(kind, id, location.clone()).await?;
        if kind == ItemKind::Game && self.settings().install_emulator_with_game
            && let Some(emu) = self.pick_emulator(id)
            && self.installed(ItemKind::Emulator, emu.id).is_none()
        {
            let _ = self.enqueue(ItemKind::Emulator, emu.id, location).await;
        }
        Ok(())
    }
}
