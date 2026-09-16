use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};
use arbiter::{apm, chart, replay};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((input, output)) = parse_args(&args) else {
        eprintln!("usage: arbiter <replay.SC2Replay> [-o <out.html>]");
        return ExitCode::from(2);
    };
    match run(&input, &output) {
        Ok(()) => {
            println!("{}", output.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn parse_args(args: &[String]) -> Option<(PathBuf, PathBuf)> {
    match args {
        [input] => {
            let input = PathBuf::from(input);
            let output = input.with_extension("html");
            Some((input, output))
        }
        [input, flag, output] if flag == "-o" => Some((PathBuf::from(input), PathBuf::from(output))),
        _ => None,
    }
}

/// `s2protocol` (and the `nom-mpq` crate it wraps) can panic on malformed
/// input instead of returning an error: `.expect(...)` when replay user data
/// is missing, `assert_eq!` on the replay signature, `.unwrap()` on file
/// reads. To keep the exit-1 contract (one `error: ...` line, no panic
/// backtrace) we run the load behind `catch_unwind` with a silenced panic
/// hook, converting any unwind into a plain error.
fn load_replay(path: &Path) -> Result<replay::Replay> {
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

fn run(input: &Path, output: &Path) -> Result<()> {
    let replay = load_replay(input)?;
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
    let html = chart::render(&chart::Chart {
        title: format!("APM - {}", replay.map),
        map: replay.map.clone(),
        duration_secs: apm::loops_to_secs(replay.duration_loops),
        series,
    });
    std::fs::write(output, html).with_context(|| format!("could not write {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_path_defaults_output_next_to_input() {
        let (i, o) = parse_args(&args(&["games/x.SC2Replay"])).unwrap();
        assert_eq!(i, PathBuf::from("games/x.SC2Replay"));
        assert_eq!(o, PathBuf::from("games/x.html"));
    }

    #[test]
    fn dash_o_sets_output() {
        let (_, o) = parse_args(&args(&["x.SC2Replay", "-o", "out/y.html"])).unwrap();
        assert_eq!(o, PathBuf::from("out/y.html"));
    }

    #[test]
    fn bad_shapes_are_rejected() {
        assert!(parse_args(&args(&[])).is_none());
        assert!(parse_args(&args(&["a", "b"])).is_none());
        assert!(parse_args(&args(&["a", "-x", "b"])).is_none());
        assert!(parse_args(&args(&["a", "-o", "b", "c"])).is_none());
    }

    /// A garbage input file makes `s2protocol` panic deep inside its parser
    /// rather than return an error. `run` must still honor the exit-1
    /// contract: no panic escapes, and the error message is clear.
    #[test]
    fn run_on_a_non_replay_file_returns_a_clear_error_instead_of_panicking() {
        let mut input = std::env::temp_dir();
        input.push(format!("arbiter-not-a-replay-{}.SC2Replay", std::process::id()));
        std::fs::write(&input, b"not a replay").expect("write temp file");
        let output = input.with_extension("html");

        let result = run(&input, &output);

        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&output);

        let err = result.expect_err("garbage input must not parse as a replay");
        let message = format!("{err:#}");
        assert!(
            message.contains("not a StarCraft II replay"),
            "unexpected error message: {message}"
        );
    }
}
