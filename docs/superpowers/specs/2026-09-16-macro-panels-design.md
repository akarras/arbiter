# Arbiter v4: macro panels and APM breakdown

**Date:** 2026-09-16
**Status:** Approved
**Builds on:** v1 (chart), v2 (game APM), v3 (Tauri app). The CLI and the app
both render through `pipeline::chart_html`, so both get this for free.

## Goal

Turn the single APM chart into a scrolling page of stacked panels on a shared
time axis: APM, income, army value, supply, workers, unspent resources, and a
per-player detail panel (APM by input kind, EPM, supply blocks).

## Decisions (from brainstorming)

| Question | Decision |
| --- | --- |
| Scope | Macro panel (from the tracker stream) + APM breakdown + EPM |
| Layout | Stacked panels, shared time axis, one crosshair and one zoom |
| Detail panel | One player at a time, chosen with a selector; default first player |
| Chart module | Split: `chart.rs` (Rust assembly, JSON) and `chart_assets.rs` (CSS, JS text) |

## Data (replay.rs)

- Read tracker events with `s2protocol::read_tracker_events(path_str, &mpq, &contents)`.
  `TrackerEvent { delta: u32, event }`; `delta` accumulates to the game loop on
  the same 22.4 loops/second clock as game events (verified on the fixture:
  the last tracker loop is within a few loops of `duration_loops`).
- `Player.player_id: u8`: 1-based position in `replay.details` player order
  (the same id the metadata uses). Tracker events carry this id, not `user_id`.
- `Action.kind: ActionKind` with variants `Command` (`Cmd`), `Selection`
  (`SelectionDelta`), `ControlGroup` (`ControlGroupUpdate`), `Repeat`
  (`CommandManagerState`), `Retarget` (`CmdUpdateTargetUnit`).
- `Replay.stats: Vec<StatsSample>` in loop order:

```rust
pub struct StatsSample {
    pub player_id: u8,
    pub game_loop: i64,
    pub minerals_rate: i32,      // minerals_collection_rate, per minute
    pub vespene_rate: i32,
    pub minerals_unspent: i32,   // minerals_current
    pub vespene_unspent: i32,
    pub workers: i32,            // workers_active_count
    pub supply_used: f64,        // food_used / 4096
    pub supply_made: f64,        // food_made / 4096
    pub army_minerals: i32,      // minerals_used_current_army
    pub army_vespene: i32,
    pub lost_minerals: i32,      // minerals_lost_army (cumulative)
    pub lost_vespene: i32,
}
```

  `food_used`/`food_made` are 4096× fixed point in the raw event; the
  implementer confirms on the fixture (a full army reads 200.0 after scaling)
  before relying on the divisor.
- Samples for players that were filtered out (observers) are dropped. A replay
  with no tracker events (very old protocol) yields an empty `stats` and the
  macro panels render their empty state.

## Metrics (new `src/metrics.rs`)

All functions are pure and unit-tested with synthetic samples.

- `series_from_stats(samples: &[StatsSample], f: impl Fn(&StatsSample) -> f64) -> Vec<Point>`:
  maps each sample to `(secs, value)`; used for income
  (`minerals_rate + vespene_rate`), unspent (`minerals_unspent + vespene_unspent`),
  workers, supply used, army value (`army_minerals + army_vespene`), losses
  (`lost_minerals + lost_vespene`).
- `supply_blocks(samples) -> Vec<(f64, f64)>`: intervals `[from_secs, to_secs]`
  where `supply_used >= supply_made - 0.5` and `supply_made < 200`, spanning
  at least two consecutive samples; adjacent blocked samples merge.
- `apm_breakdown(actions: &[(i64, ActionKind)], last_event_loop) -> [Vec<Point>; 5]`:
  `apm::rolling_apm` applied per kind, in the order Command, Selection,
  ControlGroup, Repeat, Retarget.
- `effective_loops(actions: &[(i64, ActionKind)]) -> Vec<i64>`: drops an
  action when it has the same kind as the player's previous action and falls
  within 0.25 s (5.6 loops) of it. EPM is `apm::rolling_apm` over the result.
  Documented as a spam heuristic, not Blizzard's definition.

## Page (chart.rs + chart_assets.rs)

```rust
pub enum PanelKind { Lines, Stacked }
pub struct PanelSeries { pub player: usize, pub label: String, pub points: Vec<Point> }
pub struct Panel { pub id: String, pub title: String, pub unit: String, pub kind: PanelKind, pub series: Vec<PanelSeries> }
pub struct Detail { pub player: usize, pub breakdown: Vec<PanelSeries>, pub epm: Vec<Point>, pub blocks: Vec<(f64, f64)> }
pub struct Chart { pub title, pub map, pub duration_secs, pub players: Vec<PlayerLegend { name, race, result, average, game_apm }>, pub panels: Vec<Panel>, pub details: Vec<Detail> }
pub fn render(chart: &Chart) -> String
```

- Rust emits: header, one legend (swatch, name, race, result, avg APM, game
  APM), a `<section class="panel">` per panel with title, unit, an empty
  `<svg>` sized 960×260, and a collapsible data table; then the detail section
  with a `<select id="detail-player">` and its own `<svg>`; then the JSON
  block and the script.
- JSON: `{duration, players:[{name}], panels:[{id,title,unit,kind,series:[{player,label,points:[[t,v]]}]}], details:[{player,breakdown:[...],epm:[...],blocks:[[from,to]]}]}`.
  `<` is escaped as `<` as before.
- JS: one `view {x0,x1}` shared by every panel; `draw()` redraws all SVGs;
  hovering any panel snaps a crosshair in every panel to the nearest sample
  of that panel's first series and shows a tooltip for the hovered panel
  only; drag-zoom and double-click reset apply to all. The detail panel
  draws a stacked area (cumulative sums of the five kinds, painted back to
  front, colours from the categorical palette slots 1–5 with the player's
  own colour for the EPM line) and supply blocks as translucent bands.
  Changing the selector redraws the detail panel only.
- Y axis: nice ticks from the visible maximum, label = panel unit. Stacked
  panels clamp at their visible stacked maximum.
- Empty state per panel ("No tracker data in this replay").
- Sizes: 8 players × ~110 samples × 6 panels ≈ 5,000 points, plus the APM
  series; fine inline.

## Pipeline

`pipeline::chart_html` builds: legend entries as before; the APM panel
(existing series); five macro panels from `metrics::series_from_stats`; the
losses panel; one `Detail` per player. Averages and game APM unchanged.

## Testing

- `replay.rs`: unit test that `ActionKind` is assigned per event variant;
  integration test on the fixture asserts stats exist for all 8 players, the
  last tracker loop is within 50 loops of `duration_loops`, every
  `supply_made <= 200`, and workers peak above 10 for every player.
- `metrics.rs`: synthetic tests for each function including the block-merge
  rule and the EPM 0.25 s rule.
- `chart.rs`: JSON contains one entry per panel and per detail, tables per
  panel, `<option>` per player, escaping unchanged.
- Manual: CLI output opened in the built-in browser: crosshair spans panels,
  zoom syncs, selector switches the detail panel; app shows the same page.

## Out of scope

Build order, camera heat map, hotkey usage, cross-replay trends, unit cost
tables (army value comes from the tracker directly), per-race benchmarks.
