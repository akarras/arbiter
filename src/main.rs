use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use arbiter::pipeline;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((input, output)) = parse_args(&args) else {
        eprintln!("usage: arbiter <replay.SC2Replay> [-o <out.html>]");
        return ExitCode::from(2);
    };
    let result = run(&input, &output).map(|()| println!("{}", output.display()));
    match result {
        Ok(()) => ExitCode::SUCCESS,
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
    let html = pipeline::chart_html(input)?;
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
        let (input, output) = parse_args(&args(&["games/x.SC2Replay"])).unwrap();
        assert_eq!(input, PathBuf::from("games/x.SC2Replay"));
        assert_eq!(output, PathBuf::from("games/x.html"));
    }

    #[test]
    fn dash_o_sets_output() {
        let (_, output) = parse_args(&args(&["x.SC2Replay", "-o", "out/y.html"])).unwrap();
        assert_eq!(output, PathBuf::from("out/y.html"));
    }

    #[test]
    fn bad_shapes_are_rejected() {
        for bad in [vec![], vec!["a", "b"], vec!["a", "-x", "b"], vec!["a", "-o", "b", "c"]] {
            assert!(parse_args(&args(&bad)).is_none(), "{bad:?}");
        }
    }
}
