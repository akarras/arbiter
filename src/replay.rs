//! Loads a .SC2Replay and reduces it to players and countable actions.
//! This is the only module that knows about the `s2protocol` crate.

use std::path::Path;

use anyhow::{Context, Result, bail};
use s2protocol::details::PlayerLobbyDetails;
use s2protocol::game_events::ReplayGameEvent;
use s2protocol::init_data::InitData;
use s2protocol::tracker_events::{PlayerStatsEvent, ReplayTrackerEvent, TrackerEvent};

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
    /// Game loop of this player's last event of any kind: the end of their
    /// time in the game, which is the denominator Blizzard uses for APM.
    pub last_event_loop: i64,
    /// The tracker stream's own player id, resolved by `resolve_player_ids`
    /// from the `PlayerSetupEvent` whose `user_id` matches this player;
    /// falls back to the player's 1-based position in `replay.details`
    /// order when no tracker `PlayerSetup` event matches it (e.g. the
    /// replay has no tracker stream at all). Tracker samples and the game
    /// metadata's APM are keyed by this id.
    pub player_id: u8,
}

/// What a counted action was, for the APM breakdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Command,
    Selection,
    ControlGroup,
    Repeat,
    Retarget,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub user_id: i64,
    pub game_loop: i64,
    pub kind: ActionKind,
}

/// One tracker `PlayerStats` sample (about every 160 loops per player).
/// Supply values are already in supply units; resources are raw counts.
#[derive(Debug, Clone, PartialEq)]
pub struct StatsSample {
    pub player_id: u8,
    pub game_loop: i64,
    pub minerals_rate: i32,
    pub vespene_rate: i32,
    pub minerals_unspent: i32,
    pub vespene_unspent: i32,
    pub workers: i32,
    pub supply_used: f64,
    pub supply_made: f64,
    pub army_minerals: i32,
    pub army_vespene: i32,
    pub lost_minerals: i32,
    pub lost_vespene: i32,
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
    /// Tracker samples in loop order for kept players; empty when the
    /// replay has no tracker stream.
    pub stats: Vec<StatsSample>,
}

pub fn load(path: &Path) -> Result<Replay> {
    if !path.is_file() {
        bail!("replay file not found: {}", path.display());
    }
    let bytes = std::fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    load_bytes(&name, &bytes)
}

/// Parses a replay already in memory. `name` only labels error messages and
/// the crate's per-file metadata; it does not need to exist on disk.
pub fn load_bytes(name: &str, bytes: &[u8]) -> Result<Replay> {
    let (_, mpq) = s2protocol::parser::parse(bytes).map_err(|e| {
        anyhow::anyhow!("could not read MPQ archive of {name}: not a StarCraft II replay or the file is corrupt ({e:?})")
    })?;
    let init = InitData::new(name, 0, &mpq, bytes).with_context(|| {
        format!("could not parse replay header of {name}: not a StarCraft II replay or the file is corrupt")
    })?;
    let lobby: Vec<PlayerLobbyDetails> = join_lobby_details(name, &mpq, bytes, &init).context(
        "could not join lobby slots to player details: not a StarCraft II replay or the file is corrupt",
    )?;
    let map = lobby.first().map(|p| p.title.clone()).unwrap_or_default();
    if let Some(speed) = lobby.first().map(|p| p.game_description.game_speed)
        && speed != GAME_SPEED_FASTER
    {
        bail!("replay was recorded at game speed {speed} (only Faster is supported)");
    }
    let contents = bytes;
    let metadata = read_game_metadata(&mpq, contents);
    let game_duration_secs = metadata.as_ref().and_then(|m| m.duration_game_secs).map(game_secs_to_real);

    let mut players: Vec<Player> = lobby
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
                game_apm: None,
                last_event_loop: 0,
                player_id: position as u8 + 1,
            })
        })
        .collect();
    if players.is_empty() {
        bail!("no players found in {}", name);
    }

    // The tracker stream's `PlayerSetup` events are the authoritative
    // user_id -> player_id mapping; resolve ids from them (fail-soft to an
    // empty `Vec`, and so positional ids, when the replay has no tracker
    // stream or it fails to decode) before using `player_id` for anything.
    let tracker_events = s2protocol::read_tracker_events(name, &mpq, contents).unwrap_or_default();
    let setups: Vec<(u8, Option<i64>)> = tracker_events
        .iter()
        .filter_map(|ev| match &ev.event {
            ReplayTrackerEvent::PlayerSetup(s) => Some((s.player_id, s.user_id.map(i64::from))),
            _ => None,
        })
        .collect();
    resolve_player_ids(&setups, &mut players);
    for player in &mut players {
        player.game_apm = apm_for_player_id(metadata.as_ref(), player.player_id);
    }

    let events = s2protocol::read_game_events(name, &mpq, contents).context(
        "could not decode game events (unsupported protocol version?): not a StarCraft II replay or the file is corrupt",
    )?;

    let mut game_loop = 0i64;
    let mut actions = Vec::new();
    for ev in &events {
        game_loop += ev.delta;
        let Some(player) = players.iter_mut().find(|p| p.user_id == ev.user_id) else {
            continue;
        };
        player.last_event_loop = game_loop;
        if let Some(kind) = action_kind(&ev.event) {
            actions.push(Action { user_id: ev.user_id, game_loop, kind });
        }
    }
    if !(0..=MAX_DURATION_LOOPS).contains(&game_loop) {
        bail!(
            "replay duration of {game_loop} game loops is implausible (expected 0..={MAX_DURATION_LOOPS}, i.e. up to 24 hours); the file is likely corrupt"
        );
    }
    let stats = stats_from_tracker_events(&tracker_events, &players);
    Ok(Replay { map, duration_loops: game_loop, players, actions, game_duration_secs, stats })
}

/// Joins the replay's `Details` sector to `init`'s lobby slots, replicating
/// `s2protocol`'s own `TryFrom<&InitData> for Vec<PlayerLobbyDetails>`. That
/// impl re-reads the file from disk through `InitData::ext_fs_file_name`
/// instead of using the bytes already in hand, which breaks as soon as
/// `name` is not itself a readable path (e.g. `load_bytes` called with an
/// in-memory buffer and a plain label, as the wasm build always does) with
/// an unhelpful "file not found" error. `s2protocol::read_details` decodes
/// `Details` from `bytes` directly, so this join never touches a filesystem.
fn join_lobby_details(name: &str, mpq: &s2protocol::MPQ, bytes: &[u8], init: &InitData) -> Result<Vec<PlayerLobbyDetails>> {
    let details = s2protocol::read_details(name, mpq, bytes).map_err(|e| anyhow::anyhow!("could not read replay details: {e:?}"))?;
    let lobby = details
        .player_list
        .iter()
        .filter_map(|player| {
            let slot_idx = init.sync_lobby_state.lobby_state.slots.iter().position(|slot| {
                matches!((slot.working_set_slot_id, player.working_set_slot_id), (Some(a), Some(b)) if a == b)
            })?;
            Some(PlayerLobbyDetails {
                title: details.title.clone(),
                game_description: init.sync_lobby_state.game_description.clone(),
                lobby_slot: init.sync_lobby_state.lobby_state.slots[slot_idx].clone(),
                player_details: player.clone(),
                time_utc: details.time_utc,
                time_local_offset: details.time_local_offset,
                user_init_data_name: init.sync_lobby_state.user_initial_data.get(slot_idx).map_or_else(String::new, |u| u.name.clone()),
                user_init_data_clan_tag: init
                    .sync_lobby_state
                    .user_initial_data
                    .get(slot_idx)
                    .map_or_else(String::new, |u| u.clan_tag.clone().unwrap_or_default()),
                tracker_setup_player_id: None,
                tracker_setup_slot_id: None,
                cache_handles: details.cache_handles.clone(),
                ext_fs_id: details.ext_fs_id,
                ext_fs_sha256: init.ext_fs_sha256.clone(),
                ext_fs_file_name: init.ext_fs_file_name.clone(),
                ext_datetime: details.ext_datetime,
            })
        })
        .collect();
    Ok(lobby)
}

/// Resolves each player's tracker `player_id` from the authoritative
/// `PlayerSetup` events rather than trusting lobby position. `setups` is
/// `(player_id, user_id)` per tracker `PlayerSetupEvent`; a player whose
/// `user_id` matches no setup's `user_id` keeps the positional id `load`
/// assigned it (e.g. no tracker stream, or this player has no matching
/// setup event).
pub(crate) fn resolve_player_ids(setups: &[(u8, Option<i64>)], players: &mut [Player]) {
    for player in players.iter_mut() {
        if let Some((player_id, _)) = setups.iter().find(|(_, user_id)| *user_id == Some(player.user_id)) {
            player.player_id = *player_id;
        }
    }
}

/// The event types SC2's own APM counts, tagged by kind: commands,
/// selections, control groups, repeats of the previous command
/// (`CommandManagerState`), and re-issuing it on a new unit
/// (`CmdUpdateTargetUnit`). Camera moves and `CmdUpdateTargetPoint` are not
/// counted; fitting every combination against Blizzard's own numbers for an
/// 8-player fixture picked this set, matching within a few percent.
fn action_kind(event: &ReplayGameEvent) -> Option<ActionKind> {
    match event {
        ReplayGameEvent::Cmd(_) => Some(ActionKind::Command),
        ReplayGameEvent::SelectionDelta(_) => Some(ActionKind::Selection),
        ReplayGameEvent::ControlGroupUpdate(_) => Some(ActionKind::ControlGroup),
        ReplayGameEvent::CommandManagerState(_) => Some(ActionKind::Repeat),
        ReplayGameEvent::CmdUpdateTargetUnit(_) => Some(ActionKind::Retarget),
        _ => None,
    }
}

/// Tracker `PlayerStats` samples for kept players, decoded from the tracker
/// events `load` already read (fail-soft to an empty `Vec` there when the
/// replay has no tracker stream or it fails to decode; the macro panels
/// then show their empty state rather than failing the load).
fn stats_from_tracker_events(events: &[TrackerEvent], players: &[Player]) -> Vec<StatsSample> {
    let mut game_loop = 0i64;
    let mut out = Vec::new();
    for ev in events {
        game_loop += i64::from(ev.delta);
        let ReplayTrackerEvent::PlayerStats(s) = &ev.event else {
            continue;
        };
        if !players.iter().any(|p| p.player_id == s.player_id) {
            continue;
        }
        out.push(sample_from(game_loop, s));
    }
    out
}

/// Pure mapping from one tracker `PlayerStats` event to a `StatsSample`.
fn sample_from(game_loop: i64, ev: &PlayerStatsEvent) -> StatsSample {
    let st = &ev.stats;
    StatsSample {
        player_id: ev.player_id,
        game_loop,
        minerals_rate: st.minerals_collection_rate,
        vespene_rate: st.vespene_collection_rate,
        minerals_unspent: st.minerals_current,
        vespene_unspent: st.vespene_current,
        workers: st.workers_active_count,
        supply_used: f64::from(st.food_used),
        supply_made: f64::from(st.food_made),
        army_minerals: st.minerals_used_current_army,
        army_vespene: st.vespene_used_current_army,
        lost_minerals: st.minerals_lost_army,
        lost_vespene: st.vespene_lost_army,
    }
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

/// Looks up `player_id`'s Blizzard-reported APM in the game metadata.
/// `player_id` is now resolved from the tracker's own `PlayerSetup` events
/// (see `resolve_player_ids`), and the game metadata's `PlayerID` is that
/// same id, so this is a direct, exact lookup rather than the positional
/// guess the old `game_apm_by_position` made (and the length-matching guard
/// it needed to detect a shifted lobby join no longer applies).
fn apm_for_player_id(meta: Option<&GameMetadata>, player_id: u8) -> Option<f64> {
    meta?.apm_by_player_id.iter().find(|(pid, _)| *pid == u64::from(player_id)).map(|(_, apm)| *apm)
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
    fn trigger_events_have_no_kind() {
        let ev = ReplayGameEvent::TriggerKeyPressed(GameSTriggerKeyPressedEvent { m_key: 0, m_flags: 0 });
        assert_eq!(action_kind(&ev), None);
    }

    #[test]
    fn repeated_commands_are_repeat_actions() {
        use s2protocol::game_events::{GameECommandManagerState, GameSCommandManagerStateEvent};
        let ev = ReplayGameEvent::CommandManagerState(GameSCommandManagerStateEvent {
            m_state: GameECommandManagerState::EFireOnce,
            m_sequence: None,
        });
        assert_eq!(action_kind(&ev), Some(ActionKind::Repeat));
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
    fn apm_by_player_id_maps_by_id_for_a_subset_of_players() {
        // Three metadata entries, but only two kept players (ids 1 and 3):
        // the lookup is by id, not by position or list length.
        let m = meta(vec![(1, 10.0), (2, 20.0), (3, 30.0)]);
        assert_eq!(apm_for_player_id(Some(&m), 1), Some(10.0));
        assert_eq!(apm_for_player_id(Some(&m), 3), Some(30.0));
    }

    #[test]
    fn apm_by_player_id_is_none_for_an_unmatched_id_or_missing_metadata() {
        let m = meta(vec![(1, 10.0)]);
        assert_eq!(apm_for_player_id(Some(&m), 2), None);
        assert_eq!(apm_for_player_id(None, 1), None);
    }

    fn player(user_id: i64, player_id: u8) -> Player {
        Player {
            user_id,
            name: String::new(),
            race: String::new(),
            team: 0,
            result: String::new(),
            game_apm: None,
            last_event_loop: 0,
            player_id,
        }
    }

    #[test]
    fn resolve_player_ids_matches_setups_in_order() {
        let setups = vec![(1, Some(0)), (2, Some(1)), (3, Some(2))];
        let mut players = vec![player(0, 1), player(1, 2), player(2, 3)];
        resolve_player_ids(&setups, &mut players);
        assert_eq!(players.iter().map(|p| p.player_id).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn resolve_player_ids_corrects_a_shifted_join() {
        // Positional ids (1, 2) are what a lobby join that dropped an
        // earlier details entry would produce; the tracker's own ids (2, 3)
        // are authoritative and must win.
        let setups = vec![(2, Some(5)), (3, Some(7))];
        let mut players = vec![player(5, 1), player(7, 2)];
        resolve_player_ids(&setups, &mut players);
        assert_eq!(players.iter().map(|p| p.player_id).collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn resolve_player_ids_keeps_positional_ids_without_setups() {
        let mut players = vec![player(0, 1), player(1, 2)];
        resolve_player_ids(&[], &mut players);
        assert_eq!(players.iter().map(|p| p.player_id).collect::<Vec<_>>(), vec![1, 2]);
    }
}
