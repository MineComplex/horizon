//! The spectator menu (`SpectatorGui`): its players and teams from the
//! server's player list and teams, and the teleport request.
use crate::hc::{
    hud::HudBatch,
    hud_layout, keybinds,
    scoreboard::{comp_text, team_color_rgb},
    spectator_menu::{Member, Outcome, SpectatorMenu, Team},
    tab_list::{SPECTATOR, TabPlayer},
    text::Text,
};
use horizon_protocol::packets::play::{self, Serverbound as S};

impl crate::game::Sim<'_> {
    /// `Util.getMillis` for the menu's fade.
    fn spectator_now(&self) -> i64 {
        self.started.elapsed().as_millis() as i64
    }
    /// `new SpectatorMenu`: the listed players, and the teams with a player
    /// to go to (`TeleportToTeamMenuCategory.createTeamEntries`).
    fn new_spectator_menu(&mut self) -> SpectatorMenu {
        let tab: Vec<TabPlayer> = self
            .remote_tab_players()
            .map_or_else(Vec::new, |(players, _, _)| players);
        let member = |p: &TabPlayer| Member {
            uuid: p.uuid,
            name: p.name.clone(),
            skin: p.skin.clone(),
            enabled: p.game_mode != SPECTATOR,
        };
        let players: Vec<Member> = tab.iter().map(member).collect();
        let teams = self
            .score
            .board
            .teams
            .iter()
            .filter_map(|team| {
                let members: Vec<Member> = team
                    .players
                    .iter()
                    .filter_map(|name| tab.iter().find(|p| &p.name == name))
                    .filter(|p| p.game_mode != SPECTATOR)
                    .map(member)
                    .collect();
                if members.is_empty() {
                    return None;
                }
                // A random member's face is the team's icon.
                let pick = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos() as usize)
                    % members.len();
                Some(Team {
                    name: comp_text(&team.display_name),
                    color: team.color.as_deref().and_then(team_color_rgb),
                    icon: members[pick].skin.clone(),
                    players: members,
                })
            })
            .collect();
        SpectatorMenu::new(players, teams)
    }
    fn spectator_outcome(&mut self, outcome: Outcome) {
        if let Outcome::Teleport(uuid) = outcome {
            // PlayerMenuItem.selectItem.
            self.send(S::TeleportToEntity(play::TeleportToEntity { uuid }));
        }
    }
}
