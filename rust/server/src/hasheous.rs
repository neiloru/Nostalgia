//! File hashing and hasheous.org lookups (port of hasheous.ts).

use md5::Md5;
use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::Digest;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

const API: &str = "https://hasheous.org/api/v1/Lookup/ByHash";

#[derive(Debug, Clone)]
pub struct FileHashes {
    pub md5: String,
    pub sha1: String,
    pub crc32: String,
}

/// Hash a file with md5, sha1 and crc32 in a single streaming pass (blocking).
pub fn hash_file(file: &Path, mut on_progress: impl FnMut(u64)) -> std::io::Result<FileHashes> {
    let mut f = std::fs::File::open(file)?;
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut crc = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        md5.update(&buf[..n]);
        sha1.update(&buf[..n]);
        crc.update(&buf[..n]);
        on_progress(n as u64);
    }
    Ok(FileHashes { md5: hex::encode(md5.finalize()), sha1: hex::encode(sha1.finalize()), crc32: format!("{:08x}", crc.finalize()) })
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HasheousResult {
    /// match | nomatch | error
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub igdb_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rom_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

impl HasheousResult {
    fn status(s: &str) -> Self {
        HasheousResult { status: s.into(), ..Default::default() }
    }
}

async fn query(client: &reqwest::Client, body: Value, timeout: Duration) -> Result<HasheousResult, reqwest::Error> {
    let res = client.post(API).header("Accept", "application/json").json(&body).timeout(timeout).send().await?;
    if res.status().as_u16() == 404 {
        return Ok(HasheousResult::status("nomatch"));
    }
    if !res.status().is_success() {
        return Ok(HasheousResult { error: Some(format!("HTTP {}", res.status().as_u16())), ..HasheousResult::status("error") });
    }
    let j: Value = res.json().await?;
    let s = |v: Option<&Value>| v.and_then(|x| x.as_str()).map(String::from);
    let igdb = j.get("metadata").and_then(|m| m.as_array()).and_then(|a| {
        a.iter().find(|m| m.get("source").and_then(|s| s.as_str()) == Some("IGDB") && m.get("id").is_some_and(|i| !i.is_null() && i != ""))
    });
    let igdb_id = igdb.and_then(|m| m.get("id")).and_then(|v| match v {
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    });
    let rom = j.get("signature").and_then(|s| s.get("rom"));
    Ok(HasheousResult {
        status: "match".into(),
        name: s(j.get("name")),
        platform: s(j.get("platform").and_then(|p| p.get("name"))),
        igdb_id: igdb_id.filter(|f| f.is_finite()).map(|f| f as i64),
        rom_name: s(rom.and_then(|r| r.get("name"))),
        source: s(rom.and_then(|r| r.get("signatureSource"))),
        error: None,
        raw: Some(json!({
            "name": j.get("name"),
            "platform": j.get("platform").and_then(|p| p.get("name")),
            "rom": rom,
            "metadata": j.get("metadata"),
        })),
    })
}

/// Look a file up by hash. MD5 alone is answered quickly by hasheous.org, combined
/// hash queries can be very slow - so MD5 first and SHA1 only as a fallback.
pub async fn lookup_hasheous(client: &reqwest::Client, h: &FileHashes) -> HasheousResult {
    let t = Duration::from_secs(120);
    match query(client, json!({ "md5": h.md5 }), t).await {
        Ok(first) if first.status != "nomatch" => first,
        Ok(first) => query(client, json!({ "sha1": h.sha1 }), t).await.unwrap_or(first),
        Err(e) => {
            crate::log::warn(format!("Hasheous lookup failed: {e}"));
            HasheousResult { error: Some(e.to_string()), ..HasheousResult::status("error") }
        }
    }
}

static SKIP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(cue|m3u|txt|nfo|jpg|png|json|yaml|yml|sfv|md5|dat)$").unwrap());

/// Files that are worth hashing (skip cue sheets, text, our own asset folder).
pub fn should_hash(rel: &str) -> bool {
    !rel.starts_with("igdb/") && !SKIP_RE.is_match(rel)
}
