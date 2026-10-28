//! Glowing entities (`EntityRenderer.extractRenderState`'s outlineColor): which entities
//! appear glowing, in which team colour, and their silhouettes for the renderer's entity
//! outline (`OutlineBufferSource`).
use crate::hc::{mobs::Mob, scoreboard as sb};

/// `Entity.getTeamColor`: the team's colour, else white.
pub(super) fn team_color(team: Option<&sb::Team>) -> u32 {
    team.and_then(|t| t.color.as_deref())
        .and_then(sb::team_color_rgb)
        .unwrap_or(0xFF_FFFF)
}

impl crate::game::Sim<'_> {
}

impl crate::game::Sim<'_> {
    /// `Entity.isCurrentlyGlowing` of an entity that is not living (an item, a projectile):
    /// its `Glowing` tag.
    pub(super) fn entity_glowing(&self, id: u32) -> bool {
        self.persist_raw(id)
            .and_then(|t| crate::hc::save::number(t.get("Glowing")))
            .is_some_and(|b| b != 0)
    }
}
