//! Campfires' cooking: `CampfireBlock.useItemOn` (`CampfireBlockEntity.placeFood`)
//! and the block entity's `cookTick` (lit) and `cooldownTick` (unlit). The
//! items drawn on the fire are `campfire_geometry`'s; a server's clients get
//! them through the block entity's update tag (`lan::chunks::entity_tag`).
use crate::hc::{
    Result, block_entities::Pos, containers::BlockEntity, items::ItemStack, mobs::Who,
};
use horizon_core::position::BlockPos;

/// The `campfire_cooking` recipe kind (`RecipeType.CAMPFIRE_COOKING`).
const KIND: &str = "minecraft:campfire_cooking";

fn is_campfire(name: &str) -> bool {
    name == "minecraft:campfire" || name == "minecraft:soul_campfire"
}

impl crate::game::Server<'_> {
    /// `CampfireBlock.useItemOn` with a held item that is a campfire recipe's
    /// input (`RecipePropertySet.CAMPFIRE_INPUT`): on a server level the food
    /// goes into the first free slot (`placeFood`), and the use is consumed
    /// whether or not there was room. A client of a server leaves the placing
    /// to the server. Whether the food went in; None when this isn't a
    /// campfire use, so the item's own use runs.
    pub(super) fn use_on_campfire(&mut self, pos: [i32; 3]) -> Result<Option<bool>> {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if !is_campfire(&self.res.facts().get(state).name) {
            return Ok(None);
        }
        let Some(held) = self.held().cloned() else {
            return Ok(None);
        };
        let Some(time) = self.res.recipes().cook(KIND, &held).map(|r| r.time as i32) else {
            return Ok(None);
        };
        // A client of a server predicts the placing; the server decides.

        // CampfireStartEvent: a plugin may change the cook time.
        let time = self.plugin_campfire_start(pos, &held, time);
        let key = (pos[0], pos[1], pos[2]);
        let placed = match self.world.block_entities.get_mut(&key) {
            Some(BlockEntity::Campfire(c)) => {
                let mut one = held.clone();
                c.place_food(&mut one, time)
            }
            _ => false,
        };
        if placed {
            // ItemStack.consumeAndReturn: creative keeps the item.
            self.consume_held();
            self.stats.custom("interact_with_campfire", 1);
            self.post_block_game_event(
                "minecraft:block_change",
                pos,
                Some(Who::Player),
                Some(state),
            );
            self.mark_block_entity_updated(key);
        }
        Ok(Some(placed))
    }
    /// `CampfireBlockEntity.cookTick` of a lit campfire, `cooldownTick` of an
    /// unlit one. A finished item's result (the recipe's, or the item itself
    /// when no recipe takes it any more) drops at the block's corner
    /// (`Containers.dropItemStack` at its integer position), then the clients
    /// see the slot empty and a `block_change` goes out.
    pub(super) fn tick_campfire(&mut self, pos: Pos) -> Result<()> {
        let state = self.world.block(pos.0, pos.1, pos.2);
        let lit = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("lit"))
            .is_some_and(|v| v == "true");
        let Some(BlockEntity::Campfire(c)) = self.world.block_entities.get_mut(&pos) else {
            return Ok(());
        };
        // BlockEntity.setChanged: a timer that moved is saved with the
        // chunk (no update to the clients, who don't see the timers).
        let changed = if lit {
            c.items.iter().any(Option::is_some)
        } else {
            c.cooldown_tick()
        };
        if changed {
            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        }
        if !lit {
            return Ok(());
        }
        let Some(BlockEntity::Campfire(c)) = self.world.block_entities.get_mut(&pos) else {
            return Ok(());
        };
        let done = c.cook_tick();
        for (slot, item) in done {
            let result = self
                .res
                .recipes()
                .cook(KIND, &item)
                .map_or_else(|| item.clone(), |r| r.result.clone());
            // BlockCookEvent: a plugin may change what drops, or keep it cooking.
            let Some(result) = self.plugin_campfire_cook(pos, slot, &item, result) else {
                continue;
            };
            self.campfire_drop(pos, result);
            self.mark_block_entity_updated(pos);
            self.post_block_game_event(
                "minecraft:block_change",
                [pos.0, pos.1, pos.2],
                None,
                Some(state),
            );
        }
        Ok(())
    }
    /// `Containers.dropItemStack(level, x, y, z, stack)` at the block's
    /// integer position.
    fn campfire_drop(&mut self, pos: Pos, stack: ItemStack) {
        let at = BlockPos::new(pos.0, pos.1, pos.2);
        for piece in
            horizon_world::level::removal::drop_item_stack(&mut self.world.random, at, stack.count)
        {
            self.spawn_item(
                ItemStack {
                    count: piece.count,
                    ..stack.clone()
                },
                piece.pos,
                piece.motion,
                0,
            );
        }
    }
}
