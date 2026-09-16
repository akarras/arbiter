//! Watches every replay root and emits `replays-changed` after a burst of
//! `.SC2Replay` file events has been quiet for `QUIET`. Watcher failures are
//! also emitted, as `DEGRADED_EVENT`, since `eprintln!` is invisible once a
//! release build hides its console window (`windows_subsystem = "windows"`).

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher as _};
use tauri::{AppHandle, Emitter as _, Manager as _};

use crate::AppState;

pub const QUIET: Duration = Duration::from_millis(500);
pub const EVENT: &str = "replays-changed";

/// Event emitted (with the failure message as payload) when the watcher
/// could not be started, could not watch a root, or reported a backend
/// error while running.
pub const DEGRADED_EVENT: &str = "watcher-degraded";

/// Minimum gap between two `DEGRADED_EVENT` emissions caused by backend
/// errors from the running watcher, so a burst of errors (e.g. a removable
/// drive disappearing) cannot spam the UI.
const ERROR_QUIET: Duration = Duration::from_secs(10);

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

/// Rate-limits repeated "degraded" notifications: `allow` returns `true` (at
/// most once per `quiet` window) when enough time has passed since the last
/// time it returned `true`.
struct ErrorThrottle {
    quiet: Duration,
    last: Option<Instant>,
}

impl ErrorThrottle {
    fn new(quiet: Duration) -> Self {
        Self { quiet, last: None }
    }

    fn allow(&mut self, now: Instant) -> bool {
        if self.last.is_some_and(|t| now.duration_since(t) < self.quiet) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

/// Prints `msg` to stderr (useful in a debug console) and emits it as
/// `DEGRADED_EVENT` so it is visible in the UI even when stderr is not.
fn report_degraded(app: &AppHandle, msg: String) {
    eprintln!("{msg}");
    let _ = app.emit(DEGRADED_EVENT, msg);
}

/// Starts watching the roots in `AppState`; failures are surfaced via
/// `DEGRADED_EVENT`, never fatal.
pub fn install(app: AppHandle) {
    let state = app.state::<AppState>();
    let roots = state.roots.lock().expect("roots lock").clone();
    let (tx, rx) = mpsc::channel::<()>();
    let handler_app = app.clone();
    let throttle = Arc::new(Mutex::new(ErrorThrottle::new(ERROR_QUIET)));
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
        Ok(ev) if is_replay_event(&ev.paths) => {
            let _ = tx.send(());
        }
        Ok(_) => {}
        Err(e) => {
            if throttle.lock().expect("throttle lock").allow(Instant::now()) {
                report_degraded(&handler_app, format!("replay watcher error: {e}"));
            }
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            report_degraded(&app, format!("replay watcher unavailable: {e}"));
            return;
        }
    };
    for root in &roots {
        if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
            report_degraded(&app, format!("could not watch {}: {e}", root.display()));
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

    #[test]
    fn error_throttle_allows_once_per_quiet_window() {
        let t0 = Instant::now();
        let mut throttle = ErrorThrottle::new(Duration::from_secs(10));
        assert!(throttle.allow(t0), "first error always reported");
        assert!(!throttle.allow(t0 + Duration::from_secs(1)), "burst within the window is suppressed");
        assert!(!throttle.allow(t0 + Duration::from_secs(9)));
        assert!(throttle.allow(t0 + Duration::from_secs(11)), "reported again once the window passes");
    }
}
