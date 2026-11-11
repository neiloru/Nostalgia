//! `/admin/api`: the admin web interface (port of routes/admin.ts) and the static admin UI.

use crate::auth::{ADMIN_COOKIE, cookie, validate_username};
use crate::db::now;
use crate::error::{HttpError, HttpResult};
use crate::http::{Admin, AppState, JsonBody, attachment, send_file};
use crate::library::{parse_metadata, read_emu_config, to_emulator_summary, to_game_summary};
use crate::routes_api::MEDIA_RE;
use crate::util::{id_param, js_num, js_str, truthy};
use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use nostalgia_shared::{PLATFORMS, ServerEvent, platform_name};
use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/overview", get(overview))
        .route("/jobs", get(jobs))
        .route("/scan", post(scan))
        .route("/platforms", get(platforms))
        .route("/games", get(games))
        .route("/games/{id}", get(game))
        .route("/games/{id}/media/{name}", get(game_media))
        .route("/games/{id}/refresh", post(refresh))
        .route("/games/{id}/hash", post(hash))
        .route("/games/{id}/match", post(rematch))
        .route("/games/{id}/hidden", post(hidden))
        .route("/unsorted", get(unsorted))
        .route("/unsorted/identify", post(identify))
        .route("/import", post(import))
        .route("/igdb/search", get(igdb_search))
        .route("/emulators", get(emulators))
        .route("/emulators/{id}/yaml", get(get_yaml).put(put_yaml))
        .route("/users", get(users).post(create_user))
        .route("/users/{id}", get(user))
        .route("/users/{id}/password", post(user_password))
        .route("/users/{id}/disabled", post(user_disabled))
        .route("/users/{id}/rename", post(user_rename))
        .route("/saves/{id}/download", get(save_download))
        .fallback(admin_fallback)
}

/// Unknown admin API paths: 401 without a session, 404 with one (like the Express middleware order).
async fn admin_fallback(_a: Admin) -> HttpError {
    HttpError::not_found("Not found")
}

fn id(raw: &str) -> HttpResult<i64> {
    id_param(raw, "id")
}

async fn login(State(s): State<AppState>, body: JsonBody) -> HttpResult<Response> {
    let token = s.auth.admin_login(&js_str(body.get("username")), &js_str(body.get("password")))?;
    Ok((
        [(header::SET_COOKIE, format!("{ADMIN_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200"))],
        Json(json!({ "ok": true })),
    )
        .into_response())
}

async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(t) = cookie(&headers, ADMIN_COOKIE) {
        s.auth.admin_logout(&t);
    }
    ([(header::SET_COOKIE, format!("{ADMIN_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"))], Json(json!({ "ok": true }))).into_response()
}

async fn overview(State(s): State<AppState>, _a: Admin) -> Json<Value> {
    let c = |sql: &str| s.db.count(sql);
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "games": c("SELECT COUNT(*) c FROM games WHERE missing = 0"),
        "missingGames": c("SELECT COUNT(*) c FROM games WHERE missing = 1"),
        "emulators": c("SELECT COUNT(*) c FROM emulators WHERE missing = 0"),
        "users": c("SELECT COUNT(*) c FROM users"),
        "saves": c("SELECT COUNT(*) c FROM saves"),
        "saveBytes": c("SELECT COALESCE(SUM(size),0) c FROM saves"),
        "playSeconds": c("SELECT COALESCE(SUM(seconds),0) c FROM play_sessions"),
        "librarySize": c("SELECT COALESCE(SUM(size_bytes),0) c FROM games WHERE missing = 0"),
        "metadataMissing": c("SELECT COUNT(*) c FROM games WHERE missing = 0 AND metadata_status != 'ok'"),
        "unsorted": s.jobs.unsorted().len(),
        "connectedClients": s.events.count(),
        "onlineUsers": s.events.online_user_ids().len(),
        "igdbConfigured": s.igdb.configured(),
        "registration": s.settings.allow_registration,
        "libraryPath": s.settings.library_path,
        "databasePath": s.settings.database_path,
        "savesPath": s.settings.saves_path,
        "metadataPath": s.settings.metadata_path,
        "savesStorage": s.saves.check(),
        "settingsFile": s.settings.file,
        "jobs": s.jobs.snapshot(),
    }))
}

async fn jobs(State(s): State<AppState>, _a: Admin) -> Json<Value> {
    Json(serde_json::to_value(s.jobs.snapshot()).unwrap_or_default())
}

async fn scan(State(s): State<AppState>, _a: Admin, body: JsonBody) -> HttpResult<Response> {
    let hash = truthy(body.get("hash"));
    if s.jobs.scan_running() {
        return Err(HttpError::new(409, "A scan is already running"));
    }
    let jobs = s.jobs.clone();
    tokio::spawn(async move { jobs.scan(hash).await });
    Ok((StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response())
}

async fn platforms(_a: Admin) -> Json<Value> {
    Json(serde_json::to_value(PLATFORMS).unwrap_or_default())
}

/// Summary JSON object with extra fields appended (like `{ ...toGameSummary(g), relPath, ... }`).
fn extend(summary: impl serde::Serialize, extra: Value) -> Value {
    let mut m: Map<String, Value> = match serde_json::to_value(summary) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    if let Value::Object(e) = extra {
        for (k, v) in e {
            m.insert(k, v);
        }
    }
    Value::Object(m)
}

async fn games(State(s): State<AppState>, _a: Admin) -> HttpResult<Json<Value>> {
    let (hashes, lib_count) = {
        let conn = s.db.conn();
        let mut st = conn.prepare(
            "SELECT game_id, COUNT(*) files, SUM(hasheous_status = 'match') matched, SUM(hasheous_status = 'nomatch') unmatched,
                    SUM(hasheous_status = 'error') errors FROM file_hashes GROUP BY game_id",
        )?;
        let hashes: HashMap<i64, Value> = st
            .query_map([], |r| {
                let gid: i64 = r.get(0)?;
                Ok((
                    gid,
                    json!({
                        "game_id": gid,
                        "files": r.get::<_, i64>(1)?,
                        "matched": r.get::<_, Option<i64>>(2)?,
                        "unmatched": r.get::<_, Option<i64>>(3)?,
                        "errors": r.get::<_, Option<i64>>(4)?,
                    }),
                ))
            })?
            .flatten()
            .collect();
        let mut st = conn.prepare("SELECT game_id, COUNT(*) c FROM user_games GROUP BY game_id")?;
        let lib: HashMap<i64, i64> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.flatten().collect();
        (hashes, lib)
    };
    let out: Vec<Value> = s
        .library
        .list_all_games()
        .iter()
        .map(|g| {
            extend(
                to_game_summary(g),
                json!({
                    "relPath": g.rel_path,
                    "missing": g.missing,
                    "hidden": g.hidden,
                    "metadataStatus": g.metadata_status,
                    "metadataError": g.metadata_error,
                    "hash": hashes.get(&g.id).cloned().unwrap_or(Value::Null),
                    "inLibraries": lib_count.get(&g.id).copied().unwrap_or(0),
                }),
            )
        })
        .collect();
    Ok(Json(Value::Array(out)))
}

async fn game(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>) -> HttpResult<Json<Value>> {
    let g = s.library.get_game(id(&raw)?).ok_or_else(|| HttpError::not_found("Game not found"))?;
    let files: Vec<Value> = {
        let conn = s.db.conn();
        let mut st = conn.prepare(
            "SELECT rel_path path, size, md5, sha1, crc32, hasheous_status status, hasheous_name name, checked_at checkedAt FROM file_hashes WHERE game_id = ? ORDER BY rel_path",
        )?;
        st.query_map([g.id], |r| {
            Ok(json!({
                "path": r.get::<_, String>(0)?,
                "size": r.get::<_, i64>(1)?,
                "md5": r.get::<_, Option<String>>(2)?,
                "sha1": r.get::<_, Option<String>>(3)?,
                "crc32": r.get::<_, Option<String>>(4)?,
                "status": r.get::<_, Option<String>>(5)?,
                "name": r.get::<_, Option<String>>(6)?,
                "checkedAt": r.get::<_, Option<String>>(7)?,
            }))
        })?
        .flatten()
        .collect()
    };
    Ok(Json(extend(
        to_game_summary(&g),
        json!({
            "relPath": g.rel_path,
            "missing": g.missing,
            "hidden": g.hidden,
            "metadataStatus": g.metadata_status,
            "metadataError": g.metadata_error,
            "metadata": parse_metadata(&g),
            "hashes": files,
        }),
    )))
}

async fn game_media(State(s): State<AppState>, _a: Admin, Path((raw, name)): Path<(String, String)>, headers: HeaderMap) -> HttpResult<Response> {
    let g = s.library.get_game(id(&raw)?);
    let Some(g) = g.filter(|_| MEDIA_RE.is_match(&name)) else { return Err(HttpError::not_found("Not found")) };
    let file = s.library.metadata_dir(g.igdb_id).join(&name);
    if !file.is_file() {
        return Err(HttpError::not_found("Not found"));
    }
    send_file(&file, &headers, &[(header::CACHE_CONTROL, "private, max-age=3600".into())]).await
}

async fn refresh(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>) -> HttpResult<Response> {
    let gid = id(&raw)?;
    s.library.reset_metadata(gid);
    s.jobs.queue_metadata(gid);
    Ok((StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response())
}

async fn hash(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>) -> HttpResult<Response> {
    s.jobs.queue_hash(id(&raw)?);
    Ok((StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response())
}

async fn rematch(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>, body: JsonBody) -> HttpResult<Json<Value>> {
    let name = body.get("name").filter(|v| !v.is_null()).map(|v| js_str(Some(v)));
    let row = s.library.rematch_game(id(&raw)?, js_num(body.get("igdbId")).unwrap_or(f64::NAN), name.as_deref())?;
    s.jobs.queue_metadata(row.id);
    s.events.to_all(ServerEvent::Catalog);
    Ok(Json(serde_json::to_value(to_game_summary(&row)).unwrap_or_default()))
}

async fn hidden(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>, body: JsonBody) -> HttpResult<Json<Value>> {
    s.library.set_hidden(id(&raw)?, truthy(body.get("hidden")))?;
    s.events.to_all(ServerEvent::Catalog);
    Ok(Json(json!({ "ok": true })))
}

async fn unsorted(State(s): State<AppState>, _a: Admin) -> Json<Value> {
    Json(Value::Array(
        s.jobs.unsorted().into_iter().map(|u| {
            let pn = platform_name(&u.platform);
            extend(u, json!({ "platformName": pn }))
        }).collect(),
    ))
}

async fn identify(State(s): State<AppState>, _a: Admin, body: JsonBody) -> HttpResult<Json<Value>> {
    let r = s.jobs.identify(&js_str(body.get("platform")), &js_str(body.get("path"))).await?;
    Ok(Json(serde_json::to_value(r).unwrap_or_default()))
}

async fn import(State(s): State<AppState>, _a: Admin, body: JsonBody) -> HttpResult<Json<Value>> {
    let platform = body.get("platform").filter(|v| truthy(Some(v)));
    let Some(Value::Array(entries)) = body.get("entries") else { return Err(HttpError::new(400, "platform and entries are required")) };
    let Some(platform) = platform else { return Err(HttpError::new(400, "platform and entries are required")) };
    let entries: Vec<String> = entries.iter().map(|e| js_str(Some(e))).collect();
    let rel = s.library.import_entries(&js_str(Some(platform)), &entries, js_num(body.get("igdbId")).unwrap_or(f64::NAN), &js_str(body.get("name")))?;
    s.jobs.scan(false).await;
    Ok(Json(json!({ "ok": true, "relPath": rel })))
}

async fn igdb_search(State(s): State<AppState>, _a: Admin, Query(q): Query<HashMap<String, String>>) -> HttpResult<Json<Value>> {
    if !s.igdb.configured() {
        return Err(HttpError::new(400, "IGDB credentials are not configured in the settings file"));
    }
    let hits = s.igdb.search(q.get("q").map(String::as_str).unwrap_or(""), q.get("platform").map(String::as_str)).await?;
    Ok(Json(serde_json::to_value(hits).unwrap_or_default()))
}

async fn emulators(State(s): State<AppState>, _a: Admin) -> HttpResult<Json<Value>> {
    let lib: HashMap<i64, i64> = {
        let conn = s.db.conn();
        let mut st = conn.prepare("SELECT emulator_id, COUNT(*) c FROM user_emulators GROUP BY emulator_id")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.flatten().collect()
    };
    Ok(Json(Value::Array(
        s.library
            .list_emulators(true)
            .iter()
            .map(|e| {
                extend(to_emulator_summary(e), json!({ "relPath": e.rel_path, "missing": e.missing, "inLibraries": lib.get(&e.id).copied().unwrap_or(0) }))
            })
            .collect(),
    )))
}

async fn get_yaml(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>) -> HttpResult<Json<Value>> {
    let e = s.library.get_emulator(id(&raw)?).ok_or_else(|| HttpError::not_found("Emulator not found"))?;
    let dir = s.library.emulator_dir(&e.rel_path);
    let file = read_emu_config(&dir).file;
    let text = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    Ok(Json(json!({ "text": text, "exists": file.is_some(), "path": file.unwrap_or_else(|| dir.join("emu.yaml")) })))
}

async fn put_yaml(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>, body: JsonBody) -> HttpResult<Json<Value>> {
    s.library.write_emu_yaml(id(&raw)?, &js_str(body.get("text")))?;
    s.events.to_all(ServerEvent::Catalog);
    Ok(Json(json!({ "ok": true })))
}

async fn users(State(s): State<AppState>, _a: Admin) -> HttpResult<Json<Value>> {
    let online: Vec<i64> = s.events.online_user_ids();
    let rows: Vec<(i64, String, String, i64)> = {
        let conn = s.db.conn();
        let mut st = conn.prepare("SELECT id, username, created_at createdAt, disabled FROM users ORDER BY username COLLATE NOCASE")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.flatten().collect()
    };
    let mut out = Vec::new();
    for (uid, username, created_at, disabled) in rows {
        let conn = s.db.conn();
        let games: i64 = conn.query_row("SELECT COUNT(*) c FROM user_games WHERE user_id = ?", [uid], |r| r.get(0))?;
        let play: i64 = conn.query_row("SELECT COALESCE(SUM(seconds),0) c FROM play_sessions WHERE user_id = ?", [uid], |r| r.get(0))?;
        let last_seen: Option<String> = conn.query_row("SELECT MAX(last_seen) c FROM sessions WHERE user_id = ?", [uid], |r| r.get(0))?;
        drop(conn);
        let (count, bytes) = s.saves.count_for_user(uid);
        out.push(json!({
            "id": uid,
            "username": username,
            "createdAt": created_at,
            "disabled": disabled != 0,
            "online": online.contains(&uid),
            "games": games,
            "playSeconds": play,
            "saves": { "count": count, "bytes": bytes },
            "lastSeen": last_seen,
        }));
    }
    Ok(Json(Value::Array(out)))
}

async fn create_user(State(s): State<AppState>, _a: Admin, body: JsonBody) -> HttpResult<Response> {
    let u = s.auth.register(&js_str(body.get("username")), &js_str(body.get("password")))?;
    Ok((StatusCode::CREATED, Json(u)).into_response())
}

async fn user(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>) -> HttpResult<Json<Value>> {
    let uid = id(&raw)?;
    let u = s.auth.get_user(uid).ok_or_else(|| HttpError::not_found("User not found"))?;
    let conn = s.db.conn();
    let mut st = conn.prepare(
        "SELECT g.id, g.name, g.platform, ug.added_at addedAt,
           (SELECT COALESCE(SUM(seconds),0) FROM play_sessions p WHERE p.user_id = ug.user_id AND p.game_id = g.id) seconds
         FROM user_games ug JOIN games g ON g.id = ug.game_id WHERE ug.user_id = ? ORDER BY g.name",
    )?;
    let games: Vec<Value> = st
        .query_map([uid], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "platform": r.get::<_, String>(2)?,
                "addedAt": r.get::<_, String>(3)?,
                "seconds": r.get::<_, i64>(4)?,
            }))
        })?
        .flatten()
        .collect();
    let mut st = conn.prepare(
        "SELECT s.id, s.version, s.size, s.device, s.note, s.created_at createdAt, e.name emulator, e.platform
         FROM saves s JOIN emulators e ON e.id = s.emulator_id WHERE s.user_id = ? ORDER BY s.created_at DESC LIMIT 200",
    )?;
    let saves: Vec<Value> = st
        .query_map([uid], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "version": r.get::<_, i64>(1)?,
                "size": r.get::<_, i64>(2)?,
                "device": r.get::<_, Option<String>>(3)?,
                "note": r.get::<_, Option<String>>(4)?,
                "createdAt": r.get::<_, String>(5)?,
                "emulator": r.get::<_, String>(6)?,
                "platform": r.get::<_, String>(7)?,
            }))
        })?
        .flatten()
        .collect();
    Ok(Json(json!({ "user": u, "games": games, "saves": saves })))
}

async fn user_password(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>, body: JsonBody) -> HttpResult<Json<Value>> {
    let uid = id(&raw)?;
    if s.auth.get_user(uid).is_none() {
        return Err(HttpError::not_found("User not found"));
    }
    s.auth.set_password(uid, &js_str(body.get("password")))?;
    s.auth.revoke_all_for_user(uid, None)?;
    Ok(Json(json!({ "ok": true })))
}

async fn user_disabled(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>, body: JsonBody) -> HttpResult<Json<Value>> {
    let uid = id(&raw)?;
    let disabled = truthy(body.get("disabled"));
    s.db.conn().execute("UPDATE users SET disabled = ? WHERE id = ?", params![disabled as i64, uid])?;
    if disabled {
        s.auth.revoke_all_for_user(uid, None)?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn user_rename(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>, body: JsonBody) -> HttpResult<Json<Value>> {
    let uid = id(&raw)?;
    let name = validate_username(&js_str(body.get("username")))?;
    let conn = s.db.conn();
    let clash: Option<i64> = conn.query_row("SELECT id FROM users WHERE username = ? AND id != ?", params![name, uid], |r| r.get(0)).optional()?;
    if clash.is_some() {
        return Err(HttpError::new(409, "Username already taken"));
    }
    conn.execute("UPDATE users SET username = ? WHERE id = ?", params![name, uid])?;
    Ok(Json(json!({ "ok": true, "at": now() })))
}

async fn save_download(State(s): State<AppState>, _a: Admin, Path(raw): Path<String>, headers: HeaderMap) -> HttpResult<Response> {
    let sid = id(&raw)?;
    let owner: Option<i64> = s.db.conn().query_row("SELECT user_id FROM saves WHERE id = ?", [sid], |r| r.get(0)).optional()?;
    let owner = owner.ok_or_else(|| HttpError::not_found("Save not found"))?;
    let (version, file) = s.saves.get(owner, sid)?;
    send_file(&file, &headers, &[attachment(&format!("save-u{owner}-e{}-v{}.zip", version.emulator_id, version.version))]).await
}

// ---------- static admin UI (embedded, shared with the Node.js server) ----------
const INDEX_HTML: &[u8] = include_bytes!("../admin/index.html");
const ADMIN_JS: &[u8] = include_bytes!("../admin/admin.js");
const ADMIN_CSS: &[u8] = include_bytes!("../admin/admin.css");
const FAVICON: &[u8] = include_bytes!("../admin/favicon.svg");

pub fn static_router() -> Router<AppState> {
    Router::new()
        .route("/admin", get(|| async { Redirect::permanent("/admin/") }))
        .route("/admin/", get(|| async { asset("index.html") }))
        .route("/admin/{file}", get(|Path(f): Path<String>| async move { asset(&f) }))
        .route("/", get(|| async { Redirect::to("/admin/") }))
}

fn asset(name: &str) -> Response {
    let (body, ct) = match name {
        "index.html" => (INDEX_HTML, "text/html; charset=UTF-8"),
        "admin.js" => (ADMIN_JS, "application/javascript; charset=UTF-8"),
        "admin.css" => (ADMIN_CSS, "text/css; charset=UTF-8"),
        "favicon.svg" => (FAVICON, "image/svg+xml"),
        _ => return HttpError::not_found("Not found").into_response(),
    };
    // Working on the admin UI: NOSTALGIA_ADMIN_DIR=server/admin serves the files from disk,
    // so changes show up on reload without rebuilding the server.
    let body: Vec<u8> = std::env::var_os("NOSTALGIA_ADMIN_DIR")
        .and_then(|dir| std::fs::read(std::path::Path::new(&dir).join(name)).ok())
        .unwrap_or_else(|| body.to_vec());
    ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "public, max-age=0")], body).into_response()
}
