//! Users, sessions and the admin login. Password hashes and tokens use the Node.js formats:
//! `scrypt$<salt b64>$<key b64>` (N=16384, r=8, p=1) and sha256(token) as session key.

use crate::db::{Db, now};
use crate::error::{HttpError, HttpResult};
use crate::util::now_ms;
use axum::http::HeaderMap;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use nostalgia_shared::User;
use rand::RngCore;
use regex::Regex;
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

fn scrypt_params(len: usize) -> scrypt::Params {
    scrypt::Params::new(14, 8, 1, len).expect("scrypt params")
}

pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; 16];
    rand::rng().fill_bytes(&mut salt);
    let mut key = [0u8; 32];
    scrypt::scrypt(password.as_bytes(), &salt, &scrypt_params(32), &mut key).expect("scrypt");
    format!("scrypt${}${}", STANDARD.encode(salt), STANDARD.encode(key))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let mut parts = stored.split('$');
    let (Some("scrypt"), Some(salt), Some(key)) = (parts.next(), parts.next(), parts.next()) else { return false };
    if salt.is_empty() || key.is_empty() {
        return false;
    }
    let (Ok(salt), Ok(key)) = (STANDARD.decode(salt), STANDARD.decode(key)) else { return false };
    if key.is_empty() || key.len() > 1024 {
        return false;
    }
    let mut test = vec![0u8; key.len()];
    if scrypt::scrypt(password.as_bytes(), &salt, &scrypt_params(key.len()), &mut test).is_err() {
        return false;
    }
    constant_eq(&key, &test)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Compare through sha256 so the length of the secret is not leaked either.
pub fn safe_equal(a: &str, b: &str) -> bool {
    constant_eq(&Sha256::digest(a.as_bytes()), &Sha256::digest(b.as_bytes()))
}

pub fn sha(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

pub fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

static USERNAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]{3,32}$").unwrap());

pub fn validate_username(u: &str) -> HttpResult<String> {
    let s = u.trim();
    if !USERNAME_RE.is_match(s) {
        return Err(HttpError::new(400, "Username must be 3-32 characters (letters, digits, _ . -)"));
    }
    Ok(s.to_string())
}

pub fn validate_password(p: &str) -> HttpResult<String> {
    let units: usize = p.chars().map(char::len_utf16).sum();
    if units < 6 {
        return Err(HttpError::new(400, "Password must be at least 6 characters"));
    }
    if units > 256 {
        return Err(HttpError::new(400, "Password too long"));
    }
    Ok(p.to_string())
}

fn to_user(r: &rusqlite::Row) -> rusqlite::Result<User> {
    Ok(User { id: r.get("id")?, username: r.get("username")?, created_at: r.get("created_at")? })
}

pub struct Auth {
    db: Db,
    admin_username: String,
    admin_password: String,
    /// token -> expiry (ms)
    admin_tokens: Mutex<HashMap<String, i64>>,
}

impl Auth {
    pub fn new(db: Db, admin_username: String, admin_password: String) -> Auth {
        Auth { db, admin_username, admin_password, admin_tokens: Mutex::new(HashMap::new()) }
    }

    pub fn create_session(&self, user_id: i64, device: Option<&str>) -> HttpResult<String> {
        let token = random_token();
        let t = now();
        self.db.conn().execute(
            "INSERT INTO sessions (token_hash, user_id, device, created_at, last_seen) VALUES (?, ?, ?, ?, ?)",
            params![sha(&token), user_id, device, t, t],
        )?;
        Ok(token)
    }

    pub fn revoke_session(&self, token: &str) -> HttpResult<()> {
        self.db.conn().execute("UPDATE sessions SET revoked = 1 WHERE token_hash = ?", [sha(token)])?;
        Ok(())
    }

    pub fn revoke_all_for_user(&self, user_id: i64, except_token: Option<&str>) -> HttpResult<()> {
        self.db.conn().execute(
            "UPDATE sessions SET revoked = 1 WHERE user_id = ? AND token_hash != ?",
            params![user_id, except_token.map(sha).unwrap_or_default()],
        )?;
        Ok(())
    }

    pub fn user_for_token(&self, token: &str) -> Option<User> {
        let conn = self.db.conn();
        let row = conn
            .query_row(
                "SELECT u.id, u.username, u.created_at, u.disabled, s.last_seen FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ? AND s.revoked = 0",
                [sha(token)],
                |r| Ok((to_user(r)?, r.get::<_, i64>("disabled")?, r.get::<_, String>("last_seen")?)),
            )
            .optional()
            .ok()
            .flatten()?;
        let (user, disabled, last_seen) = row;
        if disabled != 0 {
            return None;
        }
        // Touch at most once a minute.
        let seen = nostalgia_shared::parse_iso_millis(&last_seen);
        if seen.map(|s| now_ms() - s > 60_000).unwrap_or(false) {
            let _ = conn.execute("UPDATE sessions SET last_seen = ? WHERE token_hash = ?", params![now(), sha(token)]);
        }
        Some(user)
    }

    pub fn register(&self, username: &str, password: &str) -> HttpResult<User> {
        let u = validate_username(username)?;
        let p = validate_password(password)?;
        let hash = hash_password(&p);
        let conn = self.db.conn();
        let exists: Option<i64> = conn.query_row("SELECT id FROM users WHERE username = ?", [&u], |r| r.get(0)).optional()?;
        if exists.is_some() {
            return Err(HttpError::new(409, "Username already taken"));
        }
        conn.execute("INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?)", params![u, hash, now()])?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.get_user(id).ok_or_else(|| HttpError::internal("user vanished"))
    }

    pub fn login(&self, username: &str, password: &str) -> HttpResult<User> {
        let row = self
            .db
            .conn()
            .query_row(
                "SELECT id, username, created_at, disabled, password_hash FROM users WHERE username = ?",
                [username.trim()],
                |r| Ok((to_user(r)?, r.get::<_, i64>("disabled")?, r.get::<_, String>("password_hash")?)),
            )
            .optional()?;
        let Some((user, disabled, hash)) = row else { return Err(HttpError::new(401, "Invalid username or password")) };
        if !verify_password(password, &hash) {
            return Err(HttpError::new(401, "Invalid username or password"));
        }
        if disabled != 0 {
            return Err(HttpError::new(403, "This account is disabled"));
        }
        Ok(user)
    }

    pub fn set_password(&self, user_id: i64, password: &str) -> HttpResult<()> {
        let p = validate_password(password)?;
        self.db.conn().execute("UPDATE users SET password_hash = ? WHERE id = ?", params![hash_password(&p), user_id])?;
        Ok(())
    }

    pub fn check_password(&self, user_id: i64, password: &str) -> bool {
        let hash: Option<String> = self.db.conn().query_row("SELECT password_hash FROM users WHERE id = ?", [user_id], |r| r.get(0)).optional().ok().flatten();
        hash.map(|h| verify_password(password, &h)).unwrap_or(false)
    }

    pub fn get_user(&self, id: i64) -> Option<User> {
        self.db.conn().query_row("SELECT id, username, created_at, disabled FROM users WHERE id = ?", [id], to_user).optional().ok().flatten()
    }

    // ---- admin (credentials come from the settings file) ----
    pub fn admin_login(&self, username: &str, password: &str) -> HttpResult<String> {
        let ok_user = safe_equal(username, &self.admin_username);
        let ok_pass = safe_equal(password, &self.admin_password);
        if !ok_user || !ok_pass {
            return Err(HttpError::new(401, "Invalid admin credentials"));
        }
        let token = random_token();
        self.admin_tokens.lock().unwrap().insert(token.clone(), now_ms() + 12 * 3_600_000);
        Ok(token)
    }

    pub fn admin_logout(&self, token: &str) {
        self.admin_tokens.lock().unwrap().remove(token);
    }

    pub fn is_admin(&self, token: Option<&str>) -> bool {
        let Some(token) = token else { return false };
        let mut tokens = self.admin_tokens.lock().unwrap();
        match tokens.get(token) {
            None => false,
            Some(exp) if *exp < now_ms() => {
                tokens.remove(token);
                false
            }
            Some(_) => true,
        }
    }
}

/// `Authorization: Bearer <token>` or `?t=<token>`.
pub fn bearer(headers: &HeaderMap, query: &HashMap<String, String>) -> Option<String> {
    if let Some(h) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(t) = h.strip_prefix("Bearer ")
    {
        return Some(t.trim().to_string());
    }
    query.get("t").cloned()
}

pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get("cookie")?.to_str().ok()?;
    for part in raw.split(';') {
        let mut kv = part.trim().splitn(2, '=');
        if kv.next() == Some(name) {
            return Some(crate::util::decode_uri_component(kv.next().unwrap_or("")));
        }
    }
    None
}

pub const ADMIN_COOKIE: &str = "nostalgia_admin";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_hashed_by_node_verify() {
        // Generated with Node.js: crypto.scryptSync('password1', salt, 32, { N: 16384, r: 8, p: 1 })
        let node = "scrypt$63I+8ewBaHjXBqKAk7ohQg==$g+Et0vNylhvdEMXv9ueq2986fMJ7QGRW52eJmU7/I+4=";
        assert!(verify_password("password1", node));
        assert!(!verify_password("password2", node));
        let h = hash_password("secret1");
        assert!(verify_password("secret1", &h));
        assert!(!verify_password("secret2", &h));
        assert!(!verify_password("x", "plain"));
    }
}
