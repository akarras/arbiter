//! Minimal percent-encoding for URL query values.

use std::fmt::Write as _;

pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Decodes `%XX` and `+`; `None` on a bad escape or invalid UTF-8.
pub fn decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3)?;
                let v = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                out.push(v);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_everything_but_unreserved() {
        assert_eq!(encode("C:/a b/x(1).SC2Replay"), "C%3A%2Fa%20b%2Fx%281%29.SC2Replay");
        assert_eq!(encode("çñ"), "%C3%A7%C3%B1");
    }

    #[test]
    fn decodes_percent_and_plus() {
        assert_eq!(decode("C%3A%2Fa%20b+c").unwrap(), "C:/a b c");
        assert_eq!(decode("%C3%A7").unwrap(), "ç");
        assert!(decode("%zz").is_none());
        assert!(decode("%C3").is_none(), "invalid utf-8");
    }
}
