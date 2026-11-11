//! Game / emulator library on disk and its database records (port of library.ts).

use crate::config::Settings;
use crate::db::{Db, now};
use crate::error::{HttpError, HttpResult};
use crate::log;
use crate::util::{mtime_ms, resolve_inside};
use nostalgia_shared::{
    EmuConfig, EmulatorSummary, GameSummary, ItemKind, PLATFORMS, RemoteFile, locale_compare, parse_emu_yaml, platform_name,
};
use regex::Regex;
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

pub static IGDB_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[igdbid-(\d+)\]").unwrap());
/// The metadata and artwork of a game are kept in `<metadataDirectory>/games/<igdb id>/`, never in
/// the library. Clients get them as `igdb/<name>` in the game's file listing. Older servers kept
/// them in an `igdb/` folder inside the game folder: that folder is ignored (not sent, not counted)
/// and its content is copied to the metadata folder once.
pub const ASSET_DIR: &str = "igdb";

#[derive(Debug, Clone)]
pub struct GameRow {
    pub id: i64,
    pub platform: String,
    pub folder: String,
    pub rel_path: String,
    pub igdb_id: i64,
    pub name: String,
    pub size_bytes: i64,
    pub file_count: i64,
    pub missing: bool,
    pub hidden: bool,
    pub metadata_json: Option<String>,
    pub metadata_status: String,
    pub metadata_error: Option<String>,
    pub added_at: String,
    pub updated_at: String,
}

impl GameRow {
    pub fn from_row(r: &Row) -> rusqlite::Result<GameRow> {
        Ok(GameRow {
            id: r.get("id")?,
            platform: r.get("platform")?,
            folder: r.get("folder")?,
            rel_path: r.get("rel_path")?,
            igdb_id: r.get("igdb_id")?,
            name: r.get("name")?,
            size_bytes: r.get("size_bytes")?,
            file_count: r.get("file_count")?,
            missing: r.get::<_, i64>("missing")? != 0,
            hidden: r.get::<_, i64>("hidden")? != 0,
            metadata_json: r.get("metadata_json")?,
            metadata_status: r.get("metadata_status")?,
            metadata_error: r.get("metadata_error")?,
            added_at: r.get("added_at")?,
            updated_at: r.get("updated_at")?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct EmulatorRow {
    pub id: i64,
    pub platform: String,
    pub folder: String,
    pub rel_path: String,
    pub name: String,
    pub size_bytes: i64,
    pub file_count: i64,
    pub config_json: Option<String>,
    pub config_error: Option<String>,
    pub missing: bool,
    pub added_at: String,
}

impl EmulatorRow {
    pub fn from_row(r: &Row) -> rusqlite::Result<EmulatorRow> {
        Ok(EmulatorRow {
            id: r.get("id")?,
            platform: r.get("platform")?,
            folder: r.get("folder")?,
            rel_path: r.get("rel_path")?,
            name: r.get("name")?,
            size_bytes: r.get("size_bytes")?,
            file_count: r.get("file_count")?,
            config_json: r.get("config_json")?,
            config_error: r.get("config_error")?,
            missing: r.get::<_, i64>("missing")? != 0,
            added_at: r.get("added_at")?,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsortedFile {
    pub platform: String,
    /// Relative to games/{platform}
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    /// Suggested game title (cleaned file name).
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub started_at: String,
    pub finished_at: String,
    pub games: i64,
    pub emulators: i64,
    pub new_games: i64,
    pub missing_games: i64,
    pub new_emulators: i64,
    pub unsorted: Vec<UnsortedFile>,
    pub errors: Vec<String>,
}

/// Recursively list files below `dir` (relative, forward slashes). Symlinks are followed only for files.
pub fn walk_files(dir: &Path) -> Vec<RemoteFile> {
    fn rec(abs: &Path, rel: &str, out: &mut Vec<RemoteFile>) {
        let Ok(entries) = std::fs::read_dir(abs) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let a = abs.join(&name);
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                rec(&a, &r, out);
            } else if let Ok(st) = std::fs::metadata(&a)
                && st.is_file()
            {
                out.push(RemoteFile { path: r, size: st.len(), mtime: mtime_ms(&st) });
            }
        }
    }
    let mut out = Vec::new();
    rec(dir, "", &mut out);
    out.sort_by(|x, y| locale_compare(&x.path, &y.path));
    out
}

/// The files of a game folder, without the `igdb/` folder of the old layout.
pub fn walk_game_files(dir: &Path) -> Vec<RemoteFile> {
    let prefix = format!("{ASSET_DIR}/");
    walk_files(dir).into_iter().filter(|f| !f.path.starts_with(&prefix)).collect()
}

pub fn total_size(files: &[RemoteFile]) -> u64 {
    files.iter().map(|f| f.size).sum()
}

static BAD_CHARS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*\x00-\x1f]"#).unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static TRAILING_DOTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[. ]+$").unwrap());

pub fn sanitize_folder_name(name: &str) -> String {
    let s = BAD_CHARS.replace_all(name, "");
    let s = SPACES.replace_all(&s, " ");
    let s = s.trim();
    let s = TRAILING_DOTS.replace(s, "");
    crate::util::js_slice(&s, 150)
}

pub fn game_name_from_folder(folder: &str) -> String {
    let s = IGDB_RE.replace(folder, "");
    let s = SPACES.replace_all(&s, " ");
    let s = s.trim();
    if s.is_empty() { folder.to_string() } else { s.to_string() }
}

static EXT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.[a-z0-9]{1,5}$").unwrap());
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[(\[][^)\]]*[)\]]").unwrap());
static UNDERSCORES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_+").unwrap());

/// Strip region / disc / revision tags from a file name to suggest a title.
pub fn suggest_title(file: &str) -> String {
    let base = Path::new(file).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| file.to_string());
    let s = EXT_RE.replace(&base, "");
    let s = TAG_RE.replace_all(&s, "");
    let s = UNDERSCORES.replace_all(&s, " ");
    SPACES.replace_all(&s, " ").trim().to_string()
}

pub fn parse_metadata(row: &GameRow) -> Option<Value> {
    row.metadata_json.as_deref().and_then(|t| serde_json::from_str::<Value>(t).ok())
}

fn str_list(m: Option<&Value>, k: &str) -> Vec<String> {
    m.and_then(|m| m.get(k)).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn non_empty_array(m: Option<&Value>, k: &str) -> bool {
    m.and_then(|m| m.get(k)).and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false)
}

pub fn to_game_summary(row: &GameRow) -> GameSummary {
    let meta = parse_metadata(row);
    let m = meta.as_ref().filter(|v| v.is_object());
    let get = |k: &str| m.and_then(|m| m.get(k)).filter(|v| !v.is_null());
    let name = get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| row.name.clone());
    GameSummary {
        id: row.id,
        kind: ItemKind::Game,
        name,
        platform: row.platform.clone(),
        platform_name: platform_name(&row.platform),
        folder: row.folder.clone(),
        igdb_id: row.igdb_id,
        size_bytes: row.size_bytes,
        file_count: row.file_count,
        has_cover: get("cover").map(|v| crate::util::truthy(Some(v))).unwrap_or(false),
        has_artwork: non_empty_array(m, "artworks") || non_empty_array(m, "screenshots"),
        year: get("year").and_then(|v| v.as_f64()).map(|f| f as i64),
        genres: str_list(m, "genres"),
        themes: str_list(m, "themes"),
        game_modes: str_list(m, "gameModes"),
        developers: str_list(m, "developers"),
        publishers: str_list(m, "publishers"),
        rating: get("rating").or_else(|| get("aggregatedRating")).and_then(|v| v.as_f64()),
        added_at: row.added_at.clone(),
    }
}

pub fn parse_emu_config_json(json: Option<&str>) -> Option<EmuConfig> {
    json.and_then(|t| serde_json::from_str(t).ok())
}

pub fn to_emulator_summary(row: &EmulatorRow) -> EmulatorSummary {
    let config = parse_emu_config_json(row.config_json.as_deref());
    let name = config.as_ref().and_then(|c| c.name.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| row.name.clone());
    EmulatorSummary {
        id: row.id,
        kind: ItemKind::Emulator,
        name,
        platform: row.platform.clone(),
        platform_name: platform_name(&row.platform),
        folder: row.folder.clone(),
        size_bytes: row.size_bytes,
        file_count: row.file_count,
        supports: config.as_ref().map(|c| c.systems.keys()).unwrap_or_default(),
        config,
        config_error: row.config_error.clone(),
        added_at: row.added_at.clone(),
    }
}

pub struct EmuRead {
    pub config: Option<EmuConfig>,
    pub error: Option<String>,
    pub file: Option<PathBuf>,
}

pub fn read_emu_config(dir: &Path) -> EmuRead {
    let file = ["emu.yaml", "emu.yml"].iter().map(|f| dir.join(f)).find(|f| f.exists());
    let Some(file) = file else { return EmuRead { config: None, error: Some("emu.yaml is missing".into()), file: None } };
    match std::fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|t| parse_emu_yaml(&t)) {
        Ok(c) => EmuRead { config: Some(c), error: None, file: Some(file) },
        Err(e) => EmuRead { config: None, error: Some(e), file: Some(file) },
    }
}

#[derive(Clone)]
pub struct Library {
    db: Db,
    settings: Arc<Settings>,
}

impl Library {
    pub fn new(db: Db, settings: Arc<Settings>) -> Library {
        Library { db, settings }
    }

    pub fn games_path(&self) -> &Path {
        &self.settings.games_path
    }
    pub fn emulators_path(&self) -> &Path {
        &self.settings.emulators_path
    }

    /// Create the platform folders (RomM naming) if they do not exist. Never deletes anything.
    pub fn ensure_folders(&self) -> std::io::Result<()> {
        for base in [self.games_path(), self.emulators_path()] {
            std::fs::create_dir_all(base)?;
            for p in PLATFORMS {
                std::fs::create_dir_all(base.join(p.slug))?;
            }
        }
        Ok(())
    }

    pub fn game_dir(&self, rel_path: &str) -> PathBuf {
        rel_path.split('/').fold(self.games_path().to_path_buf(), |p, s| p.join(s))
    }
    pub fn emulator_dir(&self, rel_path: &str) -> PathBuf {
        rel_path.split('/').fold(self.emulators_path().to_path_buf(), |p, s| p.join(s))
    }

    /// Metadata and artwork of an IGDB game (shared by the games with that id, e.g. on several platforms).
    pub fn metadata_dir(&self, igdb_id: i64) -> PathBuf {
        self.settings.metadata_path.join("games").join(igdb_id.to_string())
    }
    /// Scaled copies of the artwork (see `thumbs`).
    pub fn thumbs_dir(&self) -> PathBuf {
        self.settings.metadata_path.join("thumbs")
    }
    /// The metadata files of a game as its file listing shows them (`igdb/<name>`).
    pub fn metadata_files(&self, igdb_id: i64) -> Vec<RemoteFile> {
        walk_files(&self.metadata_dir(igdb_id))
            .into_iter()
            .filter(|f| !f.path.contains('/') && !f.path.ends_with(".tmp"))
            .map(|f| RemoteFile { path: format!("{ASSET_DIR}/{}", f.path), ..f })
            .collect()
    }

    pub fn get_game(&self, id: i64) -> Option<GameRow> {
        self.db.conn().query_row("SELECT * FROM games WHERE id = ?", [id], GameRow::from_row).optional().ok().flatten()
    }
    pub fn get_emulator(&self, id: i64) -> Option<EmulatorRow> {
        self.db.conn().query_row("SELECT * FROM emulators WHERE id = ?", [id], EmulatorRow::from_row).optional().ok().flatten()
    }
    fn games_where(&self, sql: &str) -> Vec<GameRow> {
        let conn = self.db.conn();
        let mut st = conn.prepare(sql).expect("sql");
        st.query_map([], GameRow::from_row).map(|r| r.flatten().collect()).unwrap_or_default()
    }
    pub fn list_games(&self, include_hidden: bool) -> Vec<GameRow> {
        self.games_where(&format!(
            "SELECT * FROM games WHERE missing = 0 {} ORDER BY name COLLATE NOCASE",
            if include_hidden { "" } else { "AND hidden = 0" }
        ))
    }
    pub fn list_all_games(&self) -> Vec<GameRow> {
        self.games_where("SELECT * FROM games ORDER BY name COLLATE NOCASE")
    }
    pub fn list_emulators(&self, include_missing: bool) -> Vec<EmulatorRow> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT * FROM emulators {} ORDER BY platform, name COLLATE NOCASE",
            if include_missing { "" } else { "WHERE missing = 0" }
        );
        let mut st = conn.prepare(&sql).expect("sql");
        st.query_map([], EmulatorRow::from_row).map(|r| r.flatten().collect()).unwrap_or_default()
    }

    /// Full scan of games and emulators (blocking, run it off the async runtime).
    pub fn scan(&self) -> anyhow::Result<ScanResult> {
        let started_at = now();
        let mut errors = Vec::new();
        self.ensure_folders()?;
        let mut unsorted = Vec::new();
        let mut seen_games = HashSet::new();
        let mut new_games = 0;
        let mut copied = 0;

        let platforms: Vec<String> = std::fs::read_dir(self.games_path())?
            .flatten()
            .filter(|d| d.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|d| d.file_name().to_string_lossy().into_owned())
            .collect();

        for platform in platforms {
            let pdir = self.games_path().join(&platform);
            let entries = match std::fs::read_dir(&pdir) {
                Ok(e) => e,
                Err(e) => {
                    errors.push(format!("{platform}: {e}"));
                    continue;
                }
            };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let abs = pdir.join(&name);
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let igdb = IGDB_RE.captures(&name).and_then(|m| m[1].parse::<i64>().ok());
                if let (true, Some(igdb_id)) = (is_dir, igdb) {
                    let rel = format!("{platform}/{name}");
                    seen_games.insert(rel.clone());
                    let files = walk_game_files(&abs);
                    let size = total_size(&files) as i64;
                    let count = files.len() as i64;
                    let conn = self.db.conn();
                    let existing = conn.query_row("SELECT * FROM games WHERE rel_path = ?", [&rel], GameRow::from_row).optional()?;
                    if let Some(existing) = existing {
                        conn.execute(
                            "UPDATE games SET size_bytes = ?, file_count = ?, missing = 0, igdb_id = ?, updated_at = ? WHERE id = ?",
                            params![size, count, igdb_id, now(), existing.id],
                        )?;
                        drop(conn);
                        if existing.igdb_id != igdb_id {
                            self.reset_metadata(existing.id);
                        }
                    } else {
                        // Folder renamed outside of Nostalgia? Re-link the missing record so user libraries survive.
                        let moved = conn
                            .query_row(
                                "SELECT * FROM games WHERE platform = ? AND igdb_id = ? AND missing = 1 ORDER BY id LIMIT 1",
                                params![platform, igdb_id],
                                GameRow::from_row,
                            )
                            .optional()?;
                        if let Some(moved) = moved {
                            conn.execute(
                                "UPDATE games SET rel_path = ?, folder = ?, size_bytes = ?, file_count = ?, missing = 0, updated_at = ? WHERE id = ?",
                                params![rel, name, size, count, now(), moved.id],
                            )?;
                        } else {
                            let t = now();
                            conn.execute(
                                "INSERT INTO games (platform, folder, rel_path, igdb_id, name, size_bytes, file_count, added_at, updated_at)
                                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                                params![platform, name, rel, igdb_id, game_name_from_folder(&name), size, count, t, t],
                            )?;
                            new_games += 1;
                        }
                        drop(conn);
                    }
                    copied += usize::from(self.load_local_metadata(&rel));
                } else {
                    // Loose file or folder without an igdb id: needs to be imported by an admin.
                    let size = if is_dir { total_size(&walk_files(&abs)) } else { std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0) };
                    unsorted.push(UnsortedFile { platform: platform.clone(), path: name.clone(), size, is_dir, suggestion: suggest_title(&name) });
                }
            }
        }

        if copied > 0 {
            log::info(format!(
                "Copied the metadata and artwork of {copied} games from their {ASSET_DIR}/ folders to {}. The {ASSET_DIR}/ folders in the library are no longer used and can be deleted.",
                self.settings.metadata_path.display()
            ));
        }

        // Mark vanished games as missing (never delete rows - user libraries reference them).
        let mut missing_games = 0;
        {
            let conn = self.db.conn();
            let rows: Vec<(i64, String)> = {
                let mut st = conn.prepare("SELECT id, rel_path FROM games WHERE missing = 0")?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.flatten().collect()
            };
            for (id, rel) in rows {
                if !seen_games.contains(&rel) {
                    conn.execute("UPDATE games SET missing = 1, updated_at = ? WHERE id = ?", params![now(), id])?;
                    missing_games += 1;
                }
            }
        }

        // Emulators
        let mut seen_emus = HashSet::new();
        let mut new_emulators = 0;
        for platform in std::fs::read_dir(self.emulators_path())?.flatten().filter(|d| d.file_type().map(|t| t.is_dir()).unwrap_or(false)) {
            let pname = platform.file_name().to_string_lossy().into_owned();
            let pdir = self.emulators_path().join(&pname);
            let Ok(entries) = std::fs::read_dir(&pdir) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) || name.starts_with('.') {
                    continue;
                }
                let rel = format!("{pname}/{name}");
                seen_emus.insert(rel.clone());
                let abs = pdir.join(&name);
                let files = walk_files(&abs);
                let size = total_size(&files) as i64;
                let read = read_emu_config(&abs);
                if let Some(err) = &read.error {
                    errors.push(format!("emulator {rel}: {err}"));
                }
                let config_json = read.config.as_ref().map(|c| serde_json::to_string(c).unwrap_or_default());
                let emu_name = read.config.as_ref().and_then(|c| c.name.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| name.clone());
                let conn = self.db.conn();
                let existing: Option<i64> = conn.query_row("SELECT id FROM emulators WHERE rel_path = ?", [&rel], |r| r.get(0)).optional()?;
                if let Some(id) = existing {
                    conn.execute(
                        "UPDATE emulators SET name = ?, size_bytes = ?, file_count = ?, config_json = ?, config_error = ?, missing = 0, updated_at = ? WHERE id = ?",
                        params![emu_name, size, files.len() as i64, config_json, read.error, now(), id],
                    )?;
                } else {
                    let t = now();
                    conn.execute(
                        "INSERT INTO emulators (platform, folder, rel_path, name, size_bytes, file_count, config_json, config_error, added_at, updated_at)
                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        params![pname, name, rel, emu_name, size, files.len() as i64, config_json, read.error, t, t],
                    )?;
                    new_emulators += 1;
                }
            }
        }
        {
            let conn = self.db.conn();
            let rows: Vec<(i64, String)> = {
                let mut st = conn.prepare("SELECT id, rel_path FROM emulators WHERE missing = 0")?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.flatten().collect()
            };
            for (id, rel) in rows {
                if !seen_emus.contains(&rel) {
                    conn.execute("UPDATE emulators SET missing = 1, updated_at = ? WHERE id = ?", params![now(), id])?;
                }
            }
        }

        Ok(ScanResult {
            started_at,
            finished_at: now(),
            games: self.db.count("SELECT COUNT(*) c FROM games WHERE missing = 0"),
            emulators: self.db.count("SELECT COUNT(*) c FROM emulators WHERE missing = 0"),
            new_games,
            missing_games,
            new_emulators,
            unsorted,
            errors,
        })
    }

    /// If the game's metadata.json exists in the metadata folder, load it into the DB (it is the
    /// source of truth). True when it was first copied from the old `igdb/` folder of the game.
    pub fn load_local_metadata(&self, rel: &str) -> bool {
        let row = self.db.conn().query_row("SELECT * FROM games WHERE rel_path = ?", [rel], GameRow::from_row).optional().ok().flatten();
        let Some(row) = row else { return false };
        let dir = self.metadata_dir(row.igdb_id);
        let file = dir.join("metadata.json");
        let copied = !file.exists() && self.copy_old_metadata(&row, &dir);
        if !file.exists() {
            if row.metadata_status == "ok" {
                self.reset_metadata(row.id);
            }
            return false;
        }
        let txt = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => {
                log::warn(format!("Invalid metadata.json of {rel}: {e}"));
                return copied;
            }
        };
        let meta: Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(e) => {
                log::warn(format!("Invalid metadata.json of {rel}: {e}"));
                return copied;
            }
        };
        if meta.get("igdbId").and_then(|v| v.as_f64()) != Some(row.igdb_id as f64) {
            self.reset_metadata(row.id);
            return copied;
        }
        if Some(&txt) != row.metadata_json.as_ref() || row.metadata_status != "ok" {
            let name = meta.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from).unwrap_or(row.name.clone());
            let _ = self.db.conn().execute(
                "UPDATE games SET metadata_json = ?, metadata_status = 'ok', metadata_error = NULL, name = ? WHERE id = ?",
                params![txt, name, row.id],
            );
        }
        copied
    }

    /// Copy the `igdb/` folder of the old layout (inside the game folder) to `dir`, when it
    /// belongs to the game's IGDB id. The old folder is only read.
    fn copy_old_metadata(&self, row: &GameRow, dir: &Path) -> bool {
        let old = self.game_dir(&row.rel_path).join(ASSET_DIR);
        let Ok(txt) = std::fs::read_to_string(old.join("metadata.json")) else { return false };
        let id = serde_json::from_str::<Value>(&txt).ok().and_then(|m| m.get("igdbId")?.as_f64());
        if id != Some(row.igdb_id as f64) {
            return false;
        }
        let copy = || -> std::io::Result<()> {
            std::fs::create_dir_all(dir)?;
            let mut names: Vec<String> = std::fs::read_dir(&old)?
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.ends_with(".tmp") && n != "metadata.json")
                .collect();
            // Last: an interrupted copy is made again on the next scan.
            names.push("metadata.json".into());
            for n in names {
                let tmp = dir.join(format!("{n}.copy.tmp"));
                std::fs::copy(old.join(&n), &tmp)?;
                std::fs::rename(&tmp, dir.join(&n))?;
            }
            Ok(())
        };
        copy().map_err(|e| log::warn(format!("Could not copy {} to {}: {e}", old.display(), dir.display()))).is_ok()
    }

    pub fn reset_metadata(&self, id: i64) {
        let _ = self
            .db
            .conn()
            .execute("UPDATE games SET metadata_json = NULL, metadata_status = 'pending', metadata_error = NULL WHERE id = ?", [id]);
    }

    /// Import loose files/folders from games/{platform}/ into a proper game folder
    /// "{Name} [igdbid-{id}]". Files are moved (renamed), never deleted.
    pub fn import_entries(&self, platform: &str, entries: &[String], igdb_id: f64, name: &str) -> HttpResult<String> {
        if entries.is_empty() {
            return Err(HttpError::new(400, "Nothing to import"));
        }
        if !(igdb_id.fract() == 0.0 && igdb_id > 0.0) {
            return Err(HttpError::new(400, "Invalid IGDB id"));
        }
        let igdb_id = igdb_id as i64;
        let pdir = resolve_inside(self.games_path(), platform)?;
        let folder = format!("{} [igdbid-{igdb_id}]", sanitize_folder_name(name));
        let target = resolve_inside(&pdir, &folder)?;
        std::fs::create_dir_all(&target)?;
        for entry in entries {
            let src = resolve_inside(&pdir, entry)?;
            if src.parent() != Some(pdir.as_path()) {
                return Err(HttpError::new(400, format!("Not a top-level entry: {entry}")));
            }
            if !src.exists() {
                return Err(HttpError::not_found(format!("Not found: {entry}")));
            }
            if src.is_dir() {
                // Move the folder's content into the game folder.
                for child in std::fs::read_dir(&src)?.flatten() {
                    let cname = child.file_name();
                    let dst = target.join(&cname);
                    if dst.exists() {
                        return Err(HttpError::new(409, format!("Target already exists: {folder}/{}", cname.to_string_lossy())));
                    }
                    std::fs::rename(child.path(), dst)?;
                }
                // Remove the source folder only if it is now completely empty (remove_dir refuses otherwise).
                let _ = std::fs::remove_dir(&src);
            } else {
                let base = src.file_name().unwrap_or_default().to_os_string();
                let dst = target.join(&base);
                if dst.exists() {
                    return Err(HttpError::new(409, format!("Target already exists: {folder}/{}", base.to_string_lossy())));
                }
                std::fs::rename(&src, dst)?;
            }
        }
        log::info(format!(
            "Imported {} entr{} into {platform}/{folder}",
            entries.len(),
            if entries.len() == 1 { "y" } else { "ies" }
        ));
        Ok(format!("{platform}/{folder}"))
    }

    /// Change the IGDB id of a game by renaming its folder.
    pub fn rematch_game(&self, id: i64, igdb_id: f64, name: Option<&str>) -> HttpResult<GameRow> {
        let row = self.get_game(id).ok_or_else(|| HttpError::not_found("Game not found"))?;
        if !(igdb_id.fract() == 0.0 && igdb_id > 0.0) {
            return Err(HttpError::new(400, "Invalid IGDB id"));
        }
        let igdb_id = igdb_id as i64;
        let title = sanitize_folder_name(&match name.filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => game_name_from_folder(&row.folder),
        });
        let folder = format!("{title} [igdbid-{igdb_id}]");
        let src = self.game_dir(&row.rel_path);
        let dst = resolve_inside(&self.games_path().join(&row.platform), &folder)?;
        if src != dst {
            if dst.exists() {
                return Err(HttpError::new(409, format!("Folder already exists: {folder}")));
            }
            std::fs::rename(&src, &dst)?;
        }
        // The metadata of the new id is loaded from the metadata folder or downloaded.
        self.db.conn().execute(
            "UPDATE games SET folder = ?, rel_path = ?, igdb_id = ?, name = ?, metadata_status = 'pending', metadata_json = NULL, updated_at = ? WHERE id = ?",
            params![folder, format!("{}/{folder}", row.platform), igdb_id, title, now(), id],
        )?;
        self.get_game(id).ok_or_else(|| HttpError::not_found("Game not found"))
    }

    pub fn set_hidden(&self, id: i64, hidden: bool) -> HttpResult<()> {
        self.db.conn().execute("UPDATE games SET hidden = ? WHERE id = ?", params![hidden as i64, id])?;
        Ok(())
    }

    /// Rewrite an emulator's emu.yaml after validating it. The previous file is kept in the data directory.
    pub fn write_emu_yaml(&self, id: i64, text: &str) -> HttpResult<()> {
        let row = self.get_emulator(id).ok_or_else(|| HttpError::not_found("Emulator not found"))?;
        let config = parse_emu_yaml(text).map_err(|e| HttpError::new(400, e))?;
        let dir = self.emulator_dir(&row.rel_path);
        let file = dir.join("emu.yaml");
        if file.exists() {
            // Keep the previous version outside of the emulator folder so it is not sent to clients.
            let backup_dir = self.settings.database_path.join("emu-yaml-history").join(&row.platform).join(&row.folder);
            std::fs::create_dir_all(&backup_dir)?;
            std::fs::copy(&file, backup_dir.join(format!("emu.yaml.{}", crate::util::now_ms())))?;
        }
        std::fs::write(&file, text)?;
        let name = config.name.clone().filter(|s| !s.is_empty()).unwrap_or(row.folder.clone());
        self.db.conn().execute(
            "UPDATE emulators SET config_json = ?, config_error = NULL, name = ?, updated_at = ? WHERE id = ?",
            params![serde_json::to_string(&config).unwrap_or_default(), name, now(), id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_kept_out_of_the_library_and_copied_from_old_game_folders() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.yaml");
        std::fs::write(&file, "libraryDirectory: ./lib\nmetadataDirectory: ./meta\n").unwrap();
        let settings = Arc::new(crate::config::load_settings(&file).unwrap());
        let lib = Library::new(Db::open(&settings.database_path).unwrap(), settings.clone());
        let game = settings.games_path.join("gb").join("Dropzone [igdbid-1186]");
        std::fs::create_dir_all(game.join("igdb")).unwrap();
        std::fs::write(game.join("Dropzone.gb"), [0u8; 10]).unwrap();
        // Old layout: metadata and artwork inside the game folder.
        std::fs::write(game.join("igdb").join("metadata.json"), r#"{"igdbId":1186,"name":"Dropzone","cover":"cover.jpg"}"#).unwrap();
        std::fs::write(game.join("igdb").join("cover.jpg"), [1u8; 5]).unwrap();

        lib.scan().unwrap();
        let row = lib.list_games(true).pop().unwrap();
        assert_eq!((row.size_bytes, row.file_count), (10, 1), "the igdb/ folder is not part of the game");
        assert_eq!(row.metadata_status, "ok");
        let meta = settings.metadata_path.join("games").join("1186");
        assert!(meta.join("cover.jpg").is_file() && meta.join("metadata.json").is_file());
        assert!(game.join("igdb").join("cover.jpg").is_file(), "the old folder is left as it is");
        let listed: Vec<String> = lib.metadata_files(1186).into_iter().map(|f| f.path).collect();
        assert_eq!(listed, ["igdb/cover.jpg", "igdb/metadata.json"]);
    }

    #[test]
    fn names_from_folders_and_files() {
        assert_eq!(game_name_from_folder("Final Fantasy VIII [igdbid-425]"), "Final Fantasy VIII");
        assert_eq!(sanitize_folder_name("Loose: Game?"), "Loose Game");
        assert_eq!(sanitize_folder_name("Name. . "), "Name");
        assert_eq!(suggest_title("Final_Fantasy (USA) (Disc 1).bin"), "Final Fantasy");
    }
}
