//! The scoreboard in the running game: saved data, criteria that follow the
//! player (health, food, xp, deaths, kills, statistics), score holders for
//! entities, the selector options `scores=`, `team=` and `tag=`, the
//! commands' [`Env`] and the sidebar.
use super::{Game, chat::PLAYER_NAME};
use horizon_client::{
    hud::HudBatch,
    hud_layout, hud_overlays,
    save::uuid_text,
    scoreboard::{self as sb, Scoreboard},
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
            &self.gui.atlas.font,
            self.settings.text_background(0.3),
            self.settings.text_background(0.4),
        );
        hud_layout::paint(batch, &self.gui.atlas, &draws);
    }
}
impl crate::game::Game<'_> {
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
    /// held. A local world shows it with a joined player or a `list`
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
        let Some((players, online_mode, upside_down)) = self.remote_tab_players() else {
            self.tab_list.set_visible(false);
            return;
        };
        let board = &self.score.board;
        let objective = board.display_objective(sb::SLOT_LIST);
        let shown = tab_list::visible(key);
        self.tab_list.set_visible(shown);
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
                f64::from(self.settings.text_background_opacity),
            ),
            upside_down: &up,
        };
        let draws = self.tab_list.layout(&inputs, &self.gui.atlas.font);
        horizon_client::hud_layout::paint(batch, &self.gui.atlas, &draws);
    }
}
