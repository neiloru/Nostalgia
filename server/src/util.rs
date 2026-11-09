//! Small helpers that reproduce JavaScript / Node.js behaviour the protocol depends on.

use crate::error::HttpError;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

/// `String(value ?? '')` for a JSON body field.
pub fn js_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => js_number_string(n),
        Some(Value::Array(a)) => a.iter().map(|x| js_str(Some(x))).collect::<Vec<_>>().join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

fn js_number_string(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    let f = n.as_f64().unwrap_or(f64::NAN);
    if f.fract() == 0.0 && f.abs() < 1e21 { format!("{}", f as i128) } else { f.to_string() }
}

/// `Number(value)` (NaN as None).
pub fn js_num(v: Option<&Value>) -> Option<f64> {
    match v {
        None => None,
        Some(Value::Null) => Some(0.0),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() { Some(0.0) } else { t.parse::<f64>().ok() }
        }
        _ => None,
    }
}

/// JavaScript truthiness of a JSON value.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        _ => true,
    }
}

/// A route parameter that must be a positive integer (`Number(x)` semantics).
pub fn id_param(raw: &str, name: &str) -> Result<i64, HttpError> {
    let n = if raw.trim().is_empty() { Some(0.0) } else { raw.trim().parse::<f64>().ok() };
    match n {
        Some(f) if f.fract() == 0.0 && f > 0.0 && f < 9.0e15 => Ok(f as i64),
        _ => Err(HttpError::new(400, format!("Invalid {name}"))),
    }
}

/// JavaScript `.slice(0, n)` on a string (counts UTF-16 code units, never splits a character).
pub fn js_slice(s: &str, n: usize) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in s.chars() {
        units += c.len_utf16();
        if units > n {
            break;
        }
        out.push(c);
    }
    out
}

/// Lexically normalise a path like `path.resolve` (no file system access).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                let at_root = matches!(out.components().next_back(), None | Some(Component::RootDir) | Some(Component::Prefix(_)));
                if !at_root {
                    out.pop();
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path.resolve(base, rel)`: on Windows both `/` and `\` separate, an absolute `rel` wins.
pub fn resolve(base: &Path, rel: &str) -> PathBuf {
    let rel_path = if cfg!(windows) { PathBuf::from(rel.replace('/', "\\")) } else { PathBuf::from(rel) };
    normalize(&base.join(rel_path))
}

/// Resolve `rel` below `base`, refusing path traversal.
pub fn resolve_inside(base: &Path, rel: &str) -> Result<PathBuf, HttpError> {
    let b = normalize(base);
    let target = resolve(&b, rel);
    if target != b && !target.starts_with(&b) {
        return Err(HttpError::new(400, "Invalid path"));
    }
    // `starts_with` compares components; also reject things like "C:" relative drive tricks.
    if target != b && target.strip_prefix(&b).map(|r| r.as_os_str().is_empty()).unwrap_or(true) {
        return Err(HttpError::new(400, "Invalid path"));
    }
    Ok(target)
}

/// `Math.floor(stat.mtimeMs)`
pub fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| match t.duration_since(UNIX_EPOCH) {
            Ok(d) => Some(d.as_millis() as i64),
            Err(e) => Some(-(e.duration().as_millis() as i64) - 1),
        })
        .unwrap_or(0)
}

/// `encodeURIComponent`
pub fn encode_uri_component(s: &str) -> String {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    const SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');
    utf8_percent_encode(s, SET).to_string()
}

/// `decodeURIComponent` (lenient: invalid sequences are kept).
pub fn decode_uri_component(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()
}

/// ISO timestamp without separators and fraction: `20261108T200409`.
pub fn file_stamp() -> String {
    let iso = nostalgia_shared::iso_now();
    let s: String = iso.chars().filter(|c| *c != '-' && *c != ':').collect();
    s.split('.').next().unwrap_or(&s).to_string()
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_inside_refuses_traversal() {
        let base = std::env::temp_dir().join("lib");
        assert!(resolve_inside(&base, "../../../settings.yaml").is_err());
        assert!(resolve_inside(&base, "a/../../x").is_err());
        assert_eq!(resolve_inside(&base, "a/b.bin").unwrap(), normalize(&base.join("a").join("b.bin")));
        assert_eq!(resolve_inside(&base, "").unwrap(), normalize(&base));
        #[cfg(windows)]
        assert!(resolve_inside(&base, "C:\\Windows\\win.ini").is_err());
    }

    #[test]
    fn js_coercions() {
        assert_eq!(id_param("12", "id").unwrap(), 12);
        assert!(id_param("12abc", "id").is_err());
        assert!(id_param("0", "id").is_err());
        assert_eq!(encode_uri_component("Test (Disc 1).bin"), "Test%20(Disc%201).bin");
        assert_eq!(js_slice("abc", 2), "ab");
    }
}
