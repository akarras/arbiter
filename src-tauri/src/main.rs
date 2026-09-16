#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dirs = arbiter_app::parse_dirs(&args);
    arbiter_app::run(dirs);
}
