//! Glowing entities (`EntityRenderer.extractRenderState`'s outlineColor): which entities
//! appear glowing, in which team colour, and their silhouettes for the renderer's entity
//! outline (`OutlineBufferSource`).


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
