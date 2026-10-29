//! Shulker boxes (`ShulkerBoxBlockEntity`: 27 slots that refuse other boxes,
//! kept in the dropped item's `container` component) and ender chests (the
//! player's `PlayerEnderChestContainer`, saved as `EnderItems`).
use crate::hc::{containers::BlockEntity, items::ItemStack};
use horizon_core::random::RandomSource;

pub(super) fn is_shulker_box(name: &str) -> bool {
    name.ends_with("shulker_box")
}
impl crate::game::Server<'_> {
    /// Opening an ender chest shows the player's ender inventory in its place.
    pub(super) fn open_ender_chest(&mut self, key: (i32, i32, i32)) {
        self.world
            .block_entities
            .insert(key, BlockEntity::Chest(self.ender_items.clone()));
        self.container = Some(key);
        self.ender_open = Some(key);
    }
    /// The block above `key` is a redstone conductor.
    fn conductor_above(&self, key: (i32, i32, i32)) -> bool {
        let above = self.world.block(key.0, key.1 + 1, key.2);
        self.res.behaviours().facts(above).flags
            & horizon_world::level::data::state_flags::CONDUCTOR
            != 0
    }
    /// `ChestBlock.isChestBlockedAt`: a redstone conductor above the chest or
    /// a cat sitting on it.
    fn chest_blocked_at(&self, key: (i32, i32, i32)) -> bool {
        if self.conductor_above(key) {
            return true;
        }
        // isCatSittingOnChest: a cat whose box meets the block above.
        let area = horizon_core::geometry::Aabb::new(
            horizon_core::geometry::Vec3::new(
                f64::from(key.0),
                f64::from(key.1 + 1),
                f64::from(key.2),
            ),
            horizon_core::geometry::Vec3::new(
                f64::from(key.0 + 1),
                f64::from(key.1 + 2),
                f64::from(key.2 + 1),
            ),
        );
        self.mobs.iter().any(|m| {
            m.species() == crate::hc::mobs::Species::Cat
                && m.alive()
                && crate::hc::mobs::tame::cat_sitting(m)
                && m.bounds().intersects(area)
        })
    }
    /// `ChestBlock.getMenuProvider` (`combine(.., false)`) and
    /// `EnderChestBlock.useWithoutItem`: a blocked chest, or a double chest
    /// with either half blocked, opens no menu.
    pub(super) fn chest_menu_blocked(&self, key: (i32, i32, i32), name: &str) -> bool {
        match name {
            "minecraft:chest" | "minecraft:trapped_chest" => {
                self.chest_blocked_at(key)
                    || self
                        .chest_partner(key)
                        .is_some_and(|(other, _)| self.chest_blocked_at(other))
            }
            // The ender chest only looks at the block above (no cat check).
            "minecraft:ender_chest" => self.conductor_above(key),
            _ => false,
        }
    }
    /// Closing it copies the items back and leaves no block-entity contents.
    pub(super) fn close_ender_chest(&mut self) {
        let Some(key) = self.ender_open.take() else {
            return;
        };
        if let Some(BlockEntity::Chest(items)) = self.world.block_entities.remove(&key) {
            self.ender_items = items;
        }
    }
}

// ---- chest menus: double chests, titles and shift-click ----

/// `Direction.getClockWise` for horizontal facings.
fn clockwise(facing: &str) -> &'static str {
    match facing {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        _ => "north",
    }
}
fn counter_clockwise(facing: &str) -> &'static str {
    match clockwise(facing) {
        "east" => "west",
        "south" => "north",
        "west" => "east",
        _ => "south",
    }
}
fn facing_step(facing: &str) -> (i32, i32) {
    match facing {
        "north" => (0, -1),
        "south" => (0, 1),
        "west" => (-1, 0),
        _ => (1, 0),
    }
}

impl crate::game::Server<'_> {
    /// The other half of the open double chest and whether the open one is
    /// the right half (`ChestBlock.getConnectedDirection`).
    pub(super) fn chest_partner(&self, key: (i32, i32, i32)) -> Option<((i32, i32, i32), bool)> {
        let registry = self.pack.assets.registry;
        let state = registry.state(self.world.block(key.0, key.1, key.2))?;
        if !matches!(
            state.name.as_str(),
            "minecraft:chest" | "minecraft:trapped_chest"
        ) {
            return None;
        }
        let kind = state.properties.get("type")?.as_str();
        if kind == "single" {
            return None;
        }
        let facing = state.properties.get("facing")?.as_str();
        let dir = if kind == "left" {
            clockwise(facing)
        } else {
            counter_clockwise(facing)
        };
        let (dx, dz) = facing_step(dir);
        let other = (key.0 + dx, key.1, key.2 + dz);
        let other_state = registry.state(self.world.block(other.0, other.1, other.2))?;
        let other_kind = other_state.properties.get("type")?.as_str();
        let paired = other_state.name == state.name
            && other_kind != "single"
            && other_kind != kind
            && matches!(
                self.world.block_entities.get(&other),
                Some(BlockEntity::Chest(items)) if items.len() == 27
            );
        paired.then_some((other, kind == "right"))
    }
    /// The open or close sound of a container block (`ChestBlockEntity.playSound`,
    /// `BarrelBlockEntity.playSound`, `ShulkerBoxBlockEntity.startOpen`): volume
    /// 0.5 and pitch 0.9 to 1.0. A double chest sounds once from the middle of
    /// both halves, a barrel from the face it opens on.
    pub(super) fn container_sound(&mut self, sound: &str, key: (i32, i32, i32), name: &str) {
        let mut at = [
            f64::from(key.0) + 0.5,
            f64::from(key.1) + 0.5,
            f64::from(key.2) + 0.5,
        ];
        if let Some((other, _)) = self.chest_partner(key) {
            at[0] = f64::from(key.0 + other.0) / 2.0 + 0.5;
            at[2] = f64::from(key.2 + other.2) / 2.0 + 0.5;
        } else if name == "minecraft:barrel"
            && let Some(state) = self
                .pack
                .assets
                .registry
                .state(self.world.block(key.0, key.1, key.2))
            && let Some(facing) = state.properties.get("facing")
        {
            let (dx, dy, dz) = match facing.as_str() {
                "up" => (0.0, 0.5, 0.0),
                "down" => (0.0, -0.5, 0.0),
                f => {
                    let (x, z) = facing_step(f);
                    (f64::from(x) / 2.0, 0.0, f64::from(z) / 2.0)
                }
            };
            at = [at[0] + dx, at[1] + dy, at[2] + dz];
        }
        let pitch = self.world.random.next_float() * 0.1 + 0.9;
        self.sounds.play(sound, Some(at), 0.5, pitch);
    }
    /// The open or close sound of the container block at `key`, if it has one.
    /// Callers play it only when the opener count crosses zero
    /// (`ContainerOpenersCounter.onOpen` / `onClose`).
    pub(super) fn container_edge_sound(&mut self, key: (i32, i32, i32), open: bool) {
        let name = self
            .res
            .facts()
            .get(self.world.block(key.0, key.1, key.2))
            .name
            .clone();
        let sound = if is_shulker_box(&name) {
            if open {
                "minecraft:block.shulker_box.open"
            } else {
                "minecraft:block.shulker_box.close"
            }
        } else if &*name == "minecraft:ender_chest" {
            if open {
                "minecraft:block.ender_chest.open"
            } else {
                "minecraft:block.ender_chest.close"
            }
        } else if name.contains("chest") {
            if open {
                "minecraft:block.chest.open"
            } else {
                "minecraft:block.chest.close"
            }
        } else if &*name == "minecraft:barrel" {
            if open {
                "minecraft:block.barrel.open"
            } else {
                "minecraft:block.barrel.close"
            }
        } else {
            return;
        };
        self.container_sound(sound, key, &name);
    }
    /// The other half of a double chest at `key`, for neighbour updates.
    pub(super) fn chest_other_half(&self, key: (i32, i32, i32)) -> Option<(i32, i32, i32)> {
        self.chest_partner(key).map(|p| p.0)
    }
    /// The block entities of the open container in menu order, with their
    /// sizes: a double chest is its right half then its left half.
    pub(super) fn container_parts(&self) -> Vec<((i32, i32, i32), usize)> {
        let Some(key) = self.container else {
            return Vec::new();
        };
        let Some(entity) = self.world.block_entities.get(&key) else {
            return Vec::new();
        };
        let own = (key, entity.slots().len());
        match self.chest_partner(key) {
            Some((other, true)) => vec![own, (other, 27)],
            Some((other, false)) => vec![(other, 27), own],
            None => vec![own],
        }
    }
    /// `Container.stillValidBlockEntity`: the open block container closes once
    /// the player's eyes are out of reach of any of its blocks (a 4-block buffer).
    pub(super) fn tick_container_reach(&mut self) {
        // A server's menu ends when the server closes it.

        let Some(eye) = self.eye_position() else {
            return;
        };
        if self
            .container
            .is_none_or(|k| k.1 == super::minecarts::CART_KEY_Y)
        {
            return;
        }
        let range = self.reach() + 4.0;
        let near = self.container_parts().iter().all(|&((x, y, z), _)| {
            crate::hc::server_net::player::within_block_range(eye, range, [x, y, z])
        });
        if !near {
            self.close_container();
            self.screen = super::Screen::Playing;
            self.capture(true);
        }
    }
    /// `CraftingMenu.stillValid`: the open crafting table closes once its
    /// block is no longer a crafting table or the player's eyes are out of
    /// reach (a 4-block buffer).
    pub(super) fn tick_craft_table(&mut self) {
        // A server's menu ends when the server closes it.
        if self.craft_size != 3 {
            return;
        }
        let (Some(pos), Some(eye)) = (self.craft_table, self.eye_position()) else {
            return;
        };
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let table = &*self.res.facts().get(state).name == "minecraft:crafting_table";
        let range = self.reach() + 4.0;
        if !(table && crate::hc::server_net::player::within_block_range(eye, range, pos)) {
            self.close_container();
            self.screen = super::Screen::Playing;
            self.capture(true);
        }
    }
    /// The number of slots of the open container.
    pub(super) fn container_size(&self) -> usize {
        self.container_parts().iter().map(|p| p.1).sum()
    }
    /// Container slot `n` (menu slot `300 + n`).
    pub(super) fn container_slot_mut(&mut self, mut n: usize) -> &mut Option<ItemStack> {
        let mut target = None;
        for (pos, size) in self.container_parts() {
            if n < size {
                target = Some((pos, n));
                break;
            }
            n -= size;
        }
        match target.and_then(|(pos, i)| {
            self.world
                .block_entities
                .get_mut(&pos)
                .and_then(|e| e.slots_mut().get_mut(i))
        }) {
            Some(slot) => slot,
            None => &mut self.scratch_slot,
        }
    }
}

impl crate::game::Server<'_> {}
