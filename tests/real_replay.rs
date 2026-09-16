use std::path::Path;

use arbiter::apm;
use arbiter::replay;

const FIXTURE: &str = r"the local fixture replay";

#[test]
fn loads_the_local_replay_when_present() {
    let path = Path::new(FIXTURE);
    if !path.is_file() {
        eprintln!("skipping: fixture not present at {FIXTURE}");
        return;
    }
    let replay = replay::load(path).expect("replay should parse");
    assert_eq!(replay.map, "Tuonela LE");
    assert!(replay.players.len() >= 2, "expected at least two players: {:?}", replay.players);
    let mut ids: Vec<i64> = replay.players.iter().map(|p| p.user_id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), replay.players.len(), "user ids must be unique");
    eprintln!("{} players on map {}", replay.players.len(), replay.map);
    for p in &replay.players {
        assert!(!p.name.is_empty());
        assert!(!p.race.is_empty());
        let count = replay.actions.iter().filter(|a| a.user_id == p.user_id).count();
        assert!(count > 100, "{} has only {count} actions", p.name);
        eprintln!(
            "{} ({}, {}): {} actions, avg {:.0} APM",
            p.name,
            p.race,
            p.result,
            count,
            apm::average_apm(count, replay.duration_loops)
        );
    }
    let secs = apm::loops_to_secs(replay.duration_loops);
    eprintln!("duration {:.0}s = {}:{:02}", secs, secs as i64 / 60, secs as i64 % 60);
    assert!(secs > 60.0, "a real ladder game lasts more than a minute");
    assert!(
        replay.actions.windows(2).all(|w| w[0].game_loop <= w[1].game_loop),
        "actions are in loop order"
    );

    let game_duration = replay.game_duration_secs.expect("fixture has game metadata");
    let computed = apm::loops_to_secs(replay.duration_loops);
    eprintln!("game duration {game_duration:.0}s, computed {computed:.1}s");
    assert!((game_duration - computed).abs() < 2.0, "time base mismatch: game {game_duration} vs computed {computed}");
    for p in &replay.players {
        let count = replay.actions.iter().filter(|a| a.user_id == p.user_id).count();
        let ours = apm::average_apm(count, replay.duration_loops);
        let theirs = p.game_apm.expect("every fixture player has a game APM");
        eprintln!("{}: ours {ours:.0}, game {theirs:.0}", p.name);
        assert!(theirs > 0.0);
    }
}
