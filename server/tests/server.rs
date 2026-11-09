//! Port of server/test/server.test.ts: the same scenarios against the Rust server over real HTTP.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

struct Ctx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: String,
    http: reqwest::Client,
}

async fn start() -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let lib = root.join("library");
    let game = lib.join("games").join("psx").join("Test Game [igdbid-425]");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join("Test (Disc 1).cue"), r#"FILE "Test (Disc 1).bin" BINARY"#).unwrap();
    std::fs::write(game.join("Test (Disc 1).bin"), vec![7u8; 4096]).unwrap();
    std::fs::write(lib.join("games").join("psx").join("Loose (Disc 1).bin"), "a").unwrap();
    std::fs::write(lib.join("games").join("psx").join("Loose (Disc 2).bin"), "b").unwrap();
    let emu = lib.join("emulators").join("psx").join("Emu");
    std::fs::create_dir_all(&emu).unwrap();
    std::fs::write(
        emu.join("emu.yaml"),
        "name: TestEmu\nsystems:\n  windows:\n    executable: emu.exe\n    saves: [\"{emulatorDir}/saves\"]\n  linux:\n    executable: emu\n",
    )
    .unwrap();
    std::fs::write(emu.join("emu.exe"), "binary").unwrap();
    let settings = root.join("settings.yaml");
    std::fs::write(
        &settings,
        "adminUsername: admin\nadminPassword: secret\nlibraryDirectory: ./library\ndatabaseDirectory: ./database\nsavesDirectory: ./saves\nigdbClientId: \"\"\nigdbClientSecret: \"\"\n",
    )
    .unwrap();
    let (router, state) = nostalgia_server::create_server(&settings).unwrap();
    state.jobs.scan(false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Ctx { _dir: dir, root, base, http: reqwest::Client::new() }
}

impl Ctx {
    async fn req(&self, method: &str, path: &str, token: Option<&str>, cookie: Option<&str>, body: Option<Value>) -> (u16, Value, reqwest::header::HeaderMap) {
        let mut r = self.http.request(method.parse().unwrap(), format!("{}{}", self.base, path));
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        if let Some(c) = cookie {
            r = r.header("Cookie", c);
        }
        if let Some(b) = body {
            r = r.json(&b);
        }
        let res = r.send().await.unwrap();
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let text = res.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)), headers)
    }
}

fn files_below(dir: &Path, ext: &str) -> usize {
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_dir() {
            n += files_below(&e.path(), ext);
        } else if e.path().to_string_lossy().ends_with(ext) {
            n += 1;
        }
    }
    n
}

#[tokio::test]
async fn full_protocol() {
    let c = start().await;

    // platform folders are created (RomM names)
    for p in ["ps2", "snes", "gba", "n64"] {
        assert!(c.root.join("library").join("games").join(p).exists());
    }
    assert!(c.root.join("library").join("emulators").join("ps2").exists());

    // register, login, change password
    let (s, r, _) = c.req("POST", "/api/auth/register", None, None, Some(json!({ "username": "alice", "password": "password1" }))).await;
    assert_eq!(s, 201);
    let token = r["token"].as_str().unwrap().to_string();
    let t = Some(token.as_str());
    assert_eq!(c.req("POST", "/api/auth/register", None, None, Some(json!({ "username": "ALICE", "password": "password1" }))).await.0, 409);
    assert_eq!(c.req("POST", "/api/me/password", t, None, Some(json!({ "oldPassword": "wrong", "newPassword": "password2" }))).await.0, 403);
    assert_eq!(c.req("POST", "/api/me/password", t, None, Some(json!({ "oldPassword": "password1", "newPassword": "password2" }))).await.0, 200);
    assert_eq!(c.req("POST", "/api/auth/login", None, None, Some(json!({ "username": "alice", "password": "password2" }))).await.0, 200);
    let (s, r, _) = c.req("GET", "/api/games", None, None, None).await;
    assert_eq!((s, r), (401, json!({ "error": "Not authenticated" })));

    // catalog, library with automatic emulators, files and range downloads
    let games = c.req("GET", "/api/games", t, None, None).await.1;
    assert_eq!(games.as_array().unwrap().len(), 1);
    let g = &games[0];
    assert_eq!(g["igdbId"], 425);
    assert_eq!(g["name"], "Test Game");
    let gid = g["id"].as_i64().unwrap();
    let emus = c.req("GET", "/api/emulators", t, None, None).await.1;
    assert_eq!(emus[0]["name"], "TestEmu");
    assert_eq!(emus[0]["supports"], json!(["windows", "linux"]));
    let eid = emus[0]["id"].as_i64().unwrap();
    let lib = c.req("POST", &format!("/api/library/games/{gid}"), t, None, None).await.1;
    assert_eq!(lib, json!({ "games": [gid], "emulators": [eid] }));
    let files = c.req("GET", &format!("/api/games/{gid}/files"), t, None, None).await.1;
    assert_eq!(files["files"].as_array().unwrap().len(), 2);
    assert_eq!(files["totalBytes"], 4096 + 31);
    let res = c
        .http
        .get(format!("{}/api/games/{gid}/download?path={}", c.base, "Test%20(Disc%201).bin"))
        .bearer_auth(&token)
        .header("Range", "bytes=100-199")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 206);
    assert_eq!(res.bytes().await.unwrap().len(), 100);
    let full = c.http.get(format!("{}/api/games/{gid}/download?path=Test%20(Disc%201).bin", c.base)).bearer_auth(&token).send().await.unwrap();
    assert!(full.headers()["content-disposition"].to_str().unwrap().starts_with("attachment; filename*=UTF-8''Test%20(Disc%201).bin"));
    assert_eq!(c.req("GET", &format!("/api/games/{gid}/download?path=..%2F..%2F..%2Fsettings.yaml"), t, None, None).await.0, 400);
    let detail = c.req("GET", &format!("/api/games/{gid}"), t, None, None).await.1;
    assert_eq!(detail["metadata"], Value::Null);

    // cloud saves are versioned, deduplicated and never overwritten
    let up = |body: &'static str| {
        c.http
            .post(format!("{}/api/saves/{eid}", c.base))
            .bearer_auth(&token)
            .header("Content-Type", "application/zip")
            .header("X-Device", "test")
            .header("X-Save-Note", "after%20playing")
            .body(body)
            .send()
    };
    let v1: Value = up("one").await.unwrap().json().await.unwrap();
    assert_eq!(v1["version"]["version"], 1);
    assert_eq!(v1["version"]["note"], "after playing");
    let again = up("one").await.unwrap();
    assert_eq!(again.status().as_u16(), 200);
    assert_eq!(again.json::<Value>().await.unwrap()["created"], false);
    let v2 = up("two").await.unwrap();
    assert_eq!(v2.status().as_u16(), 201);
    let list = c.req("GET", &format!("/api/saves/{eid}"), t, None, None).await.1;
    assert_eq!(list.as_array().unwrap().iter().map(|v| v["version"].as_i64().unwrap()).collect::<Vec<_>>(), vec![2, 1]);
    let old = c.http.get(format!("{}/api/saves/version/{}/download", c.base, list[1]["id"])).bearer_auth(&token).send().await.unwrap();
    assert_eq!(old.headers()["x-save-version"], "1");
    assert_eq!(old.text().await.unwrap(), "one");
    assert_eq!(files_below(&c.root.join("saves"), ".zip"), 2);
    let latest = c.req("GET", "/api/saves", t, None, None).await.1;
    assert_eq!(latest[0]["version"], 2);

    // play time sessions are idempotent
    let sess = json!({ "sessionId": "abc", "gameId": gid, "startedAt": "2026-11-08T20:00:00.000Z", "seconds": 125 });
    c.req("POST", "/api/playtime", t, None, Some(json!([sess, sess]))).await;
    c.req("POST", "/api/playtime", t, None, Some(sess)).await;
    let pt = c.req("GET", "/api/playtime", t, None, None).await.1;
    assert_eq!(pt[0]["seconds"], 125);
    assert_eq!(pt[0]["lastPlayed"], "2026-11-08T20:02:05.000Z");

    // admin: login, unsorted files and import moves (never deletes) files
    let (s, _, _) = c.req("POST", "/admin/api/login", None, None, Some(json!({ "username": "admin", "password": "x" }))).await;
    assert_eq!(s, 401);
    let (_, _, h) = c.req("POST", "/admin/api/login", None, None, Some(json!({ "username": "admin", "password": "secret" }))).await;
    let cookie = h["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();
    let ck = Some(cookie.as_str());
    assert_eq!(c.req("GET", "/admin/api/overview", None, None, None).await.0, 401);
    let unsorted = c.req("GET", "/admin/api/unsorted", None, ck, None).await.1;
    let mut paths: Vec<String> = unsorted.as_array().unwrap().iter().map(|u| u["path"].as_str().unwrap().to_string()).collect();
    paths.sort();
    assert_eq!(paths, vec!["Loose (Disc 1).bin", "Loose (Disc 2).bin"]);
    assert_eq!(unsorted[0]["platformName"], "PlayStation");
    let (s, _, _) = c
        .req(
            "POST",
            "/admin/api/import",
            None,
            ck,
            Some(json!({ "platform": "psx", "entries": ["Loose (Disc 1).bin", "Loose (Disc 2).bin"], "igdbId": 427, "name": "Loose: Game?" })),
        )
        .await;
    assert_eq!(s, 200);
    let folder = c.root.join("library").join("games").join("psx").join("Loose Game [igdbid-427]");
    let mut moved: Vec<String> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    moved.sort();
    assert_eq!(moved, vec!["Loose (Disc 1).bin", "Loose (Disc 2).bin"]);
    assert_eq!(c.req("GET", "/api/games", t, None, None).await.1.as_array().unwrap().len(), 2);
    let users = c.req("GET", "/admin/api/users", None, ck, None).await.1;
    assert_eq!(users[0]["username"], "alice");
    assert_eq!(users[0]["saves"]["count"], 2);
    let overview = c.req("GET", "/admin/api/overview", None, ck, None).await.1;
    assert_eq!(overview["games"], 2);
    assert_eq!(overview["savesStorage"]["writable"], true);

    // static admin UI and fallbacks
    let html = c.http.get(format!("{}/admin/", c.base)).send().await.unwrap();
    assert_eq!(html.status().as_u16(), 200);
    assert!(html.text().await.unwrap().contains("<html"));
    let (s, body, _) = c.req("GET", "/api/nope", t, None, None).await;
    assert_eq!((s, body), (404, json!({ "error": "Not found" })));
}
