//! The scoreboard in the running game: saved data, criteria that follow the
//! player (health, food, xp, deaths, kills, statistics), score holders for
//! entities, the selector options `scores=`, `team=` and `tag=`, the
//! commands' [`Env`] and the sidebar.
use super::{Game, chat::PLAYER_NAME, commands::Target};
use horizon_client::{
    hud::HudBatch,
    hud_layout, hud_overlays,
    lan::guest::Guest,
    save::uuid_text,
    scoreboard::{self as sb, Comp, Scoreboard, literal},
    tab_list::TabPlayer,
};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct ScoreState {
    pub board: Scoreboard,
}

impl Game<'_> {
    // ---- score holders ---------------------------------------------------

    // ---- criteria ----------------------------------------------------------

    // ---- teams in gameplay -----------------------------------------------------

    // ---- commands ------------------------------------------------------------

    // ---- the sidebar -----------------------------------------------------------

    /// `Hud.extractScoreboardSidebar` / `displayScoreboardSidebar`.
    pub(super) fn draw_scoreboard(&self, batch: &mut HudBatch) {
        let board = &self.score.board;
        let Some(objective) = self.sidebar_objective() else {
            return;
        };
        let draws = hud_overlays::sidebar(
            batch.width as i32,
            batch.height as i32,
            board,
            objective,
            &self.front.gui.atlas.font,
            self.settings.text_background(0.3),
            self.settings.text_background(0.4),
        );
        hud_layout::paint(batch, &self.front.gui.atlas, &draws);
    }
}
impl crate::game::Sim<'_> {
    /// `ScoreHolder.getScoreboardName`: the player's name, an entity's UUID.
    pub(super) fn holder_name(&mut self, t: Target) -> Option<String> {
        match t {
            Target::Player => Some(self.acting_player_name().to_owned()),
            Target::Guest(conn) => self.command_guest(conn).map(|g| g.name.clone()),
            Target::Mob(id)
            | Target::Item(id)
            | Target::Stand(id)
            | Target::Frame(id)
            | Target::Painting(id)
            | Target::Boat(id)
            | Target::Cart(id)
            | Target::Crystal(id)
            | Target::Dragon(id)
            | Target::Other(id) => Some(uuid_text(self.uuid_for(id))),
            // Orbs and projectiles have no UUID here.
            Target::Orb(_) | Target::Projectile(_) => None,
        }
    }
    /// `ServerPlayer.awardKillScore`: total kills and the team kill counts.
    pub(super) fn score_player_killed(&mut self, victim: Target) {
        let victim_name = self.holder_name(victim);
        let display = self.player_display_comp();
        let me = self.acting_player_name().to_owned();
        let board = &mut self.score.board;
        let inc = |v: i32| v.wrapping_add(1);
        board.for_all_objectives_with(sb::CRITERIA_TOTAL_KILLS, &me, Some(&display), inc);
        if victim == Target::Player {
            board.for_all_objectives_with(sb::CRITERIA_PLAYER_KILLS, &me, Some(&display), inc);
        }
        let color_of =
            |board: &Scoreboard, name: &str| board.players_team(name).and_then(|t| t.color.clone());
        if let Some(victim_name) = victim_name {
            if let Some(color) = color_of(board, &victim_name) {
                let criteria = format!("teamkill.{color}");
                board.for_all_objectives_with(&criteria, &me, Some(&display), inc);
            }
            if let Some(color) = color_of(board, &me) {
                let criteria = format!("killedByTeam.{color}");
                board.for_all_objectives(&criteria, &victim_name, inc);
            }
        }
    }
    /// `Entity.getTeam` of a mob (a mob joins a team by its UUID).
    pub(super) fn mob_team(&self, id: u32) -> Option<&sb::Team> {
        let uuid = self.persist.uuids.get(&id)?;
        self.score.board.players_team(&uuid_text(*uuid))
    }
    pub(super) fn player_team(&self) -> Option<&sb::Team> {
        self.score.board.players_team(self.acting_player_name())
    }
    /// The teams of the player and of the mobs on one (empty without teams).
    pub(super) fn mob_teams(&self) -> horizon_client::mobs::Teams<'_> {
        let board = &self.score.board;
        if board.teams.is_empty() {
            return Default::default();
        }
        horizon_client::mobs::Teams {
            player: self.player_team(),
            mobs: self
                .persist
                .uuids
                .iter()
                .filter_map(|(id, uuid)| Some((*id, board.players_team(&uuid_text(*uuid))?)))
                .collect(),
        }
    }
    /// The mobs on the player's team (`isAlliedTo`).
    pub(super) fn player_allies(&self) -> HashSet<u32> {
        let Some(team) = self.player_team() else {
            return HashSet::new();
        };
        self.persist
            .uuids
            .iter()
            .filter(|(_, uuid)| team.players.contains(&uuid_text(**uuid)))
            .map(|(id, _)| *id)
            .collect()
    }
    /// `Player.getDisplayName`: the name with the team's prefix, suffix and colour.
    pub(super) fn player_display_comp(&self) -> Comp {
        let name = literal(self.acting_player_name());
        match self.player_team() {
            Some(t) => t.formatted_name_comp(&name),
            None => name,
        }
    }
    /// The objective in the player's team's sidebar slot, else the sidebar's.
    fn sidebar_objective(&self) -> Option<&sb::Objective> {
        let board = &self.score.board;
        if board.display.is_empty() {
            return None;
        }
        // A server's client is in its team by its profile name.
        let name = self
            .remote
            .as_ref()
            .map_or(PLAYER_NAME, |r| r.profile_name.as_str());
        let team_slot = board
            .players_team(name)
            .and_then(|t| t.color.as_ref())
            .map(|c| format!("sidebar.team.{c}"))
            .and_then(|slot| board.display_objective(&slot));
        team_slot.or_else(|| board.display_objective(sb::SLOT_SIDEBAR))
    }
    /// The sidebar the HUD shows: its objective and its lines, for `status`.
    pub(super) fn sidebar_status(&self) -> serde_json::Value {
        let board = &self.score.board;
        let Some(objective) = self.sidebar_objective() else {
            return serde_json::Value::Null;
        };
        let scores: serde_json::Map<String, serde_json::Value> = board
            .scores
            .iter()
            .filter_map(|(holder, s)| Some((holder.clone(), s.get(&objective.name)?.value.into())))
            .collect();
        serde_json::json!({ "objective": objective.name, "title": objective.display_text().plain(), "scores": scores })
    }
}

impl Game<'_> {
    /// `Hud.extractTabList`: the player list while the player-list key is
    /// held. A local world shows it with a joined LAN guest or a `list`
    /// objective.
    pub(super) fn draw_tab_list(&mut self, batch: &mut HudBatch) {
        use horizon_client::tab_list::{self, TabInputs};
        // Player heads and the player list both draw from the same skin slots.
        let ready: Vec<_> = self.skin_downloads.borrow_mut().take_ready();
        for (location, rgba) in ready {
            self.store_skin(location, rgba);
        }
        let key = self.screen == super::Screen::Playing
            && self.keys.contains(&winit::keyboard::KeyCode::Tab);
        let (players, online_mode, upside_down) = match self.remote_tab_players() {
            Some(remote) => remote,
            None => {
                // The integrated server: the local player, online mode, no ping.
                let uuid = self.level.player_uuid.map_or(0, |u| {
                    u.iter()
                        .fold(0u128, |a, &w| (a << 32) | u128::from(w as u32))
                });
                let mut me = TabPlayer::new(uuid, PLAYER_NAME);
                me.game_mode = self.mode_id();
                me.skin = "entity/player/wide/steve".into();
                let guests = &[][..];
                let players = local_tab_players(me, guests);
                let upside_down = players
                    .iter()
                    .filter(|p| p.name == "Dinnerbone" || p.name == "Grumm")
                    .map(|p| p.uuid)
                    .collect();
                (players, true, upside_down)
            }
        };
        let board = &self.sim.score.board;
        let objective = board.display_objective(sb::SLOT_LIST);
        let shown = tab_list::visible(key, false, players.len(), objective.is_some());
        self.front.tab_list.set_visible(shown);
        if !shown {
            return;
        }
        let up = |id: u128| upside_down.contains(&id);
        let inputs = TabInputs {
            players: &players,
            board,
            objective,
            screen_width: batch.width as i32,
            online_mode,
            gui_ticks: self.ticks as i64,
            // Horizon has no "chat background only" option: vanilla's default (on).
            background: tab_list::row_background(
                true,
                f64::from(self.sim.settings.text_background_opacity),
            ),
            upside_down: &up,
        };
        let draws = self
            .front
            .tab_list
            .layout(&inputs, &self.front.gui.atlas.font);
        horizon_client::hud_layout::paint(batch, &self.front.gui.atlas, &draws);
    }
}

/// The host and guests that have entered the world, rebuilt from live state.
fn local_tab_players(me: TabPlayer, guests: &[Guest]) -> Vec<TabPlayer> {
    let mut players = vec![me];
    players.extend(guests.iter().filter(|g| g.placed).map(|g| {
        let mut player = TabPlayer::new(g.uuid, &g.name);
        player.game_mode = g.game_mode;
        player.latency = g.latency.load(std::sync::atomic::Ordering::Relaxed);
        player
    }));
    players
}

#[cfg(test)]
mod tab_list_tests {
    use super::*;
    use horizon_client::{
        lan::{Joined, PlayerSender},
        tab_list,
    };
    use horizon_protocol::packets::configuration::ClientInformation;
    use std::sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
        mpsc,
    };

    fn guest(uuid: u128, name: &str) -> Guest {
        let (tx, _rx) = mpsc::channel();
        Guest::new(
            Joined {
                conn: uuid as u32,
                uuid,
                name: name.into(),
                properties: Vec::new(),
                info: ClientInformation::new(8),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(42)),
                address: "127.0.0.1:25565".parse().unwrap(),
            },
            uuid as i32,
            [0.0, 64.0, 0.0],
            8,
        )
    }

    #[test]
    fn lan_tab_list_tracks_join_and_disconnect() {
        let me = TabPlayer::new(1, PLAYER_NAME);
        let mut guests = vec![guest(2, "Alex")];
        let players = local_tab_players(me.clone(), &guests);
        assert_eq!(players.as_slice(), std::slice::from_ref(&me));
        assert!(!tab_list::visible(true, true, players.len(), false));

        guests[0].placed = true;
        let players = local_tab_players(me.clone(), &guests);
        assert_eq!(players.len(), 2);
        assert_eq!(players[0], me);
        assert_eq!(players[1].uuid, 2);
        assert_eq!(players[1].name, "Alex");
        assert!(tab_list::visible(true, true, players.len(), false));
        assert!(!tab_list::visible(false, true, players.len(), false));

        guests.clear();
        let players = local_tab_players(me, &guests);
        assert_eq!(players.len(), 1);
        assert!(!tab_list::visible(true, true, players.len(), false));
        assert!(tab_list::visible(true, true, players.len(), true));
    }

    #[test]
    fn lan_tab_list_reads_current_guest_state() {
        let me = TabPlayer::new(1, PLAYER_NAME);
        let mut guests = vec![guest(2, "Alex"), guest(3, "Grumm")];
        for guest in &mut guests {
            guest.placed = true;
        }
        guests[0].game_mode = tab_list::SPECTATOR;
        let players = local_tab_players(me.clone(), &guests);
        assert_eq!(players.len(), 3);
        assert_eq!(players[1].game_mode, tab_list::SPECTATOR);
        assert_eq!(players[1].latency, 42);
        assert_eq!(players[1].skin, tab_list::default_skin(2));
        assert_eq!(players[2].name, "Grumm");

        guests[0].game_mode = 1;
        guests[0].latency.store(123, Ordering::Relaxed);
        guests.remove(1);
        let players = local_tab_players(me, &guests);
        assert_eq!(players.len(), 2);
        assert_eq!(players[1].game_mode, 1);
        assert_eq!(players[1].latency, 123);
    }
}
