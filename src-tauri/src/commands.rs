//! Commands the page can invoke. Thin wrappers over pure functions.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use notify::{RecursiveMode, Watcher as _};
use serde::Serialize;
use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReplayRow {
    pub path: String,
    pub name: String,
    pub modified_secs: u64,
    pub size: u64,
}

impl From<&arbiter::scan::ReplayEntry> for ReplayRow {
    fn from(e: &arbiter::scan::ReplayEntry) -> Self {
        Self {
            path: e.path.to_string_lossy().into_owned(),
            name: e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            modified_secs: e.modified.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            size: e.size,
        }
    }
}

pub fn list_rows(roots: &[PathBuf]) -> Vec<ReplayRow> {
    arbiter::scan::find_replays(roots).iter().map(ReplayRow::from).collect()
}

/// Adds a folder to the roots (canonicalised, deduplicated). Errors for
/// anything that is not an existing directory.
pub fn add_root_to(roots: &mut Vec<PathBuf>, path: &Path) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!("not a folder: {}", path.display()));
    }
    let canon = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
    if !roots.iter().any(|r| r.canonicalize().ok().as_ref() == Some(&canon)) {
        roots.push(canon.clone());
    }
    Ok(canon)
}

#[tauri::command]
pub fn list_replays(state: State<'_, AppState>) -> Vec<ReplayRow> {
    list_rows(&state.roots.lock().expect("roots lock"))
}

#[tauri::command]
pub fn chart_html(path: String) -> Result<String, String> {
    arbiter::pipeline::chart_html(Path::new(&path)).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn roots(state: State<'_, AppState>) -> Vec<String> {
    state.roots.lock().expect("roots lock").iter().map(|r| r.display().to_string()).collect()
}

#[tauri::command]
pub fn add_root(state: State<'_, AppState>, path: String) -> Result<Vec<String>, String> {
    let canon = add_root_to(&mut state.roots.lock().expect("roots lock"), Path::new(&path))?;
    if let Some(w) = state.watcher.lock().expect("watcher lock").as_mut() {
        w.watch(&canon, RecursiveMode::Recursive).map_err(|e| format!("could not watch {}: {e}", canon.display()))?;
    }
    Ok(roots(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn row_carries_name_seconds_and_size() {
        let e = arbiter::scan::ReplayEntry {
            path: PathBuf::from(r"C:\r\Tuonela LE (1).SC2Replay"),
            modified: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            size: 227_146,
        };
        let row = ReplayRow::from(&e);
        assert_eq!(row.name, "Tuonela LE (1).SC2Replay");
        assert_eq!(row.modified_secs, 1_700_000_000);
        assert_eq!(row.size, 227_146);
        assert!(row.path.ends_with("Tuonela LE (1).SC2Replay"));
    }

    #[test]
    fn add_root_rejects_files_and_deduplicates() {
        let dir = std::env::temp_dir().join(format!("arbiter-app-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.txt");
        std::fs::write(&file, b"x").unwrap();
        let mut roots = Vec::new();
        assert!(add_root_to(&mut roots, &file).is_err());
        assert!(add_root_to(&mut roots, Path::new("Z:/nope")).is_err());
        add_root_to(&mut roots, &dir).unwrap();
        add_root_to(&mut roots, &dir).unwrap();
        assert_eq!(roots.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
