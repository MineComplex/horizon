//! `CampfireBlock.useItemOn`: an item in `#minecraft:douses_campfires` (the
//! shovels) put on a lit campfire puts it out.
use crate::hc::{Result, animate_tick::make_campfire_smoke, items::ItemStack};
use horizon_world::chunk::BlockState;

impl crate::game::Server<'_> {
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
    /// `CampfireBlock.useItemOn` with `held` in the hand that acts, the block
    /// at `pos` clicked: true when it doused a lit campfire (SUCCESS).
    pub(super) fn use_campfire_douse(
        &mut self,
        pos: [i32; 3],
        held: Option<&ItemStack>,
    ) -> Result<bool> {
        let (Some(used), Some(mut state)) = (held.cloned(), self.campfire_to_douse(pos, held))
        else {
            return Ok(false);
        };
        let registry = self.pack.assets.registry;
        // Level event 1009 (SOUND_EXTINGUISH_FIRE) goes to every player.
        self.level_event_at(1009, pos, 0);
        // CampfireBlock.douse: the acting client's own smoke (twenty puffs),
        // then the block change game event from the player.
        if self.net_acting.is_none() {
            let signal = state.properties.get("signal_fire").map(String::as_str) == Some("true");
            for _ in 0..20 {
                make_campfire_smoke(&mut self.particles, pos, signal, true);
            }
        }
        self.post_block_game_event("minecraft:block_change", pos, self.sculk_actor(), None);
        state.properties.insert("lit".into(), "false".into());
        if let Ok(unlit) = registry.resolve(&state) {
            self.level_set(pos, unlit)?;
        }
        self.damage_held(1);
        // ServerPlayerGameMode.useItemOn: the action was consumed.
        self.trigger_block("minecraft:item_used_on_block", pos, &used);
        Ok(true)
    }
}
