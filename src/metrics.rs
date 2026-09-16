//! Pure series builders for the macro panels and the APM breakdown.

use crate::apm::{self, Point};
use crate::replay::{ActionKind, StatsSample};

pub const KINDS: [ActionKind; 5] = [
    ActionKind::Command,
    ActionKind::Selection,
    ActionKind::ControlGroup,
    ActionKind::Repeat,
    ActionKind::Retarget,
];
pub const KIND_LABELS: [&str; 5] = ["Commands", "Selections", "Control groups", "Repeats", "Retargets"];

/// Two actions of the same kind closer than this are treated as spam for
/// EPM (about 0.27 s at 22.4 loops/s). A heuristic, not Blizzard's definition.
pub const EFFECTIVE_GAP_LOOPS: i64 = 6;

/// Supply cap above which a player cannot be "blocked".
const SUPPLY_CAP: f64 = 200.0;

pub fn series_from_stats(samples: &[StatsSample], f: impl Fn(&StatsSample) -> f64) -> Vec<Point> {
    samples
        .iter()
        .map(|s| Point { secs: apm::loops_to_secs(s.game_loop), value: f(s) })
        .collect()
}

/// `[from, to]` second intervals where supply used reached supply made
/// (within 0.5) below the cap for at least two consecutive samples.
pub fn supply_blocks(samples: &[StatsSample]) -> Vec<(f64, f64)> {
    let mut blocks = Vec::new();
    let mut start: Option<f64> = None;
    let mut run = 0usize;
    let mut prev_secs = 0.0;
    for s in samples {
        let secs = apm::loops_to_secs(s.game_loop);
        let blocked = s.supply_made < SUPPLY_CAP && s.supply_used >= s.supply_made - 0.5;
        if blocked {
            if start.is_none() {
                start = Some(secs);
                run = 0;
            }
            run += 1;
        } else if let Some(from) = start.take()
            && run >= 2
        {
            blocks.push((from, prev_secs));
        }
        prev_secs = secs;
    }
    if let Some(from) = start
        && run >= 2
    {
        blocks.push((from, prev_secs));
    }
    blocks
}

/// Rolling APM per kind, in `KINDS` order. `actions` must be in loop order.
pub fn apm_breakdown(actions: &[(i64, ActionKind)], last_event_loop: i64) -> Vec<Vec<Point>> {
    KINDS
        .iter()
        .map(|kind| {
            let loops: Vec<i64> = actions.iter().filter(|(_, k)| k == kind).map(|(l, _)| *l).collect();
            apm::rolling_apm(&loops, last_event_loop)
        })
        .collect()
}

/// Loops of the actions that survive the spam rule. `actions` must be in
/// loop order. Debounces against the last *kept* action of the same kind,
/// not merely the last-seen one: an action is dropped only when it is
/// within `EFFECTIVE_GAP_LOOPS` of the previous kept action of the same
/// kind, so a sustained same-kind burst still yields one kept action every
/// `EFFECTIVE_GAP_LOOPS` loops instead of collapsing to a single action.
pub fn effective_loops(actions: &[(i64, ActionKind)]) -> Vec<i64> {
    let mut out = Vec::with_capacity(actions.len());
    let mut last_kept: Option<(i64, ActionKind)> = None;
    for &(game_loop, kind) in actions {
        let spam = last_kept.is_some_and(|(pl, pk)| pk == kind && game_loop - pl <= EFFECTIVE_GAP_LOOPS);
        if !spam {
            out.push(game_loop);
            last_kept = Some((game_loop, kind));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apm::LOOPS_PER_SECOND;

    fn sample(loop_secs: f64, used: f64, made: f64) -> StatsSample {
        StatsSample {
            player_id: 1,
            game_loop: (loop_secs * LOOPS_PER_SECOND) as i64,
            minerals_rate: 100,
            vespene_rate: 50,
            minerals_unspent: 300,
            vespene_unspent: 20,
            workers: 12,
            supply_used: used,
            supply_made: made,
            army_minerals: 1000,
            army_vespene: 200,
            lost_minerals: 10,
            lost_vespene: 5,
        }
    }

    #[test]
    fn stats_series_maps_each_sample_to_seconds_and_value() {
        let s = vec![sample(7.0, 10.0, 15.0), sample(14.0, 12.0, 15.0)];
        let pts = series_from_stats(&s, |x| f64::from(x.minerals_rate + x.vespene_rate));
        assert_eq!(pts.len(), 2);
        assert!((pts[0].secs - 7.0).abs() < 0.05);
        assert_eq!(pts[0].value, 150.0);
        assert!((pts[1].secs - 14.0).abs() < 0.05);
    }

    #[test]
    fn supply_blocks_need_two_consecutive_blocked_samples_and_merge() {
        let s = vec![
            sample(0.0, 10.0, 15.0),  // fine
            sample(7.0, 15.0, 15.0),  // blocked (single) -> ignored
            sample(14.0, 14.0, 23.0), // fine
            sample(21.0, 23.0, 23.0), // blocked
            sample(28.0, 23.0, 23.0), // blocked
            sample(35.0, 22.6, 23.0), // blocked (within 0.5)
            sample(42.0, 24.0, 31.0), // fine
            sample(49.0, 200.0, 200.0), // maxed, not a block
            sample(56.0, 200.0, 200.0),
        ];
        let blocks = supply_blocks(&s);
        assert_eq!(blocks.len(), 1);
        assert!((blocks[0].0 - 21.0).abs() < 0.05);
        assert!((blocks[0].1 - 35.0).abs() < 0.05);
    }

    #[test]
    fn supply_blocks_yields_one_interval_per_separate_run() {
        let s = vec![
            sample(0.0, 10.0, 15.0),  // fine
            sample(7.0, 15.0, 15.0),  // blocked
            sample(14.0, 15.0, 15.0), // blocked
            sample(21.0, 16.0, 23.0), // fine (gap between blocks)
            sample(28.0, 23.0, 23.0), // blocked
            sample(35.0, 23.0, 23.0), // blocked
            sample(42.0, 24.0, 31.0), // fine
        ];
        let blocks = supply_blocks(&s);
        assert_eq!(blocks.len(), 2);
        assert!((blocks[0].0 - 7.0).abs() < 0.05);
        assert!((blocks[0].1 - 14.0).abs() < 0.05);
        assert!((blocks[1].0 - 28.0).abs() < 0.05);
        assert!((blocks[1].1 - 35.0).abs() < 0.05);
    }

    #[test]
    fn supply_block_running_to_the_end_is_closed() {
        let s = vec![sample(0.0, 15.0, 15.0), sample(7.0, 15.0, 15.0)];
        let blocks = supply_blocks(&s);
        assert_eq!(blocks.len(), 1);
        assert!((blocks[0].1 - 7.0).abs() < 0.05);
    }

    #[test]
    fn breakdown_counts_only_each_kind() {
        let one_min = (60.0 * LOOPS_PER_SECOND) as i64;
        let actions: Vec<(i64, ActionKind)> = (1..=30)
            .map(|i| (i * 40, if i % 2 == 0 { ActionKind::Command } else { ActionKind::Selection }))
            .collect();
        let series = apm_breakdown(&actions, one_min);
        assert_eq!(series.len(), 5);
        let at_end = |k: usize| series[k].last().unwrap().value;
        assert!((at_end(0) - 15.0).abs() < 1e-9, "commands");
        assert!((at_end(1) - 15.0).abs() < 1e-9, "selections");
        assert_eq!(at_end(2), 0.0);
        assert_eq!(at_end(3), 0.0);
        assert_eq!(at_end(4), 0.0);
    }

    #[test]
    fn effective_loops_drop_same_kind_spam_within_six_loops() {
        use ActionKind::*;
        let actions = vec![(100, Selection), (103, Selection), (106, Selection), (113, Selection), (115, Command), (117, Command), (130, Command)];
        assert_eq!(effective_loops(&actions), vec![100, 113, 115, 130]);
    }

    #[test]
    fn effective_loops_debounces_a_sustained_burst_against_the_last_kept_action() {
        // 20 same-kind actions 3 loops apart: debouncing against the last
        // *seen* action (the old rule) would keep only the first, since
        // every gap is 3 <= EFFECTIVE_GAP_LOOPS. Debouncing against the
        // last *kept* action keeps one every third action instead, since
        // the cumulative gap (9 loops) exceeds EFFECTIVE_GAP_LOOPS (6).
        use ActionKind::*;
        let actions: Vec<(i64, ActionKind)> = (0..20).map(|i| (i * 3, Repeat)).collect();
        let kept = effective_loops(&actions);
        assert_eq!(kept, vec![0, 9, 18, 27, 36, 45, 54]);
        assert_eq!(kept.len(), 7);
    }
}
