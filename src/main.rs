use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
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

fn run(input: &Path, output: &Path) -> Result<()> {
    let replay = replay::load(input)?;
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
}
