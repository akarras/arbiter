# Macro Panels and APM Breakdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the single APM chart into a scrolling page of stacked panels on one shared time axis (APM, income, army value, supply, workers, unspent, losses) plus a per-player detail panel (APM by input kind, EPM, supply blocks), fed by the replay's tracker-event stream.

**Architecture:** `replay.rs` additionally decodes tracker `PlayerStats` samples and tags each action with its kind. A new pure `metrics.rs` turns samples and tagged actions into series. `chart.rs` becomes a multi-panel page assembler with its CSS and JS moved to `chart_assets.rs`; the JS draws every panel from one JSON block with a shared zoom/crosshair. `pipeline.rs` wires it. The CLI and the Tauri app are unchanged and both get the new page.

**Tech Stack:** unchanged (Rust 2024, `s2protocol` 3.5, `serde_json`, `anyhow`; Tauri app crate untouched).

**Spec:** `docs/superpowers/specs/2026-09-16-macro-panels-design.md`.

## Global Constraints

- No new crates in either crate.
- Time base 22.4 loops/s; APM window 60 s, step 5 s (unchanged). Tracker `delta` is on the same clock.
- The crate already scales `food_used`/`food_made` to supply units; do not divide by 4096.
- Tracker events carry `player_id` (1-based details order); game events carry `user_id`. `Player` carries both.
- `Point` is renamed to `{ secs, value }` (Task 2) and used for every series.
- Escaping rules unchanged: all user text through `escape_html`; JSON `<` as `\u003c`.
- Supply-block rule: `supply_made < 200 && supply_used >= supply_made - 0.5`, at least two consecutive samples. EPM rule: drop an action with the same kind as the player's previous action within 6 loops.
- Panel order on the page: APM, Income, Army value, Supply, Workers, Unspent, Losses, then Player detail.
- Fixture (not committed): `the local fixture replay`. It is a custom 4v4: 8 starting workers, supply above 200. Tests skip when it is absent.
- Commit messages end with exactly `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. `cargo test --workspace` and `cargo clippy --workspace --all-targets` pristine. Shell note: the Bash tool rejects very long commands; write files with the Write/Edit tools. Never search the filesystem from `/` or `C:\`; crate sources are under `C:\Users\user\.cargo\registry\src\index.crates.io-*\<crate>-<version>\`.
- Verified crate facts: `s2protocol::read_tracker_events(&str, &MPQ, &[u8]) -> Result<Vec<TrackerEvent>>`; `TrackerEvent { delta: u32, event: ReplayTrackerEvent }`; `ReplayTrackerEvent::PlayerStats(PlayerStatsEvent { player_id: u8, stats: PlayerStats })`; `PlayerStats` fields used: `minerals_collection_rate, vespene_collection_rate, minerals_current, vespene_current, workers_active_count, food_used, food_made, minerals_used_current_army, vespene_used_current_army, minerals_lost_army, vespene_lost_army` (all `i32`). Module path: `s2protocol::tracker_events::ReplayTrackerEvent`.

---

## File structure

| File | Responsibility |
| --- | --- |
| `src/replay.rs` (modify) | `ActionKind`, `Action.kind`, `Player.player_id`, `StatsSample`, `Replay.stats`, tracker decoding |
| `src/apm.rs` (modify) | `Point { secs, value }` rename |
| `src/metrics.rs` (new) | pure series builders: stats series, supply blocks, APM breakdown, effective loops |
| `src/chart.rs` (rewrite) | multi-panel page assembly and JSON |
| `src/chart_assets.rs` (new) | `CHART_CSS`, `JS` text moved out of `chart.rs` and extended |
| `src/pipeline.rs` (modify) | builds panels and details |
| `src/lib.rs` (modify) | `pub mod metrics; pub mod chart_assets;` |
| `tests/real_replay.rs` (modify) | tracker assertions, panel assertions |

---

### Task 1: Tracker samples and action kinds in the loader

**Files:**
- Modify: `src/replay.rs`, `tests/real_replay.rs`

**Interfaces:**
- Produces:
  - `pub enum ActionKind { Command, Selection, ControlGroup, Repeat, Retarget }` (derives `Debug, Clone, Copy, PartialEq, Eq`)
  - `Action { user_id, game_loop, kind: ActionKind }`
  - `Player.player_id: u8` (1-based details order)
  - `pub struct StatsSample { player_id: u8, game_loop: i64, minerals_rate: i32, vespene_rate: i32, minerals_unspent: i32, vespene_unspent: i32, workers: i32, supply_used: f64, supply_made: f64, army_minerals: i32, army_vespene: i32, lost_minerals: i32, lost_vespene: i32 }` (derives `Debug, Clone, PartialEq`)
  - `Replay.stats: Vec<StatsSample>` in loop order, kept players only
  - `fn action_kind(&ReplayGameEvent) -> Option<ActionKind>` replaces `counts_as_action`

- [ ] **Step 1: Failing unit tests** (replace `trigger_events_do_not_count` and `repeated_commands_count_as_actions` in `src/replay.rs` tests)

```rust
    #[test]
    fn trigger_events_have_no_kind() {
        let ev = ReplayGameEvent::TriggerKeyPressed(GameSTriggerKeyPressedEvent { m_key: 0, m_flags: 0 });
        assert_eq!(action_kind(&ev), None);
    }

    #[test]
    fn repeated_commands_are_repeat_actions() {
        use s2protocol::game_events::{GameECommandManagerState, GameSCommandManagerStateEvent};
        let ev = ReplayGameEvent::CommandManagerState(GameSCommandManagerStateEvent {
            m_state: GameECommandManagerState::EFireOnce,
            m_sequence: None,
        });
        assert_eq!(action_kind(&ev), Some(ActionKind::Repeat));
    }
```

- [ ] **Step 2: Failing integration assertions** (append inside the fixture test in `tests/real_replay.rs`, before the chart JSON section)

```rust
    assert!(!replay.stats.is_empty(), "fixture has tracker stats");
    let last_stats_loop = replay.stats.last().unwrap().game_loop;
    assert!((replay.duration_loops - last_stats_loop).abs() < 50, "tracker clock matches: {last_stats_loop} vs {}", replay.duration_loops);
    for p in &replay.players {
        let mine: Vec<_> = replay.stats.iter().filter(|s| s.player_id == p.player_id).collect();
        assert!(mine.len() > 50, "{} has only {} samples", p.name, mine.len());
        assert!(mine.iter().all(|s| (0.0..=400.0).contains(&s.supply_made)), "{} supply_made out of range", p.name);
        assert!(mine.iter().map(|s| s.workers).max().unwrap() > 10, "{} never had more than 10 workers", p.name);
        assert!(mine.windows(2).all(|w| w[0].game_loop <= w[1].game_loop));
    }
    let kinds: std::collections::BTreeSet<_> = replay.actions.iter().map(|a| format!("{:?}", a.kind)).collect();
    assert!(kinds.len() >= 4, "expected several action kinds, got {kinds:?}");
    eprintln!("stats samples {} kinds {kinds:?}", replay.stats.len());
```

Run: `cargo test` → compile errors (`action_kind`, `ActionKind`, `stats`, `player_id` missing). Expected.

- [ ] **Step 3: Implement**

In `src/replay.rs`:

```rust
use s2protocol::tracker_events::ReplayTrackerEvent;

/// What a counted action was, for the APM breakdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Command,
    Selection,
    ControlGroup,
    Repeat,
    Retarget,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub user_id: i64,
    pub game_loop: i64,
    pub kind: ActionKind,
}

/// One tracker `PlayerStats` sample (about every 160 loops per player).
/// Supply values are already in supply units; resources are raw counts.
#[derive(Debug, Clone, PartialEq)]
pub struct StatsSample {
    pub player_id: u8,
    pub game_loop: i64,
    pub minerals_rate: i32,
    pub vespene_rate: i32,
    pub minerals_unspent: i32,
    pub vespene_unspent: i32,
    pub workers: i32,
    pub supply_used: f64,
    pub supply_made: f64,
    pub army_minerals: i32,
    pub army_vespene: i32,
    pub lost_minerals: i32,
    pub lost_vespene: i32,
}
```

Add `pub player_id: u8,` to `Player` (doc: "1-based index in `replay.details` player order; tracker events and the game metadata use this id") and set it to `position as u8 + 1` in the `filter_map`. Add `pub stats: Vec<StatsSample>,` to `Replay` (doc: "Tracker samples in loop order for kept players; empty when the replay has no tracker stream").

Replace `counts_as_action` with:

```rust
/// The event types SC2's own APM counts, tagged by kind: commands,
/// selections, control groups, repeats of the previous command
/// (`CommandManagerState`), and re-issuing it on a new unit
/// (`CmdUpdateTargetUnit`). Camera moves and `CmdUpdateTargetPoint` are not
/// counted; fitting every combination against Blizzard's own numbers for an
/// 8-player fixture picked this set, matching within a few percent.
fn action_kind(event: &ReplayGameEvent) -> Option<ActionKind> {
    match event {
        ReplayGameEvent::Cmd(_) => Some(ActionKind::Command),
        ReplayGameEvent::SelectionDelta(_) => Some(ActionKind::Selection),
        ReplayGameEvent::ControlGroupUpdate(_) => Some(ActionKind::ControlGroup),
        ReplayGameEvent::CommandManagerState(_) => Some(ActionKind::Repeat),
        ReplayGameEvent::CmdUpdateTargetUnit(_) => Some(ActionKind::Retarget),
        _ => None,
    }
}
```

In the event loop: `if let Some(kind) = action_kind(&ev.event) { actions.push(Action { user_id: ev.user_id, game_loop, kind }); }`.

After the game-event loop (before the duration check), decode tracker stats:

```rust
    let stats = read_stats(path_str, &mpq, &contents, &players);
```

```rust
/// Tracker `PlayerStats` samples for kept players. A replay without a
/// tracker stream (or one that fails to decode) yields no samples; the
/// macro panels then show their empty state rather than failing the load.
fn read_stats(path_str: &str, mpq: &s2protocol::MPQ, contents: &[u8], players: &[Player]) -> Vec<StatsSample> {
    let Ok(events) = s2protocol::read_tracker_events(path_str, mpq, contents) else {
        return Vec::new();
    };
    let mut game_loop = 0i64;
    let mut out = Vec::new();
    for ev in &events {
        game_loop += i64::from(ev.delta);
        let ReplayTrackerEvent::PlayerStats(s) = &ev.event else {
            continue;
        };
        if !players.iter().any(|p| p.player_id == s.player_id) {
            continue;
        }
        let st = &s.stats;
        out.push(StatsSample {
            player_id: s.player_id,
            game_loop,
            minerals_rate: st.minerals_collection_rate,
            vespene_rate: st.vespene_collection_rate,
            minerals_unspent: st.minerals_current,
            vespene_unspent: st.vespene_current,
            workers: st.workers_active_count,
            supply_used: f64::from(st.food_used),
            supply_made: f64::from(st.food_made),
            army_minerals: st.minerals_used_current_army,
            army_vespene: st.vespene_used_current_army,
            lost_minerals: st.minerals_lost_army,
            lost_vespene: st.vespene_lost_army,
        });
    }
    out
}
```

Add `stats` to the returned `Replay`. Fix any other `Action { .. }` or `Player { .. }` literal in tests.

- [ ] **Step 4: Run** `cargo test --workspace` and `cargo clippy --workspace --all-targets` → green, pristine. Read the printed `stats samples` line.

- [ ] **Step 5: Commit**

```bash
git add src/replay.rs tests/real_replay.rs
git commit -m "Decode tracker stats and tag actions by kind

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Point rename and the metrics module

**Files:**
- Modify: `src/apm.rs`, `src/chart.rs`, `src/pipeline.rs`, `tests/real_replay.rs` (rename only)
- Create: `src/metrics.rs`; modify `src/lib.rs`

**Interfaces:**
- `apm::Point { pub secs: f64, pub value: f64 }`.
- `metrics::KINDS: [ActionKind; 5]` and `metrics::KIND_LABELS: [&str; 5]` = `["Commands", "Selections", "Control groups", "Repeats", "Retargets"]`.
- `metrics::series_from_stats(samples: &[StatsSample], f: impl Fn(&StatsSample) -> f64) -> Vec<Point>`
- `metrics::supply_blocks(samples: &[StatsSample]) -> Vec<(f64, f64)>`
- `metrics::apm_breakdown(actions: &[(i64, ActionKind)], last_event_loop: i64) -> Vec<Vec<Point>>` (5 entries, `KINDS` order)
- `metrics::effective_loops(actions: &[(i64, ActionKind)]) -> Vec<i64>`; `metrics::EFFECTIVE_GAP_LOOPS: i64 = 6`

- [ ] **Step 1: Rename** `Point.apm` → `Point.value` in `src/apm.rs` (struct + `rolling_apm` literal + tests), `src/chart.rs` (`render_table`, `render_json`, tests), `src/pipeline.rs` if referenced, `tests/real_replay.rs` if referenced. `cargo test` must stay green. Commit: `Rename Point.apm to Point.value for generic series`.

- [ ] **Step 2: Failing tests** — create `src/metrics.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::apm::LOOPS_PER_SECOND;

    fn sample(loop_secs: f64, used: f64, made: f64) -> StatsSample {
        StatsSample {
            player_id: 1,
            game_loop: (loop_secs * LOOPS_PER_SECOND) as i64,
            minerals_rate: 100,
            vespene_rate: 50,
            minerals_unspent: 300,
            vespene_unspent: 20,
            workers: 12,
            supply_used: used,
            supply_made: made,
            army_minerals: 1000,
            army_vespene: 200,
            lost_minerals: 10,
            lost_vespene: 5,
        }
    }

    #[test]
    fn stats_series_maps_each_sample_to_seconds_and_value() {
        let s = vec![sample(7.0, 10.0, 15.0), sample(14.0, 12.0, 15.0)];
        let pts = series_from_stats(&s, |x| f64::from(x.minerals_rate + x.vespene_rate));
        assert_eq!(pts.len(), 2);
        assert!((pts[0].secs - 7.0).abs() < 0.05);
        assert_eq!(pts[0].value, 150.0);
        assert!((pts[1].secs - 14.0).abs() < 0.05);
    }

    #[test]
    fn supply_blocks_need_two_consecutive_blocked_samples_and_merge() {
        let s = vec![
            sample(0.0, 10.0, 15.0),  // fine
            sample(7.0, 15.0, 15.0),  // blocked (single) -> ignored
            sample(14.0, 14.0, 23.0), // fine
            sample(21.0, 23.0, 23.0), // blocked
            sample(28.0, 23.0, 23.0), // blocked
            sample(35.0, 22.6, 23.0), // blocked (within 0.5)
            sample(42.0, 24.0, 31.0), // fine
            sample(49.0, 200.0, 200.0), // maxed, not a block
            sample(56.0, 200.0, 200.0),
        ];
        let blocks = supply_blocks(&s);
        assert_eq!(blocks.len(), 1);
        assert!((blocks[0].0 - 21.0).abs() < 0.05);
        assert!((blocks[0].1 - 35.0).abs() < 0.05);
    }

    #[test]
    fn supply_block_running_to_the_end_is_closed() {
        let s = vec![sample(0.0, 15.0, 15.0), sample(7.0, 15.0, 15.0)];
        let blocks = supply_blocks(&s);
        assert_eq!(blocks.len(), 1);
        assert!((blocks[0].1 - 7.0).abs() < 0.05);
    }

    #[test]
    fn breakdown_counts_only_each_kind() {
        let one_min = (60.0 * LOOPS_PER_SECOND) as i64;
        let actions: Vec<(i64, ActionKind)> = (1..=30)
            .map(|i| (i * 40, if i % 2 == 0 { ActionKind::Command } else { ActionKind::Selection }))
            .collect();
        let series = apm_breakdown(&actions, one_min);
        assert_eq!(series.len(), 5);
        let at_end = |k: usize| series[k].last().unwrap().value;
        assert!((at_end(0) - 15.0).abs() < 1e-9, "commands");
        assert!((at_end(1) - 15.0).abs() < 1e-9, "selections");
        assert_eq!(at_end(2), 0.0);
        assert_eq!(at_end(3), 0.0);
        assert_eq!(at_end(4), 0.0);
    }

    #[test]
    fn effective_loops_drop_same_kind_spam_within_six_loops() {
        use ActionKind::*;
        let actions = vec![(100, Selection), (103, Selection), (106, Selection), (113, Selection), (115, Command), (117, Command), (130, Command)];
        assert_eq!(effective_loops(&actions), vec![100, 113, 115, 130]);
    }
}
```

Run: `cargo test --lib metrics` → compile error (functions missing). Expected. (`pub mod metrics;` must be in `src/lib.rs` for the test to be discovered; add it now.)

- [ ] **Step 3: Implement `src/metrics.rs`** above the tests

```rust
//! Pure series builders for the macro panels and the APM breakdown.

use crate::apm::{self, Point};
use crate::replay::{ActionKind, StatsSample};

pub const KINDS: [ActionKind; 5] = [
    ActionKind::Command,
    ActionKind::Selection,
    ActionKind::ControlGroup,
    ActionKind::Repeat,
    ActionKind::Retarget,
];
pub const KIND_LABELS: [&str; 5] = ["Commands", "Selections", "Control groups", "Repeats", "Retargets"];

/// Two actions of the same kind closer than this are treated as spam for
/// EPM (0.25 s at 22.4 loops/s). A heuristic, not Blizzard's definition.
pub const EFFECTIVE_GAP_LOOPS: i64 = 6;

/// Supply cap above which a player cannot be "blocked".
const SUPPLY_CAP: f64 = 200.0;

pub fn series_from_stats(samples: &[StatsSample], f: impl Fn(&StatsSample) -> f64) -> Vec<Point> {
    samples
        .iter()
        .map(|s| Point { secs: apm::loops_to_secs(s.game_loop), value: f(s) })
        .collect()
}

/// `[from, to]` second intervals where supply used reached supply made
/// (within 0.5) below the cap for at least two consecutive samples.
pub fn supply_blocks(samples: &[StatsSample]) -> Vec<(f64, f64)> {
    let mut blocks = Vec::new();
    let mut start: Option<f64> = None;
    let mut run = 0usize;
    let mut prev_secs = 0.0;
    for s in samples {
        let secs = apm::loops_to_secs(s.game_loop);
        let blocked = s.supply_made < SUPPLY_CAP && s.supply_used >= s.supply_made - 0.5;
        if blocked {
            if start.is_none() {
                start = Some(secs);
                run = 0;
            }
            run += 1;
        } else if let Some(from) = start.take()
            && run >= 2
        {
            blocks.push((from, prev_secs));
        }
        prev_secs = secs;
    }
    if let Some(from) = start
        && run >= 2
    {
        blocks.push((from, prev_secs));
    }
    blocks
}

/// Rolling APM per kind, in `KINDS` order. `actions` must be in loop order.
pub fn apm_breakdown(actions: &[(i64, ActionKind)], last_event_loop: i64) -> Vec<Vec<Point>> {
    KINDS
        .iter()
        .map(|kind| {
            let loops: Vec<i64> = actions.iter().filter(|(_, k)| k == kind).map(|(l, _)| *l).collect();
            apm::rolling_apm(&loops, last_event_loop)
        })
        .collect()
}

/// Loops of the actions that survive the spam rule. `actions` must be in loop order.
pub fn effective_loops(actions: &[(i64, ActionKind)]) -> Vec<i64> {
    let mut out = Vec::with_capacity(actions.len());
    let mut prev: Option<(i64, ActionKind)> = None;
    for &(game_loop, kind) in actions {
        let spam = prev.is_some_and(|(pl, pk)| pk == kind && game_loop - pl <= EFFECTIVE_GAP_LOOPS);
        if !spam {
            out.push(game_loop);
        }
        prev = Some((game_loop, kind));
    }
    out
}
```

- [ ] **Step 4: Run** `cargo test --workspace`, `cargo clippy --workspace --all-targets` → green, pristine.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/metrics.rs
git commit -m "Add metrics: stats series, supply blocks, APM breakdown, effective actions

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Multi-panel chart page

**Files:**
- Rewrite: `src/chart.rs`
- Create: `src/chart_assets.rs`; modify `src/lib.rs`
- Modify: `src/pipeline.rs` (minimal: build the new `Chart` with only the APM panel so the crate compiles; Task 4 adds the rest)

**Interfaces:**
- Produces in `chart.rs`:

```rust
pub enum PanelKind { Lines }
pub struct PanelSeries { pub player: usize, pub label: String, pub points: Vec<Point> }
pub struct Panel { pub id: String, pub title: String, pub unit: String, pub kind: PanelKind, pub series: Vec<PanelSeries> }
pub struct Detail { pub player: usize, pub breakdown: Vec<PanelSeries>, pub epm: Vec<Point>, pub blocks: Vec<(f64, f64)> }
pub struct PlayerLegend { pub name: String, pub race: String, pub result: String, pub average: f64, pub game_apm: Option<f64> }
pub struct Chart { pub title: String, pub map: String, pub duration_secs: f64, pub players: Vec<PlayerLegend>, pub panels: Vec<Panel>, pub details: Vec<Detail> }
pub fn render(chart: &Chart) -> String
pub fn escape_html / escape_json / fmt_time  (unchanged)
```

  (`PanelKind` has one variant today; it exists so a stacked panel can be added without changing the JSON shape.)
- `chart_assets::CHART_CSS`, `chart_assets::JS`.

- [ ] **Step 1: Failing tests** — replace the tests module in `src/chart.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::apm::Point;

    fn pts(vals: &[f64]) -> Vec<Point> {
        vals.iter().enumerate().map(|(i, &v)| Point { secs: (i as f64 + 1.0) * 5.0, value: v }).collect()
    }

    fn legend(name: &str) -> PlayerLegend {
        PlayerLegend { name: name.to_string(), race: "Zerg".to_string(), result: "Win".to_string(), average: 123.4, game_apm: None }
    }

    fn chart(players: Vec<&str>, panels: Vec<Panel>, details: Vec<Detail>) -> Chart {
        Chart {
            title: "APM".to_string(),
            map: "Tuonela LE".to_string(),
            duration_secs: 15.0,
            players: players.into_iter().map(legend).collect(),
            panels,
            details,
        }
    }

    fn panel(id: &str, series: Vec<(usize, &str, Vec<Point>)>) -> Panel {
        Panel {
            id: id.to_string(),
            title: id.to_uppercase(),
            unit: "per min".to_string(),
            kind: PanelKind::Lines,
            series: series.into_iter().map(|(player, label, points)| PanelSeries { player, label: label.to_string(), points }).collect(),
        }
    }

    #[test]
    fn escapes_html_special_characters() {
        assert_eq!(escape_html("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn escapes_json_strings_and_angle_brackets() {
        assert_eq!(escape_json("a\"b\\c\n</script>"), "a\\\"b\\\\c\\n\\u003c/script>");
    }

    #[test]
    fn formats_time_as_minutes_and_seconds() {
        assert_eq!(fmt_time(65.0), "1:05");
    }

    #[test]
    fn renders_one_section_and_table_per_panel() {
        let c = chart(
            vec!["Bob", "Ann"],
            vec![
                panel("apm", vec![(0, "Bob", pts(&[60.0, 72.0])), (1, "Ann", pts(&[30.0, 36.0]))]),
                panel("income", vec![(0, "Bob", pts(&[800.0, 900.0])), (1, "Ann", pts(&[700.0, 750.0]))]),
            ],
            vec![],
        );
        let html = render(&c);
        assert!(html.starts_with("<!doctype html>"));
        assert_eq!(html.matches("<section class=\"panel\"").count(), 2);
        assert!(html.contains(r#"id="panel-apm""#));
        assert!(html.contains(r#"id="panel-income""#));
        assert_eq!(html.matches("<details class=\"table\">").count(), 2);
        assert!(html.contains("<td>0:05</td><td>60</td><td>30</td>"));
        assert!(html.contains("<td>0:05</td><td>800</td><td>700</td>"));
    }

    #[test]
    fn json_carries_panels_players_and_details() {
        let c = chart(
            vec!["<b>Bob"],
            vec![panel("apm", vec![(0, "<b>Bob", pts(&[60.0]))])],
            vec![Detail {
                player: 0,
                breakdown: vec![PanelSeries { player: 0, label: "Commands".to_string(), points: pts(&[40.0]) }],
                epm: pts(&[50.0]),
                blocks: vec![(5.0, 10.0)],
            }],
        );
        let html = render(&c);
        let start = html.find(r#"<script id="data" type="application/json">"#).unwrap();
        let end = start + html[start..].find("</script>").unwrap();
        let json: serde_json::Value = serde_json::from_str(&html[start + r#"<script id="data" type="application/json">"#.len()..end]).unwrap();
        assert_eq!(json["players"][0]["name"], "<b>Bob");
        assert_eq!(json["panels"][0]["id"], "apm");
        assert_eq!(json["panels"][0]["series"][0]["player"], 0);
        assert_eq!(json["panels"][0]["series"][0]["points"][0][1], 60.0);
        assert_eq!(json["details"][0]["blocks"][0][0], 5.0);
        assert_eq!(json["details"][0]["breakdown"][0]["label"], "Commands");
        assert_eq!(json["details"][0]["epm"][0][1], 50.0);
        assert!(!html[..start].contains("<b>Bob"), "raw name never in rendered HTML");
        assert!(!html[start..end].contains("<b>"), "JSON escapes every <");
    }

    #[test]
    fn detail_section_has_one_option_per_player() {
        let c = chart(vec!["Bob", "Ann"], vec![], vec![
            Detail { player: 0, breakdown: vec![], epm: vec![], blocks: vec![] },
            Detail { player: 1, breakdown: vec![], epm: vec![], blocks: vec![] },
        ]);
        let html = render(&c);
        assert!(html.contains(r#"<select id="detail-player">"#));
        assert_eq!(html.matches("<option value=").count(), 2);
        assert!(html.contains(r#"<option value="1">Ann</option>"#));
    }

    #[test]
    fn legend_shows_race_result_average_and_game_apm() {
        let mut l = legend("Bob");
        l.game_apm = Some(61.4);
        let c = Chart { title: "APM".to_string(), map: "M".to_string(), duration_secs: 1.0, players: vec![l], panels: vec![], details: vec![] };
        let html = render(&c);
        assert!(html.contains("Zerg &middot; Win &middot; 123 APM &middot; game says 61"));
        assert!(html.contains("--series-1"));
    }

    #[test]
    fn panel_without_points_renders_empty_note() {
        let c = chart(vec!["Bob"], vec![panel("income", vec![(0, "Bob", vec![])])], vec![]);
        let html = render(&c);
        assert!(html.contains("No tracker data in this replay."));
    }
}
```

Run: `cargo test --lib chart` → compile errors. Expected.

- [ ] **Step 2: Create `src/chart_assets.rs`**

Move the existing `CHART_CSS` and `JS` constants out of `chart.rs` into this file as `pub const CHART_CSS: &str` and `pub const JS: &str`, then replace their contents with the versions below.

```rust
//! Static CSS and JS for the chart page. Kept apart from the Rust assembly in
//! `chart.rs` so each file has one job.

pub const CHART_CSS: &str = r#"
figure{margin:0 auto;max-width:1040px;background:var(--surface);border:1px solid var(--border);border-radius:8px;padding:16px}
.legend{list-style:none;display:flex;flex-wrap:wrap;gap:8px 24px;margin:0 0 4px;padding:0;max-width:1040px;margin-left:auto;margin-right:auto}
.legend li{display:flex;align-items:center;gap:8px}
.swatch{width:12px;height:12px;border-radius:3px;display:inline-block;flex:none}
.legend .name{font-weight:600}
.legend .sub{color:var(--text-2)}
.panel{max-width:1040px;margin:16px auto 0;background:var(--surface);border:1px solid var(--border);border-radius:8px;padding:12px 16px}
.panel h2{font-size:14px;font-weight:600;margin:0}
.panel .unit{color:var(--muted);font-size:12px;margin:0 0 6px}
.panel .head{display:flex;align-items:baseline;gap:12px;flex-wrap:wrap}
.panel select{margin-left:auto;padding:4px 8px;border:1px solid var(--border);border-radius:6px;background:var(--page);color:var(--text)}
.plot{position:relative}
svg{width:100%;height:auto;display:block;user-select:none;cursor:crosshair}
.grid{stroke:var(--grid);stroke-width:1}
.axis{stroke:var(--axis);stroke-width:1}
.tick{fill:var(--muted);font-size:12px;font-variant-numeric:tabular-nums}
.line{fill:none;stroke-width:2;stroke-linejoin:round;stroke-linecap:round}
.area{stroke:none;opacity:.85}
.epm{fill:none;stroke:var(--text-2);stroke-width:2;stroke-dasharray:5 4}
.band{fill:var(--series-8);opacity:.14}
.label{fill:var(--text);font-size:12px}
.xhair{stroke:var(--muted);stroke-dasharray:3 3;pointer-events:none}
.dot{stroke:var(--surface);stroke-width:2;pointer-events:none}
.brush{fill:var(--series-1);opacity:.12;pointer-events:none}
.tooltip{position:absolute;top:8px;pointer-events:none;background:var(--surface);color:var(--text);border:1px solid var(--border);border-radius:6px;padding:6px 10px;font-size:12px;font-variant-numeric:tabular-nums;box-shadow:0 2px 8px rgba(0,0,0,.15);white-space:nowrap}
.tooltip .t{color:var(--text-2);margin-bottom:2px}
.tooltip .row{display:flex;align-items:center;gap:6px}
.klegend{display:flex;flex-wrap:wrap;gap:6px 16px;margin:6px 0 0;padding:0;list-style:none;font-size:12px;color:var(--text-2)}
.klegend li{display:flex;align-items:center;gap:6px}
.hint,.empty{color:var(--muted);font-size:12px;margin:8px 0 0;text-align:center}
.empty{font-size:14px;padding:32px 0}
.table{margin:8px 0 0;color:var(--text-2)}
.table summary{cursor:pointer;font-size:12px}
table{border-collapse:collapse;margin-top:8px;font-variant-numeric:tabular-nums;font-size:12px}
th,td{text-align:right;padding:2px 10px;border-bottom:1px solid var(--grid)}
th:first-child,td:first-child{text-align:left}
"#;

pub const JS: &str = r#"
(() => {
  const dataEl = document.getElementById('data');
  if (!dataEl) return;
  const data = JSON.parse(dataEl.textContent);
  const NS = 'http://www.w3.org/2000/svg';
  const W = 960, H = 260, M = {top: 14, right: 110, bottom: 32, left: 60};
  const pw = W - M.left - M.right, ph = H - M.top - M.bottom;
  const colour = i => 'var(--series-' + (i % 8 + 1) + ')';
  const KIND_COLOURS = [1, 2, 3, 4, 5].map(colour);
  let view = {x0: 0, x1: data.duration};
  let detailPlayer = 0;
  const escapeHtml = s => s.replace(/[&<>"']/g, c => ({'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'}[c]));
  const fmtTime = s => { s = Math.round(s); return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0'); };
  const niceStep = (range, target) => {
    const raw = Math.max(range, 1e-9) / target, pow = Math.pow(10, Math.floor(Math.log10(raw)));
    for (const m of [1, 2, 5, 10]) if (m * pow >= raw) return m * pow;
    return 10 * pow;
  };
  const timeStep = range => { for (const s of [5, 10, 15, 30, 60, 120, 300, 600, 900]) if (range / s <= 8) return s; return 1800; };
  const nearest = (pts, t) => {
    let best = null, bd = Infinity;
    for (const p of pts) { const d = Math.abs(p[0] - t); if (d < bd) { bd = d; best = p; } }
    return best;
  };

  // One drawable per <svg>: panels are line charts, the detail is stacked.
  const drawables = [];
  for (const p of data.panels) {
    const svg = document.getElementById('svg-' + p.id);
    if (svg) drawables.push({svg, tip: document.getElementById('tip-' + p.id), panel: p, kind: 'lines'});
  }
  const detailSvg = document.getElementById('svg-detail');
  if (detailSvg && data.details.length) drawables.push({svg: detailSvg, tip: document.getElementById('tip-detail'), kind: 'detail'});

  function el(svg, tag, attrs, text) {
    const e = document.createElementNS(NS, tag);
    for (const k in attrs) e.setAttribute(k, attrs[k]);
    if (text !== undefined) e.textContent = text;
    svg.appendChild(e);
    return e;
  }

  function frame(d, ymax) {
    const {svg} = d, {x0, x1} = view;
    const ystep = niceStep(ymax, 4), ytop = Math.max(ystep, Math.ceil(ymax / ystep) * ystep);
    const sx = t => M.left + (t - x0) / (x1 - x0) * pw;
    const sy = v => M.top + ph - v / ytop * ph;
    for (let v = 0; v <= ytop + 1e-9; v += ystep) {
      el(svg, 'line', {x1: M.left, x2: M.left + pw, y1: sy(v), y2: sy(v), class: 'grid'});
      el(svg, 'text', {x: M.left - 8, y: sy(v) + 4, class: 'tick', 'text-anchor': 'end'}, Math.round(v));
    }
    const ts = timeStep(x1 - x0);
    for (let t = Math.ceil(x0 / ts) * ts; t <= x1 + 1e-9; t += ts) {
      el(svg, 'line', {x1: sx(t), x2: sx(t), y1: M.top + ph, y2: M.top + ph + 5, class: 'axis'});
      el(svg, 'text', {x: sx(t), y: M.top + ph + 18, class: 'tick', 'text-anchor': 'middle'}, fmtTime(t));
    }
    el(svg, 'line', {x1: M.left, x2: M.left + pw, y1: M.top + ph, y2: M.top + ph, class: 'axis'});
    d.sx = sx; d.sy = sy;
  }

  function pathOf(pts, sx, sy) {
    return pts.map((p, j) => (j ? 'L' : 'M') + sx(p[0]).toFixed(1) + ' ' + sy(p[1]).toFixed(1)).join(' ');
  }

  function drawLines(d) {
    const {svg, panel} = d, {x0, x1} = view;
    const visible = panel.series.map(s => s.points.filter(p => p[0] >= x0 && p[0] <= x1));
    const ymax = Math.max(1, ...visible.flat().map(p => p[1]));
    frame(d, ymax);
    const labelYs = [];
    panel.series.forEach((s, i) => {
      const pts = visible[i];
      if (!pts.length) return;
      el(svg, 'path', {d: pathOf(pts, d.sx, d.sy), class: 'line', style: 'stroke:' + colour(s.player)});
      if (data.players.length <= 4) {
        const last = pts[pts.length - 1];
        let y = d.sy(last[1]) + 4;
        while (labelYs.some(o => Math.abs(o - y) < 14)) y += 14;
        labelYs.push(y);
        el(svg, 'text', {x: d.sx(last[0]) + 8, y, class: 'label'}, s.label);
      }
    });
    d.visible = visible;
  }

  function drawDetail(d) {
    const {svg} = d, {x0, x1} = view;
    const det = data.details[detailPlayer];
    const n = det.breakdown.length ? det.breakdown[0].points.length : 0;
    // cumulative stacks share the breakdown's sample times
    const cum = det.breakdown.map(() => []);
    for (let i = 0; i < n; i++) {
      let acc = 0;
      det.breakdown.forEach((s, k) => { acc += s.points[i][1]; cum[k].push([s.points[i][0], acc]); });
    }
    const top = cum.length ? cum[cum.length - 1] : [];
    const inView = p => p[0] >= x0 && p[0] <= x1;
    const ymax = Math.max(1, ...top.filter(inView).map(p => p[1]), ...det.epm.filter(inView).map(p => p[1]));
    frame(d, ymax);
    for (const [from, to] of det.blocks) {
      const a = Math.max(from, x0), b = Math.min(to, x1);
      if (b <= a) continue;
      el(svg, 'rect', {x: d.sx(a), y: M.top, width: d.sx(b) - d.sx(a), height: ph, class: 'band'});
    }
    for (let k = cum.length - 1; k >= 0; k--) {
      const upper = cum[k].filter(inView);
      if (!upper.length) continue;
      const lower = k ? cum[k - 1].filter(inView) : upper.map(p => [p[0], 0]);
      const dpath = pathOf(upper, d.sx, d.sy) + ' ' + lower.slice().reverse().map(p => 'L' + d.sx(p[0]).toFixed(1) + ' ' + d.sy(p[1]).toFixed(1)).join(' ') + ' Z';
      el(svg, 'path', {d: dpath, class: 'area', style: 'fill:' + KIND_COLOURS[k]});
    }
    const epm = det.epm.filter(inView);
    if (epm.length) el(svg, 'path', {d: pathOf(epm, d.sx, d.sy), class: 'epm'});
    d.visible = det.breakdown.map((s, k) => cum[k]).concat([det.epm]);
    d.labels = det.breakdown.map(s => s.label).concat(['EPM']);
  }

  function draw(d) {
    d.svg.innerHTML = '';
    if (d.kind === 'lines') drawLines(d); else drawDetail(d);
    el(d.svg, 'line', {class: 'xhair', y1: M.top, y2: M.top + ph, x1: 0, x2: 0, visibility: 'hidden'});
    el(d.svg, 'rect', {class: 'brush', y: M.top, height: ph, x: 0, width: 0});
    const hit = el(d.svg, 'rect', {x: M.left, y: M.top, width: pw, height: ph, fill: 'transparent'});
    hit.addEventListener('mousemove', e => onMove(d, e));
    hit.addEventListener('mouseleave', hideHover);
    hit.addEventListener('mousedown', e => onDown(d, e));
    d.svg.addEventListener('dblclick', () => { view = {x0: 0, x1: data.duration}; hideHover(); drawAll(); });
  }
  function drawAll() { drawables.forEach(draw); }

  const toSvgX = (svg, e) => {
    const pt = svg.createSVGPoint();
    pt.x = e.clientX; pt.y = e.clientY;
    return pt.matrixTransform(svg.getScreenCTM().inverse()).x;
  };
  const xToTime = x => view.x0 + (x - M.left) / pw * (view.x1 - view.x0);

  function onMove(d, e) {
    const t = xToTime(toSvgX(d.svg, e));
    const x = d.sx(t);
    for (const o of drawables) {
      const xh = o.svg.querySelector('.xhair');
      xh.setAttribute('x1', x); xh.setAttribute('x2', x); xh.setAttribute('visibility', 'visible');
      if (o !== d) o.tip.hidden = true;
    }
    let rows = '<div class="t">' + fmtTime(t) + '</div>';
    if (d.kind === 'lines') {
      d.panel.series.forEach((s, i) => {
        const p = nearest(d.visible[i], t);
        rows += '<div class="row"><span class="swatch" style="background:' + colour(s.player) + '"></span><span>' + escapeHtml(s.label) + '</span><span style="margin-left:auto;padding-left:12px">' + (p ? Math.round(p[1]) : '-') + '</span></div>';
      });
    } else {
      const det = data.details[detailPlayer];
      det.breakdown.forEach((s, k) => {
        const p = nearest(s.points, t);
        rows += '<div class="row"><span class="swatch" style="background:' + KIND_COLOURS[k] + '"></span><span>' + escapeHtml(s.label) + '</span><span style="margin-left:auto;padding-left:12px">' + (p ? Math.round(p[1]) : '-') + '</span></div>';
      });
      const p = nearest(det.epm, t);
      rows += '<div class="row"><span>EPM</span><span style="margin-left:auto;padding-left:12px">' + (p ? Math.round(p[1]) : '-') + '</span></div>';
    }
    d.tip.innerHTML = rows;
    d.tip.hidden = false;
    const rect = d.svg.getBoundingClientRect(), px = x / W * rect.width;
    d.tip.style.left = (px + 12 + d.tip.offsetWidth > rect.width ? px - 12 - d.tip.offsetWidth : px + 12) + 'px';
  }
  function hideHover() {
    for (const o of drawables) {
      o.tip.hidden = true;
      const xh = o.svg.querySelector('.xhair');
      if (xh) xh.setAttribute('visibility', 'hidden');
    }
  }
  function onDown(d, e) {
    if (e.button !== 0) return;
    e.preventDefault();
    const clamp = x => Math.min(Math.max(x, M.left), M.left + pw);
    const startX = clamp(toSvgX(d.svg, e));
    const brush = d.svg.querySelector('.brush');
    const move = ev => {
      const x = clamp(toSvgX(d.svg, ev));
      brush.setAttribute('x', Math.min(startX, x)); brush.setAttribute('width', Math.abs(x - startX));
    };
    const up = ev => {
      window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up);
      const x = clamp(toSvgX(d.svg, ev));
      brush.setAttribute('width', 0);
      if (Math.abs(x - startX) < 6) return;
      const a = xToTime(Math.min(startX, x)), b = xToTime(Math.max(startX, x));
      if (b - a < 10) return;
      view = {x0: a, x1: b};
      hideHover();
      drawAll();
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  }

  const sel = document.getElementById('detail-player');
  if (sel) sel.addEventListener('change', () => { detailPlayer = +sel.value; const d = drawables.find(o => o.kind === 'detail'); if (d) draw(d); });
  drawAll();
})();
"#;
```

- [ ] **Step 3: Rewrite `src/chart.rs`** (keep `escape_html`, `escape_json`, `fmt_time` exactly as they are)

```rust
//! Renders the interactive HTML page: header, legend, one section per panel
//! with a data table, the per-player detail section, and a JSON block the
//! inline JS (in `chart_assets`) draws from.

use std::fmt::Write as _;

use crate::apm::Point;
use crate::chart_assets::{CHART_CSS, JS};
use crate::theme;

pub enum PanelKind {
    Lines,
}

pub struct PanelSeries {
    pub player: usize,
    pub label: String,
    pub points: Vec<Point>,
}

pub struct Panel {
    pub id: String,
    pub title: String,
    pub unit: String,
    pub kind: PanelKind,
    pub series: Vec<PanelSeries>,
}

/// One player's breakdown: APM by kind (stacked), EPM, supply-block bands.
pub struct Detail {
    pub player: usize,
    pub breakdown: Vec<PanelSeries>,
    pub epm: Vec<Point>,
    pub blocks: Vec<(f64, f64)>,
}

pub struct PlayerLegend {
    pub name: String,
    pub race: String,
    pub result: String,
    pub average: f64,
    pub game_apm: Option<f64>,
}

pub struct Chart {
    pub title: String,
    pub map: String,
    pub duration_secs: f64,
    pub players: Vec<PlayerLegend>,
    pub panels: Vec<Panel>,
    pub details: Vec<Detail>,
}

// escape_html, escape_json, fmt_time unchanged

pub fn render(chart: &Chart) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = write!(html, "<title>{}</title>\n<style>{}{}</style>\n</head>\n<body>\n", escape_html(&chart.title), theme::CSS, CHART_CSS);
    html.push_str("<main class=\"viz-root\">\n<header>\n");
    let _ = writeln!(html, "<h1>{}</h1>", escape_html(&chart.map));
    let _ = writeln!(
        html,
        "<p class=\"meta\">Match length {} &middot; {} players &middot; drag to zoom, double-click to reset</p>\n</header>",
        fmt_time(chart.duration_secs),
        chart.players.len()
    );
    render_legend(&mut html, chart);
    for panel in &chart.panels {
        render_panel(&mut html, panel);
    }
    render_detail(&mut html, chart);
    html.push_str("</main>\n");
    let _ = writeln!(html, "<script id=\"data\" type=\"application/json\">{}</script>", render_json(chart));
    let _ = write!(html, "<script>{}</script>\n</body>\n</html>\n", JS);
    html
}

fn render_legend(html: &mut String, chart: &Chart) {
    html.push_str("<ul class=\"legend\">\n");
    for (i, p) in chart.players.iter().enumerate() {
        let game = match p.game_apm {
            Some(g) => format!(" &middot; game says {g:.0}"),
            None => String::new(),
        };
        let _ = writeln!(
            html,
            "<li><span class=\"swatch\" style=\"background:var(--series-{})\"></span><span class=\"name\">{}</span><span class=\"sub\">{} &middot; {} &middot; {:.0} APM{}</span></li>",
            i % 8 + 1,
            escape_html(&p.name),
            escape_html(&p.race),
            escape_html(&p.result),
            p.average,
            game
        );
    }
    html.push_str("</ul>\n");
}

fn render_panel(html: &mut String, panel: &Panel) {
    let has_points = panel.series.iter().any(|s| !s.points.is_empty());
    let _ = writeln!(html, "<section class=\"panel\" id=\"panel-{}\">", escape_html(&panel.id));
    let _ = writeln!(html, "<div class=\"head\"><h2>{}</h2><p class=\"unit\">{}</p></div>", escape_html(&panel.title), escape_html(&panel.unit));
    if has_points {
        let _ = writeln!(
            html,
            "<div class=\"plot\"><svg id=\"svg-{0}\" viewBox=\"0 0 960 260\" role=\"img\" aria-label=\"{1}\"></svg><div id=\"tip-{0}\" class=\"tooltip\" hidden></div></div>",
            escape_html(&panel.id),
            escape_html(&panel.title)
        );
        render_table(html, &panel.series);
    } else {
        html.push_str("<p class=\"empty\">No tracker data in this replay.</p>\n");
    }
    html.push_str("</section>\n");
}

fn render_detail(html: &mut String, chart: &Chart) {
    if chart.details.is_empty() {
        return;
    }
    html.push_str("<section class=\"panel\" id=\"panel-detail\">\n<div class=\"head\"><h2>Player detail</h2><p class=\"unit\">APM by input kind, EPM (dashed), supply blocks (shaded)</p>");
    html.push_str("<select id=\"detail-player\">");
    for d in &chart.details {
        let name = chart.players.get(d.player).map(|p| p.name.as_str()).unwrap_or("?");
        let _ = write!(html, "<option value=\"{}\">{}</option>", d.player, escape_html(name));
    }
    html.push_str("</select></div>\n");
    html.push_str("<div class=\"plot\"><svg id=\"svg-detail\" viewBox=\"0 0 960 260\" role=\"img\" aria-label=\"Player detail\"></svg><div id=\"tip-detail\" class=\"tooltip\" hidden></div></div>\n");
    html.push_str("<ul class=\"klegend\">");
    for (i, label) in crate::metrics::KIND_LABELS.iter().enumerate() {
        let _ = write!(html, "<li><span class=\"swatch\" style=\"background:var(--series-{})\"></span>{}</li>", i + 1, label);
    }
    html.push_str("<li><span class=\"swatch\" style=\"background:var(--text-2)\"></span>EPM</li></ul>\n</section>\n");
}

/// Rows follow the first series' sample times; other series are aligned by index.
fn render_table(html: &mut String, series: &[PanelSeries]) {
    let rows = series.iter().map(|s| s.points.len()).max().unwrap_or(0);
    html.push_str("<details class=\"table\">\n<summary>Data table</summary>\n<table>\n<tr><th>Time</th>");
    for s in series {
        let _ = write!(html, "<th>{}</th>", escape_html(&s.label));
    }
    html.push_str("</tr>\n");
    let times = series.first().map(|s| &s.points[..]).unwrap_or(&[]);
    for row in 0..rows {
        let secs = times.get(row).map(|p| p.secs).unwrap_or(0.0);
        let _ = write!(html, "<tr><td>{}</td>", fmt_time(secs));
        for s in series {
            match s.points.get(row) {
                Some(p) => {
                    let _ = write!(html, "<td>{:.0}</td>", p.value);
                }
                None => html.push_str("<td></td>"),
            }
        }
        html.push_str("</tr>\n");
    }
    html.push_str("</table>\n</details>\n");
}

fn write_points(json: &mut String, points: &[Point]) {
    json.push('[');
    for (j, p) in points.iter().enumerate() {
        if j > 0 {
            json.push(',');
        }
        let _ = write!(json, "[{:.1},{:.1}]", p.secs, p.value);
    }
    json.push(']');
}

fn write_series(json: &mut String, series: &[PanelSeries]) {
    json.push('[');
    for (i, s) in series.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "{{\"player\":{},\"label\":\"{}\",\"points\":", s.player, escape_json(&s.label));
        write_points(json, &s.points);
        json.push('}');
    }
    json.push(']');
}

fn render_json(chart: &Chart) -> String {
    let mut json = String::new();
    let _ = write!(json, "{{\"duration\":{:.1},\"players\":[", chart.duration_secs);
    for (i, p) in chart.players.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "{{\"name\":\"{}\"}}", escape_json(&p.name));
    }
    json.push_str("],\"panels\":[");
    for (i, p) in chart.panels.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let kind = match p.kind {
            PanelKind::Lines => "lines",
        };
        let _ = write!(json, "{{\"id\":\"{}\",\"title\":\"{}\",\"unit\":\"{}\",\"kind\":\"{}\",\"series\":", escape_json(&p.id), escape_json(&p.title), escape_json(&p.unit), kind);
        write_series(&mut json, &p.series);
        json.push('}');
    }
    json.push_str("],\"details\":[");
    for (i, d) in chart.details.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "{{\"player\":{},\"breakdown\":", d.player);
        write_series(&mut json, &d.breakdown);
        json.push_str(",\"epm\":");
        write_points(&mut json, &d.epm);
        json.push_str(",\"blocks\":[");
        for (j, (a, b)) in d.blocks.iter().enumerate() {
            if j > 0 {
                json.push(',');
            }
            let _ = write!(json, "[{a:.1},{b:.1}]");
        }
        json.push_str("]}");
    }
    json.push_str("]}");
    json
}
```

- [ ] **Step 4: Minimal `pipeline.rs` update so it compiles** — build `players: Vec<PlayerLegend>` from the existing per-player data and one `Panel { id: "apm", title: "APM", unit: "actions per minute", kind: Lines, series: one PanelSeries per player with label = name }`, `details: vec![]`. Task 4 replaces this body.

- [ ] **Step 5: `src/lib.rs`**: add `pub mod chart_assets;` (and `pub mod metrics;` if not already).

- [ ] **Step 6: Run** `cargo test --workspace`, `cargo clippy --workspace --all-targets` → green, pristine. The integration test's JSON assertions from v2 (series order/last point) must be updated to read `panels[0].series` instead of `series`.

- [ ] **Step 7: Commit**

```bash
git add src/lib.rs src/chart.rs src/chart_assets.rs src/pipeline.rs tests/real_replay.rs
git commit -m "Render a multi-panel chart page with a shared time axis

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Wire the panels and verify end to end

**Files:**
- Modify: `src/pipeline.rs`, `tests/real_replay.rs`

**Interfaces:**
- Consumes: `metrics::*`, `chart::*`, `replay::{Replay, StatsSample, ActionKind}`.

- [ ] **Step 1: Failing integration assertions** (append to the fixture test)

```rust
    let html = arbiter::pipeline::chart_html(path).expect("chart renders");
    let start = html.find(r#"<script id="data" type="application/json">"#).unwrap() + r#"<script id="data" type="application/json">"#.len();
    let end = start + html[start..].find("</script>").unwrap();
    let data: serde_json::Value = serde_json::from_str(&html[start..end]).unwrap();
    let ids: Vec<&str> = data["panels"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["apm", "income", "army", "supply", "workers", "unspent", "losses"]);
    for p in data["panels"].as_array().unwrap() {
        assert_eq!(p["series"].as_array().unwrap().len(), replay.players.len(), "panel {} has one series per player", p["id"]);
        assert!(p["series"].as_array().unwrap().iter().all(|s| !s["points"].as_array().unwrap().is_empty()), "panel {} has points", p["id"]);
    }
    assert_eq!(data["details"].as_array().unwrap().len(), replay.players.len());
    let d0 = &data["details"][0];
    assert_eq!(d0["breakdown"].as_array().unwrap().len(), 5);
    assert!(!d0["epm"].as_array().unwrap().is_empty());
    eprintln!("supply blocks per player: {:?}", data["details"].as_array().unwrap().iter().map(|d| d["blocks"].as_array().unwrap().len()).collect::<Vec<_>>());
```

Remove the older v2 per-series assertion block if it duplicates this. Run: `cargo test --test real_replay` → fails (only the apm panel exists). Expected.

- [ ] **Step 2: Implement `pipeline::chart_html`**

```rust
pub fn chart_html(path: &Path) -> Result<String> {
    let replay = load_guarded(path)?;
    let mut players = Vec::new();
    let mut apm_series = Vec::new();
    let mut details = Vec::new();
    for (i, player) in replay.players.iter().enumerate() {
        let mut actions: Vec<(i64, ActionKind)> = replay
            .actions
            .iter()
            .filter(|a| a.user_id == player.user_id)
            .map(|a| (a.game_loop, a.kind))
            .collect();
        actions.sort_by_key(|(l, _)| *l);
        let loops: Vec<i64> = actions.iter().map(|(l, _)| *l).collect();
        players.push(chart::PlayerLegend {
            name: player.name.clone(),
            race: player.race.clone(),
            result: player.result.clone(),
            // Over the player's own time in the game, as Blizzard does.
            average: apm::average_apm(loops.len(), player.last_event_loop),
            game_apm: player.game_apm,
        });
        apm_series.push(chart::PanelSeries { player: i, label: player.name.clone(), points: apm::rolling_apm(&loops, player.last_event_loop) });
        let samples: Vec<StatsSample> = replay.stats.iter().filter(|s| s.player_id == player.player_id).cloned().collect();
        details.push(chart::Detail {
            player: i,
            breakdown: metrics::apm_breakdown(&actions, player.last_event_loop)
                .into_iter()
                .zip(metrics::KIND_LABELS)
                .map(|(points, label)| chart::PanelSeries { player: i, label: label.to_string(), points })
                .collect(),
            epm: apm::rolling_apm(&metrics::effective_loops(&actions), player.last_event_loop),
            blocks: metrics::supply_blocks(&samples),
        });
    }
    let stat_panel = |id: &str, title: &str, unit: &str, f: fn(&StatsSample) -> f64| chart::Panel {
        id: id.to_string(),
        title: title.to_string(),
        unit: unit.to_string(),
        kind: chart::PanelKind::Lines,
        series: replay
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let samples: Vec<StatsSample> = replay.stats.iter().filter(|s| s.player_id == p.player_id).cloned().collect();
                chart::PanelSeries { player: i, label: p.name.clone(), points: metrics::series_from_stats(&samples, f) }
            })
            .collect(),
    };
    let panels = vec![
        chart::Panel { id: "apm".into(), title: "APM".into(), unit: "actions per minute".into(), kind: chart::PanelKind::Lines, series: apm_series },
        stat_panel("income", "Income", "minerals + gas per minute", |s| f64::from(s.minerals_rate + s.vespene_rate)),
        stat_panel("army", "Army value", "minerals + gas in current army", |s| f64::from(s.army_minerals + s.army_vespene)),
        stat_panel("supply", "Supply used", "supply", |s| s.supply_used),
        stat_panel("workers", "Workers", "active workers", |s| f64::from(s.workers)),
        stat_panel("unspent", "Unspent resources", "minerals + gas banked", |s| f64::from(s.minerals_unspent + s.vespene_unspent)),
        stat_panel("losses", "Army lost", "cumulative minerals + gas", |s| f64::from(s.lost_minerals + s.lost_vespene)),
    ];
    Ok(chart::render(&chart::Chart {
        title: format!("Arbiter - {}", replay.map),
        map: replay.map.clone(),
        duration_secs: apm::loops_to_secs(replay.duration_loops),
        players,
        panels,
        details,
    }))
}
```

with `use crate::{apm, chart, metrics, replay::{self, ActionKind, StatsSample}};`. (Filtering samples per player twice is fine at this size; do not optimise.)

- [ ] **Step 3: Run** `cargo test --workspace`, `cargo clippy --workspace --all-targets` → green, pristine. Note the printed supply-block counts.

- [ ] **Step 4: Browser check** — `cargo run --release -p arbiter -- "<fixture>" -o "<scratchpad>/panels.html"`, then open it in the built-in browser (serve the scratchpad with a throwaway static server as before, removed afterwards). Confirm: seven panels plus the detail section render; hovering in the income panel shows the crosshair in every panel and a tooltip only in income; drag-zooming in one panel zooms all; double-click resets all; the detail selector switches players and shows stacked areas with the kind legend, a dashed EPM line, and shaded blocks where the printed count is nonzero; the console is clean. Take a full-page screenshot for the report.

- [ ] **Step 5: App check** — `cargo tauri dev` in the background from the repo root; confirm it starts and stays alive 20 s without a panic, then stop it (`Get-Process arbiter-app | Stop-Process`). The app needs no changes; it renders the same page.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline.rs tests/real_replay.rs
git commit -m "Wire income, army, supply, workers, unspent, losses, and player detail panels

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review (done while writing)

- Spec coverage: data (Task 1), metrics incl. block and EPM rules (Task 2), page with shared axis/crosshair/zoom, detail selector, per-panel tables, empty state, chart module split (Task 3), wiring, panel order, browser and app checks (Task 4).
- Type consistency: `Point { secs, value }` after Task 2 is what Tasks 3 and 4 use; `PanelSeries`, `Panel`, `Detail`, `PlayerLegend`, `Chart` fields identical between Tasks 3 and 4; `KIND_LABELS` used by `chart.rs` (Task 3) and `pipeline.rs` (Task 4); JSON keys match the JS (`duration`, `players[].name`, `panels[].{id,title,unit,kind,series[].{player,label,points}}`, `details[].{player,breakdown,epm,blocks}`); element ids `svg-<id>`, `tip-<id>`, `svg-detail`, `tip-detail`, `detail-player` match between Rust and JS.
- No placeholders.
