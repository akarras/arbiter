# Arbiter v3: Tauri desktop app

**Date:** 2026-09-16
**Status:** Approved
**Builds on:** `2026-09-15-apm-chart-design.md` (CLI, unchanged) and
`2026-09-16-serve-and-game-apm-design.md` (game APM stays; `serve` is removed)

## Goal

Replace `arbiter serve` with a native desktop window: a replay list on the
left that updates live as games finish, the chart on the right, and a native
"Open file" dialog for replays outside the scanned folders.

## Decisions (from brainstorming)

| Question | Decision |
| --- | --- |
| Framework | Tauri 2 (WebView2 on Windows), stable 2.x crates |
| Fate of `serve` | Removed, along with `list_page.rs`, `percent.rs`, `tiny_http`, and the chart back link |
| Layout | Two panes: list left, chart right |
| Live updates | Yes: watch every root with `notify`, debounce 500 ms |
| Front end | Plain static HTML/CSS/JS in `ui/`, `withGlobalTauri`, no Node or bundler |
| Chart hosting | Sandboxed `<iframe srcdoc>` fed the existing self-contained chart HTML |

## Repository layout

```
Cargo.toml            workspace root AND the `arbiter` library + CLI package (unchanged API)
src/                  library: apm, chart, pipeline, replay, scan, theme; bin main.rs (CLI only)
src-tauri/            package `arbiter-app` (bin), depends on `arbiter = { path = ".." }`
  Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/, src/{main.rs,lib.rs,commands.rs,watch.rs}
ui/                   index.html, app.css, app.js, mock.js (dev-only bridge stand-in)
```

`.claude/launch.json` loses the `arbiter-serve` entry (nothing replaces it:
`cargo tauri dev` opens a native window, not a URL).

## Library changes (root crate)

- Delete `src/serve.rs`, `src/list_page.rs`, `src/percent.rs`, their `lib.rs`
  lines, the `serve` subcommand and `run_server` in `main.rs`, and the
  `tiny_http` dependency.
- `chart::Chart` loses `back_link`; `pipeline::chart_html(path)` loses the
  second parameter. The CLI and the app both call `chart_html(path)`.
- `scan.rs` and everything else stay as they are.
- Tests for the deleted modules go with them. The `parse_args` tests drop the
  `serve` cases and keep the chart cases. Integration and unit suites stay green.

## App crate (`src-tauri`)

### Commands (`commands.rs`)

Exposed to the page with `#[tauri::command]`; each is a thin wrapper around a
plain function so the logic is unit-testable without a Tauri runtime.

```rust
#[derive(Serialize)] pub struct ReplayRow { path: String, name: String, modified_secs: u64, size: u64 }
list_replays(state) -> Vec<ReplayRow>            // scan::find_replays over the current roots, newest first
chart_html(path: String) -> Result<String, String> // pipeline::chart_html, error text for the page
roots(state) -> Vec<String>
add_root(state, path: String) -> Result<Vec<String>, String>  // must be an existing directory; deduplicated; restarts the watcher
```

Shared state: `Arc<Mutex<Vec<PathBuf>>>` of roots, initialised from
`scan::default_roots()`; the app refuses to start with a clear dialog if that
is empty and no root is passed on the command line (`arbiter-app --dir <p>`).

### Watcher (`watch.rs`)

A `notify::RecommendedWatcher` (recursive) over every root, running on a
background thread. Events whose path ends in `.SC2Replay` (case-insensitive)
feed a debouncer; 500 ms after the last event it emits the Tauri event
`replays-changed` (no payload). Pure helpers: `is_replay_event(&Event) -> bool`
and a `Debouncer` with `fn poke(&mut self, now: Instant)` /
`fn due(&self, now: Instant) -> bool`, both unit-tested.

### Tauri configuration

- `tauri.conf.json`: `productName` "Arbiter", `identifier` `com.arbiter.app`,
  `build.frontendDist` `../ui`, no dev server, `app.withGlobalTauri` true,
  one window 1280×800 titled "Arbiter", `app.security.csp` null (the chart
  runs inline scripts inside the sandboxed iframe, not in the page).
- `capabilities/default.json`: `core:default`, `core:event:allow-listen`,
  `dialog:allow-open`.
- Plugins: `tauri-plugin-dialog` (native open dialog, filtered to
  `*.SC2Replay`).
- Icons: generated with `cargo tauri icon` from a placeholder 1024×1024 PNG
  committed at `src-tauri/app-icon.png`; the user can replace it later.

## Front end (`ui/`)

- `index.html`: left `<aside>` with a filter `<input type="search">`, an
  "Open file…" button, a roots line, and a `<ul id="list">`; right `<main>`
  with `<iframe id="chart" sandbox="allow-scripts">` and an overlay `<div id="status">`
  for "Select a replay", "Parsing…", and error text.
- `app.css`: the same design tokens as `theme.rs` (copied; the page does not
  load Rust-generated CSS), light/dark via `prefers-color-scheme`.
- `app.js` (no dependencies):
  - `bridge` = `window.__TAURI__` when present, otherwise `window.__MOCK__`
    from `mock.js`.
  - On load: `invoke('roots')`, `invoke('list_replays')`, render rows
    (name, `toLocaleString` date from `modified_secs`, KB), and
    `listen('replays-changed', reload)`. Reload keeps the selected path and
    the filter text.
  - Click or ↑/↓ selects a row; selection calls `invoke('chart_html', {path})`,
    shows "Parsing…", then sets `iframe.srcdoc`. Errors show in the overlay.
  - Filter hides rows whose lowercased name lacks the lowercased query.
  - "Open file…" calls `dialog.open({filters: [{name: 'StarCraft II replay', extensions: ['SC2Replay']}]})`
    then charts the chosen path (not added to the list).
  - All text goes through `textContent`; nothing user-derived is put into
    `innerHTML`.
- `mock.js`: loaded only when `window.__TAURI__` is undefined. Provides
  `invoke` returning three sample rows, a small static chart HTML string for
  `chart_html`, and a no-op `listen`, so the page can be exercised in the
  built-in browser.

## Error handling

- A replay that fails to parse shows the error text in the right pane; the
  list is unaffected.
- The watcher failing to start (for example a root on a disconnected drive)
  is logged to stderr and the app keeps working with manual refresh via the
  list reloading on window focus.
- Startup with no roots: a native error dialog, exit code 1.

## Dependencies

Root crate: `anyhow`, `s2protocol` (default features off), `serde_json`.
App crate: `arbiter` (path), `tauri = "2"`, `tauri-plugin-dialog = "2"`,
`notify = "8"`, `serde = { version = "1", features = ["derive"] }`,
`serde_json = "1"`, `anyhow = "1"`; build-dependency `tauri-build = "2"`.
Tooling: `tauri-cli` 2.x installed with cargo.

## Testing

- Root crate: existing suites minus the removed modules; `chart_html` tests
  updated for the one-argument signature.
- App crate: unit tests for `ReplayRow::from(&ReplayEntry)`, `add_root`
  validation and deduplication, `is_replay_event`, and the `Debouncer` timing.
- Front end: open `ui/index.html` in the built-in browser with the mock
  bridge; check rows render, filter works, selection loads the chart, arrow
  keys move, console is clean.
- Manual, with the real app: `cargo tauri dev` starts; the list shows the
  user's 2,722 replays newest first; clicking one shows the chart with the
  "game says" legend; Open file charts the fixture; copying a replay into the
  folder makes it appear at the top within about a second; `cargo tauri build`
  produces an installer under `src-tauri/target/release/bundle`.

## Out of scope

Per-replay parsing for the list (map, players), pagination, multi-window,
auto-update, code signing, non-Windows packaging, an icon beyond a placeholder.
