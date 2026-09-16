#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() {
    let mut dirs = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--dir"
            && let Some(d) = args.next()
        {
            dirs.push(PathBuf::from(d));
        }
    }
    let roots = if dirs.is_empty() { arbiter::scan::default_roots() } else { dirs };
    arbiter_app::run(roots);
}
