//! Watches every replay root and emits `replays-changed` after a burst of
//! `.SC2Replay` file events has been quiet for `QUIET`.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher as _};
use tauri::{AppHandle, Emitter as _, Manager as _};

use crate::AppState;

pub const QUIET: Duration = Duration::from_millis(500);
pub const EVENT: &str = "replays-changed";

pub fn is_replay_event(paths: &[PathBuf]) -> bool {
    paths.iter().any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("SC2Replay")))
}

/// Trailing-edge debounce: `poke` on every event, `take_due` fires once when
/// `quiet` has elapsed since the last poke.
pub struct Debouncer {
    quiet: Duration,
    last: Option<Instant>,
}

impl Debouncer {
    pub fn new(quiet: Duration) -> Self {
        Self { quiet, last: None }
    }

    pub fn poke(&mut self, now: Instant) {
        self.last = Some(now);
    }

    pub fn take_due(&mut self, now: Instant) -> bool {
        match self.last {
            Some(t) if now.duration_since(t) >= self.quiet => {
                self.last = None;
                true
            }
            _ => false,
        }
    }
}

/// Starts watching the roots in `AppState`; failures are logged, never fatal.
pub fn install(app: AppHandle) {
    let state = app.state::<AppState>();
    let roots = state.roots.lock().expect("roots lock").clone();
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res
            && is_replay_event(&ev.paths)
        {
            let _ = tx.send(());
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("replay watcher unavailable: {e}");
            return;
        }
    };
    for root in &roots {
        if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
            eprintln!("could not watch {}: {e}", root.display());
        }
    }
    *state.watcher.lock().expect("watcher lock") = Some(watcher);

    std::thread::spawn(move || {
        let mut debounce = Debouncer::new(QUIET);
        loop {
            match rx.recv_timeout(QUIET) {
                Ok(()) => debounce.poke(Instant::now()),
                Err(RecvTimeoutError::Timeout) => {
                    if debounce.take_due(Instant::now()) {
                        let _ = app.emit(EVENT, ());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn only_replay_paths_count() {
        assert!(is_replay_event(&[PathBuf::from("a/b.SC2Replay")]));
        assert!(is_replay_event(&[PathBuf::from("a/x.txt"), PathBuf::from("a/c.sc2replay")]));
        assert!(!is_replay_event(&[PathBuf::from("a/x.txt")]));
        assert!(!is_replay_event(&[]));
    }

    #[test]
    fn debouncer_fires_once_after_quiet_period() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_millis(500));
        assert!(!d.take_due(t0), "nothing pending");
        d.poke(t0);
        assert!(!d.take_due(t0 + Duration::from_millis(100)));
        d.poke(t0 + Duration::from_millis(400));
        assert!(!d.take_due(t0 + Duration::from_millis(700)), "poke at 400 resets the clock");
        assert!(d.take_due(t0 + Duration::from_millis(900)));
        assert!(!d.take_due(t0 + Duration::from_millis(2000)), "fires once");
    }
}
