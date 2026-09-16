//! Arbiter desktop app: Tauri shell over the `arbiter` library.

pub mod commands;
pub mod config;
pub mod watch;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager as _;

pub struct AppState {
    pub roots: Mutex<Vec<PathBuf>>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

/// Parses `--dir <path>` pairs out of `args` (typically
/// `std::env::args().skip(1)` collected into a `Vec`). A trailing `--dir`
/// with no following value, and any other argument, is ignored.
pub fn parse_dirs(args: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--dir"
            && let Some(d) = it.next()
        {
            dirs.push(PathBuf::from(d));
        }
    }
    dirs
}

/// `initial_roots` are the folders passed on the command line via `--dir`
/// (possibly empty). The full root list used at runtime is computed once
/// `setup` has an `AppHandle`: `initial_roots` merged with the persisted
/// config (`config::load`) and the auto-detected default folders
/// (`scan::default_roots`), de-duplicated with the same containment rule as
/// adding a folder from the UI.
pub fn run(initial_roots: Vec<PathBuf>) {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { roots: Mutex::new(Vec::new()), watcher: Mutex::new(None) })
        .invoke_handler(tauri::generate_handler![
            commands::list_replays,
            commands::chart_html,
            commands::roots,
            commands::add_root
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let candidates =
                initial_roots.into_iter().chain(config::load(&handle)).chain(arbiter::scan::default_roots());
            let roots = commands::merge_roots(candidates);
            *app.state::<AppState>().roots.lock().expect("roots lock") = roots;
            watch::install(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Arbiter");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dirs_reads_dir_pairs_and_ignores_the_rest() {
        let args: Vec<String> = ["--dir", "a", "--dir", "b"].iter().map(|s| s.to_string()).collect();
        assert_eq!(parse_dirs(&args), vec![PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[test]
    fn parse_dirs_ignores_a_trailing_flag_with_no_value() {
        let args: Vec<String> = ["--dir"].iter().map(|s| s.to_string()).collect();
        assert!(parse_dirs(&args).is_empty());
    }

    #[test]
    fn parse_dirs_ignores_unknown_arguments() {
        let args: Vec<String> = ["--foo", "bar", "baz"].iter().map(|s| s.to_string()).collect();
        assert!(parse_dirs(&args).is_empty());
    }
}
