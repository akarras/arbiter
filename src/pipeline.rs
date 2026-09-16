//! Replay path in, chart HTML out. Shared by the CLI and the server.

use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use anyhow::{Result, anyhow};

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

/// `s2protocol` (and the `nom-mpq` crate it wraps) can panic on malformed
/// input instead of returning an error: `.expect(...)` when replay user data
/// is missing, `assert_eq!` on the replay signature, `.unwrap()` on file
/// reads. To keep the exit-1 contract (one `error: ...` line, no panic
/// backtrace) we run the load behind `catch_unwind` with a silenced panic
/// hook, converting any unwind into a plain error.
fn load_guarded(path: &Path) -> Result<replay::Replay> {
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(AssertUnwindSafe(|| replay::load(path)));
    let _ = panic::take_hook(); // restore the default hook
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

        let result = chart_html(&input, false);

        let _ = std::fs::remove_file(&input);

        let err = result.expect_err("garbage input must not parse as a replay");
        let message = format!("{err:#}");
        assert!(
            message.contains("not a StarCraft II replay"),
            "unexpected error message: {message}"
        );
    }
}
