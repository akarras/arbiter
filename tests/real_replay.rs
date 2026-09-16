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
        assert!(
            p.last_event_loop > 0 && p.last_event_loop <= replay.duration_loops,
            "{}: last event loop {} outside (0, {}]",
            p.name,
            p.last_event_loop,
            replay.duration_loops
        );
        let count = replay.actions.iter().filter(|a| a.user_id == p.user_id).count();
        let ours = apm::average_apm(count, p.last_event_loop);
        let theirs = p.game_apm.expect("every fixture player has a game APM");
        let off = (ours - theirs).abs() / theirs;
        eprintln!("{}: ours {ours:.0}, game {theirs:.0} ({:.1}% off)", p.name, off * 100.0);
        assert!(off < 0.08, "{}: ours {ours:.0} vs game {theirs:.0} differ by more than 8%", p.name);
    }
    assert!(
        replay.players.iter().any(|p| p.last_event_loop < replay.duration_loops),
        "in this 4v4 at least one player stops acting before the replay ends"
    );

    assert!(!replay.stats.is_empty(), "fixture has tracker stats");
    let last_stats_loop = replay.stats.last().unwrap().game_loop;
    assert!((replay.duration_loops - last_stats_loop).abs() < 50, "tracker clock matches: {last_stats_loop} vs {}", replay.duration_loops);
    for p in &replay.players {
        let mine: Vec<_> = replay.stats.iter().filter(|s| s.player_id == p.player_id).collect();
        assert!(mine.len() > 50, "{} has only {} samples", p.name, mine.len());
        assert!(mine.iter().all(|s| (0.0..=400.0).contains(&s.supply_made)), "{} supply_made out of range", p.name);
        assert!(mine.iter().map(|s| s.workers).max().unwrap() > 10, "{} never had more than 10 workers", p.name);
        assert!(mine.windows(2).all(|w| w[0].game_loop <= w[1].game_loop));
    }
    let kinds: std::collections::BTreeSet<_> = replay.actions.iter().map(|a| format!("{:?}", a.kind)).collect();
    assert!(kinds.len() >= 4, "expected several action kinds, got {kinds:?}");
    eprintln!("stats samples {} kinds {kinds:?}", replay.stats.len());

    // Each player's line must end when they stop acting, not at the replay's end.
    let html = arbiter::pipeline::chart_html(path).expect("chart renders");
    let start = html.find(r#"<script id="data" type="application/json">"#).unwrap()
        + r#"<script id="data" type="application/json">"#.len();
    let end = start + html[start..].find("</script>").unwrap();
    let data: serde_json::Value = serde_json::from_str(&html[start..end]).unwrap();
    let series = data["panels"][0]["series"].as_array().unwrap();
    assert_eq!(series.len(), replay.players.len());
    for (s, p) in series.iter().zip(&replay.players) {
        assert_eq!(s["label"].as_str().unwrap(), p.name);
        let last_secs = s["points"].as_array().unwrap().last().unwrap()[0].as_f64().unwrap();
        let active_secs = apm::loops_to_secs(p.last_event_loop);
        assert!(
            last_secs <= active_secs,
            "{}: series runs to {last_secs}s but the player's last event is at {active_secs:.1}s",
            p.name
        );
    }

    let ids: Vec<&str> = data["panels"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["apm", "income", "army", "supply", "workers", "unspent", "losses"]);
    for p in data["panels"].as_array().unwrap() {
        assert_eq!(p["series"].as_array().unwrap().len(), replay.players.len(), "panel {} has one series per player", p["id"]);
        assert!(p["series"].as_array().unwrap().iter().all(|s| !s["points"].as_array().unwrap().is_empty()), "panel {} has points", p["id"]);
    }
    assert_eq!(data["details"].as_array().unwrap().len(), replay.players.len());
    let d0 = &data["details"][0];
    assert_eq!(d0["breakdown"].as_array().unwrap().len(), 5);
    assert!(!d0["epm"].as_array().unwrap().is_empty());
    eprintln!("supply blocks per player: {:?}", data["details"].as_array().unwrap().iter().map(|d| d["blocks"].as_array().unwrap().len()).collect::<Vec<_>>());
}
