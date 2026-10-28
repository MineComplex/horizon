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
}
