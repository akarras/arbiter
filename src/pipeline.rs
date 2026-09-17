//! Replay path in, chart HTML out.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};

use crate::{apm, chart, metrics, replay::{self, ActionKind, StatsSample}};

pub fn chart_html(path: &Path) -> Result<String> {
    let label = path.display().to_string();
    let replay = guarded(&label, || replay::load(path))?;
    Ok(build(&replay))
}

/// Same page from bytes already in memory (the web version).
pub fn chart_html_bytes(name: &str, bytes: &[u8]) -> Result<String> {
    let replay = guarded(name, || replay::load_bytes(name, bytes))?;
    Ok(build(&replay))
}

fn build(replay: &replay::Replay) -> String {
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
    chart::render(&chart::Chart {
        title: format!("Arbiter - {}", replay.map),
        map: replay.map.clone(),
        duration_secs: apm::loops_to_secs(replay.duration_loops),
        players,
        panels,
        details,
    })
}

thread_local! {
    /// While set, the process-wide panic hook installed by
    /// `ensure_hook_installed` swallows panics on this thread instead of
    /// forwarding them to whatever hook was active before Arbiter's own.
    static SILENCE: Cell<bool> = const { Cell::new(false) };
}

/// A process panic hook, as accepted by `std::panic::set_hook`.
type PanicHook = dyn Fn(&panic::PanicHookInfo<'_>) + Sync + Send;

/// The panic hook that was active before `ensure_hook_installed` first ran,
/// captured once so panics that are not being silenced still reach it.
static PREVIOUS_HOOK: OnceLock<Box<PanicHook>> = OnceLock::new();

/// Installs Arbiter's panic hook exactly once for the life of the process.
///
/// Earlier code swapped the process-global hook on every `load_guarded`
/// call: `panic::set_hook` a no-op hook, run the load, then
/// `panic::take_hook` to restore the previous one. That swap is racy under
/// concurrent callers (e.g. multiple command invocations handled on
/// different threads): one thread's `set_hook`/`take_hook` pair can
/// interleave with another's, permanently discarding a hook or leaving a
/// panic on some thread with no hook installed while the swap is
/// mid-flight. Installing
/// once avoids the race: the single installed hook consults a `thread_local`
/// flag (`SILENCE`) to decide, per panic, whether to swallow it or forward
/// it to the hook that was active before this function's first call.
fn ensure_hook_installed() {
    PREVIOUS_HOOK.get_or_init(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(|info| {
            let silenced = SILENCE.with(Cell::get);
            if !silenced && let Some(previous) = PREVIOUS_HOOK.get() {
                previous(info);
            }
        }));
        previous
    });
}

/// `s2protocol` (and the `nom-mpq` crate it wraps) can panic on malformed
/// input instead of returning an error: `.expect(...)` when replay user data
/// is missing, `assert_eq!` on the replay signature, `.unwrap()` on file
/// reads. To keep the exit-1 contract (one `error: ...` line, no panic
/// backtrace) we run the load behind `catch_unwind` with this thread's
/// panics silenced, converting any unwind into a plain error.
///
/// Runs `load` with panics converted to errors. On native targets a panic
/// inside `s2protocol` unwinds and is caught; on wasm32 panics abort the
/// worker instead, and the page restarts it.
fn guarded(label: &str, load: impl FnOnce() -> Result<replay::Replay>) -> Result<replay::Replay> {
    ensure_hook_installed();
    SILENCE.with(|s| s.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(load));
    SILENCE.with(|s| s.set(false));
    match result {
        Ok(replay_result) => replay_result,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned());
            match detail {
                Some(detail) => Err(anyhow!("could not parse {label}: not a StarCraft II replay or the file is corrupt ({detail})")),
                None => Err(anyhow!("could not parse {label}: not a StarCraft II replay or the file is corrupt")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A garbage input file makes `s2protocol` panic deep inside its parser
    /// rather than return an error. `chart_html` must still honor the
    /// exit-1 contract: no panic escapes, and the error message is clear.
    #[test]
    fn run_on_a_non_replay_file_returns_a_clear_error_instead_of_panicking() {
        let mut input = std::env::temp_dir();
        input.push(format!("arbiter-not-a-replay-{}.SC2Replay", std::process::id()));
        std::fs::write(&input, b"not a replay").expect("write temp file");

        let result = chart_html(&input);

        let _ = std::fs::remove_file(&input);

        let err = result.expect_err("garbage input must not parse as a replay");
        let message = format!("{err:#}");
        assert!(
            message.contains("not a StarCraft II replay"),
            "unexpected error message: {message}"
        );
    }

    #[test]
    fn garbage_bytes_give_a_clear_error_instead_of_panicking() {
        let err = chart_html_bytes("junk.SC2Replay", b"definitely not a replay").unwrap_err();
        assert!(err.to_string().contains("not a StarCraft II replay"), "{err}");
    }
}
