use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn iso_now() -> String {
    nostalgia_shared::iso_now()
}

/// Lexical normalisation (like `path.resolve` without touching the disk).
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

/// `path.resolve(base, rel)`.
pub fn resolve(base: &Path, rel: &str) -> PathBuf {
    let rel = if cfg!(windows) { rel.replace('/', "\\") } else { rel.to_string() };
    normalize(&base.join(rel))
}

/// `base/<rel with forward slashes>`, refusing anything that leaves `base`.
pub fn safe_join(base: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let target = normalize(&rel.split('/').fold(base.to_path_buf(), |p, s| p.join(s)));
    let b = normalize(base);
    if target == b || !target.starts_with(&b) {
        anyhow::bail!("Refusing unsafe path {rel}");
    }
    Ok(target)
}

pub fn is_inside(child: &Path, parent: &Path) -> bool {
    let c = normalize(child);
    let p = normalize(parent);
    c != p && c.starts_with(&p)
}

pub fn fmt_bytes(n: u64) -> String {
    if n == 0 {
        return "0 B".into();
    }
    let u = ["B", "KB", "MB", "GB", "TB"];
    let i = ((n as f64).ln() / 1024f64.ln()).floor().clamp(0.0, 4.0) as usize;
    let v = n as f64 / 1024f64.powi(i as i32);
    if i > 1 { format!("{v:.1} {}", u[i]) } else { format!("{v:.0} {}", u[i]) }
}

pub fn fmt_duration(s: i64) -> String {
    let s = s.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min")
    } else if s > 0 {
        format!("{s} s")
    } else {
        "0 min".into()
    }
}

pub fn fmt_clock(s: i64) -> String {
    let s = s.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 { format!("{h}:{m:02}:{sec:02}") } else { format!("{m}:{sec:02}") }
}

/// "12 min ago", "3 days ago", ...
pub fn fmt_relative(iso: Option<&str>) -> String {
    let Some(ms) = iso.and_then(nostalgia_shared::parse_iso_millis) else { return "never".into() };
    let diff = (now_ms() - ms) / 1000;
    if diff < 60 {
        "just now".into()
    } else if diff < 3600 {
        format!("{} min ago", diff / 60)
    } else if diff < 86400 {
        format!("{} h ago", diff / 3600)
    } else if diff < 86400 * 2 {
        "1 day ago".into()
    } else if diff < 86400 * 30 {
        format!("{} days ago", diff / 86400)
    } else {
        fmt_date_only(iso)
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn parts(iso: &str) -> Option<(i64, usize, i64, String)> {
    let ms = nostalgia_shared::parse_iso_millis(iso)?;
    let local = ms + local_offset_ms();
    let s = nostalgia_shared::iso_from_millis(local);
    Some((s[0..4].parse().ok()?, s[5..7].parse::<usize>().ok()?, s[8..10].parse().ok()?, s[11..16].to_string()))
}

pub fn fmt_date(iso: Option<&str>) -> String {
    match iso.and_then(parts) {
        Some((y, m, d, hm)) => format!("{} {d}, {y}, {hm}", MONTHS[m - 1]),
        None => "–".into(),
    }
}

pub fn fmt_date_only(iso: Option<&str>) -> String {
    match iso.and_then(parts) {
        Some((y, m, d, _)) => format!("{} {d}, {y}", MONTHS[m - 1]),
        None => "–".into(),
    }
}

pub fn fmt_long_date(iso: &str) -> String {
    match parts(iso) {
        Some((y, m, d, _)) => format!("{} {d}, {y}", MONTHS_LONG[m - 1]),
        None => iso.to_string(),
    }
}

/// Local UTC offset (no time zone database; uses the C library on each platform).
pub fn local_offset_ms() -> i64 {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime([u16; 8]);
        #[repr(C)]
        struct TimeZoneInformation {
            bias: i32,
            standard_name: [u16; 32],
            standard_date: SystemTime,
            standard_bias: i32,
            daylight_name: [u16; 32],
            daylight_date: SystemTime,
            daylight_bias: i32,
        }
        unsafe extern "system" {
            fn GetTimeZoneInformation(info: *mut TimeZoneInformation) -> u32;
        }
        let mut info: TimeZoneInformation = unsafe { std::mem::zeroed() };
        let r = unsafe { GetTimeZoneInformation(&mut info) };
        let bias = match r {
            2 => info.bias + info.daylight_bias,
            1 | 0 => info.bias + info.standard_bias,
            _ => 0,
        };
        -(bias as i64) * 60_000
    }
    #[cfg(not(windows))]
    {
        // struct tm: 9 ints, then long tm_gmtoff (glibc / musl on 64-bit Linux).
        #[repr(C)]
        struct Tm {
            ints: [i32; 9],
            gmtoff: i64,
            zone: *const i8,
        }
        unsafe extern "C" {
            fn time(t: *mut i64) -> i64;
            fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
        }
        let mut now = 0i64;
        unsafe { time(&mut now) };
        let mut tm: Tm = unsafe { std::mem::zeroed() };
        if unsafe { localtime_r(&now, &mut tm) }.is_null() { 0 } else { tm.gmtoff * 1000 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_the_electron_ui() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(2_791_728_742), "2.6 GB");
        assert_eq!(fmt_bytes(1536), "2 KB");
        assert_eq!(fmt_duration(125), "2 min");
        assert_eq!(fmt_duration(3 * 3600 + 120), "3 h 2 min");
        assert_eq!(fmt_clock(65), "1:05");
        assert_eq!(fmt_clock(3725), "1:02:05");
        let base = Path::new("C:/g");
        assert!(safe_join(base, "../x").is_err());
        assert!(safe_join(base, "a/b").is_ok());
    }
}
