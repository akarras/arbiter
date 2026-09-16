//! Loads a .SC2Replay and reduces it to players and countable actions.
//! This is the only module that knows about the `s2protocol` crate.

use std::path::Path;

use anyhow::{Context, Result, bail};
use s2protocol::details::PlayerLobbyDetails;
use s2protocol::game_events::ReplayGameEvent;
use s2protocol::init_data::InitData;

/// Blizzard's `GameDescription.game_speed` value for "Faster", the only
/// speed at which `apm::LOOPS_PER_SECOND` (22.4) is the correct loop rate.
const GAME_SPEED_FASTER: u8 = 4;

/// Upper bound on a plausible replay length, in game loops: 24 hours at
/// 22.4 loops/second. Anything beyond this (or negative) is treated as a
/// corrupt replay rather than trusted, since `apm::rolling_apm` allocates
/// one `Vec` slot per 5-second step of the duration.
const MAX_DURATION_LOOPS: i64 = (24.0 * 3600.0 * 22.4) as i64;

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub user_id: i64,
    pub name: String,
    pub race: String,
    pub team: u8,
    pub result: String,
    pub game_apm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub user_id: i64,
    pub game_loop: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub map: String,
    /// Game loop of the last event of any kind.
    pub duration_loops: i64,
    pub players: Vec<Player>,
    /// In game-loop order, only counted event types, only kept players.
    pub actions: Vec<Action>,
    /// From replay.gamemetadata.json, written by the game client.
    pub game_duration_secs: Option<f64>,
}

pub fn load(path: &Path) -> Result<Replay> {
    if !path.is_file() {
        bail!("replay file not found: {}", path.display());
    }

    let init = InitData::try_from((path.to_path_buf(), 0u64)).with_context(|| {
        format!(
            "could not parse replay header of {}: not a StarCraft II replay or the file is corrupt",
            path.display()
        )
    })?;
    let lobby: Vec<PlayerLobbyDetails> = (&init).try_into().context(
        "could not join lobby slots to player details: not a StarCraft II replay or the file is corrupt",
    )?;
    let map = lobby.first().map(|p| p.title.clone()).unwrap_or_default();
    if let Some(speed) = lobby.first().map(|p| p.game_description.game_speed)
        && speed != GAME_SPEED_FASTER
    {
        bail!("replay was recorded at game speed {speed} (only Faster is supported)");
    }
    let path_str = path.to_str().context("replay path is not valid UTF-8")?;
    let (mpq, contents) = s2protocol::read_mpq(path_str)
        .context("could not read MPQ archive: not a StarCraft II replay or the file is corrupt")?;

    let metadata = read_game_metadata(&mpq, &contents);
    let game_duration_secs = metadata.as_ref().and_then(|m| m.duration_game_secs).map(game_secs_to_real);
    let game_apm = game_apm_by_position(metadata.as_ref(), lobby.len());

    let players: Vec<Player> = lobby
        .iter()
        .enumerate()
        .filter(|(_, p)| p.lobby_slot.observe == 0)
        .filter_map(|(position, p)| {
            Some(Player {
                user_id: p.lobby_slot.user_id?,
                name: strip_clan_markup(&p.player_details.name),
                race: p.player_details.race.clone(),
                team: p.player_details.team_id,
                result: p.player_details.result.clone(),
                game_apm: game_apm[position],
            })
        })
        .collect();
    if players.is_empty() {
        bail!("no players found in {}", path.display());
    }

    let events = s2protocol::read_game_events(path_str, &mpq, &contents).context(
        "could not decode game events (unsupported protocol version?): not a StarCraft II replay or the file is corrupt",
    )?;

    let mut game_loop = 0i64;
    let mut actions = Vec::new();
    for ev in &events {
        game_loop += ev.delta;
        if counts_as_action(&ev.event) && players.iter().any(|p| p.user_id == ev.user_id) {
            actions.push(Action { user_id: ev.user_id, game_loop });
        }
    }
    if !(0..=MAX_DURATION_LOOPS).contains(&game_loop) {
        bail!(
            "replay duration of {game_loop} game loops is implausible (expected 0..={MAX_DURATION_LOOPS}, i.e. up to 24 hours); the file is likely corrupt"
        );
    }
    Ok(Replay { map, duration_loops: game_loop, players, actions, game_duration_secs })
}

/// The event types SC2's own APM counts: commands, selections, control groups.
fn counts_as_action(event: &ReplayGameEvent) -> bool {
    matches!(
        event,
        ReplayGameEvent::Cmd(_)
            | ReplayGameEvent::SelectionDelta(_)
            | ReplayGameEvent::ControlGroupUpdate(_)
    )
}

/// Clan tags are stored as pre-escaped markup, e.g. `&lt;CLAN&gt;<sp/>Name`.
/// Strip everything up to and including the last `>` so only the player's
/// own chosen name remains.
///
/// This is a narrow heuristic: it looks only for a literal `>` character and
/// knows nothing about the markup's actual grammar, so a name that legitimately
/// ends in `>` (or is nothing but markup) is handled by falling back to the
/// original, unstripped name below rather than returning an empty string.
fn strip_clan_markup(name: &str) -> String {
    match name.rfind('>') {
        Some(pos) => {
            let stripped = &name[pos + 1..];
            if stripped.is_empty() { name.to_string() } else { stripped.to_string() }
        }
        None => name.to_string(),
    }
}

/// The parts of `replay.gamemetadata.json` we use.
#[derive(Debug, Clone, PartialEq)]
struct GameMetadata {
    /// Blizzard's legacy "game seconds" (16 loops each), not real seconds.
    duration_game_secs: Option<f64>,
    /// (PlayerID, APM) as written by the game; PlayerID is 1-based in
    /// `replay.details` player order. APM is per real minute.
    apm_by_player_id: Vec<(u64, f64)>,
}

/// Game loops per legacy game second (the "Normal" speed clock).
const LOOPS_PER_GAME_SECOND: f64 = 16.0;

fn game_secs_to_real(game_secs: f64) -> f64 {
    game_secs * LOOPS_PER_GAME_SECOND / crate::apm::LOOPS_PER_SECOND
}

/// Maps `lobby` positions (indices into the `Vec<PlayerLobbyDetails>` built
/// by `load`) to each player's Blizzard-reported APM, in `lobby` order.
///
/// Assumption: `apm_by_player_id`'s PlayerID is 1-based in `replay.details`
/// `player_list` order, and when every details player has a matching lobby
/// slot, that order coincides with `lobby`'s order, so PlayerID `i + 1`
/// belongs at `lobby` index `i`. But `s2protocol`'s join
/// (`Vec<PlayerLobbyDetails>::try_from`) is a `filter_map`: any details
/// player with no matching lobby slot is dropped, which shifts every later
/// index. When that happens, position-based lookup silently attributes the
/// wrong APM to the wrong player. As a cheap guard against exactly that
/// shift, this function refuses to match position-to-position at all unless
/// `apm_by_player_id` has exactly `lobby_len` entries (returning all `None`
/// otherwise); this does not detect every possible drop (e.g. one player
/// dropped and one absent from the id-space could still leave the counts
/// equal), so a mismatch is a best-effort signal, not a guarantee.
fn game_apm_by_position(meta: Option<&GameMetadata>, lobby_len: usize) -> Vec<Option<f64>> {
    let Some(meta) = meta else {
        return vec![None; lobby_len];
    };
    if meta.apm_by_player_id.len() != lobby_len {
        return vec![None; lobby_len];
    }
    (0..lobby_len)
        .map(|position| {
            let id = position as u64 + 1;
            meta.apm_by_player_id.iter().find(|(pid, _)| *pid == id).map(|(_, apm)| *apm)
        })
        .collect()
}

fn read_game_metadata(mpq: &s2protocol::MPQ, contents: &[u8]) -> Option<GameMetadata> {
    let (_, bytes) = mpq.read_mpq_file_sector("replay.gamemetadata.json", false, contents).ok()?;
    parse_game_metadata(&bytes)
}

fn parse_game_metadata(bytes: &[u8]) -> Option<GameMetadata> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let duration_game_secs = v.get("Duration").and_then(serde_json::Value::as_f64);
    let apm_by_player_id = v
        .get("Players")
        .and_then(serde_json::Value::as_array)
        .map(|players| {
            players
                .iter()
                .filter_map(|p| Some((p.get("PlayerID")?.as_u64()?, p.get("APM")?.as_f64()?)))
                .collect()
        })
        .unwrap_or_default();
    Some(GameMetadata { duration_game_secs, apm_by_player_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2protocol::game_events::GameSTriggerKeyPressedEvent;

    #[test]
    fn trigger_events_do_not_count() {
        let ev = ReplayGameEvent::TriggerKeyPressed(GameSTriggerKeyPressedEvent { m_key: 0, m_flags: 0 });
        assert!(!counts_as_action(&ev));
    }

    #[test]
    fn missing_file_is_a_clear_error() {
        let err = load(Path::new("does-not-exist.SC2Replay")).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn strips_clan_tag_markup_from_names() {
        assert_eq!(strip_clan_markup("&lt;CLAN&gt;<sp/>Name"), "the user's player");
        assert_eq!(strip_clan_markup("Ferdwas"), "Ferdwas");
    }

    #[test]
    fn falls_back_to_original_name_when_stripping_would_empty_it() {
        assert_eq!(strip_clan_markup("<sp/>"), "<sp/>");
        assert_eq!(strip_clan_markup("&lt;CLAN&gt;"), "&lt;CLAN&gt;");
    }

    #[test]
    fn parses_game_metadata_duration_and_apm() {
        let json = br#"{"Title":"Tuonela LE","Duration":1131,"Players":[{"PlayerID":1,"APM":52.3,"MMR":3100},{"PlayerID":2,"APM":61}]}"#;
        let meta = parse_game_metadata(json).unwrap();
        assert_eq!(meta.duration_game_secs, Some(1131.0));
        assert_eq!(meta.apm_by_player_id, vec![(1, 52.3), (2, 61.0)]);
    }

    #[test]
    fn malformed_metadata_is_none() {
        assert!(parse_game_metadata(b"not json").is_none());
        let meta = parse_game_metadata(b"{}").unwrap();
        assert_eq!(meta.duration_game_secs, None);
        assert!(meta.apm_by_player_id.is_empty());
    }

    #[test]
    fn game_seconds_convert_to_real_seconds_at_faster_speed() {
        // 16 loops per game second, 22.4 per real second: the fixture's 1131
        // game seconds are the 807 real seconds Arbiter computes from loops.
        assert!((game_secs_to_real(1131.0) - 807.86).abs() < 0.01);
    }

    fn meta(apm_by_player_id: Vec<(u64, f64)>) -> GameMetadata {
        GameMetadata { duration_game_secs: None, apm_by_player_id }
    }

    #[test]
    fn exact_match_yields_values_in_order() {
        let m = meta(vec![(1, 10.0), (2, 20.0)]);
        assert_eq!(game_apm_by_position(Some(&m), 2), vec![Some(10.0), Some(20.0)]);
    }

    #[test]
    fn length_mismatch_yields_all_none() {
        let m = meta(vec![(1, 10.0), (2, 20.0), (3, 30.0)]);
        assert_eq!(game_apm_by_position(Some(&m), 2), vec![None, None]);
    }

    #[test]
    fn missing_id_yields_none_only_at_that_position() {
        let m = meta(vec![(1, 10.0), (3, 30.0)]);
        assert_eq!(game_apm_by_position(Some(&m), 2), vec![Some(10.0), None]);
    }

    #[test]
    fn no_metadata_yields_all_none() {
        assert_eq!(game_apm_by_position(None, 3), vec![None, None, None]);
    }
}
