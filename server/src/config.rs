//! settings.yaml: same keys, defaults, environment overrides and template as the Node.js server.

use crate::log;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Settings {
    pub admin_username: String,
    pub admin_password: String,
    pub library_directory: String,
    pub igdb_client_id: String,
    pub igdb_client_secret: String,
    pub port: u16,
    pub host: String,
    /// SQLite database and other server data.
    pub database_directory: String,
    /// Versioned cloud saves. Separate from the database.
    pub saves_directory: String,
    /// IGDB metadata and artwork of the games. Separate from the library.
    pub metadata_directory: String,
    pub allow_registration: bool,
    pub scan_interval_minutes: f64,
    /// Check file hashes against hasheous.org during scheduled scans.
    pub hasheous_on_scheduled_scan: bool,

    pub file: PathBuf,
    pub library_path: PathBuf,
    pub database_path: PathBuf,
    pub saves_path: PathBuf,
    pub metadata_path: PathBuf,
    /// "<database>/saves" of servers from before the split; moved to saves_path on start.
    pub legacy_saves_path: PathBuf,
    pub games_path: PathBuf,
    pub emulators_path: PathBuf,
}

pub const TEMPLATE: &str = r#"# Nostalgia server settings
# Paths are relative to this file.

# Credentials for the admin web interface (http://<server>:<port>/admin)
adminUsername: admin
adminPassword: change-me

# Root of the game library. Contains "games/{platform}/{Name} [igdbid-123]/" and
# "emulators/{platform}/{Emulator name}/" (with an emu.yaml).
libraryDirectory: ./library

# Twitch / IGDB API credentials (https://api-docs.igdb.com/#account-creation)
igdbClientId: ""
igdbClientSecret: ""

# --- optional ---
port: 8080
host: 0.0.0.0
# SQLite database (users, library, play time). Keep it on a local disk.
databaseDirectory: ./database
# Versioned cloud saves, one zip per upload. Never deleted.
savesDirectory: ./saves
# IGDB metadata, covers, artworks and screenshots of the games (and scaled copies for the
# clients). Kept out of the game library; nothing is written to the library folders.
metadataDirectory: ./metadata
allowRegistration: true
scanIntervalMinutes: 30
hasheousOnScheduledScan: false
"#;

/// `--settings <file>`, else `NOSTALGIA_SETTINGS`, else `./settings.yaml`.
pub fn resolve_settings_file(args: &[String]) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "--settings")
        && let Some(f) = args.get(i + 1)
    {
        return absolute(Path::new(f));
    }
    if let Ok(f) = std::env::var("NOSTALGIA_SETTINGS")
        && !f.is_empty()
    {
        return absolute(Path::new(&f));
    }
    absolute(Path::new("settings.yaml"))
}

fn absolute(p: &Path) -> PathBuf {
    let joined = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
    crate::util::normalize(&joined)
}

/// JavaScript `String(v)` for YAML scalars.
fn yaml_str(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Tagged(t) => yaml_str(&t.value),
        _ => String::new(),
    }
}

/// JavaScript `Number(v)` for YAML scalars.
fn yaml_num(v: &Value) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}

fn yaml_bool(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

pub fn load_settings(file: &Path) -> anyhow::Result<Settings> {
    if !file.exists() {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(file, TEMPLATE)?;
        log::warn(format!("Created default settings file at {} - change the admin password!", file.display()));
    }
    let text = std::fs::read_to_string(file)?;
    let doc: Value = if text.trim().is_empty() { Value::Null } else { serde_yaml::from_str(&text)? };
    let raw = match doc {
        Value::Mapping(m) => m,
        _ => serde_yaml::Mapping::new(),
    };
    // `undefined` and `null` both fall back to the default only when the key is missing.
    let get = |k: &str| raw.get(Value::String(k.into()));

    let mut s = Settings {
        admin_username: get("adminUsername").map(yaml_str).unwrap_or_else(|| "admin".into()),
        admin_password: get("adminPassword").map(yaml_str).unwrap_or_else(|| "change-me".into()),
        library_directory: get("libraryDirectory").map(yaml_str).unwrap_or_else(|| "./library".into()),
        igdb_client_id: get("igdbClientId").map(|v| if v.is_null() { String::new() } else { yaml_str(v) }).unwrap_or_default(),
        igdb_client_secret: get("igdbClientSecret").map(|v| if v.is_null() { String::new() } else { yaml_str(v) }).unwrap_or_default(),
        port: 8080,
        host: get("host").map(yaml_str).unwrap_or_else(|| "0.0.0.0".into()),
        // Older settings files: "dataDirectory" held the database and the saves.
        database_directory: get("databaseDirectory").or_else(|| get("dataDirectory")).map(yaml_str).unwrap_or_else(|| "./database".into()),
        saves_directory: get("savesDirectory").map(yaml_str).unwrap_or_else(|| "./saves".into()),
        metadata_directory: get("metadataDirectory").map(yaml_str).unwrap_or_else(|| "./metadata".into()),
        allow_registration: get("allowRegistration").map(yaml_bool).unwrap_or(true),
        scan_interval_minutes: 30.0,
        hasheous_on_scheduled_scan: get("hasheousOnScheduledScan").map(yaml_bool).unwrap_or(false),
        file: file.to_path_buf(),
        library_path: PathBuf::new(),
        database_path: PathBuf::new(),
        saves_path: PathBuf::new(),
        metadata_path: PathBuf::new(),
        legacy_saves_path: PathBuf::new(),
        games_path: PathBuf::new(),
        emulators_path: PathBuf::new(),
    };
    let mut port = get("port").map(yaml_num).unwrap_or(8080.0);
    let mut interval = get("scanIntervalMinutes").map(yaml_num).unwrap_or(30.0);

    // Environment overrides are handy for containers.
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    if let Some(p) = env("NOSTALGIA_PORT") {
        port = p.trim().parse().unwrap_or(f64::NAN);
    }
    if let Some(v) = env("NOSTALGIA_LIBRARY") {
        s.library_directory = v;
    }
    if let Some(v) = env("NOSTALGIA_DATABASE").or_else(|| env("NOSTALGIA_DATA")) {
        s.database_directory = v;
    }
    if let Some(v) = env("NOSTALGIA_SAVES") {
        s.saves_directory = v;
    }
    if let Some(v) = env("NOSTALGIA_METADATA") {
        s.metadata_directory = v;
    }
    if let Some(v) = env("IGDB_CLIENT_ID") {
        s.igdb_client_id = v;
    }
    if let Some(v) = env("IGDB_CLIENT_SECRET") {
        s.igdb_client_secret = v;
    }
    s.port = if port.is_finite() && port > 0.0 && port <= 65535.0 { port as u16 } else { 8080 };
    if !interval.is_finite() {
        interval = 0.0;
    }
    s.scan_interval_minutes = interval;
    s.igdb_client_id = s.igdb_client_id.trim().to_string();
    s.igdb_client_secret = s.igdb_client_secret.trim().to_string();

    let base = file.parent().unwrap_or(Path::new("."));
    let or = |v: &str, d: &str| if v.is_empty() { d.to_string() } else { v.to_string() };
    s.library_path = crate::util::resolve(base, &s.library_directory);
    s.database_path = crate::util::resolve(base, &or(&s.database_directory, "./database"));
    s.saves_path = crate::util::resolve(base, &or(&s.saves_directory, "./saves"));
    s.metadata_path = crate::util::resolve(base, &or(&s.metadata_directory, "./metadata"));
    s.legacy_saves_path = s.database_path.join("saves");
    s.games_path = s.library_path.join("games");
    s.emulators_path = s.library_path.join("emulators");
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.yaml");
        std::fs::write(&file, body).unwrap();
        (dir, file)
    }

    #[test]
    fn database_and_cloud_saves_are_separate_folders() {
        let (dir, file) = write("");
        let s = load_settings(&file).unwrap();
        assert_eq!(s.database_path, dir.path().join("database"));
        assert_eq!(s.saves_path, dir.path().join("saves"));
        let (dir, file) = write("databaseDirectory: ./db\nsavesDirectory: ./elsewhere/saves\n");
        let c = load_settings(&file).unwrap();
        assert_eq!(c.database_path, dir.path().join("db"));
        assert_eq!(c.saves_path, dir.path().join("elsewhere").join("saves"));
    }

    #[test]
    fn metadata_has_its_own_folder() {
        let (dir, file) = write("libraryDirectory: ./lib
");
        assert_eq!(load_settings(&file).unwrap().metadata_path, dir.path().join("metadata"));
        let (dir, file) = write("libraryDirectory: ./lib
metadataDirectory: ./art
");
        assert_eq!(load_settings(&file).unwrap().metadata_path, dir.path().join("art"));
    }

    #[test]
    fn old_settings_files_data_directory_is_the_database_folder() {
        let (dir, file) = write("dataDirectory: ./data\n");
        let s = load_settings(&file).unwrap();
        assert_eq!(s.database_path, dir.path().join("data"));
        assert_eq!(s.saves_path, dir.path().join("saves"));
        assert_eq!(s.legacy_saves_path, dir.path().join("data").join("saves"));
    }

    #[test]
    fn missing_file_is_created_from_the_template() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join("settings.yaml");
        let s = load_settings(&file).unwrap();
        assert!(file.exists());
        assert_eq!(s.admin_password, "change-me");
        assert_eq!(s.port, 8080);
        assert!(s.allow_registration);
    }
}
