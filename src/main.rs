use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use arbiter::{pipeline, scan, serve};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = parse_args(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match command {
        Command::Chart { input, output } => run(&input, &output).map(|()| println!("{}", output.display())),
        Command::Serve { dirs, port } => run_server(dirs, port),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

#[derive(Debug, PartialEq)]
enum Command {
    Chart { input: PathBuf, output: PathBuf },
    Serve { dirs: Vec<PathBuf>, port: u16 },
}

const USAGE: &str = "usage:\n  arbiter <replay.SC2Replay> [-o <out.html>]\n  arbiter serve [--dir <folder>]... [--port <n>]";

fn parse_args(args: &[String]) -> Option<Command> {
    match args {
        [first, rest @ ..] if first == "serve" => parse_serve(rest),
        [input] => {
            let input = PathBuf::from(input);
            let output = input.with_extension("html");
            Some(Command::Chart { input, output })
        }
        [input, flag, output] if flag == "-o" => Some(Command::Chart { input: PathBuf::from(input), output: PathBuf::from(output) }),
        _ => None,
    }
}

fn parse_serve(args: &[String]) -> Option<Command> {
    let mut dirs = Vec::new();
    let mut port = 8321u16;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dir" => dirs.push(PathBuf::from(it.next()?)),
            "--port" => port = it.next()?.parse().ok()?,
            _ => return None,
        }
    }
    Some(Command::Serve { dirs, port })
}

fn run(input: &Path, output: &Path) -> Result<()> {
    let html = pipeline::chart_html(input, false)?;
    std::fs::write(output, html).with_context(|| format!("could not write {}", output.display()))?;
    Ok(())
}

fn run_server(dirs: Vec<PathBuf>, port: u16) -> Result<()> {
    let roots = if dirs.is_empty() { scan::default_roots() } else { dirs };
    if roots.is_empty() {
        anyhow::bail!("no StarCraft II replay folders found under your profile; pass --dir <folder>");
    }
    for d in &roots {
        if !d.is_dir() {
            anyhow::bail!("not a folder: {}", d.display());
        }
    }
    serve::run(roots, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_path_defaults_output_next_to_input() {
        match parse_args(&args(&["games/x.SC2Replay"])).unwrap() {
            Command::Chart { input, output } => {
                assert_eq!(input, PathBuf::from("games/x.SC2Replay"));
                assert_eq!(output, PathBuf::from("games/x.html"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dash_o_sets_output() {
        match parse_args(&args(&["x.SC2Replay", "-o", "out/y.html"])).unwrap() {
            Command::Chart { output, .. } => assert_eq!(output, PathBuf::from("out/y.html")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn serve_parses_dirs_and_port() {
        assert_eq!(parse_args(&args(&["serve"])).unwrap(), Command::Serve { dirs: vec![], port: 8321 });
        assert_eq!(
            parse_args(&args(&["serve", "--dir", "a", "--port", "9000", "--dir", "b"])).unwrap(),
            Command::Serve { dirs: vec![PathBuf::from("a"), PathBuf::from("b")], port: 9000 }
        );
    }

    #[test]
    fn bad_shapes_are_rejected() {
        for bad in [vec![], vec!["a", "b"], vec!["a", "-x", "b"], vec!["a", "-o", "b", "c"], vec!["serve", "--port"], vec!["serve", "--port", "x"], vec!["serve", "--what"]] {
            assert!(parse_args(&args(&bad)).is_none(), "{bad:?}");
        }
    }
}
