//! Mobs and the guests of an open world. The mob code sees one player
//! (`MobWorld.player`, `Who::Player`); with guests on, each mob sees the
//! player vanilla's `Level.getNearestPlayer` would give it, and keeps the
//! guest it targets until it lets go. What it then does to `Who::Player`
//! (hits, pushes, fire, its projectiles) runs with that guest swapped in.

impl crate::game::Sim<'_> {
    /// The guests' boxes for the blocks entities trigger (pressure plates,
    /// tripwires, detector rails): `Entity.checkInsideBlocks` and
    /// `Level.getEntities` see every player in the level, but not spectators
    /// (`Entity.isIgnoringBlockTriggers`).
    pub(super) fn lan_block_trigger_boxes(&self) -> Vec<([f64; 3], [f64; 3])> {
        Vec::new()
    }
}

impl crate::game::Sim<'_> {}
