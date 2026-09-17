# Arbiter: StarCraft II replay APM charts

**Date:** 2026-09-15
**Status:** Approved

## Goal

A command-line program that reads one StarCraft II replay (`.SC2Replay`) and
writes a self-contained interactive HTML page charting each player's APM
(actions per minute) over the course of the match.

## Decisions (from brainstorming)

| Question | Decision |
| --- | --- |
| Language | Rust (existing `arbiter` crate, edition 2024) |
| Output | One self-contained `.html` file per replay |
| Windowing | Trailing 60-second window, sampled every 5 seconds |
| Action set | Match SC2's in-game APM: commands, selections, control groups, command repeats (`CommandManagerState`), unit retargets (`CmdUpdateTargetUnit`). Exclude camera and `CmdUpdateTargetPoint`. Denominator is each player's own time to their last event. (Amended 2026-09-16 after fitting against Blizzard's metadata APM; see the v2 spec.) |
| Input | One replay path per run |
| Parsing | `s2protocol` crate (v3.5.x), default features off |
| Charting | Hand-written SVG + inline vanilla JS, no third-party JS |

## CLI

```
arbiter <path/to/game.SC2Replay> [-o <out.html>]
```

- Default output path: the input path with its extension replaced by `.html`.
- On success, prints the output path to stdout and exits 0.
- On failure (missing file, unreadable MPQ, unsupported protocol version,
  no players found), prints one line to stderr and exits 1.
- Any other argument shape prints usage to stderr and exits 2.

## Architecture

Four source files, each with one responsibility. Only `replay.rs` knows the
`s2protocol` crate exists.

### `src/replay.rs` — replay loading

```rust
pub struct Player { pub user_id: i64, pub name: String, pub race: String, pub team: u8, pub result: String }
pub struct Replay { pub map: String, pub duration_loops: i64, pub players: Vec<Player>, pub actions: Vec<Action> }
pub struct Action { pub user_id: i64, pub game_loop: i64 }
pub fn load(path: &Path) -> anyhow::Result<Replay>
```

- Uses `s2protocol::init_data::InitData::try_from((PathBuf, u64))`, then
  `Vec<PlayerLobbyDetails>::try_from(&InitData)` to join lobby slots
  (which carry `user_id`) with details (name, race, team, result).
- Players whose lobby slot has no `user_id`, or whose `observe` flag is
  non-zero, are excluded.
- Reads game events with `s2protocol::versions::read_game_events`. Keeps an
  `Action` for each event whose variant is `Cmd`, `SelectionDelta`, or
  `ControlGroupUpdate` and whose `user_id` belongs to a kept player. Drops
  everything else (camera, sync checks, chat, triggers, `CmdUpdateTarget*`).
- `duration_loops` is the game loop of the last game event of any kind.
- Game events carry a `delta`; the absolute loop is the running sum of
  deltas, computed here so downstream code sees absolute loops only.

### `src/apm.rs` — pure math

```rust
pub const LOOPS_PER_SECOND: f64 = 22.4;
pub const WINDOW_SECS: f64 = 60.0;
pub const STEP_SECS: f64 = 5.0;
pub struct Point { pub secs: f64, pub apm: f64 }
pub fn rolling_apm(action_loops: &[i64], duration_loops: i64) -> Vec<Point>
pub fn average_apm(action_count: usize, duration_loops: i64) -> f64
```

- Time base: 22.4 game loops per real second (LotV "Faster" speed, which is
  what the in-game clock shows). To be checked against the test replay's
  known duration during implementation.
- `rolling_apm` samples at `t = 5, 10, 15, ... ≤ duration` seconds. Each
  point counts actions with `t - 60 < loop_secs ≤ t`, divided by
  `min(t, 60) / 60` minutes so the first minute is not artificially low.
- `action_loops` must be sorted ascending; the function uses two cursors,
  so it is linear in actions plus samples.
- Zero-duration or empty input returns an empty vector.
- `average_apm` is `count / (duration_secs / 60)`, or 0 when duration is 0.

### `src/chart.rs` — HTML rendering

```rust
pub struct Series { pub name: String, pub race: String, pub result: String, pub average: f64, pub points: Vec<Point> }
pub struct Chart { pub title: String, pub map: String, pub duration_secs: f64, pub series: Vec<Series> }
pub fn render(chart: &Chart) -> String
```

Output is one HTML document containing:

- A header with the map name and match duration.
- An inline SVG line chart, one `<path>` per player. X-axis ticks in `mm:ss`
  at a "nice" interval (1, 2, or 5 minutes depending on duration). Y-axis
  ticks at a nice interval covering the maximum APM across all series.
- A legend row per player: colour swatch, name, race, result, average APM.
- Embedded CSS with light and dark palettes via `prefers-color-scheme`.
  Series colours come from a fixed categorical palette indexed by player
  order (the `dataviz` skill's palette guidance applies during implementation).
- Inline JS (about 150 lines, no dependencies) providing:
  - Hover: a vertical crosshair snapped to the nearest sample, with a
    tooltip listing the time and each player's APM at that sample.
  - Drag on the plot area to zoom the x-axis to that range; the y-axis
    rescales to the visible maximum.
  - Double-click to reset the zoom.
- Series data embedded as a JSON array in a `<script type="application/json">`
  block; the JS reads it and re-draws paths on zoom.
- All user-supplied text (player names, map name) is HTML-escaped. The JSON
  block escapes `</` so a name cannot close the script tag.
- A replay with no actions renders the frame and a "no player actions found"
  note in place of the lines.

### `src/main.rs` — wiring

Parses arguments by hand (`std::env::args`), calls `replay::load`, builds
one `Series` per player via `apm`, calls `chart::render`, writes the file,
prints its path. Errors are `anyhow::Result` bubbled to a single
`eprintln!` and exit code.

## Data flow

```
path ─▶ replay::load ─▶ Replay
                          │ per player: filter actions by user_id, sort loops
                          ▼
                        apm::rolling_apm / average_apm ─▶ Series
                          ▼
                        chart::render ─▶ String ─▶ fs::write ─▶ out.html
```

## Dependencies

```toml
[dependencies]
anyhow = "1"
s2protocol = { version = "3.5", default-features = false, features = ["tracing_off"] }
```

## Testing

- `apm.rs`: unit tests with synthetic loops. Cases: empty input, one action,
  uniform 100 actions/min gives ~100 at every sample after the first minute,
  first-minute normalisation, actions falling out of the window, unsorted
  input is not supported (documented, not tested).
- `chart.rs`: unit tests asserting the output contains one `<path>` per
  series, escaped player names (`<b>` becomes `&lt;b&gt;`), the `</` escape
  in the JSON block, and the empty-state note when there are no points.
- `replay.rs`: an integration test gated on the presence of a local fixture
  path, using the user-supplied replay
  the local fixture replay (set `ARBITER_FIXTURE` to its path).
  It asserts two players are found with non-empty names and that each has
  actions. The replay is not committed to the repo. The test is skipped, not
  failed, when the file is absent.
- Manual check: open the generated HTML in the built-in browser, confirm
  hover, zoom, and reset work, and compare the averages against the score
  screen if the user has it.

## Out of scope for v1

Batch/directory input, configurable window size, EPM or per-action-type
breakdowns, PNG export, team games beyond rendering one line per player.
