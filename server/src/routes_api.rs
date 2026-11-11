//! `/api`: the client protocol (port of routes/api.ts).

use crate::db::now;
use crate::error::{HttpError, HttpResult};
use crate::http::{AppState, AuthUser, JsonBody, attachment, device, send_file};
use crate::library::{ASSET_DIR, parse_metadata, to_emulator_summary, to_game_summary, total_size, walk_files, walk_game_files};
use crate::saves::StoreMeta;
use crate::util::{decode_uri_component, id_param, js_num, js_str, resolve_inside};
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::Stream;
use nostalgia_shared::{API_VERSION, FileListing, GameDetail, LibraryState, PLATFORMS, PlaytimeEntry, ServerEvent};
use regex::Regex;
use rusqlite::params;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

pub static MEDIA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[A-Za-z0-9_.-]+\.(jpg|jpeg|png|webp)$").unwrap());

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
        .route("/me/password", post(change_password))
        .route("/platforms", get(platforms))
        .route("/games", get(games))
        .route("/games/{id}", get(game))
        .route("/games/{id}/files", get(game_files))
        .route("/games/{id}/download", get(game_download))
        .route("/games/{id}/media/{name}", get(game_media))
        .route("/emulators", get(emulators))
        .route("/emulators/{id}", get(emulator))
        .route("/emulators/{id}/files", get(emulator_files))
        .route("/emulators/{id}/download", get(emulator_download))
        .route("/library", get(library))
        .route("/library/games/{id}", post(add_game).delete(remove_game))
        .route("/library/emulators/{id}", post(add_emulator).delete(remove_emulator))
        .route("/saves", get(latest_saves))
        .route("/saves/{emulatorId}", get(list_saves).post(upload_save))
        .route("/saves/version/{id}/download", get(download_save))
        .route("/playtime", get(playtime).post(add_playtime))
        .route("/events", get(events))
}

async fn health(State(s): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "name": "nostalgia",
        "version": env!("CARGO_PKG_VERSION"),
        "apiVersion": API_VERSION,
        "time": now(),
        "registration": s.settings.allow_registration
    }))
}

// ---------- auth ----------
async fn register(State(s): State<AppState>, headers: HeaderMap, body: JsonBody) -> HttpResult<Response> {
    if !s.settings.allow_registration {
        return Err(HttpError::new(403, "Registration is disabled on this server"));
    }
    let u = s.auth.register(&js_str(body.get("username")), &js_str(body.get("password")))?;
    let token = s.auth.create_session(u.id, device(&headers).as_deref())?;
    Ok((StatusCode::CREATED, Json(json!({ "token": token, "user": u }))).into_response())
}

async fn login(State(s): State<AppState>, headers: HeaderMap, body: JsonBody) -> HttpResult<Json<Value>> {
    let u = s.auth.login(&js_str(body.get("username")), &js_str(body.get("password")))?;
    let token = s.auth.create_session(u.id, device(&headers).as_deref())?;
    Ok(Json(json!({ "token": token, "user": u })))
}

async fn logout(State(s): State<AppState>, a: AuthUser) -> HttpResult<Json<Value>> {
    s.auth.revoke_session(&a.token)?;
    Ok(Json(json!({ "ok": true })))
}

async fn me(a: AuthUser) -> Json<Value> {
    Json(serde_json::to_value(a.user).unwrap_or_default())
}

async fn change_password(State(s): State<AppState>, a: AuthUser, body: JsonBody) -> HttpResult<Json<Value>> {
    if !s.auth.check_password(a.user.id, &js_str(body.get("oldPassword"))) {
        return Err(HttpError::new(403, "Current password is wrong"));
    }
    s.auth.set_password(a.user.id, &js_str(body.get("newPassword")))?;
    s.auth.revoke_all_for_user(a.user.id, Some(&a.token))?; // sign out other devices
    Ok(Json(json!({ "ok": true })))
}

// ---------- catalog ----------
async fn platforms(_a: AuthUser) -> Json<Value> {
    Json(serde_json::to_value(PLATFORMS).unwrap_or_default())
}

async fn games(State(s): State<AppState>, _a: AuthUser) -> Json<Value> {
    Json(serde_json::to_value(s.library.list_games(false).iter().map(to_game_summary).collect::<Vec<_>>()).unwrap_or_default())
}

fn visible_game(s: &AppState, raw: &str) -> HttpResult<crate::library::GameRow> {
    let row = s.library.get_game(id_param(raw, "id")?);
    row.filter(|r| !r.missing && !r.hidden).ok_or_else(|| HttpError::not_found("Game not found"))
}

fn present_emulator(s: &AppState, raw: &str) -> HttpResult<crate::library::EmulatorRow> {
    s.library.get_emulator(id_param(raw, "id")?).filter(|r| !r.missing).ok_or_else(|| HttpError::not_found("Emulator not found"))
}

async fn game(State(s): State<AppState>, _a: AuthUser, Path(id): Path<String>) -> HttpResult<Json<GameDetail>> {
    let row = visible_game(&s, &id)?;
    Ok(Json(GameDetail { summary: to_game_summary(&row), metadata: parse_metadata(&row) }))
}

async fn game_files(State(s): State<AppState>, _a: AuthUser, Path(id): Path<String>) -> HttpResult<Json<FileListing>> {
    let row = visible_game(&s, &id)?;
    // The game's files and its metadata and artwork (`igdb/<name>`, from the metadata folder).
    let mut files = walk_game_files(&s.library.game_dir(&row.rel_path));
    files.extend(s.library.metadata_files(row.igdb_id));
    Ok(Json(FileListing { id: row.id, folder: row.folder, total_bytes: total_size(&files), files }))
}

async fn send_inside(base: &std::path::Path, rel: &str, headers: &HeaderMap, download: bool) -> HttpResult<Response> {
    let abs = resolve_inside(base, rel)?;
    if !abs.is_file() {
        return Err(HttpError::not_found("File not found"));
    }
    let mut extra = Vec::new();
    if download {
        extra.push(attachment(&abs.file_name().unwrap_or_default().to_string_lossy()));
    }
    send_file(&abs, headers, &extra).await
}

async fn game_download(
    State(s): State<AppState>,
    _a: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let row = visible_game(&s, &id)?;
    let path = q.get("path").map(|p| p.replace('\\', "/")).unwrap_or_default();
    match path.strip_prefix(&format!("{ASSET_DIR}/")) {
        Some(name) => send_inside(&s.library.metadata_dir(row.igdb_id), name, &headers, true).await,
        None => send_inside(&s.library.game_dir(&row.rel_path), &path, &headers, true).await,
    }
}

/// `?w=<pixels>`: scaled down to that width (see `thumbs`).
async fn game_media(
    State(s): State<AppState>,
    _a: AuthUser,
    Path((id, name)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let row = s.library.get_game(id_param(&id, "id")?).ok_or_else(|| HttpError::not_found("Game not found"))?;
    if !MEDIA_RE.is_match(&name) {
        return Err(HttpError::new(400, "Invalid media name"));
    }
    let dir = s.library.metadata_dir(row.igdb_id);
    let scaled = match crate::thumbs::width_param(q.get("w")) {
        Some(w) if dir.join(&name).is_file() => {
            crate::thumbs::scaled(&dir.join(&name), &s.library.thumbs_dir(), row.igdb_id, &name, w).await.unwrap_or_else(|e| {
                crate::log::warn(format!("Could not scale {name} of game {}: {e}", row.id));
                None
            })
        }
        _ => None,
    };
    let mut res = match scaled {
        Some(p) => send_file(&p, &headers, &[]).await?,
        None => send_inside(&dir, &name, &headers, false).await?,
    };
    res.headers_mut().insert(header::CACHE_CONTROL, "private, max-age=86400".parse().unwrap());
    Ok(res)
}

async fn emulators(State(s): State<AppState>, _a: AuthUser) -> Json<Value> {
    Json(serde_json::to_value(s.library.list_emulators(false).iter().map(to_emulator_summary).collect::<Vec<_>>()).unwrap_or_default())
}

async fn emulator(State(s): State<AppState>, _a: AuthUser, Path(id): Path<String>) -> HttpResult<Json<Value>> {
    Ok(Json(serde_json::to_value(to_emulator_summary(&present_emulator(&s, &id)?)).unwrap_or_default()))
}

async fn emulator_files(State(s): State<AppState>, _a: AuthUser, Path(id): Path<String>) -> HttpResult<Json<FileListing>> {
    let row = present_emulator(&s, &id)?;
    let files = walk_files(&s.library.emulator_dir(&row.rel_path));
    Ok(Json(FileListing { id: row.id, folder: row.folder, total_bytes: total_size(&files), files }))
}

async fn emulator_download(
    State(s): State<AppState>,
    _a: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let row = present_emulator(&s, &id)?;
    send_inside(&s.library.emulator_dir(&row.rel_path), q.get("path").map(String::as_str).unwrap_or(""), &headers, true).await
}

// ---------- personal library ----------
fn library_state(s: &AppState, user_id: i64) -> HttpResult<LibraryState> {
    let conn = s.db.conn();
    let ids = |sql: &str| -> HttpResult<Vec<i64>> {
        let mut st = conn.prepare(sql)?;
        Ok(st.query_map([user_id], |r| r.get(0))?.flatten().collect())
    };
    Ok(LibraryState {
        games: ids("SELECT game_id FROM user_games WHERE user_id = ? ORDER BY added_at")?,
        emulators: ids("SELECT emulator_id FROM user_emulators WHERE user_id = ? ORDER BY added_at")?,
    })
}

fn changed(s: &AppState, a: &AuthUser, headers: &HeaderMap) -> HttpResult<Json<LibraryState>> {
    let state = library_state(s, a.user.id)?;
    s.events.to_user(a.user.id, ServerEvent::Library { library: state.clone() }, device(headers).as_deref());
    Ok(Json(state))
}

async fn library(State(s): State<AppState>, a: AuthUser) -> HttpResult<Json<LibraryState>> {
    Ok(Json(library_state(&s, a.user.id)?))
}

async fn add_game(State(s): State<AppState>, a: AuthUser, Path(id): Path<String>, headers: HeaderMap) -> HttpResult<Json<LibraryState>> {
    let row = s.library.get_game(id_param(&id, "id")?).filter(|r| !r.missing).ok_or_else(|| HttpError::not_found("Game not found"))?;
    {
        let conn = s.db.conn();
        conn.execute("INSERT OR IGNORE INTO user_games (user_id, game_id, added_at) VALUES (?, ?, ?)", params![a.user.id, row.id, now()])?;
        drop(conn);
        // Emulators for this platform are added automatically.
        for emu in s.library.list_emulators(false).into_iter().filter(|e| e.platform == row.platform) {
            s.db.conn().execute(
                "INSERT OR IGNORE INTO user_emulators (user_id, emulator_id, added_at, auto) VALUES (?, ?, ?, 1)",
                params![a.user.id, emu.id, now()],
            )?;
        }
    }
    changed(&s, &a, &headers)
}

async fn remove_game(State(s): State<AppState>, a: AuthUser, Path(id): Path<String>, headers: HeaderMap) -> HttpResult<Json<LibraryState>> {
    s.db.conn().execute("DELETE FROM user_games WHERE user_id = ? AND game_id = ?", params![a.user.id, id_param(&id, "id")?])?;
    changed(&s, &a, &headers)
}

async fn add_emulator(State(s): State<AppState>, a: AuthUser, Path(id): Path<String>, headers: HeaderMap) -> HttpResult<Json<LibraryState>> {
    let row = present_emulator(&s, &id)?;
    s.db.conn().execute(
        "INSERT OR IGNORE INTO user_emulators (user_id, emulator_id, added_at, auto) VALUES (?, ?, ?, 0)",
        params![a.user.id, row.id, now()],
    )?;
    changed(&s, &a, &headers)
}

async fn remove_emulator(State(s): State<AppState>, a: AuthUser, Path(id): Path<String>, headers: HeaderMap) -> HttpResult<Json<LibraryState>> {
    s.db.conn().execute("DELETE FROM user_emulators WHERE user_id = ? AND emulator_id = ?", params![a.user.id, id_param(&id, "id")?])?;
    changed(&s, &a, &headers)
}

// ---------- cloud saves (per emulator, versioned, never deleted) ----------
async fn latest_saves(State(s): State<AppState>, a: AuthUser) -> HttpResult<Json<Value>> {
    Ok(Json(serde_json::to_value(s.saves.latest_all(a.user.id)?).unwrap_or_default()))
}

async fn list_saves(State(s): State<AppState>, a: AuthUser, Path(id): Path<String>) -> HttpResult<Json<Value>> {
    let emulator_id = id_param(&id, "emulatorId")?;
    Ok(Json(serde_json::to_value(s.saves.list(a.user.id, emulator_id)?).unwrap_or_default()))
}

async fn upload_save(State(s): State<AppState>, a: AuthUser, Path(id): Path<String>, headers: HeaderMap, body: Body) -> HttpResult<Response> {
    let emulator_id = id_param(&id, "emulatorId")?;
    if s.library.get_emulator(emulator_id).is_none() {
        return Err(HttpError::not_found("Emulator not found"));
    }
    let header = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(decode_uri_component);
    let dev = device(&headers);
    let result = s
        .saves
        .store(a.user.id, emulator_id, body, StoreMeta { device: dev.clone(), note: header("x-save-note"), expected_sha256: header("x-content-sha256") })
        .await?;
    if result.created {
        s.events.to_user(
            a.user.id,
            ServerEvent::Save { emulator_id, version: result.version.clone(), from_device: dev.clone() },
            dev.as_deref(),
        );
    }
    let status = if result.created { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(result)).into_response())
}

async fn download_save(State(s): State<AppState>, a: AuthUser, Path(id): Path<String>, headers: HeaderMap) -> HttpResult<Response> {
    let (version, file) = s.saves.get(a.user.id, id_param(&id, "id")?)?;
    if !file.is_file() {
        return Err(HttpError::not_found("File not found"));
    }
    send_file(
        &file,
        &headers,
        &[
            (header::HeaderName::from_static("x-save-version"), version.version.to_string()),
            (header::HeaderName::from_static("x-content-sha256"), version.sha256.clone()),
        ],
    )
    .await
}

// ---------- play time ----------
async fn add_playtime(State(s): State<AppState>, a: AuthUser, headers: HeaderMap, body: JsonBody) -> HttpResult<Json<Value>> {
    let sessions: Vec<Value> = match body.0 {
        Value::Array(a) => a,
        other => vec![other],
    };
    let dev = device(&headers);
    let mut stored = 0;
    for sess in sessions {
        let seconds = js_num(sess.get("seconds")).map(|f| (f + 0.5).floor());
        let session_id = sess.get("sessionId").filter(|v| crate::util::truthy(Some(v)));
        let game_id = js_num(sess.get("gameId")).filter(|f| f.fract() == 0.0).map(|f| f as i64);
        let (Some(sid), Some(seconds), Some(game_id)) = (session_id, seconds, game_id) else { continue };
        if !seconds.is_finite() || seconds <= 0.0 || s.library.get_game(game_id).is_none() {
            continue;
        }
        let started = sess.get("startedAt").filter(|v| !v.is_null()).map(|v| js_str(Some(v))).unwrap_or_else(now);
        let n = s.db.conn().execute(
            "INSERT OR IGNORE INTO play_sessions (session_id, user_id, game_id, started_at, seconds, device, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                crate::util::js_slice(&js_str(Some(sid)), 64),
                a.user.id,
                game_id,
                started,
                seconds.min(7.0 * 86400.0) as i64,
                dev,
                now()
            ],
        )?;
        stored += n;
    }
    Ok(Json(json!({ "ok": true, "stored": stored })))
}

async fn playtime(State(s): State<AppState>, a: AuthUser) -> HttpResult<Json<Vec<PlaytimeEntry>>> {
    let conn = s.db.conn();
    let mut st = conn.prepare(
        "SELECT game_id gameId, SUM(seconds) seconds, COUNT(*) sessions, MAX(datetime(started_at, '+' || seconds || ' seconds')) lastPlayed
         FROM play_sessions WHERE user_id = ? GROUP BY game_id",
    )?;
    let rows = st.query_map([a.user.id], |r| {
        let last: Option<String> = r.get(3)?;
        Ok(PlaytimeEntry {
            game_id: r.get(0)?,
            seconds: r.get(1)?,
            sessions: r.get(2)?,
            // SQLite gives "YYYY-MM-DD HH:MM:SS" in UTC.
            last_played: last.and_then(|l| nostalgia_shared::parse_iso_millis(&format!("{}Z", l.replace(' ', "T")))).map(nostalgia_shared::iso_from_millis),
        })
    })?;
    Ok(Json(rows.flatten().collect()))
}

// ---------- push events ----------
struct Registration {
    hub: std::sync::Arc<crate::events::EventHub>,
    id: u64,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.hub.remove(self.id);
    }
}

async fn events(
    State(s): State<AppState>,
    a: AuthUser,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let dev = device(&headers).or_else(|| q.get("device").cloned());
    let id = s.events.add(a.user.id, dev, tx);
    let reg = Registration { hub: s.events.clone(), id };
    let stream = futures_util::stream::unfold((rx, reg), |(mut rx, reg)| async move {
        let ev = rx.recv().await?;
        let event = Event::default().event(ev.name()).data(serde_json::to_string(&ev).unwrap_or_default());
        Some((Ok(event), (rx, reg)))
    });
    // Keep connections alive through proxies.
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25)).text("ping"))
}
