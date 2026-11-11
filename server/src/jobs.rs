//! Background jobs: library scans, metadata downloads and file hashing (port of jobs.ts).

use crate::db::{Db, now};
use crate::error::{HttpError, HttpResult};
use crate::events::EventHub;
use crate::hasheous::{HasheousResult, hash_file, lookup_hasheous, should_hash};
use crate::igdb::Igdb;
use crate::library::{Library, ScanResult, UnsortedFile, walk_files};
use crate::log;
use crate::util::{mtime_ms, resolve_inside};
use nostalgia_shared::ServerEvent;
use regex::Regex;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<ScanResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MetadataStatus {
    pub running: bool,
    pub queue: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    pub done: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HashingStatus {
    pub running: bool,
    pub queue: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    pub files_done: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct JobStatus {
    pub scan: ScanStatus,
    pub metadata: MetadataStatus,
    pub hashing: HashingStatus,
}

#[derive(Default)]
struct Queues {
    meta: VecDeque<i64>,
    meta_current: Option<i64>,
    hash: VecDeque<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifyResult {
    #[serde(flatten)]
    pub result: HasheousResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
}

pub struct Jobs {
    db: Db,
    library: Library,
    igdb: Arc<Igdb>,
    events: Arc<EventHub>,
    http: reqwest::Client,
    pub status: Mutex<JobStatus>,
    queues: Mutex<Queues>,
    identify_cache: Mutex<HashMap<String, IdentifyResult>>,
}

static CUE_FILE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)FILE\s+"([^"]+)""#).unwrap());

impl Jobs {
    pub fn new(db: Db, library: Library, igdb: Arc<Igdb>, events: Arc<EventHub>, http: reqwest::Client) -> Arc<Jobs> {
        Arc::new(Jobs {
            db,
            library,
            igdb,
            events,
            http,
            status: Mutex::new(JobStatus::default()),
            queues: Mutex::new(Queues::default()),
            identify_cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn snapshot(&self) -> JobStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn unsorted(&self) -> Vec<UnsortedFile> {
        self.status.lock().unwrap().scan.last.as_ref().map(|l| l.unsorted.clone()).unwrap_or_default()
    }

    pub fn scan_running(&self) -> bool {
        self.status.lock().unwrap().scan.running
    }

    /// Scan the library. Metadata for new games is fetched in the background; hashing is optional.
    pub async fn scan(self: &Arc<Self>, hash: bool) -> Option<ScanResult> {
        {
            let mut st = self.status.lock().unwrap();
            if st.scan.running {
                return None;
            }
            st.scan.running = true;
            st.scan.started_at = Some(now());
            st.scan.last_error = None;
        }
        let lib = self.library.clone();
        let res = tokio::task::spawn_blocking(move || lib.scan()).await.map_err(anyhow::Error::from).and_then(|r| r);
        let out = match res {
            Ok(result) => {
                log::info(format!(
                    "Scan done: {} games (+{}), {} emulators (+{}), {} unsorted",
                    result.games,
                    result.new_games,
                    result.emulators,
                    result.new_emulators,
                    result.unsorted.len()
                ));
                self.status.lock().unwrap().scan.last = Some(result.clone());
                let pending: Vec<i64> = {
                    let conn = self.db.conn();
                    let mut st = conn.prepare("SELECT id FROM games WHERE missing = 0 AND metadata_status = 'pending'").expect("sql");
                    st.query_map([], |r| r.get(0)).map(|r| r.flatten().collect()).unwrap_or_default()
                };
                for id in pending {
                    self.queue_metadata(id);
                }
                if hash {
                    for g in self.library.list_games(true) {
                        self.queue_hash(g.id);
                    }
                }
                if result.new_games > 0 || result.new_emulators > 0 || result.missing_games > 0 {
                    self.events.to_all(ServerEvent::Catalog);
                }
                Some(result)
            }
            Err(e) => {
                self.status.lock().unwrap().scan.last_error = Some(e.to_string());
                log::error(format!("Scan failed {e}"));
                None
            }
        };
        self.status.lock().unwrap().scan.running = false;
        out
    }

    pub fn queue_metadata(self: &Arc<Self>, game_id: i64) {
        let mut q = self.queues.lock().unwrap();
        if !q.meta.contains(&game_id) && q.meta_current != Some(game_id) {
            q.meta.push_back(game_id);
        }
        self.status.lock().unwrap().metadata.queue = q.meta.len();
        drop(q);
        let me = self.clone();
        tokio::spawn(async move { me.run_metadata().await });
    }

    async fn run_metadata(self: Arc<Self>) {
        {
            let mut st = self.status.lock().unwrap();
            if st.metadata.running {
                return;
            }
            st.metadata.running = true;
        }
        let mut changed = false;
        loop {
            let id = {
                let mut q = self.queues.lock().unwrap();
                let Some(id) = q.meta.pop_front() else { break };
                q.meta_current = Some(id);
                self.status.lock().unwrap().metadata.queue = q.meta.len();
                id
            };
            let Some(row) = self.library.get_game(id).filter(|r| !r.missing) else { continue };
            self.status.lock().unwrap().metadata.current = Some(row.folder.clone());
            if !self.igdb.configured() {
                let _ = self.db.conn().execute(
                    "UPDATE games SET metadata_status = 'error', metadata_error = 'IGDB credentials not configured' WHERE id = ?",
                    [id],
                );
                self.status.lock().unwrap().metadata.failed += 1;
                continue;
            }
            let dir = self.library.game_dir(&row.rel_path);
            match self.igdb.download_into(&dir, &self.library.metadata_dir(row.igdb_id), row.igdb_id).await {
                Ok(meta) => {
                    let json = serde_json::to_string_pretty(&meta).unwrap_or_default();
                    let _ = self.db.conn().execute(
                        "UPDATE games SET metadata_json = ?, metadata_status = 'ok', metadata_error = NULL, name = ?, updated_at = ? WHERE id = ?",
                        params![json, meta.name, now(), id],
                    );
                    self.status.lock().unwrap().metadata.done += 1;
                    changed = true;
                    log::info(format!("Metadata downloaded for {}", row.folder));
                }
                Err(e) => {
                    self.status.lock().unwrap().metadata.failed += 1;
                    let _ = self.db.conn().execute(
                        "UPDATE games SET metadata_status = 'error', metadata_error = ? WHERE id = ?",
                        params![e.to_string(), id],
                    );
                    log::warn(format!("Metadata failed for {}: {e}", row.folder));
                }
            }
        }
        {
            let mut st = self.status.lock().unwrap();
            st.metadata.running = false;
            st.metadata.current = None;
        }
        self.queues.lock().unwrap().meta_current = None;
        if changed {
            self.events.to_all(ServerEvent::Catalog);
        }
    }

    pub fn queue_hash(self: &Arc<Self>, game_id: i64) {
        let mut q = self.queues.lock().unwrap();
        if !q.hash.contains(&game_id) {
            q.hash.push_back(game_id);
        }
        self.status.lock().unwrap().hashing.queue = q.hash.len();
        drop(q);
        let me = self.clone();
        tokio::spawn(async move { me.run_hash().await });
    }

    async fn run_hash(self: Arc<Self>) {
        {
            let mut st = self.status.lock().unwrap();
            if st.hashing.running {
                return;
            }
            let h = &mut st.hashing;
            h.running = true;
            h.files_done = 0;
            h.bytes_done = 0;
            h.bytes_total = 0;
        }
        loop {
            let id = {
                let mut q = self.queues.lock().unwrap();
                let Some(id) = q.hash.pop_front() else { break };
                self.status.lock().unwrap().hashing.queue = q.hash.len();
                id
            };
            let Some(row) = self.library.get_game(id).filter(|r| !r.missing) else { continue };
            let dir = self.library.game_dir(&row.rel_path);
            for f in walk_files(&dir).into_iter().filter(|x| should_hash(&x.path)) {
                let known = self
                    .db
                    .conn()
                    .query_row(
                        "SELECT size, mtime, md5, sha1, crc32, hasheous_status FROM file_hashes WHERE game_id = ? AND rel_path = ?",
                        params![id, f.path],
                        |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, i64>(1)?,
                                r.get::<_, Option<String>>(2)?,
                                r.get::<_, Option<String>>(3)?,
                                r.get::<_, Option<String>>(4)?,
                                r.get::<_, Option<String>>(5)?,
                            ))
                        },
                    )
                    .optional()
                    .ok()
                    .flatten();
                let unchanged = known.as_ref().filter(|k| k.0 == f.size as i64 && k.1 == f.mtime && k.2.as_deref().is_some_and(|m| !m.is_empty()));
                if let Some(k) = unchanged
                    && k.5.as_deref() != Some("error")
                {
                    continue;
                }
                self.status.lock().unwrap().hashing.current = Some(format!("{}/{}", row.folder, f.path));
                // Only the lookup failed last time: reuse the stored hashes instead of reading the file again.
                let hashes = if let Some(k) = unchanged {
                    crate::hasheous::FileHashes { md5: k.2.clone().unwrap_or_default(), sha1: k.3.clone().unwrap_or_default(), crc32: k.4.clone().unwrap_or_default() }
                } else {
                    self.status.lock().unwrap().hashing.bytes_total += f.size;
                    let me = self.clone();
                    let path = dir.join(&f.path);
                    let r = tokio::task::spawn_blocking(move || hash_file(&path, |n| me.status.lock().unwrap().hashing.bytes_done += n)).await;
                    match r {
                        Ok(Ok(h)) => h,
                        Ok(Err(e)) => {
                            log::error(format!("Hashing failed {e}"));
                            continue;
                        }
                        Err(e) => {
                            log::error(format!("Hashing failed {e}"));
                            continue;
                        }
                    }
                };
                let res = lookup_hasheous(&self.http, &hashes).await;
                let _ = self.db.conn().execute(
                    "INSERT INTO file_hashes (game_id, rel_path, size, mtime, md5, sha1, crc32, hasheous_status, hasheous_name, hasheous_json, checked_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(game_id, rel_path) DO UPDATE SET size = excluded.size, mtime = excluded.mtime, md5 = excluded.md5,
                       sha1 = excluded.sha1, crc32 = excluded.crc32, hasheous_status = excluded.hasheous_status,
                       hasheous_name = excluded.hasheous_name, hasheous_json = excluded.hasheous_json, checked_at = excluded.checked_at",
                    params![
                        id,
                        f.path,
                        f.size as i64,
                        f.mtime,
                        hashes.md5,
                        hashes.sha1,
                        hashes.crc32,
                        res.status,
                        res.rom_name.clone().or(res.name.clone()).or(res.error.clone()),
                        res.raw.as_ref().map(|r| r.to_string()),
                        now()
                    ],
                );
                self.status.lock().unwrap().hashing.files_done += 1;
            }
        }
        let mut st = self.status.lock().unwrap();
        st.hashing.running = false;
        st.hashing.current = None;
    }

    /// Hash an unsorted entry (file or the first hashable file of a folder) and ask Hasheous what it is.
    pub async fn identify(&self, platform: &str, entry: &str) -> HttpResult<IdentifyResult> {
        let pdir = resolve_inside(self.library.games_path(), platform)?;
        let abs = resolve_inside(&pdir, entry)?;
        let st = std::fs::metadata(&abs).map_err(|e| HttpError::new(404, e.to_string()))?;
        let mut file = abs.clone();
        if st.is_dir() {
            match walk_files(&abs).into_iter().find(|x| should_hash(&x.path)) {
                Some(f) => file = f.path.split('/').fold(abs.clone(), |p, s| p.join(s)),
                None => {
                    return Ok(IdentifyResult { result: HasheousResult { status: "nomatch".into(), ..Default::default() }, file: None, md5: None });
                }
            }
        } else if abs.extension().map(|e| e.eq_ignore_ascii_case("cue")).unwrap_or(false) {
            // Prefer the bin referenced by a cue sheet.
            if let Ok(txt) = std::fs::read_to_string(&abs)
                && let Some(m) = CUE_FILE_RE.captures(&txt)
            {
                let bin = pdir.join(&m[1]);
                if bin.exists() {
                    file = bin;
                }
            }
        }
        let fst = std::fs::metadata(&file)?;
        let key = format!("{}|{}|{}", file.display(), fst.len(), mtime_ms(&fst));
        if let Some(c) = self.identify_cache.lock().unwrap().get(&key) {
            return Ok(c.clone());
        }
        let f2 = file.clone();
        let hashes = tokio::task::spawn_blocking(move || hash_file(&f2, |_| {})).await.map_err(HttpError::internal)??;
        let mut res = lookup_hasheous(&self.http, &hashes).await;
        res.raw = None;
        let out = IdentifyResult { result: res, file: Some(relative(&pdir, &file)), md5: Some(hashes.md5) };
        if out.result.status != "error" {
            self.identify_cache.lock().unwrap().insert(key, out.clone());
        }
        Ok(out)
    }
}

/// `path.relative(from, to)` for a path below `from`.
fn relative(from: &Path, to: &Path) -> String {
    to.strip_prefix(from).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| to.to_string_lossy().into_owned())
}
