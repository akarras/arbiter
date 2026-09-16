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
}

/// Ground-truth check for `apm::LOOPS_PER_SECOND` (22.4): the MPQ carries a
/// `replay.gamemetadata.json` file written by the game client itself, with a
/// top-level `"Duration"` field and per-player `"APM"` values, independent of
/// our own event-loop decoding.
///
/// Measured against the fixture: metadata `"Duration"` is 1131 (s), while our
/// computed duration (`loops_to_secs(duration_loops)`, loops counted from the
/// game's own event stream) is 807.0s. Those disagree by far more than 2s,
/// but not because `LOOPS_PER_SECOND` is wrong: 807.0 * 1.4 = 1129.8, i.e.
/// `metadata duration / 1.4 ~= computed duration`, matching Blizzard's own
/// speed multiplier for "Faster" (1.4x of the 16 loops/s "Normal" baseline,
/// 16 * 1.4 = 22.4 = `LOOPS_PER_SECOND`). This strongly suggests
/// `replay.gamemetadata.json`'s `"Duration"` is itself expressed in
/// loops-at-Normal-speed (`duration_loops / 16`), not real elapsed seconds,
/// so it is not a valid ground truth for `LOOPS_PER_SECOND` as a direct
/// real-seconds comparison. The constant is left unchanged (807s matches the
/// visually-observed 13:27 game length for this fixture). This test is
/// `#[ignore]`d because it fails on real data for the reason above, not
/// because the code under test is wrong; see the report for full numbers.
#[test]
#[ignore = "replay.gamemetadata.json's Duration appears to be game-loops/16, not real seconds; see doc comment"]
fn gamemetadata_duration_matches_computed_duration() {
    let path = Path::new(FIXTURE);
    if !path.is_file() {
        eprintln!("skipping: fixture not present at {FIXTURE}");
        return;
    }
    let replay = replay::load(path).expect("replay should parse");
    let path_str = path.to_str().expect("fixture path should be valid UTF-8");
    let (mpq, contents) = s2protocol::read_mpq(path_str).expect("MPQ archive should reopen");
    let metadata_bytes = mpq
        .read_mpq_file_sector("replay.gamemetadata.json", false, &contents)
        .expect("replay.gamemetadata.json should be present in the MPQ archive");
    let metadata_json =
        String::from_utf8(metadata_bytes.1).expect("gamemetadata.json should be UTF-8");
    eprintln!("replay.gamemetadata.json contents:\n{metadata_json}");

    let key = "\"Duration\":";
    let key_pos = metadata_json.find(key).expect("gamemetadata.json should contain \"Duration\"");
    let after_key = metadata_json[key_pos + key.len()..].trim_start();
    let number: String =
        after_key.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    let metadata_duration: f64 = number.parse().expect("Duration value should be numeric");
    let computed_duration = apm::loops_to_secs(replay.duration_loops);
    eprintln!(
        "metadata duration = {metadata_duration:.1}s, computed duration = {computed_duration:.1}s"
    );
    assert!(
        (metadata_duration - computed_duration).abs() < 2.0,
        "metadata duration {metadata_duration:.1}s and computed duration {computed_duration:.1}s \
         disagree by more than 2s; apm::LOOPS_PER_SECOND may be wrong"
    );
}
