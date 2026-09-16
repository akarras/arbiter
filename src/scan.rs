//! Finds StarCraft II replay files on disk.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayEntry {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub size: u64,
}

/// Every `Accounts/*/*/Replays` directory under the user's StarCraft II
/// document folders (`Documents` and any `OneDrive*/Documents*`).
pub fn default_roots() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let mut sc2_dirs = vec![home.join("Documents").join("StarCraft II")];
    for onedrive in read_dirs(&home).into_iter().filter(|p| name_starts_with(p, "OneDrive")) {
        for docs in read_dirs(&onedrive).into_iter().filter(|p| name_starts_with(p, "Documents")) {
            sc2_dirs.push(docs.join("StarCraft II"));
        }
    }
    let mut roots = Vec::new();
    for sc2 in sc2_dirs {
        for account in read_dirs(&sc2.join("Accounts")) {
            for toon in read_dirs(&account) {
                let replays = toon.join("Replays");
                if replays.is_dir() {
                    roots.push(replays);
                }
            }
        }
    }
    roots
}

/// Recursion limit for `walk`, deep enough for any real replay folder layout.
const MAX_WALK_DEPTH: u32 = 32;

/// All `.SC2Replay` files under the roots, newest first.
pub fn find_replays(roots: &[PathBuf]) -> Vec<ReplayEntry> {
    let mut out = Vec::new();
    for root in roots {
        walk(root, 0, &mut out);
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.modified));
    out
}

/// The canonical path if `path` is inside one of the roots, else `None`.
pub fn is_within(roots: &[PathBuf], path: &Path) -> Option<PathBuf> {
    let canon = path.canonicalize().ok()?;
    let inside = roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|r| canon.starts_with(&r));
    inside.then_some(canon)
}

/// Recursively collects replay files under `dir`. `depth` is the number of
/// directory levels already descended from the root passed to
/// `find_replays`; once it reaches `MAX_WALK_DEPTH`, subdirectories are not
/// descended into. This guards against a symlink or junction loop under a
/// replay root (e.g. a directory junction pointing back at an ancestor)
/// recursing without bound and overflowing the stack, which would abort the
/// whole process — including a long-running `arbiter serve`.
fn walk(dir: &Path, depth: u32, out: &mut Vec<ReplayEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_WALK_DEPTH {
                walk(&path, depth + 1, out);
            }
            continue;
        }
        let is_replay = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("SC2Replay"));
        if !is_replay {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        out.push(ReplayEntry { path, modified, size: meta.len() });
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

fn name_starts_with(path: &Path, prefix: &str) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempTree(PathBuf);
    impl TempTree {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("arbiter-scan-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("a").join("sub")).unwrap();
            fs::create_dir_all(dir.join("b")).unwrap();
            Self(dir)
        }
        fn touch(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::write(&p, b"x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
            p
        }
    }
    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_replays_recursively_case_insensitively_newest_first() {
        let t = TempTree::new("find");
        let old = t.touch("a/old.SC2Replay");
        t.touch("a/notes.txt");
        let mid = t.touch("a/sub/mid.sc2replay");
        let new = t.touch("a/new.SC2Replay");
        let found: Vec<PathBuf> = find_replays(&[t.0.join("a")]).into_iter().map(|e| e.path).collect();
        assert_eq!(found, vec![new, mid, old]);
    }

    #[test]
    fn missing_root_yields_nothing() {
        assert!(find_replays(&[PathBuf::from("Z:/definitely/not/here")]).is_empty());
    }

    #[test]
    fn depth_guard_stops_a_runaway_nesting_but_not_a_normal_one() {
        let root = std::env::temp_dir().join(format!("arbiter-scan-depth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for i in 0..34 {
            deep = deep.join(format!("d{i}"));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("too-deep.SC2Replay"), b"x").unwrap();

        let shallow_dir = root.join("d0").join("d1").join("d2");
        fs::write(shallow_dir.join("shallow.SC2Replay"), b"x").unwrap();

        let found: Vec<PathBuf> = find_replays(std::slice::from_ref(&root)).into_iter().map(|e| e.path).collect();
        assert!(found.iter().any(|p| p.ends_with("shallow.SC2Replay")), "{found:?}");
        assert!(!found.iter().any(|p| p.ends_with("too-deep.SC2Replay")), "{found:?}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn is_within_accepts_only_paths_under_a_root() {
        let t = TempTree::new("within");
        let inside = t.touch("a/sub/x.SC2Replay");
        let outside = t.touch("b/y.SC2Replay");
        let roots = vec![t.0.join("a")];
        assert!(is_within(&roots, &inside).is_some());
        assert!(is_within(&roots, &outside).is_none());
        let dotdot = t.0.join("a").join("..").join("b").join("y.SC2Replay");
        assert!(is_within(&roots, &dotdot).is_none());
        assert!(is_within(&roots, Path::new("nope.SC2Replay")).is_none());
    }
}
