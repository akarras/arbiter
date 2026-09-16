//! Arbiter desktop app: Tauri shell over the `arbiter` library.

pub mod commands;
pub mod watch;

use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub roots: Mutex<Vec<PathBuf>>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

pub fn run(initial_roots: Vec<PathBuf>) {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { roots: Mutex::new(initial_roots), watcher: Mutex::new(None) })
        .invoke_handler(tauri::generate_handler![
            commands::list_replays,
            commands::chart_html,
            commands::roots,
            commands::add_root
        ])
        .setup(|app| {
            watch::install(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Arbiter");
}
