//! `CampfireBlock.useItemOn`: an item in `#minecraft:douses_campfires` (the
//! shovels) put on a lit campfire puts it out.
use horizon_client::{animate_tick::make_campfire_smoke, items::ItemStack};
use horizon_core::position::BlockPos;
use horizon_world::chunk::BlockState;
use winit::keyboard::KeyCode;

impl crate::game::Game<'_> {
    /// The lit campfire at `pos` that `held` puts out, when the item is in
    /// `#minecraft:douses_campfires` (`CampfireBlock.useItemOn`).
    fn campfire_to_douse(&self, pos: [i32; 3], held: Option<&ItemStack>) -> Option<BlockState> {
        held.filter(|s| {
            self.res
                .items()
                .item_tags
                .get("minecraft:douses_campfires")
                .is_some_and(|members| members.contains(&*s.item))
        })?;
        self.pack
            .assets
            .registry
            .state(self.world.block(pos[0], pos[1], pos[2]))
            .filter(|s| matches!(&*s.name, "minecraft:campfire" | "minecraft:soul_campfire"))
            .filter(|s| s.properties.get("lit").map(String::as_str) == Some("true"))
            .cloned()
    }
    /// `CampfireBlock.useItemOn` as this client runs it before the server's
    /// answer (`MultiPlayerGameMode.useItemOn`): the twenty smoke puffs of
    /// `CampfireBlock.douse` and the campfire gone out, kept as predicted
    /// under `seq`. The level event's sound comes from the server. True when
    /// it doused (SUCCESS), which ends the click for both hands.
    pub(super) fn predict_campfire_douse(
        &mut self,
        pos: [i32; 3],
        held: Option<&ItemStack>,
        seq: i32,
    ) -> bool {
        // Player.isSecondaryUseActive: a held item means both hands count.
        if self.keys.contains(&KeyCode::ShiftLeft) {
            return false;
        }
        let Some(mut state) = self.campfire_to_douse(pos, held) else {
            return false;
        };
        let signal = state.properties.get("signal_fire").map(String::as_str) == Some("true");
        for _ in 0..20 {
            make_campfire_smoke(&mut self.particles, pos, signal, true);
        }
        state.properties.insert("lit".into(), "false".into());
        if let Ok(unlit) = self.pack.assets.registry.resolve(&state) {
            let at = BlockPos::new(pos[0], pos[1], pos[2]);
            self.predict_blocks(&vec![(at, unlit)], seq);
        }
        true
    }
}
