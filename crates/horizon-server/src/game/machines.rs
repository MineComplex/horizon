//! Item-moving blocks: `HopperBlockEntity` (push, pull, item entities,
//! transfer cooldowns), `DispenserBlock` / `DropperBlock.dispenseFrom` with
//! the vanilla dispense behaviours Horizon has entities for, and container
//! comparator signals (`AbstractContainerMenu.getRedstoneSignalFromContainer`).

mod crafter;
use crate::hc::{
    Result,
    containers::{BlockEntity, HOPPER_SLOTS},
    items::{ItemStack, Items},
    world::World,
};
use horizon_core::{geometry::Direction, position::BlockPos, random::RandomSource};
use horizon_world::{level::Behaviours, registry::BlockStateId};

type Pos = (i32, i32, i32);

/// `HopperBlockEntity` transient state (`TransferCooldown`, `tickedGameTime`).
#[derive(Clone, Copy, Debug)]
pub(super) struct HopperState {
    cooldown: i32,
    ticked: i64,
}
impl Default for HopperState {
    fn default() -> Self {
        Self {
            cooldown: -1,
            ticked: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Generic,
    /// `AbstractFurnaceBlockEntity` (a `WorldlyContainer`).
    Furnace,
    Hopper,
    /// `CrafterBlockEntity`: disabled slots and the smallest-stack-first rule.
    Crafter,
    /// `BrewingStandBlockEntity` (a `WorldlyContainer`).
    Brewing,
    /// `ChiseledBookShelfBlockEntity`: books only, one to a slot.
    Bookshelf,
    /// `ComposterBlock.getContainer` at this level (a `WorldlyContainerHolder`
    /// without a block entity): input below 7, empty at 7, bone meal at 8.
    Composter(i32),
}

/// A container: one block entity, or a double chest (`CompoundContainer`).
#[derive(Clone, Debug)]
struct View {
    parts: Vec<(Pos, usize)>,
    kind: Kind,
}
impl View {
    fn len(&self) -> usize {
        self.parts.iter().map(|p| p.1).sum()
    }
    fn locate(&self, mut slot: usize) -> Option<(Pos, usize)> {
        for &(pos, size) in &self.parts {
            if slot < size {
                return Some((pos, slot));
            }
            slot -= size;
        }
        None
    }
    /// `WorldlyContainer.getSlotsForFace` or every slot.
    fn slots_for_face(&self, face: Option<Direction>) -> Vec<usize> {
        match (self.kind, face) {
            (Kind::Furnace, Some(Direction::Down)) => vec![2, 1],
            (Kind::Furnace, Some(Direction::Up)) => vec![0],
            (Kind::Furnace, Some(_)) => vec![1],
            (Kind::Brewing, Some(Direction::Up)) => vec![3],
            (Kind::Brewing, Some(Direction::Down)) => vec![0, 1, 2, 3],
            (Kind::Brewing, Some(_)) => vec![0, 1, 2, 4],
            (Kind::Composter(8), Some(Direction::Down)) => vec![0],
            (Kind::Composter(level), Some(Direction::Up)) if level < 7 => vec![0],
            (Kind::Composter(_), _) => vec![],
            _ => (0..self.len()).collect(),
        }
    }
}

pub(super) fn dir_from_name(name: &str) -> Option<Direction> {
    Some(match name {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => return None,
    })
}
fn step(d: Direction) -> [i32; 3] {
    match d {
        Direction::Down => [0, -1, 0],
        Direction::Up => [0, 1, 0],
        Direction::North => [0, 0, -1],
        Direction::South => [0, 0, 1],
        Direction::West => [-1, 0, 0],
        Direction::East => [1, 0, 0],
    }
}
fn offset(p: Pos, d: Direction) -> Pos {
    let s = step(d);
    (p.0 + s[0], p.1 + s[1], p.2 + s[2])
}
fn clockwise(d: Direction) -> Direction {
    match d {
        Direction::North => Direction::East,
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        other => other,
    }
}

/// State-level container queries shared by the game and comparator signals.
struct Containers<'a> {
    world: &'a World,
    behaviours: &'a Behaviours,
    items: &'a Items,
    registry: &'static horizon_world::registry::BlockRegistry,
    /// Brewing recipes, for what a brewing stand accepts (not needed for signals).
    brewing: Option<&'a crate::hc::brewing::Brewing>,
    /// The stack of a full composter's `OutputContainer`.
    bone_meal: ItemStack,
}
impl Containers<'_> {
    fn prop(&self, state: BlockStateId, key: &str) -> Option<&str> {
        self.registry
            .state(state)?
            .properties
            .get(key)
            .map(String::as_str)
    }
    fn name(&self, state: BlockStateId) -> &str {
        self.registry.state(state).map_or("", |s| s.name.as_str())
    }
    fn max_stack(&self, stack: &ItemStack) -> u32 {
        self.items
            .get(&stack.item)
            .map_or(64, |i| i.max_stack)
            .max(1)
    }
    /// `HopperBlockEntity.getBlockContainer` (double chests combined, right half
    /// first). `blocked_chests`: `ChestBlock.getContainer(.., false)` returns
    /// nothing for a chest under a redstone conductor.
    fn view(&self, pos: Pos, blocked_chests: bool) -> Option<View> {
        let state = self.world.block(pos.0, pos.1, pos.2);
        let name = self.name(state);
        if name == "minecraft:composter" {
            let level = self.prop(state, "level").and_then(|l| l.parse().ok())?;
            return Some(View {
                parts: vec![(pos, usize::from(level != 7))],
                kind: Kind::Composter(level),
            });
        }
        let entity = self.world.block_entities.get(&pos)?;
        // A beehive keeps bees, not items, and a campfire cooks its own: neither
        // is a `Container` (HopperBlockEntity.getContainerAt), so a dropper drops
        // in front of them and a hopper leaves them alone.
        if matches!(entity, BlockEntity::Beehive(_) | BlockEntity::Campfire(_)) {
            return None;
        }
        let kind = match (entity, name) {
            (BlockEntity::Furnace(_), _) => Kind::Furnace,
            (BlockEntity::Crafter(_), _) => Kind::Crafter,
            (BlockEntity::Brewing(_), _) => Kind::Brewing,
            (BlockEntity::ChiseledBookshelf(_), _) => Kind::Bookshelf,
            (_, "minecraft:hopper") => Kind::Hopper,
            _ => Kind::Generic,
        };
        let size = entity.slots().len();
        if matches!(name, "minecraft:chest" | "minecraft:trapped_chest") {
            let is_blocked = |p: Pos| {
                let above = self.world.block(p.0, p.1 + 1, p.2);
                self.behaviours.facts(above).flags
                    & horizon_world::level::data::state_flags::CONDUCTOR
                    != 0
            };
            if blocked_chests && is_blocked(pos) {
                return None;
            }
            let kind_prop = self.prop(state, "type").unwrap_or("single");
            if kind_prop != "single"
                && let Some(facing) = self.prop(state, "facing").and_then(dir_from_name)
            {
                // ChestBlock.getConnectedDirection
                let connected = if kind_prop == "left" {
                    clockwise(facing)
                } else {
                    clockwise(clockwise(clockwise(facing)))
                };
                let other = offset(pos, connected);
                let other_state = self.world.block(other.0, other.1, other.2);
                let paired = self.name(other_state) == name
                    && self
                        .prop(other_state, "type")
                        .is_some_and(|t| t != "single" && t != kind_prop)
                    && self
                        .world
                        .block_entities
                        .get(&other)
                        .is_some_and(|e| e.slots().len() == 27);
                if paired {
                    if blocked_chests && is_blocked(other) {
                        return None;
                    }
                    let parts = if kind_prop == "right" {
                        vec![(pos, size), (other, 27)]
                    } else {
                        vec![(other, 27), (pos, size)]
                    };
                    return Some(View { parts, kind });
                }
            }
        }
        Some(View {
            parts: vec![(pos, size)],
            kind,
        })
    }
    fn get(&self, view: &View, slot: usize) -> Option<&ItemStack> {
        if let Kind::Composter(level) = view.kind {
            // OutputContainer holds one bone meal; InputContainer composts at once.
            return (level == 8 && slot == 0).then_some(&self.bone_meal);
        }
        let (pos, i) = view.locate(slot)?;
        self.world
            .block_entities
            .get(&pos)?
            .slots()
            .get(i)?
            .as_ref()
    }
    fn is_empty(&self, view: &View) -> bool {
        (0..view.len()).all(|i| self.get(view, i).is_none())
    }
    /// `Container.canPlaceItem` (+ `canPlaceItemThroughFace` for furnaces).
    fn can_place(&self, view: &View, slot: usize, stack: &ItemStack) -> bool {
        if view.kind == Kind::Crafter {
            return match self.world.block_entities.get(&view.parts[0].0) {
                Some(BlockEntity::Crafter(c)) => {
                    // The slot's own stack sets the limit (`slotStack.getMaxStackSize`).
                    let max = c.items.get(slot).and_then(Option::as_ref).unwrap_or(stack);
                    c.can_place(slot, stack, self.max_stack(max))
                }
                _ => false,
            };
        }
        if view.kind == Kind::Bookshelf {
            // ListBackedContainer.canPlaceItem: `acceptsItemType` (the books)
            // and a free slot, as `getMaxStackSize` is 1.
            return self
                .items
                .item_tags
                .get("minecraft:bookshelf_books")
                .is_some_and(|m| m.contains(&*stack.item))
                && self.get(view, slot).is_none();
        }
        if view.kind == Kind::Brewing {
            // BrewingStandBlockEntity.canPlaceItem
            return match slot {
                4 => self
                    .items
                    .get(&stack.item)
                    .is_some_and(|i| i.brewing_fuel.is_some()),
                3 => self.brewing.is_some_and(|b| b.is_reagent(stack)),
                _ => {
                    let input = self.brewing.is_some_and(|b| b.is_bottle(stack))
                        || self
                            .items
                            .item_tags
                            .get("minecraft:brewing_potion_inputs")
                            .is_some_and(|m| m.contains(&*stack.item));
                    input && self.get(view, slot).is_none()
                }
            };
        }
        if view.kind != Kind::Furnace {
            return true;
        }
        match slot {
            2 => false,
            1 => {
                let fuel = self
                    .items
                    .get(&stack.item)
                    .is_some_and(|i| i.fuel.is_some());
                let current_bucket = self
                    .get(view, 1)
                    .is_some_and(|s| &*s.item == "minecraft:bucket");
                fuel || (&*stack.item == "minecraft:bucket" && !current_bucket)
            }
            _ => true,
        }
    }
    /// `canTakeItemThroughFace` (furnaces and brewing stands restrict it).
    fn can_take(&self, view: &View, slot: usize, stack: &ItemStack, face: Direction) -> bool {
        if let Kind::Composter(level) = view.kind {
            // OutputContainer.canTakeItemThroughFace.
            return level == 8 && face == Direction::Down && &*stack.item == "minecraft:bone_meal";
        }
        if view.kind == Kind::Brewing {
            return slot != 3 || &*stack.item == "minecraft:glass_bottle";
        }
        view.kind != Kind::Furnace
            || face != Direction::Down
            || slot != 1
            || matches!(&*stack.item, "minecraft:water_bucket" | "minecraft:bucket")
    }
    /// `HopperBlockEntity.isFullContainer`.
    fn is_full(&self, view: &View, face: Direction) -> bool {
        view.slots_for_face(Some(face)).into_iter().all(|i| {
            self.get(view, i)
                .is_some_and(|s| s.count >= self.max_stack(s))
        })
    }
    /// `AbstractContainerMenu.getRedstoneSignalFromContainer`.
    fn signal(&self, view: &View) -> i32 {
        if view.kind == Kind::Crafter {
            // CrafterBlockEntity.getRedstoneSignal: filled or disabled slots.
            return match self.world.block_entities.get(&view.parts[0].0) {
                Some(BlockEntity::Crafter(c)) => c.redstone_signal(),
                _ => 0,
            };
        }
        let mut total = 0.0f32;
        for i in 0..view.len() {
            if let Some(s) = self.get(view, i) {
                total += s.count as f32 / self.max_stack(s).min(64) as f32;
            }
        }
        total /= view.len().max(1) as f32;
        // Mth.lerpDiscrete(total, 0, 15)
        (total * 14.0).floor() as i32 + i32::from(total > 0.0)
    }
}

/// The block columns any item entity's box reaches, so that a hopper with
/// no item over it skips the scan of every item. Kept for one block-entity
/// pass, in which items do not move: removing one leaves a superset, still
/// right, and adding one (always pushed at the end, under a new id) or
/// removing the last rebuilds the columns.
#[derive(Default)]
pub(super) struct ItemColumns {
    /// The item count and last item id when last asked, if built.
    seen: Option<(usize, Option<u32>)>,
    cells: crate::hc::fast_hash::FxHashSet<(i32, i32)>,
}
impl ItemColumns {
    /// Whether an item's box may reach into column (x, z).
    fn may_reach(&mut self, items: &[crate::hc::entities::ItemEntity], x: i32, z: i32) -> bool {
        let (len, last) = (items.len(), items.last().map(|i| i.id));
        let stale = self
            .seen
            .is_none_or(|(seen_len, seen_last)| len > seen_len || last != seen_last);
        self.seen = Some((len, last));
        if stale {
            self.cells.clear();
            for item in items {
                let b = item.bounds();
                let (lo, hi) = (b.min_position(), b.max_position());
                for cx in lo.x.floor() as i32..=hi.x.floor() as i32 {
                    for cz in lo.z.floor() as i32..=hi.z.floor() as i32 {
                        self.cells.insert((cx, cz));
                    }
                }
            }
        }
        self.cells.contains(&(x, z))
    }
}

fn bone_meal() -> ItemStack {
    ItemStack {
        item: "minecraft:bone_meal".into(),
        count: 1,
        damage: 0,
        extra: None,
    }
}

/// Comparator input from a container block entity (0 when there is none).
pub(super) fn container_signal(
    world: &World,
    behaviours: &Behaviours,
    items: &Items,
    registry: &'static horizon_world::registry::BlockRegistry,
    pos: BlockPos,
) -> i32 {
    let c = Containers {
        world,
        behaviours,
        items,
        registry,
        brewing: None,
        bone_meal: bone_meal(),
    };
    c.view((pos.x, pos.y, pos.z), true)
        .filter(|v| !matches!(v.kind, Kind::Composter(_)))
        .map_or(0, |v| c.signal(&v))
}

/// `ChiseledBookShelfBlock.getAnalogOutputSignal` and
/// `ShelfBlock.getAnalogOutputSignal`, from the block entity Horizon keeps as
/// it was loaded; `None` for any other block. `direction` points from the
/// block to the comparator.
pub(super) fn shelf_signal(
    world: &World,
    registry: &'static horizon_world::registry::BlockRegistry,
    state: BlockStateId,
    pos: BlockPos,
    direction: Direction,
) -> Option<i32> {
    let info = registry.state(state)?;
    let chiseled = info.name == "minecraft:chiseled_bookshelf";
    if !chiseled && !info.name.ends_with("_shelf") {
        return None;
    }
    if chiseled {
        // The last slot touched, plus one (`lastInteractedSlot` starts at -1).
        return Some(match world.block_entities.get(&(pos.x, pos.y, pos.z)) {
            Some(BlockEntity::ChiseledBookshelf(b)) => b.last_slot + 1,
            _ => 0,
        });
    }
    let tag = super::spill::saved_entity(world, [pos.x, pos.y, pos.z]);
    // Only read from the back: a bit for each of the three slots in use.
    let facing = info.properties.get("facing").and_then(|f| dir_from_name(f));
    if facing.map(Direction::opposite) != Some(direction) {
        return Some(0);
    }
    let items = crate::hc::save::decode_items(tag.as_ref().and_then(|t| t.get("Items")), 3);
    Some(
        items
            .iter()
            .enumerate()
            .map(|(slot, item)| i32::from(item.is_some()) << slot)
            .sum(),
    )
}

impl crate::game::Server<'_> {
    fn containers(&self) -> Containers<'_> {
        Containers {
            world: &self.world,
            behaviours: self.res.behaviours(),
            items: self.res.items(),
            registry: self.pack.assets.registry,
            brewing: Some(&self.res.stations().brewing),
            bone_meal: bone_meal(),
        }
    }
    fn slot_mut_at(&mut self, view: &View, slot: usize) -> Option<&mut Option<ItemStack>> {
        let (pos, i) = view.locate(slot)?;
        self.world
            .block_entities
            .get_mut(&pos)?
            .slots_mut()
            .get_mut(i)
    }
    /// `BlockEntity.setChanged` for every block entity of a container.
    fn view_changed(&mut self, view: &View) {
        // A composter's containers change its block state instead.
        if let Kind::Composter(_) = view.kind {
            return;
        }
        for &(pos, _) in &view.parts {
            self.container_changed(pos);
        }
    }
    /// `BlockEntity.setChanged` at `pos`: comparators re-read the container.
    pub(super) fn container_changed(&mut self, pos: Pos) {
        if pos.1 == super::minecarts::CART_KEY_Y {
            return;
        }
        self.chiseled_bookshelf_synced(pos);
        // A double chest's other half changes with it.
        let halves: Vec<Pos> = std::iter::once(pos)
            .chain(self.chest_other_half(pos))
            .collect();
        for pos in halves {
            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
            self.with_level(|b, level, ticks| {
                b.container_changed(level, ticks, BlockPos::new(pos.0, pos.1, pos.2))
            });
        }
    }
    /// `HopperBlockEntity.tryMoveInItem`.
    fn try_move_in_item(
        &mut self,
        from_hopper: Option<Pos>,
        view: &View,
        mut stack: ItemStack,
        slot: usize,
    ) -> Option<ItemStack> {
        let c = self.containers();
        if !c.can_place(view, slot, &stack) {
            return Some(stack);
        }
        let was_empty = c.is_empty(view);
        let max = c.max_stack(&stack);
        let mut success = false;
        let target = self.slot_mut_at(view, slot)?;
        match target {
            None => {
                *target = Some(stack.clone());
                stack.count = 0;
                success = true;
            }
            Some(current)
                // HopperBlockEntity.canMergeItems
                if current.count <= max && current.same_item_same_components(&stack) =>
            {
                let space = max.saturating_sub(current.count);
                let count = stack.count.min(space);
                stack.count -= count;
                current.count += count;
                success = count > 0;
            }
            Some(_) => {}
        }
        if success {
            if was_empty && view.kind == Kind::Hopper {
                let dest = view.parts[0].0;
                let from_ticked = from_hopper.and_then(|p| self.hoppers.get(&p).map(|h| h.ticked));
                let state = self.hoppers.entry(dest).or_default();
                if state.cooldown <= 8 {
                    let skip = i32::from(from_ticked.is_some_and(|t| state.ticked >= t));
                    state.cooldown = 8 - skip;
                }
            }
            self.view_changed(view);
        }
        (stack.count > 0).then_some(stack)
    }
    /// `HopperBlockEntity.addItem(from, container, stack, direction)`; the remainder.
    fn add_item(
        &mut self,
        from_hopper: Option<Pos>,
        view: &View,
        stack: ItemStack,
        face: Option<Direction>,
    ) -> Option<ItemStack> {
        if let Kind::Composter(_) = view.kind {
            let (x, y, z) = view.parts[0].0;
            return self.composter_container_insert([x, y, z], stack, face == Some(Direction::Up));
        }
        let slots = if matches!(view.kind, Kind::Furnace | Kind::Brewing) && face.is_some() {
            view.slots_for_face(face)
        } else {
            (0..view.len()).collect()
        };
        let mut rest = Some(stack);
        for slot in slots {
            let Some(stack) = rest.take() else { break };
            rest = self.try_move_in_item(from_hopper, view, stack, slot);
        }
        rest
    }
    /// Remove one item from a slot (`removeItem(slot, 1)`).
    fn take_one(&mut self, view: &View, slot: usize) -> Option<ItemStack> {
        let target = self.slot_mut_at(view, slot)?;
        let current = target.as_mut()?;
        let one = ItemStack {
            count: 1,
            ..current.clone()
        };
        current.count -= 1;
        if current.count == 0 {
            *target = None;
        }
        Some(one)
    }
    /// Undo `take_one` after a failed move.
    fn put_back(&mut self, view: &View, slot: usize, one: ItemStack) {
        if let Some(target) = self.slot_mut_at(view, slot) {
            match target {
                Some(current) => current.count += one.count,
                None => *target = Some(one),
            }
        }
    }
    /// `HopperBlockEntity.pushItemsTick` of the hopper at `pos`, if its block
    /// is still a hopper (`BlockEntityType.isValid`).
    pub(super) fn tick_hopper(&mut self, pos: Pos) {
        let time = self.world.game_time;
        let waiting = {
            let state = self.hoppers.entry(pos).or_default();
            state.cooldown -= 1;
            state.ticked = time;
            state.cooldown > 0
        };
        if waiting {
            return;
        }
        // Most ticks only advance the cooldown: the block is checked when the
        // hopper is about to move, not on every tick it waits.
        let hopper = self
            .res
            .facts()
            .get(self.world.block(pos.0, pos.1, pos.2))
            .name
            .as_ref()
            == "minecraft:hopper";
        if !hopper {
            self.hoppers.remove(&pos);
            return;
        }
        if let Some(state) = self.hoppers.get_mut(&pos) {
            state.cooldown = 0;
        }
        self.hopper_try_move_items(pos);
    }
    /// Drops the cooldowns of hoppers whose block entity is gone.
    pub(super) fn forget_gone_hoppers(&mut self) {
        let entities = &self.world.block_entities;
        self.hoppers.retain(|p, _| {
            matches!(entities.get(p), Some(BlockEntity::Chest(items)) if items.len() == HOPPER_SLOTS)
        });
    }
    /// `HopperBlockEntity.tryMoveItems` with `suckInItems`.
    pub(super) fn hopper_try_move_items(&mut self, pos: Pos) {
        let state = self.world.block(pos.0, pos.1, pos.2);
        let c = self.containers();
        let enabled = c.prop(state, "enabled") != Some("false");
        let Some(view) = c.view(pos, false) else {
            return;
        };
        if self.hoppers.get(&pos).is_some_and(|h| h.cooldown > 0) || !enabled {
            return;
        }
        let mut changed = false;
        if !c.is_empty(&view) {
            changed = self.hopper_eject(pos, &view);
        }
        let c = self.containers();
        let full =
            (0..view.len()).all(|i| c.get(&view, i).is_some_and(|s| s.count == c.max_stack(s)));
        if !full {
            changed |= self.hopper_suck(pos, &view);
        }
        if changed {
            self.hoppers.entry(pos).or_default().cooldown = 8;
            self.container_changed(pos);
        }
    }
    /// `HopperBlockEntity.ejectItems`: into the block container in front, else
    /// a container entity there (`getContainerAt`).
    fn hopper_eject(&mut self, pos: Pos, hopper: &View) -> bool {
        let state = self.world.block(pos.0, pos.1, pos.2);
        let c = self.containers();
        let Some(facing) = c.prop(state, "facing").and_then(dir_from_name) else {
            return false;
        };
        let front = offset(pos, facing);
        let face = facing.opposite();
        if let Some(target) = c.view(front, false) {
            return self.eject_into(pos, hopper, &target, face);
        }
        let Some(id) = self.entity_container_at([
            f64::from(front.0) + 0.5,
            f64::from(front.1) + 0.5,
            f64::from(front.2) + 0.5,
        ]) else {
            return false;
        };
        self.with_entity_container(id, |g, target| g.eject_into(pos, hopper, target, face))
            .unwrap_or(false)
    }
    /// The rest of `HopperBlockEntity.ejectItems`: one item into `target`.
    fn eject_into(&mut self, pos: Pos, hopper: &View, target: &View, face: Direction) -> bool {
        if self.containers().is_full(target, face) {
            return false;
        }
        for slot in 0..hopper.len() {
            let Some(one) = self.take_one(hopper, slot) else {
                continue;
            };
            match self.add_item(Some(pos), target, one, Some(face)) {
                None => {
                    self.view_changed(target);
                    return true;
                }
                Some(rest) => self.put_back(hopper, slot, rest),
            }
        }
        false
    }
    /// `HopperBlockEntity.suckInItems`: from the container above, or item entities.
    fn hopper_suck(&mut self, pos: Pos, hopper: &View) -> bool {
        let above = (pos.0, pos.1 + 1, pos.2);
        let at = [
            f64::from(pos.0) + 0.5,
            f64::from(pos.1) + 1.5,
            f64::from(pos.2) + 0.5,
        ];
        if let Some(took) = self.suck_from_container(above, at, hopper) {
            return took;
        }
        let above_state = self.world.block(above.0, above.1, above.2);
        let blocked = self.res.facts().get(above_state).full_collision
            && !self
                .res
                .behaviours()
                .tag("minecraft:does_not_block_hoppers")
                .is_some_and(|t| t.contains(self.res.behaviours().block(above_state)));
        if blocked {
            return false;
        }
        // Hopper.SUCK_AABB: the hopper bowl and the block above.
        let (min, max) = (
            [
                f64::from(pos.0),
                f64::from(pos.1) + 11.0 / 16.0,
                f64::from(pos.2),
            ],
            [
                f64::from(pos.0) + 1.0,
                f64::from(pos.1) + 2.0,
                f64::from(pos.2) + 1.0,
            ],
        );
        if let Some(columns) = self.item_columns.as_mut()
            && !columns.may_reach(&self.dropped, pos.0, pos.2)
        {
            return false;
        }
        self.suck_item_entities(hopper, min, max)
    }
    /// The container half of `HopperBlockEntity.suckInItems`: one item from the
    /// container at `above`, through its bottom face; with no block container
    /// there, from a container entity whose box meets the one around `at`
    /// (`getSourceContainerOrHandler`). `None` when there is no container.
    fn suck_from_container(&mut self, above: Pos, at: [f64; 3], hopper: &View) -> Option<bool> {
        let c = self.containers();
        let Some(source) = c.view(above, false) else {
            let id = self.entity_container_at(at)?;
            return self
                .with_entity_container(id, |g, source| g.suck_from_slots(None, source, hopper));
        };
        if let Kind::Composter(level) = source.kind {
            if level != 8 {
                return Some(false);
            }
            // tryTakeInItemFromSlot on the OutputContainer: removeItem calls
            // its setChanged, which empties the composter, and so does the
            // second setChanged after the move or, when the hopper has no
            // room, the setItem putting the bone meal back into the detached
            // container (whose bone meal is lost with it).
            let bone_meal = c.bone_meal.clone();
            let at = BlockPos::new(above.0, above.1, above.2);
            self.with_level(|b, level, ticks| b.composter_empty(level, ticks, at));
            let moved = self.add_item(None, hopper, bone_meal, None).is_none();
            self.with_level(|b, level, ticks| b.composter_empty(level, ticks, at));
            self.world.modified.insert((above.0 >> 4, above.2 >> 4));
            return Some(moved);
        }
        let source_hopper = (source.kind == Kind::Hopper).then_some(above);
        Some(self.suck_from_slots(source_hopper, &source, hopper))
    }
    /// `HopperBlockEntity.tryTakeInItemFromSlot` over the `DOWN` face of `source`.
    fn suck_from_slots(
        &mut self,
        source_hopper: Option<Pos>,
        source: &View,
        hopper: &View,
    ) -> bool {
        for slot in source.slots_for_face(Some(Direction::Down)) {
            let c = self.containers();
            let Some(stack) = c.get(source, slot) else {
                continue;
            };
            if !c.can_take(source, slot, stack, Direction::Down) {
                continue;
            }
            let Some(one) = self.take_one(source, slot) else {
                continue;
            };
            match self.add_item(source_hopper, hopper, one, None) {
                None => {
                    self.view_changed(source);
                    return true;
                }
                Some(rest) => self.put_back(source, slot, rest),
            }
        }
        false
    }
    /// `HopperBlockEntity.getEntityContainer`: one of the alive container
    /// entities (`EntitySelector.CONTAINER_ENTITY_SELECTOR`: chest and hopper
    /// minecarts, chest boats) whose box meets the 1x1x1 box around `at`,
    /// picked at random.
    fn entity_container_at(&mut self, at: [f64; 3]) -> Option<u32> {
        let meets = |b: horizon_core::geometry::Aabb| {
            let (lo, hi) = (b.min_position(), b.max_position());
            lo.x < at[0] + 0.5
                && hi.x > at[0] - 0.5
                && lo.y < at[1] + 0.5
                && hi.y > at[1] - 0.5
                && lo.z < at[2] + 0.5
                && hi.z > at[2] - 0.5
        };
        let mut found: Vec<u32> = self
            .carts
            .iter()
            .filter(|c| {
                matches!(
                    c.kind,
                    super::minecarts::CartKind::Chest | super::minecarts::CartKind::Hopper
                ) && meets(c.bounds())
            })
            .map(|c| c.id)
            .collect();
        found.extend(
            self.boats
                .iter()
                .filter(|b| b.chest && meets(b.bounds()))
                .map(|b| b.id),
        );
        if found.is_empty() {
            return None;
        }
        let pick = self
            .world
            .random
            .next_int_bound(found.len() as i32)
            .unwrap_or(0);
        found.get(pick as usize).copied()
    }
    /// Runs `f` on a container entity's items as a container, under the
    /// placeholder key its menu uses (the open menu's copy when it is open),
    /// then hands the items back to the entity.
    fn with_entity_container<R>(
        &mut self,
        id: u32,
        f: impl FnOnce(&mut Self, &View) -> R,
    ) -> Option<R> {
        let mut items = if let Some(c) = self.carts.iter_mut().find(|c| c.id == id) {
            std::mem::take(&mut c.items)
        } else {
            std::mem::take(&mut self.boats.iter_mut().find(|b| b.id == id)?.items)
        };
        let key = (i32::MAX - 15, super::minecarts::CART_KEY_Y, id as i32);
        let (view, open) = self.lend_items(key, &mut items);
        let result = f(self, &view);
        self.reclaim_items(key, open, &mut items);
        if let Some(c) = self.carts.iter_mut().find(|c| c.id == id) {
            c.items = items;
        } else if let Some(b) = self.boats.iter_mut().find(|b| b.id == id) {
            b.items = items;
        }
        Some(result)
    }
    /// A vehicle's slots as a container under its placeholder `key`. When its
    /// menu is open the placeholder is the live copy and stays as it is.
    /// Returns the view and whether the menu was open.
    fn lend_items(&mut self, key: Pos, items: &mut Vec<Option<ItemStack>>) -> (View, bool) {
        let open = self.world.block_entities.contains_key(&key);
        if !open {
            let slots = BlockEntity::Chest(std::mem::take(items));
            self.world.block_entities.insert(key, slots);
        }
        let size = self
            .world
            .block_entities
            .get(&key)
            .map_or(0, |e| e.slots().len());
        let view = View {
            parts: vec![(key, size)],
            kind: Kind::Generic,
        };
        (view, open)
    }
    /// Gives `lend_items`' slots back to the vehicle, so its own items stay
    /// the one source of truth: a menu copy that is still around (open, or
    /// left by a player's close) gets the changed items copied to `items`.
    fn reclaim_items(&mut self, key: Pos, open: bool, items: &mut Vec<Option<ItemStack>>) {
        let slots = if open {
            self.world.block_entities.get(&key).cloned()
        } else {
            self.world.block_entities.remove(&key)
        };
        if let Some(BlockEntity::Chest(slots)) = slots {
            *items = slots;
        }
    }
    /// The item half of `HopperBlockEntity.suckInItems` (`getItemsAtAndAbove`):
    /// adds the item entities that meet the box, until one is taken whole.
    fn suck_item_entities(&mut self, hopper: &View, min: [f64; 3], max: [f64; 3]) -> bool {
        let mut i = 0;
        while i < self.dropped.len() {
            let b = self.dropped[i].bounds();
            let (lo, hi) = (b.min_position(), b.max_position());
            let inside = lo.x < max[0]
                && hi.x > min[0]
                && lo.y < max[1]
                && hi.y > min[1]
                && lo.z < max[2]
                && hi.z > min[2];
            if inside {
                let stack = self.dropped[i].stack.clone();
                match self.add_item(None, hopper, stack, None) {
                    None => {
                        self.dropped.remove(i);
                        return true;
                    }
                    Some(rest) => self.dropped[i].stack = rest,
                }
            }
            i += 1;
        }
        false
    }
    /// `MinecartHopper.tick`'s `tryConsumeItems` (`suckInItems`) for an enabled
    /// hopper minecart: one item from the container above, else the item
    /// entities over it or touching it, each tick without a cooldown.
    pub(super) fn hopper_cart_suck(&mut self, cart: &mut super::minecarts::Minecart) {
        // The cart's slots are edited as a container under the placeholder key
        // its menu uses; when that menu is open the placeholder is the live copy.
        let key = (i32::MAX - 15, super::minecarts::CART_KEY_Y, cart.id as i32);
        let (view, open) = self.lend_items(key, &mut cart.items);
        let p = cart.position;
        // getLevelY is the cart's y + 0.5, so the container is the block at
        // y + 1.5, and Hopper.SUCK_AABB (moved by getLevel - 0.5) starts 11/16
        // above the cart.
        let above = (
            p.x.floor() as i32,
            (p.y + 1.5).floor() as i32,
            p.z.floor() as i32,
        );
        let at = [p.x, p.y + 1.5, p.z];
        let took = match self.suck_from_container(above, at, &view) {
            Some(took) => took,
            None => self.suck_item_entities(
                &view,
                [p.x - 0.5, p.y + 11.0 / 16.0, p.z - 0.5],
                [p.x + 0.5, p.y + 2.0, p.z + 0.5],
            ),
        };
        if !took {
            // The cart's own box, inflated by 0.25 sideways.
            let b = cart.bounds();
            let (lo, hi) = (b.min_position(), b.max_position());
            self.suck_item_entities(
                &view,
                [lo.x - 0.25, lo.y, lo.z - 0.25],
                [hi.x + 0.25, hi.y, hi.z + 0.25],
            );
        }
        self.reclaim_items(key, open, &mut cart.items);
    }
    /// `DispenserBlock.dispenseFrom` / `DropperBlock.dispenseFrom`.
    pub(super) fn dispense_from(&mut self, pos: BlockPos, state: BlockStateId) -> Result<()> {
        let key = (pos.x, pos.y, pos.z);
        let name = self.res.facts().get(state).name.clone();
        if &*name == "minecraft:crafter" {
            return self.crafter_dispense(pos, state);
        }
        let dropper = &*name == "minecraft:dropper";
        if !dropper && &*name != "minecraft:dispenser" {
            return Ok(());
        }
        let Some(view) = self.containers().view(key, false) else {
            return Ok(());
        };
        // DispenserBlockEntity.getRandomSlot
        let mut slot = None;
        let mut odds = 1;
        for i in 0..view.len() {
            if self.containers().get(&view, i).is_some() {
                if self.world.random.next_int_bound(odds).unwrap_or(1) == 0 {
                    slot = Some(i);
                }
                odds += 1;
            }
        }
        let Some(slot) = slot else {
            self.dispenser_event(1001, key, 0);
            if !dropper {
                self.post_block_game_event(
                    "minecraft:block_activate",
                    [pos.x, pos.y, pos.z],
                    None,
                    Some(state),
                );
            }
            return Ok(());
        };
        // BlockDispenseEvent: a cancelled dispense keeps the item inside.
        if let Some(stack) = self.containers().get(&view, slot).cloned()
            && !self.plugin_dispense(pos, &stack)
        {
            return Ok(());
        }
        let Some(facing) = self
            .containers()
            .prop(state, "facing")
            .and_then(dir_from_name)
        else {
            return Ok(());
        };
        let Some(stack) = self.containers().get(&view, slot).cloned() else {
            return Ok(());
        };
        let target = offset(key, facing);
        let remaining = if dropper {
            match self.containers().view(target, false) {
                None => self.dispense_default(key, facing, stack),
                Some(into) => {
                    let one = ItemStack {
                        count: 1,
                        ..stack.clone()
                    };
                    if self
                        .add_item(None, &into, one, Some(facing.opposite()))
                        .is_none()
                    {
                        self.view_changed(&into);
                        Some(ItemStack {
                            count: stack.count - 1,
                            ..stack
                        })
                        .filter(|s| s.count > 0)
                    } else {
                        Some(stack)
                    }
                }
            }
        } else {
            self.dispense_item(key, facing, stack)?
        };
        if let Some(entry) = self.slot_mut_at(&view, slot) {
            *entry = remaining;
        }
        self.container_changed(key);
        Ok(())
    }
}

// ---- hopper, dispenser and dropper screens ----

impl crate::game::Server<'_> {}
