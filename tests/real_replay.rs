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
    assert_eq!(replay.players.len(), 2, "1v1 replay: {:?}", replay.players);
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
}
