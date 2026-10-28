//! Client glue for sapling growth and bone meal: installs the live feature
//! registry and routes `BoneMealItem.useOn` through the behaviour library.
use super::Game;
use horizon_client::{Result, items::ItemStack};
use horizon_core::{geometry::Direction, position::BlockPos};

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// `BoneMealItem.useOn` for the targeted block.
    pub(super) fn use_bone_meal(&mut self, stack: &ItemStack) -> Result<bool> {
        if &*stack.item != "minecraft:bone_meal" {
            return Ok(false);
        }
        let Some(hit) = self.pick() else {
            return Ok(false);
        };
        let face = match hit.face {
            [0, 1, 0] => Direction::Up,
            [0, -1, 0] => Direction::Down,
            [0, 0, -1] => Direction::North,
            [0, 0, 1] => Direction::South,
            [-1, 0, 0] => Direction::West,
            _ => Direction::East,
        };
        let pos = BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]);
        // BlockFertilizeEvent: a cancelled use grows nothing and keeps the meal.
        let used = self.with_level(|b, level, ticks| b.use_bone_meal(level, ticks, pos, face));
        // ItemStack.causeUseVibration at the player, after the meal is used
        // up (shrunk even in creative): an emptied stack has no use effects.
        if used
            && stack.count > 1
            && let Some(p) = self.player.as_ref().map(|p| p.position)
        {
            let ctx = self.player_context(None);
            self.post_game_event("minecraft:item_interact_finish", [p.x, p.y, p.z], ctx);
        }
        self.process_level_events()?;
        if used {
            self.swing_arm();
            // ItemStack.useOn: an item interaction counts as a use.
            self.stats.add("used", &stack.item, 1);
            self.consume_held();
        }
        Ok(used)
    }
}

