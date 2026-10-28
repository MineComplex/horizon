//! Glowing entities (`EntityRenderer.extractRenderState`'s outlineColor): which entities
//! appear glowing, in which team colour, and their silhouettes for the renderer's entity
//! outline (`OutlineBufferSource`).
use super::Vertex;
use crate::hc::{mobs::Mob, scoreboard as sb};

/// `Entity.getTeamColor`: the team's colour, else white.
pub(super) fn team_color(team: Option<&sb::Team>) -> u32 {
    team.and_then(|t| t.color.as_deref())
        .and_then(sb::team_color_rgb)
        .unwrap_or(0xFF_FFFF)
}

impl crate::game::Sim<'_> {
    /// `Entity.isCurrentlyGlowing` of an entity that is not living (an item, a projectile):
    /// its `Glowing` tag, or shared flag 6 as a server sent it.
    pub(super) fn entity_glowing(&self, id: u32) -> bool {
        self.remote_glowing(id).unwrap_or_else(|| {
            self.persist_raw(id)
                .and_then(|t| crate::hc::save::number(t.get("Glowing")))
                .is_some_and(|b| b != 0)
        })
    }
    /// `Minecraft.shouldEntityAppearGlowing` of a mob or another player, with its outline
    /// colour (`getTeamColor`; another player's team goes by name).
    pub(super) fn mob_outline(&self, m: &Mob) -> Option<u32> {
        if !m.is_currently_glowing() {
            return None;
        }
        let team = if &*m.kind.id == "minecraft:player" {
            let names = self.remote_player_names();
            let name = names.into_iter().find(|(id, _)| *id == m.id);
            name.and_then(|(_, name)| self.score.board.players_team(&name))
        } else {
            self.mob_team(m.id)
        };
        Some(team_color(team))
    }
}
