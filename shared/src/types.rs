//! DTOs shared between server and client.
//!
//! Field names, field order and "missing vs null" follow the TypeScript implementation exactly,
//! so the Rust and the Node.js server / client can be mixed freely.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Game,
    Emulator,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Game => "game",
            ItemKind::Emulator => "emulator",
        }
    }
    /// Path segment used by the API (`games` / `emulators`).
    pub fn plural(self) -> &'static str {
        match self {
            ItemKind::Game => "games",
            ItemKind::Emulator => "emulators",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub username: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: User,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub youtube_id: String,
}

/// Normalised IGDB metadata stored as `igdb/metadata.json` inside each game folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameMetadata {
    pub igdb_id: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storyline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregated_rating: Option<i64>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub themes: Vec<String>,
    #[serde(default)]
    pub game_modes: Vec<String>,
    #[serde(default)]
    pub perspectives: Vec<String>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub franchises: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub videos: Vec<Video>,
    /// File names relative to the `igdb` folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default)]
    pub artworks: Vec<String>,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub fetched_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    pub id: i64,
    pub kind: ItemKind,
    pub name: String,
    pub platform: String,
    pub platform_name: String,
    pub folder: String,
    pub igdb_id: i64,
    pub size_bytes: i64,
    pub file_count: i64,
    pub has_cover: bool,
    pub has_artwork: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub themes: Vec<String>,
    #[serde(default)]
    pub game_modes: Vec<String>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", serialize_with = "js_number")]
    pub rating: Option<f64>,
    pub added_at: String,
}

/// Whole numbers are written without ".0", like JavaScript does.
fn js_number<S: serde::Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => s.serialize_i64(*x as i64),
        Some(x) => s.serialize_f64(*x),
        None => s.serialize_none(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDetail {
    #[serde(flatten)]
    pub summary: GameSummary,
    /// Kept as raw JSON so unknown fields pass through unchanged.
    pub metadata: Option<Value>,
}

impl GameDetail {
    pub fn metadata_typed(&self) -> Option<GameMetadata> {
        self.metadata.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Arguments {
    Line(String),
    List(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorSystemConfig {
    pub executable: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    /// Folders (one or more) holding the emulator's save data on the device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saves: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_folder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OsKey {
    Windows,
    Linux,
}

impl OsKey {
    pub fn current() -> OsKey {
        if cfg!(windows) { OsKey::Windows } else { OsKey::Linux }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            OsKey::Windows => "windows",
            OsKey::Linux => "linux",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Systems {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<EmulatorSystemConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<EmulatorSystemConfig>,
}

impl Systems {
    pub fn get(&self, os: OsKey) -> Option<&EmulatorSystemConfig> {
        match os {
            OsKey::Windows => self.windows.as_ref(),
            OsKey::Linux => self.linux.as_ref(),
        }
    }
    pub fn keys(&self) -> Vec<OsKey> {
        let mut v = Vec::new();
        if self.windows.is_some() {
            v.push(OsKey::Windows);
        }
        if self.linux.is_some() {
            v.push(OsKey::Linux);
        }
        v
    }
}

/// Parsed `emu.yaml`. `systems` comes first, like the object built by the TypeScript validator.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmuConfig {
    #[serde(default)]
    pub systems: Systems,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Launchable extensions in priority order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorSummary {
    pub id: i64,
    pub kind: ItemKind,
    pub name: String,
    pub platform: String,
    pub platform_name: String,
    pub folder: String,
    pub size_bytes: i64,
    pub file_count: i64,
    pub config: Option<EmuConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
    /// OSes this emulator has a configuration for.
    #[serde(default)]
    pub supports: Vec<OsKey>,
    pub added_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteFile {
    /// Relative, forward slashes.
    pub path: String,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileListing {
    pub id: i64,
    pub folder: String,
    pub files: Vec<RemoteFile>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryState {
    #[serde(default)]
    pub games: Vec<i64>,
    #[serde(default)]
    pub emulators: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveVersion {
    pub id: i64,
    pub emulator_id: i64,
    pub version: i64,
    pub size: i64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaytimeEntry {
    pub game_id: i64,
    pub seconds: i64,
    pub sessions: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaySession {
    pub session_id: String,
    pub game_id: i64,
    pub started_at: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerEvent {
    #[serde(rename_all = "camelCase")]
    Hello { server_time: String },
    #[serde(rename_all = "camelCase")]
    Save {
        emulator_id: i64,
        version: SaveVersion,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_device: Option<String>,
    },
    Library { library: LibraryState },
    Catalog,
}

impl ServerEvent {
    pub fn name(&self) -> &'static str {
        match self {
            ServerEvent::Hello { .. } => "hello",
            ServerEvent::Save { .. } => "save",
            ServerEvent::Library { .. } => "library",
            ServerEvent::Catalog => "catalog",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_serialise_like_typescript() {
        let g = GameSummary {
            id: 1,
            kind: ItemKind::Game,
            name: "A".into(),
            platform: "psx".into(),
            platform_name: "PlayStation".into(),
            folder: "A [igdbid-1]".into(),
            igdb_id: 1,
            size_bytes: 3,
            file_count: 1,
            has_cover: false,
            has_artwork: false,
            year: None,
            genres: vec![],
            themes: vec![],
            game_modes: vec![],
            developers: vec![],
            publishers: vec![],
            rating: Some(86.0),
            added_at: "x".into(),
        };
        let s = serde_json::to_string(&g).unwrap();
        assert!(s.starts_with(r#"{"id":1,"kind":"game","name":"A""#));
        assert!(s.contains(r#""rating":86,"#));
        assert!(!s.contains("year"));
        let d = GameDetail { summary: g, metadata: None };
        assert!(serde_json::to_string(&d).unwrap().ends_with(r#""addedAt":"x","metadata":null}"#));
        let ev = ServerEvent::Save {
            emulator_id: 2,
            version: SaveVersion { id: 1, emulator_id: 2, version: 1, size: 1, sha256: "s".into(), device: None, note: None, created_at: "t".into() },
            from_device: None,
        };
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            r#"{"type":"save","emulatorId":2,"version":{"id":1,"emulatorId":2,"version":1,"size":1,"sha256":"s","createdAt":"t"}}"#
        );
        assert_eq!(serde_json::to_string(&ServerEvent::Catalog).unwrap(), r#"{"type":"catalog"}"#);
    }
}
