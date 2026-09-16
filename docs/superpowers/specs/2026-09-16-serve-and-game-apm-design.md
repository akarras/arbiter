# Arbiter v2: game APM comparison and `serve` mode

**Date:** 2026-09-16
**Status:** Approved
**Builds on:** `2026-09-15-apm-chart-design.md` (the one-shot CLI, which stays unchanged)

## Goal

1. Show Blizzard's own per-player APM next to the value Arbiter computes, so the
   two can be compared at a glance.
2. Add `arbiter serve`, a local web server that lists the user's replays and
   charts any of them on click, and can also chart a replay chosen from disk.

## Decisions (from brainstorming)

| Question | Decision |
| --- | --- |
| Replay source for the server | Auto-detect StarCraft II replay folders under the user profile; `--dir` overrides |
| Page layout | List page (newest first) linking to the existing chart page, plus an "Open file" button |
| Game APM display | In the legend, side by side: `52 APM · game says 61` |
| HTTP server | `tiny_http` 0.12 (sync, MIT), bound to 127.0.0.1 and Host-checked |
| JSON parsing | `serde_json` (already in the dependency tree via `s2protocol`) |

## Part 1: game APM comparison

### Source

Every LotV replay archive contains `replay.gamemetadata.json`, written by the
game client. Shape (abridged):

```json
{"Title":"Tuonela LE","GameVersion":"5.0.14.94137","DataBuild":"94137","BaseBuild":"94137",
 "Duration":807,"IsNotAvailable":false,
 "Players":[{"PlayerID":1,"APM":52.3,"MMR":3100,"Result":"Win","SelectedRace":"Terr","AssignedRace":"Terr"}, ...]}
```

`Duration` is in Blizzard's legacy **game seconds** (16 game loops each), not
real seconds: on the fixture it reads 1131 while the real length is 807 s, a
ratio of exactly 1.4 (the Faster speed multiplier, 22.4 / 16). Arbiter converts
it to real seconds (`Duration * 16 / 22.4`) so it can be compared with
`loops_to_secs(duration_loops)`; on the fixture they agree within a second,
which independently confirms the loop summation.

`APM` is per **real** minute: the fixture's top player reads 415 in the
metadata against 380 by Arbiter's original count (ratio 1.09), which a 1.4
clock factor would contradict.

**Why the original count was low (resolved 2026-09-16).** Fitting every
combination of game-event types against Blizzard's eight fixture values showed
the gap was two event kinds the v1 filter dropped: `CommandManagerState`
(since patch 2.0.8 a repeat of the previous command, e.g. pressing Z again, is
stored this way rather than as a `Cmd`) and `CmdUpdateTargetUnit` (the previous
command re-issued on a new unit). Camera moves and `CmdUpdateTargetPoint` do
not fit. Blizzard also divides by each player's own time to their last event
rather than the match length. With both changes Arbiter reads 74, 75, 235, 93,
79, 84, 76, 412 against the game's 74, 78, 233, 96, 83, 83, 78, 415: within
5.3% for every player. The integration test enforces an 8% bound.

`PlayerID` is 1-based and follows the order of `replay.details` `player_list`
(the same order `Vec<PlayerLobbyDetails>` iterates). Verified on the fixture:
metadata APM 74, 78, 233, 96, 83, 83, 78, 415 lines up with Arbiter's 33, 31,
158, 57, 52, 37, 42, 380 in that order.

### Changes

- `replay.rs`: read the file from the already-open MPQ with
  `mpq.read_mpq_file_sector("replay.gamemetadata.json", false, &contents)`.
  Parse with `serde_json::Value`. New fields:
  - `Replay.game_duration_secs: Option<f64>`
  - `Player.game_apm: Option<f64>`
  Missing file, unparsable JSON, or a player without a match leave the
  `Option`s as `None`; they never fail the load.
- `chart.rs`: `Series.game_apm: Option<f64>`. Legend row:
  `{race} · {result} · {avg:.0} APM · game says {game:.0}` when present, the
  current text otherwise.
- `main.rs`: pass the field through.
- Integration test: assert every fixture player has `Some(game_apm)` and print
  ours vs. theirs; assert `|game_duration_secs − loops_to_secs(duration_loops)| < 2`
  using the converted field. This replaces the `#[ignore]`d string-based
  duration test the v1 fix wave left in `tests/real_replay.rs`, which compared
  the raw game-second value and therefore could not pass.

Arbiter's number and the game's will not be identical: SC2 measures each player
over their own time in the game and counts a slightly different event set. The
comparison is informational; no reconciliation is attempted.

## Part 2: `arbiter serve`

### CLI

```
arbiter serve [--dir <path>] [--port <n>]
```

- `--dir` may repeat. Without it, replay roots are auto-detected (below).
- `--port` defaults to 8321. Binds `127.0.0.1` only.
- Prints `Listening on http://127.0.0.1:8321` and the list of roots being
  served, then runs until interrupted.
- The existing `arbiter <file> [-o out.html]` form is unchanged. `serve` is
  recognised only as the first argument.

### Replay discovery (`scan.rs`)

```rust
pub fn default_roots() -> Vec<PathBuf>
pub fn find_replays(roots: &[PathBuf]) -> Vec<ReplayEntry>   // newest first
pub struct ReplayEntry { pub path: PathBuf, pub modified: SystemTime, pub size: u64 }
pub fn is_within(roots: &[PathBuf], path: &Path) -> Option<PathBuf>  // canonical path if inside a root
```

- `default_roots()`: from `%USERPROFILE%` (or `$HOME`), the candidates are
  `Documents\StarCraft II` and every directory matching
  `OneDrive*\Documents*\StarCraft II`. For each that exists, every
  `Accounts\*\*\Replays` directory beneath it is a root. The user's machine has
  its replays at `OneDrive\Documents\StarCraft II\Accounts\<account>\<toon>\Replays\Multiplayer`.
- `find_replays` walks each root recursively, keeps files whose extension is
  `SC2Replay` (case-insensitive), and sorts by modified time descending.
  Unreadable entries are skipped silently.
- `is_within` canonicalises both sides and checks prefix; symlink escapes fail
  the check.

### Routes (`serve.rs`)

The request handling is a pure function so it can be unit-tested without
sockets:

```rust
pub struct Req<'a> { pub method: &'a str, pub path: &'a str, pub query: &'a str, pub body: &'a [u8] }
pub struct Resp { pub status: u16, pub content_type: &'static str, pub body: Vec<u8> }
pub fn handle(req: &Req, roots: &[PathBuf]) -> Resp
pub fn run(roots: Vec<PathBuf>, port: u16) -> anyhow::Result<()>   // tiny_http loop calling handle
```

| Route | Behaviour |
| --- | --- |
| `GET /` | `find_replays(roots)` rendered by `list_page::render` |
| `GET /replay?path=<percent-encoded absolute path>` | `is_within` check; on success load + chart, return the chart HTML. 404 if not found, 403 if outside roots, 500 with the error text (escaped) if parsing fails |
| `POST /open` (body = raw replay bytes, ≤ 64 MiB) | write to a unique file in `std::env::temp_dir()`, chart it, delete the temp file, return the chart HTML. 413 if too large, 500 with error text if parsing fails |
| anything else | 404 |

Chart pages get a small "← All replays" link at the top; `chart::Chart` gains
`back_link: bool` for this (the one-shot CLI passes `false`).

Percent-decoding is implemented locally (a dozen lines); no crate.

### List page (`list_page.rs`)

```rust
pub fn render(entries: &[ReplayEntry], roots: &[PathBuf]) -> String
```

- Same visual language and CSS tokens as the chart page (shared `CSS` constant
  moved to a small `theme.rs` so both pages use one copy).
- Header: "Replays", count, the roots being served.
- A filter box that hides rows client-side as you type (name match).
- An "Open file" control: `<input type="file" accept=".SC2Replay">` and a
  button; JS `fetch('/open', {method:'POST', body: file})` then replaces the
  document with the response (`document.open(); document.write(html); document.close()`),
  or shows the error text inline on non-200.
- Rows: file name (link to `/replay?path=…`), modified date `YYYY-MM-DD HH:MM`
  local time, size in KB. File names are HTML-escaped; the link path is
  percent-encoded.
- Empty state when no replays are found, naming the roots that were scanned.

### Errors

`serve` fails at startup with a clear message if no roots exist (and no
`--dir` was given) or the port is in use. Per-request failures never take the
server down: `handle` catches parse panics the same way the CLI does and
returns 500.

## Dependencies (v2)

```toml
anyhow = "1"
s2protocol = { version = "3.5", default-features = false, features = ["tracing_off"] }
serde_json = "1"
tiny_http = "0.12"
```

## Testing

- `scan.rs`: unit tests using a temp directory tree with `.SC2Replay`,
  `.sc2replay`, and other files; ordering by mtime; `is_within` with a path
  outside the root and with `..` segments.
- `serve.rs`: `handle` tests for each route: `/` renders the list, `/replay`
  without `path` is 400, outside root is 403, missing file is 404, unknown route
  is 404, `/open` with garbage bytes is 500 with a message and leaves no temp
  file behind. Percent-decoding tests.
- `list_page.rs`: escaping of file names, count, empty state, link encoding.
- `replay.rs` / integration: game APM and duration assertions on the fixture.
- Manual: run `arbiter serve`, open the list in the built-in browser, click a
  replay, use the filter, use Open file with the fixture, confirm the back link.

## Out of scope

Parsing every replay for the list (map, players, result), thumbnails, MMR
display, non-Faster game speeds, HTTPS, remote access.
