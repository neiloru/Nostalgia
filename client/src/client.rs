//! Client core (port of core.ts / api.ts): session, server connection, offline cache, play time,
//! save sync state. Runs on a tokio runtime; the UI is told about changes through `UiEvent`s.

use crate::media_cache::{MediaCache, MediaKey};
use crate::model::*;
use crate::store::JsonStore;
use crate::util::iso_now;
use futures_util::StreamExt;
use nostalgia_shared::{
    AuthResponse, EmulatorSummary, GameDetail, GameSummary, ItemKind, LibraryState, OsKey, PlaytimeEntry, SaveVersion, ServerEvent,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Media of a game: a file of an installed game, or an image held in memory (catalog browsing).
#[derive(Clone)]
pub enum Media {
    File(PathBuf),
    Image(Arc<gpui_kit::RenderImage>),
}

impl From<Media> for gpui_kit::ImageSource {
    fn from(m: Media) -> Self {
        match m {
            Media::File(p) => p.into(),
            Media::Image(i) => i.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    State,
    Downloads,
    Toast { message: String, error: bool },
    /// Minimise the window (a game was started).
    Minimize,
    /// Bring the window back (the game exited).
    Restore,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    pub network: bool,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    pub fn new(status: u16, message: impl Into<String>) -> ApiError {
        ApiError { status, message: message.into(), network: false }
    }
}

pub fn normalize_server_url(input: &str) -> Result<String, ApiError> {
    let mut s = input.trim().to_string();
    if s.is_empty() {
        return Err(ApiError::new(0, "Server address is required"));
    }
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        s = format!("http://{s}");
    }
    Ok(s.trim_end_matches('/').to_string())
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub user_data: PathBuf,
    pub home: PathBuf,
    pub documents: PathBuf,
    pub app_data: PathBuf,
}

impl Paths {
    /// Same folder as the Electron client: `<appData>/@nostalgia/client`, a `nostalgia-data` folder
    /// next to the executable in portable mode, or `NOSTALGIA_DATA_DIR`.
    pub fn detect() -> Paths {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let app_data = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        let documents = dirs::document_dir().unwrap_or_else(|| home.join("Documents"));
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
        let user_data = if let Ok(d) = std::env::var("NOSTALGIA_DATA_DIR")
            && !d.is_empty()
        {
            PathBuf::from(d)
        } else if exe_dir.join("portable").exists() {
            exe_dir.join("nostalgia-data")
        } else {
            app_data.join("@nostalgia").join("client")
        };
        Paths { user_data, home, documents, app_data }
    }
    /// Disk caches of earlier versions (catalog media and details of every game ever shown).
    fn legacy_cache_dirs(&self) -> [PathBuf; 2] {
        [self.user_data.join("media-cache"), self.user_data.join("details")]
    }
    pub fn save_backup_dir(&self) -> PathBuf {
        self.user_data.join("save-backups")
    }
}

pub(crate) struct Inner {
    pub session: JsonStore<SessionFile>,
    pub settings: JsonStore<ClientSettings>,
    pub installed: JsonStore<Installed>,
    pub game_settings: JsonStore<BTreeMap<i64, GameSettings>>,
    pub save_sync: JsonStore<SaveSyncFile>,
    pub pending: JsonStore<PendingFile>,
    pub cache: JsonStore<CacheFile>,
    pub downloads: JsonStore<DownloadsFile>,
    pub online: bool,
    pub running: Option<RunningGame>,
    pub fullscreen: bool,
    pub ui_zoom: f64,
    pub last_error: Option<String>,
    pub busy_saves: HashSet<i64>,
    pub ready: bool,
    pub media_failed: HashSet<MediaKey>,
    pub media_loading: HashSet<MediaKey>,
    pub media: MediaCache<gpui_kit::RenderImage>,
}

impl Inner {
    fn cache_key(&self) -> String {
        match &self.session.data().session {
            Some(s) => format!("{}|{}", s.server_url, s.user.id),
            None => String::new(),
        }
    }
    pub fn cache(&self) -> Cache {
        self.cache.data().get(&self.cache_key()).cloned().unwrap_or_default()
    }
    pub fn cache_ref(&self) -> Option<&Cache> {
        self.cache.data().get(&self.cache_key())
    }
    pub fn set_cache(&mut self, f: impl FnOnce(&mut Cache)) {
        let key = self.cache_key();
        if key.is_empty() {
            return;
        }
        self.cache.update(|c| f(c.entry(key).or_default()));
    }
    pub fn user_id(&self) -> Option<i64> {
        self.session.data().session.as_ref().map(|s| s.user.id)
    }
    pub fn game(&self, id: i64) -> Option<GameSummary> {
        self.cache_ref().and_then(|c| c.games.iter().find(|g| g.id == id).cloned())
    }
    pub fn emulator(&self, id: i64) -> Option<EmulatorSummary> {
        self.cache_ref().and_then(|c| c.emulators.iter().find(|e| e.id == id).cloned())
    }
    pub fn installed(&self, kind: ItemKind, id: i64) -> Option<InstalledItem> {
        self.installed.data().get(kind, id).cloned()
    }
    pub fn save_state(&self, emulator_id: i64) -> SaveSyncState {
        let uid = self.user_id().unwrap_or(0).to_string();
        self.save_sync.data().get(&uid).and_then(|m| m.get(&emulator_id)).cloned().unwrap_or_default()
    }
}

pub struct Client {
    pub rt: tokio::runtime::Handle,
    pub paths: Paths,
    pub version: String,
    pub os: OsKey,
    pub device: String,
    pub(crate) http: reqwest::Client,
    pub(crate) inner: Mutex<Inner>,
    ui_tx: Mutex<Option<futures_channel::mpsc::UnboundedSender<UiEvent>>>,
    state_pending: AtomicBool,
    snapshot: Mutex<Arc<ClientState>>,
    pub(crate) downloads_rt: Mutex<crate::downloads::DownloadRuntime>,
    pub(crate) launcher_rt: Mutex<crate::launcher::LauncherRuntime>,
    sse: Mutex<Option<tokio::task::AbortHandle>>,
    save_retry_scheduled: AtomicBool,
}

pub type Shared = Arc<Client>;

impl Client {
    pub fn new(rt: tokio::runtime::Handle, paths: Paths, ui_tx: futures_channel::mpsc::UnboundedSender<UiEvent>) -> Shared {
        let ud = paths.user_data.clone();
        let _ = std::fs::create_dir_all(&ud);
        let mut device_store: JsonStore<DeviceFile> = JsonStore::open(ud.join("device.json"));
        if device_store.data().id.is_none() {
            device_store.update(|d| d.id = Some(uuid::Uuid::new_v4().to_string()));
            device_store.save_now();
        }
        let os = OsKey::current();
        let hostname = gethostname::gethostname().to_string_lossy().into_owned();
        let mut downloads: JsonStore<DownloadsFile> = JsonStore::open(ud.join("downloads.json"));
        // A job that was running when the app quit is resumed.
        downloads.update(|d| {
            for j in &mut d.jobs {
                if j.status == DownloadStatus::Downloading {
                    j.status = DownloadStatus::Queued;
                }
            }
        });
        let inner = Inner {
            session: JsonStore::open(ud.join("session.json")),
            settings: JsonStore::open(ud.join("settings.json")),
            installed: JsonStore::open(ud.join("installed.json")),
            game_settings: JsonStore::open(ud.join("game-settings.json")),
            save_sync: JsonStore::open(ud.join("save-sync.json")),
            pending: JsonStore::open(ud.join("pending.json")),
            cache: JsonStore::open(ud.join("cache.json")),
            downloads,
            online: false,
            running: None,
            fullscreen: false,
            ui_zoom: 1.0,
            last_error: None,
            busy_saves: HashSet::new(),
            ready: false,
            media_failed: HashSet::new(),
            media_loading: HashSet::new(),
            media: MediaCache::new(crate::media_cache::LIMIT),
        };
        let device = format!("{hostname} ({})", os.as_str());
        let http = reqwest::Client::builder()
            .user_agent(concat!("Nostalgia/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("http client");
        let client = Arc::new(Client {
            rt,
            paths,
            version: env!("CARGO_PKG_VERSION").to_string(),
            os,
            device,
            http,
            inner: Mutex::new(inner),
            ui_tx: Mutex::new(Some(ui_tx)),
            state_pending: AtomicBool::new(false),
            snapshot: Mutex::new(Arc::new(ClientState::default())),
            downloads_rt: Mutex::new(Default::default()),
            launcher_rt: Mutex::new(Default::default()),
            sse: Mutex::new(None),
            save_retry_scheduled: AtomicBool::new(false),
        });
        client.verify_installed();
        // Earlier versions cached media and details of every catalog game on disk.
        for dir in client.paths.legacy_cache_dirs() {
            if dir.exists() {
                client.rt.spawn_blocking(move || std::fs::remove_dir_all(dir));
            }
        }
        client.rebuild_snapshot();
        client
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn push_event(&self, ev: UiEvent) {
        if let Some(tx) = self.ui_tx.lock().unwrap().as_ref() {
            let _ = tx.unbounded_send(ev);
        }
    }

    pub fn toast(&self, message: impl Into<String>, error: bool) {
        self.push_event(UiEvent::Toast { message: message.into(), error });
    }

    pub fn emit_downloads(&self) {
        self.push_event(UiEvent::Downloads);
    }

    pub fn minimize(&self) {
        self.push_event(UiEvent::Minimize);
    }

    pub fn restore(&self) {
        self.push_event(UiEvent::Restore);
    }

    /// Rebuild the UI snapshot soon (changes are batched for 30 ms like the Electron client).
    pub fn emit_state(self: &Arc<Self>) {
        if self.state_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let me = self.clone();
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            me.state_pending.store(false, Ordering::Release);
            me.rebuild_snapshot();
            me.push_event(UiEvent::State);
        });
    }

    pub fn state(&self) -> Arc<ClientState> {
        self.snapshot.lock().unwrap().clone()
    }

    fn rebuild_snapshot(&self) {
        let st = {
            let i = self.lock();
            let c = i.cache();
            let uid = i.user_id().unwrap_or(0).to_string();
            let mut save_sync = BTreeMap::new();
            for e in &c.emulators {
                let s = i.save_sync.data().get(&uid).and_then(|m| m.get(&e.id)).cloned().unwrap_or_default();
                let latest = c.latest_saves.iter().find(|v| v.emulator_id == e.id).cloned();
                save_sync.insert(e.id, (s, i.busy_saves.contains(&e.id), latest));
            }
            let session = i.session.data().session.as_ref().map(|s| (s.server_url.clone(), s.user.clone()));
            let user_id = i.user_id();
            ClientState {
                ready: i.ready,
                session,
                online: i.online,
                catalog_games: c.games,
                catalog_emulators: c.emulators,
                library: c.library,
                installed: i.installed.data().clone(),
                playtime: c.playtime.into_iter().map(|p| (p.game_id, p)).collect(),
                settings: i.settings.data().clone(),
                running: i.running.clone(),
                save_sync,
                pending_sessions: i.pending.data().sessions.iter().filter(|s| Some(s.user_id) == user_id).count(),
                fullscreen: i.fullscreen,
                ui_zoom: i.ui_zoom,
                os: Some(self.os),
                device: self.device.clone(),
                version: self.version.clone(),
            }
        };
        *self.snapshot.lock().unwrap() = Arc::new(st);
    }

    /// Write changed JSON files (called periodically and on exit).
    pub fn flush(&self) {
        let mut guard = self.lock();
        let i = &mut *guard;
        i.session.flush();
        i.settings.flush();
        i.installed.flush();
        i.game_settings.flush();
        i.save_sync.flush();
        i.pending.flush();
        let installed = i.installed.data();
        i.cache.flush_as(|file| file.iter().map(|(key, c)| (key.clone(), c.for_disk(installed))).collect::<CacheFile>());
        i.downloads.flush();
    }

    // ------------------------------------------------------------------ http
    pub fn api_url(&self, path: &str, server_url: Option<&str>) -> Result<String, ApiError> {
        let base = match server_url {
            Some(s) => s.to_string(),
            None => self.lock().session.data().session.as_ref().map(|s| s.server_url.clone()).ok_or_else(|| ApiError::new(0, "Not connected to a server"))?,
        };
        Ok(format!("{base}/api{path}"))
    }

    pub fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let token = self.lock().session.data().session.as_ref().map(|s| s.token.clone());
        let mut r = self.http.request(method, url).header("X-Device", &self.device);
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        r
    }

    /// Send a request; network failures switch to offline mode, 401 ends the session.
    pub async fn send(self: &Arc<Self>, path: &str, req: reqwest::RequestBuilder) -> Result<reqwest::Response, ApiError> {
        let res = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                self.set_online(false);
                let msg = if e.is_timeout() { "timeout".to_string() } else { root_cause(&e) };
                return Err(ApiError { status: 0, message: format!("Server unreachable ({msg})"), network: true });
            }
        };
        if res.status().as_u16() == 401 && !path.starts_with("/auth/") {
            self.handle_unauthorized();
        }
        Ok(res)
    }

    pub async fn json<T: DeserializeOwned>(
        self: &Arc<Self>,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        server_url: Option<&str>,
        timeout: Duration,
    ) -> Result<T, ApiError> {
        let url = self.api_url(path, server_url)?;
        let mut req = self.request(method, &url).timeout(timeout);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = self.send(path, req).await?;
        let status = res.status().as_u16();
        let text = res.text().await.map_err(|e| ApiError { status: 0, message: format!("Server unreachable ({e})"), network: true })?;
        let data: Value = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) };
        if !(200..300).contains(&status) {
            let msg = data.get("error").and_then(|e| e.as_str()).map(String::from).unwrap_or_else(|| format!("HTTP {status}"));
            return Err(ApiError::new(status, msg));
        }
        serde_json::from_value(data).map_err(|e| ApiError::new(status, format!("Unexpected answer from the server: {e}")))
    }

    pub async fn get<T: DeserializeOwned>(self: &Arc<Self>, path: &str) -> Result<T, ApiError> {
        self.json(reqwest::Method::GET, path, None, None, Duration::from_secs(20)).await
    }

    // ------------------------------------------------------------------ session
    pub fn start(self: &Arc<Self>) {
        self.recover_active_session();
        let me = self.clone();
        self.rt.spawn(async move {
            me.lock().ready = true;
            let has_session = me.lock().session.data().session.is_some();
            if has_session {
                me.check_health().await;
            }
            me.emit_state();
            // Startup: fetch the newest cloud saves of all installed emulators.
            me.sync_all_saves("startup").await;
            me.retry_pending_saves().await;
        });
        // Health checks: every 30 s online, 10 s offline.
        let me = self.clone();
        self.rt.spawn(async move {
            loop {
                let online = me.lock().online;
                tokio::time::sleep(Duration::from_secs(if online { 30 } else { 10 })).await;
                me.check_health().await;
            }
        });
        // Persist changed stores.
        let me = self.clone();
        self.rt.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(300)).await;
                me.flush();
            }
        });
    }

    pub async fn login(self: &Arc<Self>, server_url: &str, username: &str, password: &str, register: bool) -> Result<(), ApiError> {
        let url = normalize_server_url(server_url)?;
        let health: Value = self
            .json(reqwest::Method::GET, "/health", None, Some(&url), Duration::from_secs(8))
            .await
            .map_err(|e| ApiError::new(0, if e.network { format!("Cannot reach {url}") } else { e.message }))?;
        if health.get("name").and_then(|v| v.as_str()) != Some("nostalgia") {
            return Err(ApiError::new(0, format!("{url} is not a Nostalgia server")));
        }
        let res: AuthResponse = self
            .json(
                reqwest::Method::POST,
                if register { "/auth/register" } else { "/auth/login" },
                Some(json!({ "username": username, "password": password })),
                Some(&url),
                Duration::from_secs(20),
            )
            .await?;
        {
            let mut i = self.lock();
            i.session.update(|s| s.session = Some(Session { server_url: url, token: res.token, user: res.user }));
            i.session.save_now();
        }
        self.set_online(true);
        self.refresh_all().await;
        // Former 'login' event: downloads continue, newest saves are fetched.
        self.pump_downloads();
        let me = self.clone();
        self.rt.spawn(async move { me.sync_all_saves("login").await });
        self.emit_state();
        Ok(())
    }

    pub async fn logout(self: &Arc<Self>) -> Result<(), ApiError> {
        if self.launcher_running() {
            return Err(ApiError::new(0, "Close the running game first"));
        }
        self.pause_all_downloads();
        let (has, online) = {
            let i = self.lock();
            (i.session.data().session.is_some(), i.online)
        };
        if has && online {
            let _ = self.json::<Value>(reqwest::Method::POST, "/auth/logout", None, None, Duration::from_secs(10)).await;
        }
        self.stop_events();
        {
            let mut i = self.lock();
            i.session.update(|s| s.session = None);
            i.session.save_now();
            i.online = false;
        }
        self.emit_state();
        Ok(())
    }

    fn handle_unauthorized(self: &Arc<Self>) {
        let had = self.lock().session.data().session.is_some();
        if !had {
            return;
        }
        self.toast("Your session has expired - please sign in again", true);
        self.stop_events();
        {
            let mut i = self.lock();
            i.session.update(|s| s.session = None);
            i.online = false;
        }
        self.emit_state();
    }

    pub async fn change_password(self: &Arc<Self>, old: &str, new: &str) -> Result<(), ApiError> {
        self.json::<Value>(reqwest::Method::POST, "/me/password", Some(json!({ "oldPassword": old, "newPassword": new })), None, Duration::from_secs(20))
            .await?;
        self.toast("Password changed", false);
        Ok(())
    }

    // ------------------------------------------------------------------ online / offline
    pub fn set_online(self: &Arc<Self>, v: bool) {
        {
            let mut i = self.lock();
            if i.online == v {
                return;
            }
            i.online = v;
            if v {
                i.media_failed.clear();
            }
        }
        if v {
            self.start_events();
            self.on_online_downloads();
            let me = self.clone();
            self.rt.spawn(async move { me.sync_all_saves("online").await });
        } else {
            self.stop_events();
        }
        self.emit_state();
    }

    pub fn is_online(&self) -> bool {
        self.lock().online
    }

    pub async fn check_health(self: &Arc<Self>) -> bool {
        if self.lock().session.data().session.is_none() {
            return false;
        }
        let t = Duration::from_secs(6);
        let r: Result<Value, ApiError> = async {
            self.json::<Value>(reqwest::Method::GET, "/health", None, None, t).await?;
            // Validate the token too.
            self.json::<Value>(reqwest::Method::GET, "/me", None, None, t).await
        }
        .await;
        match r {
            Ok(_) => {
                let was_offline = !self.lock().online;
                self.set_online(true);
                if was_offline {
                    self.refresh_all().await;
                }
                true
            }
            Err(e) if !e.network && e.status != 401 => {
                // Server reachable but erroring - treat as online.
                self.set_online(true);
                true
            }
            Err(_) => {
                self.set_online(false);
                false
            }
        }
    }

    pub async fn refresh_all(self: &Arc<Self>) {
        if self.lock().session.data().session.is_none() {
            return;
        }
        let r = tokio::try_join!(
            self.get::<Vec<GameSummary>>("/games"),
            self.get::<Vec<EmulatorSummary>>("/emulators"),
            self.get::<LibraryState>("/library"),
            self.get::<Vec<PlaytimeEntry>>("/playtime"),
            self.get::<Vec<SaveVersion>>("/saves"),
        );
        match r {
            Ok((games, emulators, library, playtime, latest_saves)) => {
                self.lock().set_cache(|c| {
                    c.games = games;
                    c.emulators = emulators;
                    c.library = library;
                    c.playtime = playtime;
                    c.latest_saves = latest_saves;
                    c.fetched_at = Some(iso_now());
                });
                self.flush_playtime().await;
                // Former 'refreshed' event.
                let me = self.clone();
                self.rt.spawn(async move { me.retry_pending_saves().await });
            }
            Err(e) => self.lock().last_error = Some(e.message),
        }
        self.emit_state();
    }

    pub async fn refresh_latest_saves(self: &Arc<Self>) -> Result<Vec<SaveVersion>, ApiError> {
        if !self.is_online() {
            return Ok(self.lock().cache().latest_saves);
        }
        let latest: Vec<SaveVersion> = self.get("/saves").await?;
        self.lock().set_cache(|c| c.latest_saves = latest.clone());
        Ok(latest)
    }

    /// "Retry" in the UI.
    pub async fn refresh(self: &Arc<Self>) -> bool {
        let ok = self.check_health().await;
        if ok {
            self.refresh_all().await;
        }
        ok
    }

    // ------------------------------------------------------------------ push events (SSE)
    fn start_events(self: &Arc<Self>) {
        let mut sse = self.sse.lock().unwrap();
        if sse.is_some() || self.lock().session.data().session.is_none() {
            return;
        }
        let me = self.clone();
        let task = self.rt.spawn(async move {
            let mut backoff = 2000u64;
            loop {
                let r: anyhow::Result<()> = async {
                    let url = me.api_url(&format!("/events?device={}", crate::util_url::encode(&me.device)), None)?;
                    let res = me.request(reqwest::Method::GET, &url).header("Accept", "text/event-stream").send().await?;
                    if !res.status().is_success() {
                        anyhow::bail!("HTTP {}", res.status().as_u16());
                    }
                    backoff = 2000;
                    let mut stream = res.bytes_stream();
                    let mut buf = String::new();
                    while let Some(chunk) = stream.next().await {
                        buf.push_str(&String::from_utf8_lossy(&chunk?));
                        while let Some(idx) = buf.find("\n\n") {
                            let block: String = buf[..idx].to_string();
                            buf.drain(..idx + 2);
                            let data: Vec<&str> = block.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim).collect();
                            if !data.is_empty()
                                && let Ok(ev) = serde_json::from_str::<ServerEvent>(&data.join("\n"))
                            {
                                me.handle_event(ev);
                            }
                        }
                    }
                    Ok(())
                }
                .await;
                let _ = r;
                tokio::time::sleep(Duration::from_millis(backoff)).await;
                backoff = (backoff * 2).min(60_000);
            }
        });
        *sse = Some(task.abort_handle());
    }

    fn stop_events(&self) {
        if let Some(h) = self.sse.lock().unwrap().take() {
            h.abort();
        }
    }

    fn handle_event(self: &Arc<Self>, ev: ServerEvent) {
        match ev {
            ServerEvent::Library { library } => {
                self.lock().set_cache(|c| c.library = library);
                self.emit_state();
            }
            ServerEvent::Catalog => {
                let me = self.clone();
                self.rt.spawn(async move { me.refresh_all().await });
            }
            ServerEvent::Save { emulator_id, version, .. } => {
                self.lock().set_cache(|c| {
                    c.latest_saves.retain(|s| s.emulator_id != emulator_id);
                    c.latest_saves.push(version);
                });
                self.on_remote_save(emulator_id);
                self.emit_state();
            }
            ServerEvent::Hello { .. } => {}
        }
    }

    // ------------------------------------------------------------------ catalog / library
    pub fn game(&self, id: i64) -> Option<GameSummary> {
        self.lock().game(id)
    }
    pub fn emulator(&self, id: i64) -> Option<EmulatorSummary> {
        self.lock().emulator(id)
    }
    pub fn installed(&self, kind: ItemKind, id: i64) -> Option<InstalledItem> {
        self.lock().installed(kind, id)
    }
    pub fn settings(&self) -> ClientSettings {
        self.lock().settings.data().clone()
    }

    pub async fn game_detail(self: &Arc<Self>, id: i64) -> Option<GameDetail> {
        // Not cached on disk: offline, installed games have their metadata in the install folder.
        if self.is_online()
            && let Ok(d) = self.get::<GameDetail>(&format!("/games/{id}")).await
        {
            return Some(d);
        }
        let (inst, summary) = {
            let i = self.lock();
            (i.installed(ItemKind::Game, id), i.game(id))
        };
        let summary = summary?;
        let metadata = inst
            .and_then(|it| std::fs::read_to_string(Path::new(&it.path).join("igdb").join("metadata.json")).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        Some(GameDetail { summary, metadata })
    }

    async fn library_call(self: &Arc<Self>, method: reqwest::Method, kind: &str, id: i64) -> Result<(), ApiError> {
        if !self.is_online() {
            return Err(ApiError::new(0, "You are offline - library changes need a connection to the server"));
        }
        let library: LibraryState = self.json(method, &format!("/library/{kind}/{id}"), None, None, Duration::from_secs(20)).await?;
        self.lock().set_cache(|c| c.library = library);
        self.emit_state();
        Ok(())
    }
    pub async fn add_to_library(self: &Arc<Self>, kind: ItemKind, id: i64) -> Result<(), ApiError> {
        self.library_call(reqwest::Method::POST, kind.plural(), id).await
    }
    pub async fn remove_from_library(self: &Arc<Self>, kind: ItemKind, id: i64) -> Result<(), ApiError> {
        self.library_call(reqwest::Method::DELETE, kind.plural(), id).await
    }

    // ------------------------------------------------------------------ installed items
    fn verify_installed(&self) {
        let mut i = self.lock();
        i.installed.update(|d| {
            d.games.retain(|_, it| Path::new(&it.path).exists());
            d.emulators.retain(|_, it| Path::new(&it.path).exists());
        });
    }

    pub fn register_installed(self: &Arc<Self>, item: InstalledItem) {
        {
            let mut i = self.lock();
            i.installed.update(|d| {
                d.map_mut(item.kind).insert(item.id, item);
            });
            i.installed.save_now();
        }
        self.emit_state();
    }

    pub fn unregister_installed(self: &Arc<Self>, kind: ItemKind, id: i64) {
        {
            let mut i = self.lock();
            i.installed.update(|d| {
                d.map_mut(kind).remove(&id);
            });
            i.installed.save_now();
        }
        self.emit_state();
    }

    // ------------------------------------------------------------------ play time
    pub fn record_session(self: &Arc<Self>, game_id: i64, started_at: &str, seconds: f64) {
        let Some(uid) = self.lock().user_id() else { return };
        if seconds < 1.0 {
            return;
        }
        let secs = seconds.round() as i64;
        {
            let mut i = self.lock();
            i.pending.update(|d| {
                d.sessions.push(PendingSession {
                    session_id: uuid::Uuid::new_v4().to_string(),
                    game_id,
                    started_at: started_at.to_string(),
                    seconds: secs,
                    user_id: uid,
                })
            });
            i.pending.save_now();
            // Optimistic local update so the UI shows the new time immediately.
            i.set_cache(|c| {
                let cur = c.playtime.iter().find(|p| p.game_id == game_id).cloned();
                c.playtime.retain(|p| p.game_id != game_id);
                let (s, n) = cur.map(|p| (p.seconds, p.sessions)).unwrap_or((0, 0));
                c.playtime.push(PlaytimeEntry { game_id, seconds: s + secs, sessions: n + 1, last_played: Some(iso_now()) });
            });
        }
        let me = self.clone();
        self.rt.spawn(async move { me.flush_playtime().await });
    }

    pub async fn flush_playtime(self: &Arc<Self>) {
        let (online, uid, mine) = {
            let i = self.lock();
            let uid = i.user_id();
            (i.online, uid, i.pending.data().sessions.iter().filter(|s| Some(s.user_id) == uid).cloned().collect::<Vec<_>>())
        };
        if !online || uid.is_none() || mine.is_empty() {
            return;
        }
        let body: Vec<Value> = mine
            .iter()
            .map(|s| json!({ "sessionId": s.session_id, "gameId": s.game_id, "startedAt": s.started_at, "seconds": s.seconds }))
            .collect();
        if self.json::<Value>(reqwest::Method::POST, "/playtime", Some(Value::Array(body)), None, Duration::from_secs(20)).await.is_err() {
            return; // retried on next online transition
        }
        let sent: HashSet<String> = mine.iter().map(|s| s.session_id.clone()).collect();
        {
            let mut i = self.lock();
            i.pending.update(|d| d.sessions.retain(|s| !sent.contains(&s.session_id)));
            i.pending.save_now();
        }
        if let Ok(playtime) = self.get::<Vec<PlaytimeEntry>>("/playtime").await {
            self.lock().set_cache(|c| c.playtime = playtime);
            self.emit_state();
        }
    }

    /// Heartbeat of the running game, so play time survives a crash of the launcher.
    pub fn heartbeat(&self, r: Option<RunningGame>) {
        let mut i = self.lock();
        let uid = i.user_id();
        i.pending.update(|d| {
            d.active = match (r, uid) {
                (Some(g), Some(user_id)) => Some(ActiveSession { game: g, user_id, heartbeat: iso_now() }),
                _ => None,
            }
        });
        i.pending.save_now();
    }

    fn recover_active_session(&self) {
        let mut i = self.lock();
        let Some(a) = i.pending.data().active.clone() else { return };
        let secs = match (nostalgia_shared::parse_iso_millis(&a.heartbeat), nostalgia_shared::parse_iso_millis(&a.game.started_at)) {
            (Some(h), Some(s)) => (h - s) as f64 / 1000.0,
            _ => 0.0,
        };
        i.pending.update(|d| {
            if secs > 5.0 {
                d.sessions.push(PendingSession {
                    session_id: uuid::Uuid::new_v4().to_string(),
                    game_id: a.game.game_id,
                    started_at: a.game.started_at.clone(),
                    seconds: secs.round() as i64,
                    user_id: a.user_id,
                });
            }
            d.active = None;
        });
        i.pending.save_now();
    }

    // ------------------------------------------------------------------ save sync state
    pub fn save_state(&self, emulator_id: i64) -> SaveSyncState {
        self.lock().save_state(emulator_id)
    }

    pub fn set_save_state(self: &Arc<Self>, emulator_id: i64, f: impl FnOnce(&mut SaveSyncState)) {
        {
            let mut i = self.lock();
            let uid = i.user_id().unwrap_or(0).to_string();
            i.save_sync.update(|d| f(d.entry(uid).or_default().entry(emulator_id).or_default()));
            i.save_sync.save_now();
        }
        self.emit_state();
    }

    pub(crate) fn schedule_save_retry(self: &Arc<Self>) {
        if self.save_retry_scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let me = self.clone();
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_secs(120)).await;
            me.save_retry_scheduled.store(false, Ordering::Release);
            me.retry_pending_saves().await;
        });
    }

    // ------------------------------------------------------------------ settings / window state
    pub fn update_settings(self: &Arc<Self>, f: impl FnOnce(&mut ClientSettings)) {
        self.lock().settings.update(f);
        self.emit_state();
    }

    pub fn game_settings(&self, id: i64) -> GameSettings {
        self.lock().game_settings.data().get(&id).cloned().unwrap_or_default()
    }

    pub fn update_game_settings(self: &Arc<Self>, id: i64, f: impl FnOnce(&mut GameSettings)) {
        self.lock().game_settings.update(|d| f(d.entry(id).or_default()));
        self.emit_state();
    }

    pub fn set_window_state(self: &Arc<Self>, fullscreen: bool, zoom: f64) {
        let changed = {
            let mut i = self.lock();
            let c = i.fullscreen != fullscreen || (i.ui_zoom - zoom).abs() > 0.001;
            i.fullscreen = fullscreen;
            i.ui_zoom = zoom;
            c
        };
        if changed {
            self.emit_state();
        }
    }

    pub fn add_location(self: &Arc<Self>, p: String) {
        self.update_settings(|s| {
            if !s.locations.contains(&p) {
                s.locations.push(p);
            }
        });
    }

    pub fn remove_location(self: &Arc<Self>, p: &str) -> Result<(), ApiError> {
        let used = {
            let i = self.lock();
            let d = i.installed.data();
            d.games.values().chain(d.emulators.values()).any(|it| it.location == p)
        };
        if used {
            return Err(ApiError::new(0, "This location still contains installed games or emulators"));
        }
        self.update_settings(|s| {
            s.locations.retain(|l| l != p);
            if s.default_location.as_deref() == Some(p) {
                s.default_location = None;
            }
        });
        Ok(())
    }

    // ------------------------------------------------------------------ media (covers, artwork)
    /// `igdb/<name>` of a game: the file in the install folder, else the image from the in-memory
    /// cache. Missing images are downloaded and decoded in the background (the UI re-renders when
    /// they arrive).
    pub fn media(self: &Arc<Self>, game_id: i64, name: &str) -> Option<Media> {
        let (inst, server) = {
            let i = self.lock();
            (i.installed(ItemKind::Game, game_id), i.session.data().session.as_ref().map(|s| s.server_url.clone()))
        };
        if let Some(inst) = inst {
            let p = Path::new(&inst.path).join("igdb").join(name);
            if p.exists() {
                return Some(Media::File(p));
            }
        }
        let key: MediaKey = (server?, game_id, name.to_string());
        {
            let mut i = self.lock();
            if let Some(image) = i.media.get(&key, Instant::now()) {
                return Some(Media::Image(image));
            }
            if !i.online || i.media_failed.contains(&key) || !i.media_loading.insert(key.clone()) {
                return None;
            }
        }
        let me = self.clone();
        self.rt.spawn(async move {
            let ok: anyhow::Result<()> = async {
                let path = format!("/games/{game_id}/media/{}", crate::util_url::encode(&key.2));
                let url = me.api_url(&path, Some(&key.0))?;
                let res = me.send(&path, me.request(reqwest::Method::GET, &url).timeout(Duration::from_secs(30))).await?;
                if !res.status().is_success() {
                    anyhow::bail!("HTTP {}", res.status());
                }
                let bytes = res.bytes().await?;
                let (image, size) = tokio::task::spawn_blocking(move || decode_image(&bytes)).await??;
                me.lock().media.insert(key.clone(), Arc::new(image), size, Instant::now());
                Ok(())
            }
            .await;
            {
                let mut i = me.lock();
                i.media_loading.remove(&key);
                if ok.is_err() {
                    i.media_failed.insert(key);
                }
            }
            me.emit_state();
        });
        None
    }

    /// Images dropped from the media cache, for the UI to release their GPU textures.
    pub fn take_evicted_media(&self) -> Vec<Arc<gpui_kit::RenderImage>> {
        self.lock().media.take_evicted()
    }
}

/// Decodes an image for GPUI (BGRA), with its size in memory.
fn decode_image(bytes: &[u8]) -> anyhow::Result<(gpui_kit::RenderImage, usize)> {
    let mut data = image::load_from_memory(bytes)?.into_rgba8();
    for pixel in data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let size = data.as_raw().len();
    Ok((gpui_kit::RenderImage::new(vec![image::Frame::new(data)]), size))
}

fn root_cause(e: &reqwest::Error) -> String {
    let mut s: &dyn std::error::Error = e;
    while let Some(src) = s.source() {
        s = src;
    }
    s.to_string()
}
