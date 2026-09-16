//! Persists the user's replay-folder roots across runs.
//!
//! Stored as a JSON array of path strings at
//! `<app_config_dir>/roots.json` — on Windows that resolves to
//! `%APPDATA%/com.arbiter.desktop/roots.json`. The file holds the full root
//! list (not just user-added folders); on load it is merged with the
//! `--dir` arguments and the auto-detected default folders, so removing the
//! file just falls back to auto-detection.

use std::path::PathBuf;

use tauri::{AppHandle, Manager as _};

const FILE_NAME: &str = "roots.json";

/// Loads the persisted roots, or an empty list if the config directory is
/// unavailable, the file is missing or unreadable, or its contents are not
/// valid JSON.
pub fn load(app: &AppHandle) -> Vec<PathBuf> {
    let Ok(dir) = app.path().app_config_dir() else {
        return Vec::new();
    };
    match std::fs::read(dir.join(FILE_NAME)) {
        Ok(bytes) => parse(&bytes),
        Err(_) => Vec::new(),
    }
}

/// Persists `roots` as a JSON array of path strings, creating the app
/// config directory if it does not already exist.
pub fn save(app: &AppHandle, roots: &[PathBuf]) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(FILE_NAME), serialize(roots)).map_err(|e| e.to_string())
}

/// Parses a JSON array of path strings. Malformed JSON (or anything that is
/// not an array of strings) yields an empty list rather than an error, so a
/// corrupt config file degrades to auto-detected roots instead of blocking
/// startup.
fn parse(bytes: &[u8]) -> Vec<PathBuf> {
    serde_json::from_slice::<Vec<String>>(bytes).unwrap_or_default().into_iter().map(PathBuf::from).collect()
}

/// Serializes `roots` as a JSON array of path strings.
fn serialize(roots: &[PathBuf]) -> String {
    let strs: Vec<String> = roots.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    serde_json::to_string(&strs).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let roots = vec![PathBuf::from(r"C:\r\a"), PathBuf::from(r"C:\r\b")];
        let json = serialize(&roots);
        assert_eq!(parse(json.as_bytes()), roots);
    }

    #[test]
    fn malformed_json_yields_empty() {
        assert!(parse(b"not json").is_empty());
        assert!(parse(b"").is_empty());
        assert!(parse(br#"{"not":"an array"}"#).is_empty());
    }
}
