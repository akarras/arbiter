# Arbiter

Arbiter analyzes StarCraft II replays (`.SC2Replay` files) and renders
per-player charts and breakdowns: APM matched to the number the game itself
reports, income, army value, supply blocks, worker count, unspent resources,
losses, a per-player input breakdown, and EPM (effective actions per
minute).

## Ways to use it

### Website

`https://akarras.github.io/arbiter/`

Replays are read directly by your browser and never uploaded anywhere.
Chromium-based browsers remember the folder you pick between visits; other
browsers ask you to pick it again each time.

### Desktop app

Build with:

```
cargo tauri build
```

Installers are written to `target/release/bundle/`.

### CLI

```
cargo run --release -- <replay.SC2Replay> [-o out.html]
```

Renders a standalone HTML chart for the given replay. If `-o` is omitted,
the output is written next to the input file with an `.html` extension.

## Building the website locally

```
wasm-pack build arbiter-wasm --target web --release --out-dir ../web/pkg
```

Then serve the `web/` directory with any static file server.

## Running tests

```
cargo test --workspace
```

One test in `tests/real_replay.rs` exercises the pipeline against a real
replay file rather than a synthetic fixture. It is skipped unless you point
it at a replay on your own machine:

```
ARBITER_FIXTURE=<path to a .SC2Replay file> cargo test --workspace
```

## How APM is counted

APM is counted from commands, selections, control group actions, repeats,
and retargets over each player's own active time in the replay, matching
the APM the game itself reports (and shown in the replay's metadata) to
within a few percent. EPM (effective APM) filters out some of that noise —
for example, repeated commands of the same kind issued in quick succession —
using a documented heuristic rather than the game's own count, since the
game does not expose one.

## Design docs

Design and planning documents for each feature live under
`docs/superpowers/`.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
