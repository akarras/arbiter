//! Commands the page can invoke. Thin wrappers over pure functions.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use notify::{RecursiveMode, Watcher as _};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

use crate::{config, AppState};

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

/// Strips a Windows extended-length prefix (`\\?\`) from `path`'s display
/// form. `PathBuf::canonicalize` adds this prefix on Windows; it is faithful
/// but ugly to show a user, so the canonical form is kept in state and only
/// stripped at the point of display.
pub fn display_path(path: &Path) -> String {
    let s = path.display().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// Adds a folder to `roots` (canonicalised), applying containment
/// de-duplication so nested folders never double-count the same replays:
///
/// - If `path` is already inside (or equal to) a root in `roots`, that
///   existing root is returned unchanged and nothing is added.
/// - If any roots in `roots` are inside `path`, they are removed and
///   replaced by `path`, since `path` now covers them.
///
/// Errors for anything that is not an existing directory.
pub fn add_root_to(roots: &mut Vec<PathBuf>, path: &Path) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!("not a folder: {}", path.display()));
    }
    let canon = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;

    if arbiter::scan::is_within(roots, &canon).is_some() {
        let covering = roots.iter().find(|r| canon.starts_with(r)).cloned().unwrap_or_else(|| canon.clone());
        return Ok(covering);
    }

    roots.retain(|r| arbiter::scan::is_within(std::slice::from_ref(&canon), r).is_none());
    roots.push(canon.clone());
    Ok(canon)
}

/// Merges every candidate folder into a fresh root list, applying the same
/// containment rule as [`add_root_to`]. Candidates that are not (or are no
/// longer) real directories are silently skipped, so a stale persisted
/// entry or a `--dir` argument for a folder that has since been removed
/// does not stop startup.
pub fn merge_roots<I: IntoIterator<Item = PathBuf>>(candidates: I) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for candidate in candidates {
        let _ = add_root_to(&mut roots, &candidate);
    }
    roots
}

#[tauri::command(async)]
pub async fn list_replays(state: State<'_, AppState>) -> Result<Vec<ReplayRow>, String> {
    let roots = state.roots.lock().expect("roots lock").clone();
    Ok(list_rows(&roots))
}

#[tauri::command(async)]
pub async fn chart_html(path: String) -> Result<String, String> {
    arbiter::pipeline::chart_html(Path::new(&path)).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn roots(state: State<'_, AppState>) -> Vec<String> {
    state.roots.lock().expect("roots lock").iter().map(|r| display_path(r)).collect()
}

/// Validates and canonicalises `path` first; only once a watcher is
/// confirmed to accept it (if a watcher exists) does this commit the change
/// to `state.roots`, so a failure never leaves `state` mutated. The updated
/// list is persisted to disk before returning.
#[tauri::command]
pub fn add_root(app: AppHandle, path: String) -> Result<Vec<String>, String> {
    let state = app.state::<AppState>();
    let original = state.roots.lock().expect("roots lock").clone();
    let mut scratch = original.clone();
    let canon = add_root_to(&mut scratch, Path::new(&path))?;

    if scratch != original {
        if let Some(w) = state.watcher.lock().expect("watcher lock").as_mut() {
            w.watch(&canon, RecursiveMode::Recursive).map_err(|e| format!("could not watch {}: {e}", canon.display()))?;
        }
        *state.roots.lock().expect("roots lock") = scratch.clone();
        config::save(&app, &scratch)?;
    }

    Ok(scratch.iter().map(|r| display_path(r)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.txt");
        fs::write(&file, b"x").unwrap();
        let mut roots = Vec::new();
        assert!(add_root_to(&mut roots, &file).is_err());
        assert!(add_root_to(&mut roots, Path::new("Z:/nope")).is_err());
        add_root_to(&mut roots, &dir).unwrap();
        add_root_to(&mut roots, &dir).unwrap();
        assert_eq!(roots.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_root_to_applies_containment_in_both_directions() {
        let dir = std::env::temp_dir().join(format!("arbiter-app-contain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();

        // Adding a subfolder of an already-tracked root keeps just the root.
        let mut roots = Vec::new();
        let top = add_root_to(&mut roots, &dir).unwrap();
        assert_eq!(roots, vec![top.clone()]);
        let returned = add_root_to(&mut roots, &dir.join("sub")).unwrap();
        assert_eq!(returned, top, "should hand back the existing covering root");
        assert_eq!(roots, vec![top]);

        // Adding a folder that contains an already-tracked (nested) root
        // replaces that root with the broader folder.
        let mut roots2 = Vec::new();
        add_root_to(&mut roots2, &dir.join("sub")).unwrap();
        assert_eq!(roots2.len(), 1);
        let replaced = add_root_to(&mut roots2, &dir).unwrap();
        assert_eq!(roots2, vec![replaced]);
        assert_eq!(roots2.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_roots_dedupes_and_applies_containment() {
        let dir = std::env::temp_dir().join(format!("arbiter-app-merge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();

        let merged = merge_roots(vec![
            dir.join("sub"),
            dir.clone(),
            PathBuf::from("Z:/does/not/exist"),
        ]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].ends_with(dir.file_name().unwrap()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn display_path_strips_windows_extended_prefix() {
        assert_eq!(display_path(Path::new(r"\\?\C:\r\a")), r"C:\r\a");
        assert_eq!(display_path(Path::new(r"C:\r\a")), r"C:\r\a");
    }
}
