# Serve Mode and Game APM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show Blizzard's own APM beside Arbiter's in the chart legend, and add `arbiter serve`, a localhost web server that lists the user's replays and charts any of them on click or from an uploaded file.

**Architecture:** The v1 pipeline (replay → apm → chart) is unchanged in shape. The metadata reader lives in `replay.rs`. The chart-building orchestration moves out of `main.rs` into `pipeline.rs` so both the CLI and the server call it. Shared CSS moves to `theme.rs`. New modules: `scan.rs` (replay folder discovery), `percent.rs` (URL encoding), `list_page.rs` (HTML list), `serve.rs` (pure request handler plus a `tiny_http` loop).

**Tech Stack:** Rust 2024, `s2protocol` 3.5 (default features off), `anyhow`, `serde_json` 1, `tiny_http` 0.12.

**Spec:** `docs/superpowers/specs/2026-09-16-serve-and-game-apm-design.md`.

## Global Constraints

- Dependencies exactly: `anyhow = "1"`, `s2protocol = { version = "3.5", default-features = false, features = ["tracing_off"] }`, `serde_json = "1"`, `tiny_http = "0.12"`. No dev-dependencies.
- The server binds `127.0.0.1` only. `/replay` serves only files inside a scanned root (canonicalised prefix check). Uploads are capped at 64 MiB.
- All user-derived text in HTML is escaped with `chart::escape_html`. Link paths are percent-encoded.
- Legend text with game APM: `{race} · {result} · {avg:.0} APM · game says {game:.0}`; without it, the v1 text.
- Game metadata never fails a load: missing file or bad JSON leaves the `Option`s `None`.
- Time base 22.4 loops/s, window 60 s, step 5 s (unchanged).
- Exit codes unchanged: 0 / 1 / 2. `serve` startup failures (no roots, port busy) are exit 1 with one `error:` line.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Test and clippy output pristine. Shell note: the Bash tool rejects very long commands; write files with the Write/Edit tools.
- Fixture (not committed): the local fixture replay (set `ARBITER_FIXTURE` to its path). Tests that need it skip when it is absent.
- Crate facts (verified): `nom_mpq::MPQ::read_mpq_file_sector(&self, filename: &str, force_decompress: bool, orig_input: &[u8]) -> MPQResult<&[u8], Vec<u8>>` is nom-style: `Ok((rest, bytes))`. `tiny_http::Server::http(addr)`, `server.incoming_requests()`, `request.method()` implements `Display`, `request.url() -> &str`, `request.body_length() -> Option<usize>`, `request.as_reader() -> &mut dyn Read`, `request.respond(Response)`, `Response::from_data(Vec<u8>).with_status_code(u16).with_header(Header)`, `Header::from_bytes(name, value) -> Result<Header, ()>`.

---

## File structure

| File | Responsibility |
| --- | --- |
| `src/replay.rs` (modify) | + read `replay.gamemetadata.json`; `Player.game_apm`, `Replay.game_duration_secs` |
| `src/chart.rs` (modify) | + `Series.game_apm`, `Chart.back_link`; chart-only CSS stays here, shared CSS moves out |
| `src/theme.rs` (new) | shared CSS tokens and base styles used by both pages |
| `src/pipeline.rs` (new) | `chart_html(path, back_link) -> Result<String>`: load → series → render, with the panic guard |
| `src/main.rs` (modify) | subcommand parsing: chart (v1) and `serve` |
| `src/scan.rs` (new) | `default_roots`, `find_replays`, `is_within` |
| `src/percent.rs` (new) | `encode`, `decode` |
| `src/list_page.rs` (new) | list page HTML |
| `src/serve.rs` (new) | `Req`/`Resp`, `handle`, `run` |
| `src/lib.rs` (modify) | declare the new modules |
| `tests/real_replay.rs` (modify) | game APM and duration assertions |

---

### Task 1: Game metadata (Blizzard APM and duration)

**Files:**
- Modify: `Cargo.toml` (add `serde_json = "1"`), `src/replay.rs`, `src/chart.rs`, `src/main.rs`, `tests/real_replay.rs`

**Interfaces:**
- Consumes: existing `replay::load`, `chart::render`.
- Produces: `Player.game_apm: Option<f64>`, `Replay.game_duration_secs: Option<f64>`, `Series.game_apm: Option<f64>`.

Read `src/replay.rs`, `src/chart.rs`, `src/main.rs`, and `tests/real_replay.rs` first. The v1 final-review fix wave may have added a string-based `"Duration":` check to the integration test and a `game_speed` guard to `load`; keep the guard, and replace the string-based check with the parsed field as described below.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml` `[dependencies]` add `serde_json = "1"` (keep the other two lines unchanged).

- [ ] **Step 2: Write the failing unit tests in `src/replay.rs`** (inside the existing `mod tests`)

```rust
    #[test]
    fn parses_game_metadata_duration_and_apm() {
        let json = br#"{"Title":"Tuonela LE","Duration":1131,"Players":[{"PlayerID":1,"APM":52.3,"MMR":3100},{"PlayerID":2,"APM":61}]}"#;
        let meta = parse_game_metadata(json).unwrap();
        assert_eq!(meta.duration_game_secs, Some(1131.0));
        assert_eq!(meta.apm_by_player_id, vec![(1, 52.3), (2, 61.0)]);
    }

    #[test]
    fn malformed_metadata_is_none() {
        assert!(parse_game_metadata(b"not json").is_none());
        let meta = parse_game_metadata(b"{}").unwrap();
        assert_eq!(meta.duration_game_secs, None);
        assert!(meta.apm_by_player_id.is_empty());
    }

    #[test]
    fn game_seconds_convert_to_real_seconds_at_faster_speed() {
        // 16 loops per game second, 22.4 per real second: the fixture's 1131
        // game seconds are the 807 real seconds Arbiter computes from loops.
        assert!((game_secs_to_real(1131.0) - 807.86).abs() < 0.01);
    }
```

- [ ] **Step 3: Write the failing chart test in `src/chart.rs`** (inside `mod tests`; the `series` helper there must gain `game_apm: None`)

```rust
    #[test]
    fn legend_shows_game_apm_when_present() {
        let mut s = series("Bob", &[60.0]);
        s.game_apm = Some(61.4);
        let html = render(&chart(vec![s]));
        assert!(html.contains("123 APM &middot; game says 61"));
        let html = render(&chart(vec![series("Ann", &[60.0])]));
        assert!(!html.contains("game says"));
    }
```

- [ ] **Step 4: Run to verify failure**

Run: `cargo test --lib`
Expected: compile errors: `parse_game_metadata` not found, `game_apm` field missing.

- [ ] **Step 5: Implement in `src/replay.rs`**

Add to `Player`: `pub game_apm: Option<f64>,` and to `Replay`: `/// From replay.gamemetadata.json, written by the game client.\n pub game_duration_secs: Option<f64>,`.

Add the metadata types and parser (above the tests module):

```rust
/// The parts of `replay.gamemetadata.json` we use.
#[derive(Debug, Clone, PartialEq)]
struct GameMetadata {
    /// Blizzard's legacy "game seconds" (16 loops each), not real seconds.
    duration_game_secs: Option<f64>,
    /// (PlayerID, APM) as written by the game; PlayerID is 1-based in
    /// `replay.details` player order. APM is per real minute.
    apm_by_player_id: Vec<(u64, f64)>,
}

/// Game loops per legacy game second (the "Normal" speed clock).
const LOOPS_PER_GAME_SECOND: f64 = 16.0;

fn game_secs_to_real(game_secs: f64) -> f64 {
    game_secs * LOOPS_PER_GAME_SECOND / crate::apm::LOOPS_PER_SECOND
}

fn read_game_metadata(mpq: &s2protocol::MPQ, contents: &[u8]) -> Option<GameMetadata> {
    let (_, bytes) = mpq
        .read_mpq_file_sector("replay.gamemetadata.json", false, contents)
        .ok()?;
    parse_game_metadata(&bytes)
}

fn parse_game_metadata(bytes: &[u8]) -> Option<GameMetadata> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let duration_game_secs = v.get("Duration").and_then(serde_json::Value::as_f64);
    let apm_by_player_id = v
        .get("Players")
        .and_then(serde_json::Value::as_array)
        .map(|players| {
            players
                .iter()
                .filter_map(|p| Some((p.get("PlayerID")?.as_u64()?, p.get("APM")?.as_f64()?)))
                .collect()
        })
        .unwrap_or_default();
    Some(GameMetadata { duration_game_secs, apm_by_player_id })
}
```

In `load`, after `read_mpq` succeeds and before building `actions`, read the metadata and attach it. The `players` vector is built from `lobby` (a `Vec<PlayerLobbyDetails>`, which iterates `replay.details` player order); enumerate it so each kept player knows its 1-based position:

```rust
    let metadata = read_game_metadata(&mpq, &contents);
    let game_duration_secs = metadata
        .as_ref()
        .and_then(|m| m.duration_game_secs)
        .map(game_secs_to_real);
    let game_apm_for = |position: usize| -> Option<f64> {
        let id = position as u64 + 1;
        metadata
            .as_ref()?
            .apm_by_player_id
            .iter()
            .find(|(pid, _)| *pid == id)
            .map(|(_, apm)| *apm)
    };
```

Restructure the `players` construction to `lobby.iter().enumerate()` and set `game_apm: game_apm_for(position)` on each `Player`. Because `read_mpq` currently happens after the players are built, move the `read_mpq` call (and the `path_str` line) above the player construction so `metadata` is available; keep the error contexts. Set `game_duration_secs` on the returned `Replay`.

- [ ] **Step 6: Implement in `src/chart.rs`**

Add `pub game_apm: Option<f64>,` to `Series`. In the legend loop, build the suffix:

```rust
        let game = match s.game_apm {
            Some(g) => format!(" &middot; game says {g:.0}"),
            None => String::new(),
        };
```

and change the legend `writeln!` to end `... {:.0} APM{}</span></li>` passing `s.average, game`.

- [ ] **Step 7: Pass it through in `src/main.rs`** (or `pipeline.rs` if Task 2 already ran): `game_apm: player.game_apm,` when building each `Series`.

- [ ] **Step 8: Update `tests/real_replay.rs`**

After the existing per-player loop, add:

```rust
    let game_duration = replay.game_duration_secs.expect("fixture has game metadata");
    let computed = apm::loops_to_secs(replay.duration_loops);
    eprintln!("game duration {game_duration:.0}s, computed {computed:.1}s");
    assert!((game_duration - computed).abs() < 2.0, "time base mismatch: game {game_duration} vs computed {computed}");
    for p in &replay.players {
        let count = replay.actions.iter().filter(|a| a.user_id == p.user_id).count();
        let ours = apm::average_apm(count, replay.duration_loops);
        let theirs = p.game_apm.expect("every fixture player has a game APM");
        eprintln!("{}: ours {ours:.0}, game {theirs:.0}", p.name);
        assert!(theirs > 0.0);
    }
```

Delete the `#[ignore]`d test `gamemetadata_duration_matches_computed_duration` that the v1 fix wave left in this file (it compared the raw game-second value and could never pass); the assertion above replaces it. Also update the doc comment on `apm::LOOPS_PER_SECOND` to state the verification plainly: metadata `Duration` 1131 game seconds × 16 / 22.4 = 807.9 s, matching 807.2 s from summed loops. Run `cargo test --test real_replay -- --nocapture` and **read the printed pairs**: for each player, ours and the game's should be the same order of magnitude and rank the players the same way (the fixture's high-APM players are Ferdwas ≈158 and MrRogers ≈380 by our count). If the game's values are clearly attached to the wrong names, the positional mapping is wrong: report it as DONE_WITH_CONCERNS with the printed table, and do not guess an alternative mapping.

- [ ] **Step 9: Run everything**

Run: `cargo test` and `cargo clippy --all-targets`. Expected: all green, no warnings.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock src/replay.rs src/chart.rs src/main.rs tests/real_replay.rs
git commit -m "Show the game's own APM and duration from replay metadata

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Shared theme, pipeline extraction, back link, and replay scanning

**Files:**
- Create: `src/theme.rs`, `src/pipeline.rs`, `src/scan.rs`
- Modify: `src/chart.rs`, `src/main.rs`, `src/lib.rs`

**Interfaces:**
- Produces:
  - `theme::CSS: &str` (tokens, body, header, table, link styles) and `chart::CHART_CSS` (chart-only rules); `chart::render` emits both.
  - `Chart.back_link: bool` → when true, `<p class="back"><a href="/">&larr; All replays</a></p>` at the top of the header.
  - `pipeline::chart_html(path: &Path, back_link: bool) -> anyhow::Result<String>`.
  - `scan::ReplayEntry { path: PathBuf, modified: SystemTime, size: u64 }`, `scan::default_roots() -> Vec<PathBuf>`, `scan::find_replays(&[PathBuf]) -> Vec<ReplayEntry>` (newest first), `scan::is_within(&[PathBuf], &Path) -> Option<PathBuf>`.

- [ ] **Step 1: Failing tests**

In `src/chart.rs` tests (the `chart` helper gains `back_link: false`):

```rust
    #[test]
    fn back_link_is_rendered_only_when_requested() {
        let mut c = chart(vec![series("Bob", &[60.0])]);
        assert!(!render(&c).contains("All replays"));
        c.back_link = true;
        let html = render(&c);
        assert!(html.contains(r#"<a href="/">&larr; All replays</a>"#));
        assert!(html.contains("--series-1"), "theme tokens still present");
    }
```

Create `src/scan.rs` with only this tests module:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib` → compile errors (`back_link`, `find_replays`, `is_within` missing).

- [ ] **Step 3: Create `src/theme.rs`**

Move from `chart.rs`'s `CSS` constant these rules into `pub const CSS: &str`: the three `.viz-root` token blocks (light, media-dark, data-theme-dark), `html,body`, `body`, `.viz-root`, `header`, `header h1`, `.meta`, `.table`, `.table summary`, `table`, `th,td`, `th:first-child,td:first-child`. Add: `a{color:var(--series-1)}` and `.back{margin:0 0 8px;font-size:13px}`. Everything else (figure, legend, swatch, plot, svg, grid, axis, tick, line, label, xhair, dot, brush, tooltip, hint, empty) stays in `chart.rs` renamed `const CHART_CSS: &str`. In `render`, emit `<style>{}{}</style>` with `theme::CSS, CHART_CSS`.

- [ ] **Step 4: Add `back_link` to `Chart`**

`pub back_link: bool,`. In `render`, right after `<header>\n`, when `chart.back_link` write `<p class="back"><a href="/">&larr; All replays</a></p>\n`.

- [ ] **Step 5: Create `src/pipeline.rs`** by moving the body of `main.rs::run` (everything from `replay::load` through `chart::render`, including the `catch_unwind` panic guard the v1 fix wave added) into:

```rust
//! Replay path in, chart HTML out. Shared by the CLI and the server.

use std::path::Path;

use anyhow::Result;

use crate::{apm, chart, replay};

pub fn chart_html(path: &Path, back_link: bool) -> Result<String> {
    let replay = load_guarded(path)?;
    let series = replay
        .players
        .iter()
        .map(|player| {
            let mut loops: Vec<i64> = replay
                .actions
                .iter()
                .filter(|a| a.user_id == player.user_id)
                .map(|a| a.game_loop)
                .collect();
            loops.sort_unstable();
            chart::Series {
                name: player.name.clone(),
                race: player.race.clone(),
                result: player.result.clone(),
                average: apm::average_apm(loops.len(), replay.duration_loops),
                game_apm: player.game_apm,
                points: apm::rolling_apm(&loops, replay.duration_loops),
            }
        })
        .collect();
    Ok(chart::render(&chart::Chart {
        title: format!("APM - {}", replay.map),
        map: replay.map.clone(),
        duration_secs: apm::loops_to_secs(replay.duration_loops),
        series,
        back_link,
    }))
}
```

`load_guarded` is the existing panic-guarded call to `replay::load` moved from `main.rs` verbatim (keep its test, moved along with it into `pipeline.rs`'s tests module, adjusted to call `chart_html(&tmp, false)`). `main.rs::run` becomes: `let html = pipeline::chart_html(input, false)?; std::fs::write(...)`.

- [ ] **Step 6: Implement `src/scan.rs`** above its tests:

```rust
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

/// All `.SC2Replay` files under the roots, newest first.
pub fn find_replays(roots: &[PathBuf]) -> Vec<ReplayEntry> {
    let mut out = Vec::new();
    for root in roots {
        walk(root, &mut out);
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
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

fn walk(dir: &Path, out: &mut Vec<ReplayEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
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
```

- [ ] **Step 7: Register modules in `src/lib.rs`**: `pub mod apm; pub mod chart; pub mod pipeline; pub mod replay; pub mod scan; pub mod theme;`

- [ ] **Step 8: Run** `cargo test` and `cargo clippy --all-targets`. Expected: all green, no warnings. Also `cargo run -- "<fixture>" -o "<scratchpad>/t.html"` still exits 0.

- [ ] **Step 9: Commit**

```bash
git add src/lib.rs src/theme.rs src/pipeline.rs src/scan.rs src/chart.rs src/main.rs
git commit -m "Extract theme and pipeline, add replay folder scanning

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Percent encoding and the list page

**Files:**
- Create: `src/percent.rs`, `src/list_page.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `scan::ReplayEntry`, `chart::escape_html`, `theme::CSS`.
- Produces: `percent::encode(&str) -> String`, `percent::decode(&str) -> Option<String>`, `list_page::render(entries: &[ReplayEntry], roots: &[PathBuf]) -> String`.

- [ ] **Step 1: Failing tests**

`src/percent.rs` tests module:

```rust
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
```

`src/list_page.rs` tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn entry(name: &str, secs: u64, size: u64) -> ReplayEntry {
        ReplayEntry { path: PathBuf::from("C:/r").join(name), modified: UNIX_EPOCH + Duration::from_secs(secs), size }
    }

    #[test]
    fn lists_entries_with_escaped_names_encoded_links_and_timestamps() {
        let html = render(&[entry("Tuonela LE (1).SC2Replay", 1_700_000_000, 227_146), entry("<b>.SC2Replay", 5, 2048)], &[PathBuf::from("C:/r")]);
        assert!(html.contains("2 replays"));
        assert!(html.contains("Tuonela LE (1).SC2Replay"));
        assert!(html.contains("&lt;b&gt;.SC2Replay"));
        assert!(!html.contains("<b>.SC2Replay"));
        assert!(html.contains(r#"href="/replay?path=C%3A%2Fr%2FTuonela%20LE%20%281%29.SC2Replay""#) || html.contains(r#"href="/replay?path=C%3A%2Fr%5CTuonela%20LE%20%281%29.SC2Replay""#));
        assert!(html.contains(r#"data-ts="1700000000""#));
        assert!(html.contains("222 KB"));
        assert!(html.contains(r#"data-name="tuonela le (1).sc2replay""#));
    }

    #[test]
    fn empty_state_names_the_roots() {
        let html = render(&[], &[PathBuf::from("C:/nowhere")]);
        assert!(html.contains("No replays found"));
        assert!(html.contains("C:/nowhere"));
        assert!(!html.contains("<tbody>"));
    }

    #[test]
    fn has_open_file_control_and_filter() {
        let html = render(&[], &[]);
        assert!(html.contains(r#"<input type="file" id="file" accept=".SC2Replay">"#));
        assert!(html.contains(r#"fetch('/open'"#));
        assert!(html.contains(r#"id="filter""#));
    }
}
```

- [ ] **Step 2: Run** `cargo test --lib percent list_page` → compile errors.

- [ ] **Step 3: Implement `src/percent.rs`**

```rust
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
```

- [ ] **Step 4: Implement `src/list_page.rs`**

```rust
//! The replay list page served at `/`.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use crate::chart::escape_html;
use crate::percent;
use crate::scan::ReplayEntry;
use crate::theme;

pub fn render(entries: &[ReplayEntry], roots: &[PathBuf]) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = write!(html, "<title>Arbiter replays</title>\n<style>{}{}</style>\n</head>\n<body>\n", theme::CSS, LIST_CSS);
    html.push_str("<main class=\"viz-root\">\n<header>\n<h1>Replays</h1>\n");
    let _ = writeln!(html, "<p class=\"meta\">{} replay{} in:</p>", entries.len(), if entries.len() == 1 { "" } else { "s" });
    html.push_str("<ul class=\"roots\">\n");
    for r in roots {
        let _ = writeln!(html, "<li>{}</li>", escape_html(&r.display().to_string()));
    }
    html.push_str("</ul>\n</header>\n");
    html.push_str("<section class=\"tools\">\n<label>Open a replay from anywhere: <input type=\"file\" id=\"file\" accept=\".SC2Replay\"></label> <button id=\"open\" type=\"button\">Chart it</button> <span id=\"status\" class=\"status\"></span>\n");
    html.push_str("<input id=\"filter\" type=\"search\" placeholder=\"Filter by name\" autocomplete=\"off\">\n</section>\n");
    if entries.is_empty() {
        html.push_str("<p class=\"empty\">No replays found in the folders above. Pass <code>--dir</code> to serve another folder, or use Open above.</p>\n");
    } else {
        html.push_str("<table>\n<thead><tr><th>Replay</th><th>Modified</th><th>Size</th></tr></thead>\n<tbody>\n");
        for e in entries {
            let name = e.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let ts = e.modified.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = writeln!(
                html,
                "<tr data-name=\"{}\"><td><a href=\"/replay?path={}\">{}</a></td><td data-ts=\"{}\"></td><td>{} KB</td></tr>",
                escape_html(&name.to_lowercase()),
                percent::encode(&e.path.to_string_lossy()),
                escape_html(&name),
                ts,
                e.size / 1024
            );
        }
        html.push_str("</tbody>\n</table>\n");
    }
    html.push_str("</main>\n");
    let _ = write!(html, "<script>{}</script>\n</body>\n</html>\n", LIST_JS);
    html
}

const LIST_CSS: &str = r#"
.roots{margin:0 0 16px;padding-left:20px;color:var(--text-2);font-size:13px}
.tools{max-width:1040px;margin:0 auto 16px;display:flex;flex-wrap:wrap;gap:12px 16px;align-items:center}
.tools input[type=search]{margin-left:auto;padding:6px 10px;border:1px solid var(--border);border-radius:6px;background:var(--surface);color:var(--text);min-width:240px}
button{padding:6px 12px;border:1px solid var(--border);border-radius:6px;background:var(--surface);color:var(--text);cursor:pointer}
.status{color:var(--text-2);font-size:13px}
table{max-width:1040px;margin:0 auto;width:100%}
th,td{text-align:left}
td:nth-child(3),th:nth-child(3){text-align:right}
.empty{max-width:1040px;margin:32px auto;color:var(--text-2)}
"#;

const LIST_JS: &str = r#"
(() => {
  document.querySelectorAll('[data-ts]').forEach(td => {
    const ts = +td.dataset.ts;
    if (ts) td.textContent = new Date(ts * 1000).toLocaleString(undefined, {dateStyle: 'medium', timeStyle: 'short'});
  });
  const filter = document.getElementById('filter');
  const rows = [...document.querySelectorAll('tbody tr')];
  filter.addEventListener('input', () => {
    const q = filter.value.toLowerCase();
    rows.forEach(r => { r.hidden = !r.dataset.name.includes(q); });
  });
  const file = document.getElementById('file');
  const status = document.getElementById('status');
  document.getElementById('open').addEventListener('click', async () => {
    const f = file.files[0];
    if (!f) { status.textContent = 'Choose a .SC2Replay first.'; return; }
    status.textContent = 'Parsing ' + f.name + '…';
    try {
      const res = await fetch('/open', {method: 'POST', body: f});
      const text = await res.text();
      if (!res.ok) { status.textContent = 'Error: ' + text; return; }
      document.open(); document.write(text); document.close();
    } catch (e) {
      status.textContent = 'Request failed: ' + e;
    }
  });
})();
"#;
```

- [ ] **Step 5: Register** `pub mod list_page; pub mod percent;` in `src/lib.rs`.

- [ ] **Step 6: Run** `cargo test` and `cargo clippy --all-targets`; all green, no warnings.

- [ ] **Step 7: Commit**

```bash
git add src/lib.rs src/percent.rs src/list_page.rs
git commit -m "Add replay list page and percent encoding

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: The server and the `serve` subcommand

**Files:**
- Create: `src/serve.rs`
- Modify: `Cargo.toml` (add `tiny_http = "0.12"`), `src/main.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `pipeline::chart_html`, `scan::{find_replays, is_within, default_roots}`, `list_page::render`, `percent::decode`.
- Produces: `serve::Req`, `serve::Resp`, `serve::handle(&Req, &[PathBuf]) -> Resp`, `serve::run(Vec<PathBuf>, u16) -> anyhow::Result<()>`; CLI `arbiter serve [--dir <p>]... [--port <n>]`.

- [ ] **Step 1: Failing tests**

`src/serve.rs` tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r"the local fixture replay (set ARBITER_FIXTURE to its path)";

    fn get(path: &str, query: &str, roots: &[PathBuf]) -> Resp {
        handle(&Req { method: "GET", path, query, body: &[] }, roots)
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arbiter-serve-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn index_lists_replays_in_roots() {
        let root = temp_root("index");
        std::fs::write(root.join("game one.SC2Replay"), b"x").unwrap();
        let resp = get("/", "", &[root.clone()]);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.content_type, HTML);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("game one.SC2Replay"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn replay_route_rejects_bad_requests() {
        let root = temp_root("reject");
        let outside = std::env::temp_dir().join(format!("arbiter-serve-outside-{}.SC2Replay", std::process::id()));
        std::fs::write(&outside, b"x").unwrap();
        let roots = vec![root.clone()];
        assert_eq!(get("/replay", "", &roots).status, 400);
        assert_eq!(get("/replay", "path=%zz", &roots).status, 400);
        assert_eq!(get("/replay", &format!("path={}", crate::percent::encode(&root.join("missing.SC2Replay").to_string_lossy())), &roots).status, 404);
        assert_eq!(get("/replay", &format!("path={}", crate::percent::encode(&outside.to_string_lossy())), &roots).status, 403);
        assert_eq!(get("/nope", "", &roots).status, 404);
        assert_eq!(handle(&Req { method: "DELETE", path: "/", query: "", body: &[] }, &roots).status, 404);
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn replay_route_charts_the_fixture() {
        let fixture = Path::new(FIXTURE);
        if !fixture.is_file() {
            eprintln!("skipping: fixture absent");
            return;
        }
        let roots = vec![fixture.parent().unwrap().to_path_buf()];
        let resp = get("/replay", &format!("path={}", crate::percent::encode(FIXTURE)), &roots);
        assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("All replays"), "chart pages served by the server carry the back link");
        assert!(body.contains("Tuonela LE"));
    }

    #[test]
    fn open_route_handles_garbage_and_leaves_no_temp_file() {
        let resp = handle(&Req { method: "POST", path: "/open", query: "", body: b"definitely not a replay" }, &[]);
        assert_eq!(resp.status, 500);
        assert!(String::from_utf8(resp.body).unwrap().contains("not a StarCraft II replay"));
        let leftovers = std::fs::read_dir(std::env::temp_dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&format!("arbiter-open-{}-", std::process::id()))).count();
        assert_eq!(leftovers, 0);
        assert_eq!(handle(&Req { method: "POST", path: "/open", query: "", body: &[] }, &[]).status, 400);
    }

    #[test]
    fn query_param_finds_the_named_key() {
        assert_eq!(query_param("a=1&path=x%20y&b=2", "path"), Some("x%20y"));
        assert_eq!(query_param("a=1", "path"), None);
        assert_eq!(query_param("path", "path"), None);
    }
}
```

`src/main.rs` tests: replace the existing `parse_args` tests with:

```rust
    #[test]
    fn single_path_defaults_output_next_to_input() {
        match parse_args(&args(&["games/x.SC2Replay"])).unwrap() {
            Command::Chart { input, output } => {
                assert_eq!(input, PathBuf::from("games/x.SC2Replay"));
                assert_eq!(output, PathBuf::from("games/x.html"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dash_o_sets_output() {
        match parse_args(&args(&["x.SC2Replay", "-o", "out/y.html"])).unwrap() {
            Command::Chart { output, .. } => assert_eq!(output, PathBuf::from("out/y.html")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn serve_parses_dirs_and_port() {
        assert_eq!(parse_args(&args(&["serve"])).unwrap(), Command::Serve { dirs: vec![], port: 8321 });
        assert_eq!(
            parse_args(&args(&["serve", "--dir", "a", "--port", "9000", "--dir", "b"])).unwrap(),
            Command::Serve { dirs: vec![PathBuf::from("a"), PathBuf::from("b")], port: 9000 }
        );
    }

    #[test]
    fn bad_shapes_are_rejected() {
        for bad in [vec![], vec!["a", "b"], vec!["a", "-x", "b"], vec!["a", "-o", "b", "c"], vec!["serve", "--port"], vec!["serve", "--port", "x"], vec!["serve", "--what"]] {
            assert!(parse_args(&args(&bad)).is_none(), "{bad:?}");
        }
    }
```

- [ ] **Step 2: Run** `cargo test` → compile errors.

- [ ] **Step 3: Add** `tiny_http = "0.12"` to `Cargo.toml`.

- [ ] **Step 4: Implement `src/serve.rs`**

```rust
//! Localhost web server: replay list, chart-on-click, chart-from-upload.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Result, anyhow};

use crate::{list_page, percent, pipeline, scan};

pub const HTML: &str = "text/html; charset=utf-8";
pub const TEXT: &str = "text/plain; charset=utf-8";
const MAX_UPLOAD: usize = 64 * 1024 * 1024;

pub struct Req<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub body: &'a [u8],
}

pub struct Resp {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

pub fn handle(req: &Req, roots: &[PathBuf]) -> Resp {
    match (req.method, req.path) {
        ("GET", "/") => html(list_page::render(&scan::find_replays(roots), roots)),
        ("GET", "/replay") => replay_route(req.query, roots),
        ("POST", "/open") => open_route(req.body),
        _ => text(404, "not found"),
    }
}

fn replay_route(query: &str, roots: &[PathBuf]) -> Resp {
    let Some(raw) = query_param(query, "path") else {
        return text(400, "missing path parameter");
    };
    let Some(decoded) = percent::decode(raw) else {
        return text(400, "bad path encoding");
    };
    let path = PathBuf::from(decoded);
    if !path.is_file() {
        return text(404, "replay not found");
    }
    let Some(canon) = scan::is_within(roots, &path) else {
        return text(403, "path is outside the served replay folders");
    };
    match pipeline::chart_html(&canon, true) {
        Ok(page) => html(page),
        Err(e) => text(500, &format!("{e:#}")),
    }
}

fn open_route(body: &[u8]) -> Resp {
    if body.len() > MAX_UPLOAD {
        return text(413, "replay larger than 64 MiB");
    }
    if body.is_empty() {
        return text(400, "empty upload");
    }
    let tmp = std::env::temp_dir().join(format!("arbiter-open-{}-{}.SC2Replay", std::process::id(), next_id()));
    if let Err(e) = std::fs::write(&tmp, body) {
        return text(500, &format!("could not write temp file: {e}"));
    }
    let result = pipeline::chart_html(&tmp, true);
    let _ = std::fs::remove_file(&tmp);
    match result {
        Ok(page) => html(page),
        Err(e) => text(500, &format!("{e:#}")),
    }
}

/// Runs the server until the process is killed.
pub fn run(roots: Vec<PathBuf>, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let server = tiny_http::Server::http(&addr).map_err(|e| anyhow!("could not listen on {addr}: {e}"))?;
    println!("Listening on http://{addr}");
    for root in &roots {
        println!("  serving {}", root.display());
    }
    for mut request in server.incoming_requests() {
        if request.body_length().is_some_and(|n| n > MAX_UPLOAD) {
            respond(request, text(413, "replay larger than 64 MiB"));
            continue;
        }
        let mut body = Vec::new();
        let _ = request.as_reader().take(MAX_UPLOAD as u64 + 1).read_to_end(&mut body);
        let method = request.method().to_string();
        let url = request.url().to_string();
        let (path, query) = url.split_once('?').unwrap_or((&url, ""));
        let resp = handle(&Req { method: &method, path, query, body: &body }, &roots);
        respond(request, resp);
    }
    Ok(())
}

fn respond(request: tiny_http::Request, resp: Resp) {
    let header = tiny_http::Header::from_bytes("Content-Type", resp.content_type).expect("static header is valid");
    let response = tiny_http::Response::from_data(resp.body).with_status_code(resp.status).with_header(header);
    let _ = request.respond(response);
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then_some(v)
    })
}

fn next_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn html(body: String) -> Resp {
    Resp { status: 200, content_type: HTML, body: body.into_bytes() }
}

fn text(status: u16, body: &str) -> Resp {
    Resp { status, content_type: TEXT, body: body.as_bytes().to_vec() }
}
```

If `Path` ends up unused, drop it from the import.

- [ ] **Step 5: Implement the CLI in `src/main.rs`**

```rust
#[derive(Debug, PartialEq)]
enum Command {
    Chart { input: PathBuf, output: PathBuf },
    Serve { dirs: Vec<PathBuf>, port: u16 },
}

const USAGE: &str = "usage:\n  arbiter <replay.SC2Replay> [-o <out.html>]\n  arbiter serve [--dir <folder>]... [--port <n>]";

fn parse_args(args: &[String]) -> Option<Command> {
    match args {
        [first, rest @ ..] if first == "serve" => parse_serve(rest),
        [input] => {
            let input = PathBuf::from(input);
            let output = input.with_extension("html");
            Some(Command::Chart { input, output })
        }
        [input, flag, output] if flag == "-o" => Some(Command::Chart { input: PathBuf::from(input), output: PathBuf::from(output) }),
        _ => None,
    }
}

fn parse_serve(args: &[String]) -> Option<Command> {
    let mut dirs = Vec::new();
    let mut port = 8321u16;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dir" => dirs.push(PathBuf::from(it.next()?)),
            "--port" => port = it.next()?.parse().ok()?,
            _ => return None,
        }
    }
    Some(Command::Serve { dirs, port })
}
```

`main` prints `USAGE` (exit 2) on `None`; on `Command::Chart` calls the existing `run(&input, &output)`; on `Command::Serve`:

```rust
fn serve(dirs: Vec<PathBuf>, port: u16) -> Result<()> {
    let roots = if dirs.is_empty() { scan::default_roots() } else { dirs };
    if roots.is_empty() {
        anyhow::bail!("no StarCraft II replay folders found under your profile; pass --dir <folder>");
    }
    for d in &roots {
        if !d.is_dir() {
            anyhow::bail!("not a folder: {}", d.display());
        }
    }
    serve::run(roots, port)
}
```

Register `pub mod serve;` in `src/lib.rs`.

- [ ] **Step 6: Run** `cargo test` and `cargo clippy --all-targets`. All green, no warnings.

- [ ] **Step 7: Manual check in the built-in browser**

Start the server with `mcp__Claude_Browser__preview_start` using a `.claude/launch.json` entry `{ "name": "arbiter-serve", "runtimeExecutable": "cargo", "runtimeArgs": ["run", "--release", "--", "serve"], "port": 8321 }` (commit this file; it is useful going forward). Confirm: the list shows the user's replays newest first with local dates; the filter narrows rows; clicking a replay shows the chart with "game says" values in the legend and a working back link; "Chart it" with the fixture file selected renders the chart; the console has no errors. Record what you saw in the report.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/serve.rs src/main.rs .claude/launch.json
git commit -m "Add arbiter serve: replay list, chart on click, open any file

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review (done while writing)

- Spec coverage: metadata + legend (Task 1); theme/back link/pipeline/scan (Task 2); percent + list page with filter and Open control (Task 3); routes, limits, containment, CLI, manual check (Task 4). Non-Faster speeds, HTTPS, per-replay parsing for the list are out of scope per spec.
- Type consistency: `Series.game_apm` (Task 1) used in `pipeline.rs` (Task 2). `Chart.back_link` (Task 2) used by `pipeline::chart_html` and the server (Task 4). `scan::ReplayEntry` (Task 2) consumed by `list_page` (Task 3) and `serve` (Task 4). `percent::encode` (Task 3) used by list page and serve tests; `decode` by `serve`.
- Known plan risk: positional PlayerID mapping in Task 1 is verified against the fixture, with an explicit stop-and-report if it fails.
