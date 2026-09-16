//! Rolling APM (actions per minute) from a sorted list of game loops.

/// Game loops per real second at "Faster" speed (Legacy of the Void).
///
/// Verified against the fixture `Tuonela LE (115).SC2Replay`: the MPQ's
/// `replay.gamemetadata.json` reports `"Duration"` in real seconds as
/// recorded by the game client, and `tests/real_replay.rs` asserts that
/// value matches `loops_to_secs(replay.duration_loops)` (computed with this
/// constant) to within 2 seconds. `replay::load` also rejects any replay
/// whose `game_speed` is not "Faster" (4), since this constant only holds
/// at that speed.
pub const LOOPS_PER_SECOND: f64 = 22.4;
/// Width of the trailing window.
pub const WINDOW_SECS: f64 = 60.0;
/// Distance between samples.
pub const STEP_SECS: f64 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub secs: f64,
    pub apm: f64,
}

pub fn loops_to_secs(game_loop: i64) -> f64 {
    game_loop as f64 / LOOPS_PER_SECOND
}

/// Samples APM every `STEP_SECS` from `STEP_SECS` up to the game duration.
/// Each sample counts actions in `(t - WINDOW_SECS, t]`, divided by
/// `min(t, WINDOW_SECS)` minutes so the first minute is not artificially low.
///
/// `action_loops` must be sorted ascending. `duration_loops` is trusted to be
/// bounded by the caller (see `replay::load`, which rejects durations beyond
/// 24 hours of game loops); this function itself never allocates more than
/// `duration / STEP_SECS` points and never loops unboundedly, even if
/// `duration_loops` is negative or huge.
pub fn rolling_apm(action_loops: &[i64], duration_loops: i64) -> Vec<Point> {
    let duration = loops_to_secs(duration_loops);
    // The `+ 1e-9` matches the old loop's tolerance for floating-point
    // round-off in `loops_to_secs`, so a duration that is meant to land
    // exactly on a step boundary still includes that final sample.
    let count = if duration > 0.0 { ((duration + 1e-9) / STEP_SECS).floor() as usize } else { 0 };
    let mut points = Vec::with_capacity(count);
    let mut start = 0usize; // first index inside the window
    let mut end = 0usize; // first index after `t`
    for i in 1..=count {
        let t = i as f64 * STEP_SECS;
        while end < action_loops.len() && loops_to_secs(action_loops[end]) <= t {
            end += 1;
        }
        while start < end && loops_to_secs(action_loops[start]) <= t - WINDOW_SECS {
            start += 1;
        }
        let minutes = t.min(WINDOW_SECS) / 60.0;
        points.push(Point { secs: t, apm: (end - start) as f64 / minutes });
    }
    points
}

/// Whole-game average: actions divided by game length in minutes.
pub fn average_apm(action_count: usize, duration_loops: i64) -> f64 {
    if duration_loops <= 0 {
        return 0.0;
    }
    action_count as f64 / (loops_to_secs(duration_loops) / 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One action every 0.6 s == 100 actions per minute, for `minutes` minutes.
    fn uniform_100_apm(minutes: usize) -> Vec<i64> {
        (1..=minutes * 100)
            .map(|i| (i as f64 * 0.6 * LOOPS_PER_SECOND).round() as i64)
            .collect()
    }

    #[test]
    fn empty_actions_and_zero_duration_give_no_points() {
        assert!(rolling_apm(&[], 0).is_empty());
        assert!(rolling_apm(&[100, 200], 0).is_empty());
    }

    #[test]
    fn negative_duration_gives_no_points() {
        assert!(rolling_apm(&[], -1).is_empty());
        assert!(rolling_apm(&[100, 200], i64::MIN).is_empty());
    }

    #[test]
    fn samples_every_five_seconds_up_to_duration() {
        let duration_loops = (60.0 * LOOPS_PER_SECOND) as i64; // 60 s
        let points = rolling_apm(&[], duration_loops);
        let secs: Vec<f64> = points.iter().map(|p| p.secs).collect();
        assert_eq!(secs, vec![5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0]);
        assert!(points.iter().all(|p| p.apm == 0.0));
    }

    #[test]
    fn uniform_activity_reads_about_100_after_first_minute() {
        let loops = uniform_100_apm(5);
        let duration_loops = *loops.last().unwrap();
        let points = rolling_apm(&loops, duration_loops);
        assert!(points.iter().any(|p| p.secs >= 60.0));
        for p in points.iter().filter(|p| p.secs >= 60.0) {
            assert!((p.apm - 100.0).abs() <= 2.0, "at {}s apm was {}", p.secs, p.apm);
        }
    }

    #[test]
    fn first_minute_is_normalised_by_elapsed_time() {
        // 10 actions inside the first 5 seconds -> 10 / (5/60) = 120 APM at t=5.
        let loops: Vec<i64> = (1..=10).map(|i| i * 10).collect(); // loops 10..100, all < 5 s
        let duration_loops = (10.0 * LOOPS_PER_SECOND) as i64;
        let points = rolling_apm(&loops, duration_loops);
        assert_eq!(points[0].secs, 5.0);
        assert!((points[0].apm - 120.0).abs() < 1e-9);
        // At t=10 the same 10 actions over 10 s -> 60 APM.
        assert_eq!(points[1].secs, 10.0);
        assert!((points[1].apm - 60.0).abs() < 1e-9);
    }

    #[test]
    fn actions_fall_out_of_the_trailing_window() {
        // 60 actions in the first ~2.7 s, then nothing for two minutes.
        let loops: Vec<i64> = (1..=60).collect();
        let duration_loops = (120.0 * LOOPS_PER_SECOND) as i64;
        let points = rolling_apm(&loops, duration_loops);
        let at = |t: f64| points.iter().find(|p| p.secs == t).unwrap().apm;
        assert!((at(60.0) - 60.0).abs() < 1e-9);
        assert_eq!(at(65.0), 0.0);
        assert_eq!(at(120.0), 0.0);
    }

    #[test]
    fn average_is_actions_per_minute_over_whole_game() {
        let five_minutes = (300.0 * LOOPS_PER_SECOND) as i64;
        assert!((average_apm(300, five_minutes) - 60.0).abs() < 1e-9);
        assert_eq!(average_apm(300, 0), 0.0);
        assert_eq!(average_apm(0, five_minutes), 0.0);
    }
}
