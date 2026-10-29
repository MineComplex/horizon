//! Blocks mobs carry: block items in a hand (`ItemInHandLayer`) and the
//! block an enderman holds (`CarriedBlockLayer`), drawn as shaded cubes like
//! dropped block items. Also the block models mobs wear (`SnowGolemHeadLayer`,
//! `MushroomCowMushroomLayer`).

/// `ElderGuardianParticle.lifetime`.
pub(super) const APPARITION_TICKS: i32 = 30;

impl crate::game::Sim<'_> {
    /// Count the elder guardian's apparition down.
    pub(super) fn tick_apparition(&mut self) {
        self.creatures.elder_apparition = (self.creatures.elder_apparition - 1).max(0);
    }
}
