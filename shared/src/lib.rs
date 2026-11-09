//! Types and helpers shared by the Nostalgia server and client.
//! Port of the TypeScript `@nostalgia/shared` package; both implementations speak the same protocol.

pub mod collate;
pub mod emu;
pub mod platforms;
pub mod types;

pub use collate::{locale_compare, natural_compare};
pub use emu::*;
pub use platforms::*;
pub use types::*;

pub const API_VERSION: u32 = 1;
pub const SAVE_CONTENT_TYPE: &str = "application/zip";

/// `new Date().toISOString()`: UTC with millisecond precision and a `Z`.
pub fn iso_now() -> String {
    iso(std::time::SystemTime::now())
}

pub fn iso(t: std::time::SystemTime) -> String {
    let ms = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    iso_from_millis(ms)
}

pub fn iso_from_millis(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Milliseconds since the epoch for an ISO 8601 / RFC 3339 string (`Date.parse`), None if invalid.
pub fn parse_iso_millis(s: &str) -> Option<i64> {
    let b = s.trim();
    if b.len() < 10 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| b.get(r).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (mut h, mut mi, mut sec, mut ms, mut offset) = (0, 0, 0, 0i64, 0i64);
    if b.len() > 10 {
        h = num(11..13)?;
        mi = num(14..16)?;
        let rest = &b[16..];
        let mut rest = rest;
        if let Some(r) = rest.strip_prefix(':') {
            sec = r.get(0..2)?.parse().ok()?;
            rest = &r[2..];
            if let Some(r) = rest.strip_prefix('.') {
                let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
                let frac = format!("{:0<3}", &digits[..digits.len().min(3)]);
                ms = frac.parse().ok()?;
                rest = &r[digits.len()..];
            }
        }
        if rest == "Z" || rest.is_empty() {
        } else {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let r = rest.get(1..)?.replace(':', "");
            let oh: i64 = r.get(0..2)?.parse().ok()?;
            let om: i64 = r.get(2..4).and_then(|x| x.parse().ok()).unwrap_or(0);
            offset = sign * (oh * 60 + om);
        }
    }
    let days = days_from_civil(y, mo, d);
    Some(((days * 86_400 + h * 3600 + mi * 60 + sec) - offset * 60) * 1000 + ms)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates_round_trip() {
        assert_eq!(iso_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_from_millis(1_794_168_249_123), "2026-11-08T20:04:09.123Z");
        assert_eq!(parse_iso_millis("2026-11-08T20:04:09.123Z"), Some(1_794_168_249_123));
        assert_eq!(parse_iso_millis("2026-11-08 20:04:09Z"), Some(1_794_168_249_000));
        assert_eq!(parse_iso_millis("2026-11-08T21:04:09.123+01:00"), Some(1_794_168_249_123));
        assert_eq!(parse_iso_millis("nonsense"), None);
    }
}
