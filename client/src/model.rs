//! Client data types. They are persisted as JSON in the user data folder with exactly the
//! field names of the Electron client, so both clients can share one data folder.
//! Unknown fields are kept (`extra`) so switching back and forth never loses data.

use nostalgia_shared::{EmulatorSummary, GameSummary, ItemKind, LibraryState, OsKey, PlaytimeEntry, RemoteFile, SaveVersion, User};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub server_url: String,
    pub token: String,
    pub user: User,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionFile {
    #[serde(default)]
    pub session: Option<Session>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledItem {
    pub kind: ItemKind,
    pub id: i64,
    pub name: String,
    pub platform: String,
    pub folder: String,
    pub location: String,
    pub path: String,
    #[serde(default)]
    pub files: Vec<RemoteFile>,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub installed_at: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Installed {
    #[serde(default)]
    pub games: BTreeMap<i64, InstalledItem>,
    #[serde(default)]
    pub emulators: BTreeMap<i64, InstalledItem>,
}

impl Installed {
    pub fn get(&self, kind: ItemKind, id: i64) -> Option<&InstalledItem> {
        match kind {
            ItemKind::Game => self.games.get(&id),
            ItemKind::Emulator => self.emulators.get(&id),
        }
    }
    pub fn map_mut(&mut self, kind: ItemKind) -> &mut BTreeMap<i64, InstalledItem> {
        match kind {
            ItemKind::Game => &mut self.games,
            ItemKind::Emulator => &mut self.emulators,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emulator_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_args: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientSettings {
    pub locations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_location: Option<String>,
    pub start_fullscreen: bool,
    /// UI scale factor, e.g. 1.5; 0 = automatic (by window size, 1920x1080 = 100%).
    pub ui_scale: f64,
    pub auto_sync_saves: bool,
    pub minimize_on_launch: bool,
    pub install_emulator_with_game: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ClientSettings {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let steam_deck = std::env::var("SteamDeck").as_deref() == Ok("1") || std::env::var("XDG_CURRENT_DESKTOP").as_deref() == Ok("gamescope");
        ClientSettings {
            locations: vec![home.join("Games").join("Nostalgia").to_string_lossy().into_owned()],
            default_location: None,
            start_fullscreen: steam_deck,
            ui_scale: 0.0,
            auto_sync_saves: true,
            minimize_on_launch: OsKey::current() == OsKey::Windows,
            install_emulator_with_game: true,
            extra: Map::new(),
        }
    }
}

impl ClientSettings {
    pub fn default_location(&self) -> Option<String> {
        self.default_location.clone().or_else(|| self.locations.first().cloned())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSyncState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_upload: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_result: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// user id -> emulator id -> state
pub type SaveSyncFile = BTreeMap<String, BTreeMap<i64, SaveSyncState>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Error,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    /// `${kind}:${id}`
    pub key: String,
    pub kind: ItemKind,
    pub id: i64,
    pub name: String,
    pub platform: String,
    pub folder: String,
    pub location: String,
    pub target_dir: String,
    pub files: Vec<RemoteFile>,
    pub total_bytes: u64,
    pub received_bytes: u64,
    pub status: DownloadStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// bytes/s
    #[serde(default)]
    pub speed: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,
    pub added_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadsFile {
    #[serde(default)]
    pub jobs: Vec<DownloadJob>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningGame {
    pub game_id: i64,
    pub emulator_id: i64,
    pub name: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSession {
    pub session_id: String,
    pub game_id: i64,
    pub started_at: String,
    pub seconds: i64,
    pub user_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSession {
    #[serde(flatten)]
    pub game: RunningGame,
    pub user_id: i64,
    pub heartbeat: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PendingFile {
    #[serde(default)]
    pub sessions: Vec<PendingSession>,
    #[serde(default)]
    pub active: Option<ActiveSession>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cache {
    #[serde(default)]
    pub games: Vec<GameSummary>,
    #[serde(default)]
    pub emulators: Vec<EmulatorSummary>,
    #[serde(default)]
    pub library: LibraryState,
    #[serde(default)]
    pub playtime: Vec<PlaytimeEntry>,
    #[serde(default)]
    pub latest_saves: Vec<SaveVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
}

pub type CacheFile = BTreeMap<String, Cache>;

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub emulators: Vec<LaunchEmulator>,
    pub emulator_id: Option<i64>,
    pub discs: Vec<(String, String)>,
    pub disc: Option<String>,
    pub extra_args: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LaunchEmulator {
    pub id: i64,
    pub name: String,
    pub installed: bool,
    pub supported: bool,
}

/// Everything the UI renders, rebuilt after each change (cheap to clone: Arc'd by the caller).
#[derive(Debug, Clone, Default)]
pub struct ClientState {
    pub ready: bool,
    pub session: Option<(String, User)>,
    pub online: bool,
    pub catalog_games: Vec<GameSummary>,
    pub catalog_emulators: Vec<EmulatorSummary>,
    pub library: LibraryState,
    pub installed: Installed,
    pub playtime: BTreeMap<i64, PlaytimeEntry>,
    pub settings: ClientSettings,
    pub running: Option<RunningGame>,
    /// emulator id -> state, busy, newest server version
    pub save_sync: BTreeMap<i64, (SaveSyncState, bool, Option<SaveVersion>)>,
    pub pending_sessions: usize,
    pub fullscreen: bool,
    pub ui_zoom: f64,
    pub os: Option<OsKey>,
    pub device: String,
    pub version: String,
}

impl ClientState {
    pub fn game(&self, id: i64) -> Option<&GameSummary> {
        self.catalog_games.iter().find(|g| g.id == id)
    }
    pub fn emulator(&self, id: i64) -> Option<&EmulatorSummary> {
        self.catalog_emulators.iter().find(|e| e.id == id)
    }
    pub fn installed(&self, kind: ItemKind, id: i64) -> Option<&InstalledItem> {
        self.installed.get(kind, id)
    }
    pub fn in_library(&self, kind: ItemKind, id: i64) -> bool {
        match kind {
            ItemKind::Game => self.library.games.contains(&id),
            ItemKind::Emulator => self.library.emulators.contains(&id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electron_files_round_trip() {
        // installed.json as written by the Electron client
        let text = r#"{
  "games": {
    "1": { "kind": "game", "id": 1, "name": "FF8", "platform": "psx", "folder": "FF8 [igdbid-425]", "location": "C:\\G", "path": "C:\\G\\games\\psx\\FF8 [igdbid-425]",
           "files": [{ "path": "a.cue", "size": 10, "mtime": 1700000000000 }], "sizeBytes": 10, "installedAt": "2026-11-08T20:04:09.123Z", "future": 1 }
  },
  "emulators": {}
}"#;
        let i: Installed = serde_json::from_str(text).unwrap();
        assert_eq!(i.games[&1].files[0].path, "a.cue");
        let back = serde_json::to_value(&i).unwrap();
        assert_eq!(back["games"]["1"]["future"], 1);
        let s: ClientSettings = serde_json::from_str(r#"{ "locations": ["D:\\Games"], "uiScale": 1.25 }"#).unwrap();
        assert!(s.auto_sync_saves);
        assert_eq!(s.ui_scale, 1.25);
        let sync: SaveSyncFile = serde_json::from_str(r#"{ "2": { "1": { "versionId": 5, "fingerprint": "empty" } } }"#).unwrap();
        assert_eq!(sync["2"][&1].version_id, Some(5));
    }
}
