//! Replay path in, chart HTML out.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};

use crate::{apm, chart, replay};

pub fn chart_html(path: &Path) -> Result<String> {
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
                // Measured over the player's own time in the game, as Blizzard
                // does, so the line ends when they leave and the average is
                // not diluted by minutes they were not playing.
                average: apm::average_apm(loops.len(), player.last_event_loop),
                game_apm: player.game_apm,
                points: apm::rolling_apm(&loops, player.last_event_loop),
            }
        })
        .collect();
    Ok(chart::render(&chart::Chart {
        title: format!("APM - {}", replay.map),
        map: replay.map.clone(),
        duration_secs: apm::loops_to_secs(replay.duration_loops),
        series,
    }))
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
/// concurrent callers (e.g. `serve::run` handling requests on multiple
/// threads): one thread's `set_hook`/`take_hook` pair can interleave with
/// another's, permanently discarding a hook or leaving a panic on some
/// thread with no hook installed while the swap is mid-flight. Installing
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
fn load_guarded(path: &Path) -> Result<replay::Replay> {
    ensure_hook_installed();
    SILENCE.with(|s| s.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(|| replay::load(path)));
    SILENCE.with(|s| s.set(false));
    match result {
        Ok(replay_result) => replay_result,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned());
            match detail {
                Some(detail) => Err(anyhow!(
                    "could not parse {}: not a StarCraft II replay or the file is corrupt ({detail})",
                    path.display()
                )),
                None => Err(anyhow!(
                    "could not parse {}: not a StarCraft II replay or the file is corrupt",
                    path.display()
                )),
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
}
