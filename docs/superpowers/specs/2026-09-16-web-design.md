# Arbiter v5: web version on GitHub Pages

**Date:** 2026-09-16
**Status:** Approved
**Builds on:** v1 chart, v2 game APM, v3 Tauri app (kept), v4 macro panels

## Goal

A static website where a user picks their replay folder and gets the same
replay list and chart pages as the desktop app, with parsing done in the
browser by the Rust core compiled to WebAssembly. No install, no upload.

## Decisions (from brainstorming)

| Question | Decision |
| --- | --- |
| Keep the desktop app? | Yes; web and desktop share the `arbiter` library |
| Browser support | Chromium first (`showDirectoryPicker`, remembered folder); Firefox/Safari fall back to a one-time `webkitdirectory` pick |
| Hosting | GitHub Pages from `akarras/arbiter` (public, created by the user), deployed by a workflow |
| Parser on wasm | `include_assets` replaced by an empty stand-in crate under `[patch.crates-io]` (verified: library builds for `wasm32-unknown-unknown`, native tests unchanged, zstd gone) |

## Crates

### `arbiter` (library)

- `replay::load_bytes(name: &str, bytes: &[u8]) -> Result<Replay>` does the
  work; `replay::load(path)` reads the file and delegates. Parsing goes
  through `s2protocol::parser::parse(bytes)` → `InitData::new(name, 0, &mpq, bytes)`
  → `read_game_events` / `read_tracker_events` with the same `mpq` and bytes.
  The "file not found" check stays in `load`.
- `pipeline::chart_html_bytes(name, bytes) -> Result<String>` mirrors
  `chart_html(path)`; both share the series-building code. On wasm the panic
  guard is compiled out (`catch_unwind` cannot catch there); a panic kills the
  worker and the page restarts it.

### `arbiter-wasm` (new workspace member, `cdylib`)

```rust
#[wasm_bindgen]
pub fn chart_html(name: &str, bytes: &[u8]) -> Result<String, JsError>
#[wasm_bindgen]
pub fn version() -> String   // crate version, shown in the page footer
```

Dependencies: `arbiter` (path), `wasm-bindgen`. Built with
`wasm-pack build arbiter-wasm --target web --release --out-dir ../web/pkg`
(output is git-ignored; the workflow rebuilds it). `wasm-opt` runs with `-O`.

## Site (`web/`)

Files: `index.html`, `app.css` (shared tokens as the desktop UI), `app.js`,
`worker.js`, `source.js`, `pkg/` (generated). No bundler, no dependencies.

### Folder sources (`source.js`)

One interface, three implementations, chosen at runtime:

```js
// { kind, name, list(): Promise<Entry[]>, canRemember: boolean }
// Entry: { id, name, size, modified /* ms */, bytes(): Promise<ArrayBuffer> }
```

- `HandleSource` (Chromium): wraps a `FileSystemDirectoryHandle` from
  `showDirectoryPicker({ mode: 'read' })`; `list()` walks entries recursively
  (max depth 32), keeps `*.SC2Replay` (case-insensitive), and reads `size`
  and `lastModified` from `getFile()`. The handle is stored in IndexedDB
  (`arbiter`/`handles`, key `replays`); on the next visit the page shows a
  "Reopen <folder name>" button that calls `requestPermission` on click.
- `FileListSource` (any browser): wraps the `FileList` from
  `<input type="file" webkitdirectory>`; same filtering; cannot be remembered.
- `MockSource` (`?mock=1` only): fetches `mock/manifest.json` from the served
  folder, whose entries name files under `mock/`; `bytes()` fetches them. Lets
  the full pipeline (enumeration → worker → wasm → chart) run under automation
  without an OS dialog.

Refresh: while a source is open, `list()` runs every 5 s; the list re-renders
only when the set of `(id, modified, size)` changed. Entries are sorted
newest-first.

### Worker (`worker.js`)

Loads `pkg/arbiter_wasm.js`, calls `init()`, then answers
`{id, name, bytes}` messages with `{id, html}` or `{id, error}`. The main
page creates it once, tracks in-flight ids, and recreates it if it dies
(`onerror`), failing any in-flight request with "parser crashed".

### Page (`app.js`)

Same layout as the desktop UI: sidebar (buttons, filter, count, virtualised
list, footer with source name, wasm version, and the privacy line "Replays
are read by your browser and never uploaded"), chart pane with a sandboxed
`iframe srcdoc` and status overlay. Buttons: "Choose folder…" (picker or
fallback input depending on support), "Reopen <name>" when a remembered
handle exists, "Open file…" (single `.SC2Replay` via a file input). Selection,
arrow keys, debounce, filter, and highlight behave exactly as in the app.
Unsupported browsers get the fallback silently; only a browser without both
mechanisms sees a message.

## Deployment

- `.github/workflows/pages.yml`: on push to `main` and manual dispatch:
  checkout, Rust stable with the `wasm32-unknown-unknown` target, install
  `wasm-pack`, run the build command above, `actions/upload-pages-artifact`
  with `web/`, `actions/deploy-pages`. Permissions `pages: write`, `id-token: write`.
- Pages source set to "GitHub Actions" through the API
  (`POST /repos/akarras/arbiter/pages` with `build_type: workflow`).
- Remote `origin` = `git@github.com:akarras/arbiter.git` (the CLI is
  configured for SSH). First push is the whole of `main`.
- `.gitignore` gains `/web/pkg`.

## Errors

- Unparsable replay: the error text appears in the chart pane; the list is
  unaffected.
- Permission denied on reopen: the "Reopen" button stays and a warning line
  explains; the user can choose the folder again.
- Worker crash: restarted; the selected replay shows "parser crashed on this
  file".

## Testing

- Rust: `load_bytes` gets a fixture-gated test (bytes read by the test,
  passed in) asserting the same players and action count as `load`; the wasm
  crate builds in CI and locally.
- Front end with `?mock=1` in the built-in browser: the mock manifest lists
  the fixture (copied into the served scratch folder, not the repo); check the
  list fills, clicking parses through the real wasm and shows the panels page,
  errors on a garbage file show in the pane, the filter and arrow keys work,
  the console is clean.
- Deployed: open `https://akarras.github.io/arbiter/`, confirm the page loads,
  the wasm initialises (footer shows the version), and the console is clean.
  The folder picker itself is exercised by the user.

## Out of scope

Live watching through `FileSystemObserver`, multiple folders, cross-replay
trends, service-worker offline caching, non-Chromium remembered folders.
