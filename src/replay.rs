//! Loads a .SC2Replay and reduces it to players and countable actions.
//! This is the only module that knows about the `s2protocol` crate.

use std::path::Path;

use anyhow::{Context, Result, bail};
use s2protocol::details::PlayerLobbyDetails;
use s2protocol::game_events::ReplayGameEvent;
use s2protocol::init_data::InitData;

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub user_id: i64,
    pub name: String,
    pub race: String,
    pub team: u8,
    pub result: String,
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
}

pub fn load(path: &Path) -> Result<Replay> {
    if !path.is_file() {
        bail!("replay file not found: {}", path.display());
    }
    let path_str = path.to_str().context("replay path is not valid UTF-8")?;

    let init = InitData::try_from((path.to_path_buf(), 0u64))
        .with_context(|| format!("could not parse replay header of {}", path.display()))?;
    let lobby: Vec<PlayerLobbyDetails> =
        (&init).try_into().context("could not join lobby slots to player details")?;
    let map = lobby.first().map(|p| p.title.clone()).unwrap_or_default();
    let players: Vec<Player> = lobby
        .iter()
        .filter(|p| p.lobby_slot.observe == 0)
        .filter_map(|p| {
            Some(Player {
                user_id: p.lobby_slot.user_id?,
                name: strip_clan_markup(&p.player_details.name),
                race: p.player_details.race.clone(),
                team: p.player_details.team_id,
                result: p.player_details.result.clone(),
            })
        })
        .collect();
    if players.is_empty() {
        bail!("no players found in {}", path.display());
    }

    let (mpq, contents) = s2protocol::read_mpq(path_str).context("could not read MPQ archive")?;
    let events = s2protocol::read_game_events(path_str, &mpq, &contents)
        .context("could not decode game events (unsupported protocol version?)")?;

    let mut game_loop = 0i64;
    let mut actions = Vec::new();
    for ev in &events {
        game_loop += ev.delta;
        if counts_as_action(&ev.event) && players.iter().any(|p| p.user_id == ev.user_id) {
            actions.push(Action { user_id: ev.user_id, game_loop });
        }
    }
    Ok(Replay { map, duration_loops: game_loop, players, actions })
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
fn strip_clan_markup(name: &str) -> String {
    match name.rfind('>') {
        Some(pos) => name[pos + 1..].to_string(),
        None => name.to_string(),
    }
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
}
