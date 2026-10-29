//! The crafter (`CrafterBlock`, `CrafterBlockEntity`, `CrafterMenu`,
//! `CrafterScreen`): redstone-triggered crafting of its 3x3 grid, ejecting
//! the result and remainders into the container in front or as items, the
//! crafting animation ticks, and the menu with its toggleable slots.
use super::*;
use crate::hc::{advancements as adv, containers::Crafter};
use serde_json::Value;

/// `CrafterBlock`'s crafting animation length.
const CRAFTING_TICKS: i32 = 6;

fn center(pos: Pos) -> [f64; 3] {
    [
        f64::from(pos.0) + 0.5,
        f64::from(pos.1) + 0.5,
        f64::from(pos.2) + 0.5,
    ]
}
/// Whether an advancement condition's `recipes` entry names the recipe.
fn recipe_matches(condition: &Value, id: &str) -> bool {
    match condition {
        Value::String(s) => s == id,
        Value::Array(list) => list.iter().any(|v| v.as_str() == Some(id)),
        _ => true,
    }
}

impl crate::game::Sim<'_> {
    /// `Level.setBlock` of one boolean property of the block at `pos`.
    pub(crate) fn set_bool_prop(&mut self, pos: Pos, key: &str, value: bool, flags: i32) {
        let registry = self.pack.assets.registry;
        let old = self.world.block(pos.0, pos.1, pos.2);
        let Some(mut state) = registry.state(old).cloned() else {
            return;
        };
        if !state.properties.contains_key(key) {
            return;
        }
        state.properties.insert(key.to_owned(), value.to_string());
        let Ok(id) = registry.resolve(&state) else {
            return;
        };
        self.with_level(|b, level, ticks| {
            b.set_block(level, ticks, BlockPos::new(pos.0, pos.1, pos.2), id, flags)
        });
        let _ = self.process_level_events();
    }
    fn crafter_at(&self, pos: Pos) -> Option<&Crafter> {
        match self.world.block_entities.get(&pos)? {
            BlockEntity::Crafter(c) => Some(c),
            _ => None,
        }
    }
    /// `CrafterBlockEntity.serverTick` of the crafter at `pos`, and the
    /// powered flag following the block state.
    pub(in crate::game) fn tick_crafter(&mut self, pos: Pos) {
        let state = self.world.block(pos.0, pos.1, pos.2);
        let triggered = self.containers().prop(state, "triggered") == Some("true");
        let mut finished = false;
        if let Some(BlockEntity::Crafter(c)) = self.world.block_entities.get_mut(&pos) {
            // CrafterBlock.neighborChanged sets the flag with the state; a
            // loaded crafter keeps its saved flag until then.
            if c.state_triggered.is_some_and(|t| t != triggered) {
                c.triggered = triggered;
            }
            c.state_triggered = Some(triggered);
            let remaining = c.crafting_ticks - 1;
            if remaining >= 0 {
                c.crafting_ticks = remaining;
                finished = remaining == 0;
            }
        }
        if finished {
            self.set_bool_prop(
                pos,
                "crafting",
                false,
                horizon_world::level::flags::UPDATE_ALL,
            );
        }
    }
    /// `CrafterBlock.dispenseFrom` (the scheduled tick after a redstone pulse).
    pub(super) fn crafter_dispense(&mut self, pos: BlockPos, state: BlockStateId) -> Result<()> {
        let key = (pos.x, pos.y, pos.z);
        let Some(crafter) = self.crafter_at(key) else {
            return Ok(());
        };
        let grid: Vec<Option<ItemStack>> = crafter.items.to_vec();
        // RecipeCache.get: the first crafting recipe that matches, assembled.
        let found = self
            .res
            .recipes()
            .find(&grid, 3)
            .map(|(name, result)| (name.to_owned(), result))
            .filter(|(_, result)| result.count > 0);
        let Some((recipe, result)) = found else {
            self.dispenser_event(1050, key, 0);
            return Ok(());
        };
        let Some(facing) = self
            .containers()
            .prop(state, "orientation")
            .and_then(|o| o.split('_').next())
            .and_then(dir_from_name)
        else {
            return Ok(());
        };
        if let Some(BlockEntity::Crafter(c)) = self.world.block_entities.get_mut(&key) {
            c.crafting_ticks = CRAFTING_TICKS;
        }
        self.set_bool_prop(
            key,
            "crafting",
            true,
            horizon_world::level::flags::UPDATE_CLIENTS,
        );
        // The remainders come from the grid as it was before anything moved.
        let remainders = self.res.recipes().remainders(&recipe, &grid, 3);
        self.crafter_eject(key, facing, result, &recipe);
        for rest in remainders.into_iter().flatten() {
            self.crafter_eject(key, facing, rest, &recipe);
        }
        if let Some(BlockEntity::Crafter(c)) = self.world.block_entities.get_mut(&key) {
            for slot in c.items.iter_mut() {
                if let Some(s) = slot {
                    s.count -= 1;
                    if s.count == 0 {
                        *slot = None;
                    }
                }
            }
        }
        self.container_changed(key);
        Ok(())
    }
    /// `CrafterBlock.dispenseItem`: into the container in front, else out as an item.
    fn crafter_eject(&mut self, pos: Pos, facing: Direction, stack: ItemStack, recipe: &str) {
        let target = offset(pos, facing);
        let mut rest = stack;
        if let Some(into) = self.containers().view(target, false) {
            let one_by_one = self.crafter_at(target).is_some()
                || rest.count > self.containers().max_stack(&rest);
            while rest.count > 0 {
                let before = rest.count;
                if one_by_one {
                    let one = ItemStack {
                        count: 1,
                        ..rest.clone()
                    };
                    if self
                        .add_item(None, &into, one, Some(facing.opposite()))
                        .is_some()
                    {
                        break;
                    }
                    rest.count -= 1;
                } else {
                    match self.add_item(None, &into, rest.clone(), Some(facing.opposite())) {
                        Some(left) => rest = left,
                        None => rest.count = 0,
                    }
                    if rest.count == before {
                        break;
                    }
                }
            }
        }
        if rest.count == 0 {
            return;
        }
        self.spawn_dispensed(pos, facing, rest, 6);
        // CrafterRecipeCraftedTrigger for players within the 17-block box.
        let near = self.eye_position().is_some_and(|e| {
            let c = center(pos);
            (0..3).all(|i| (e[i] - c[i]).abs() <= 8.5 + 1.0)
        });
        if near {
            let id = format!("minecraft:{recipe}");
            let me = self.player_entity();
            self.trigger("minecraft:crafter_recipe_crafted", move |c, w| {
                let ctx = adv::Entities {
                    this: Some(&me),
                    origin: me.position,
                    tool: None,
                    block_pos: None,
                };
                adv::conditions(&c["player"], &ctx, w) && recipe_matches(&c["recipes"], &id)
            });
        }
        self.dispenser_event(1049, pos, 0);
        self.dispenser_event(2010, pos, facing as i32);
    }
}
