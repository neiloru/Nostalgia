//! `encodeURIComponent`
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

const SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

pub fn encode(s: &str) -> String {
    utf8_percent_encode(s, SET).to_string()
}
