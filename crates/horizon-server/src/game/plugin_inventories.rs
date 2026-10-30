//! Where the server fires the plugins' inventories events (docs/plugins.md):
//! the results menus work out (crafting grids, anvils, smithing tables,
//! grindstones, looms, stonecutters, cartography tables), furnaces, brewing
//! stands and crafters at work, hoppers and droppers moving items, and a
//! player's drags, trades, recipe book and lectern.
use super::{plugin_events::stack_ref, server_net::Network};
use crate::hc::{
    brewing::{BrewTick, brewing_fuel},
    containers::{BlockEntity, Furnace},
    furnace::FurnaceCooking,
    items::ItemStack,
    menu::{Kind, Menu},
};
use horizon_protocol::packets::play;
use horizon_server_api::{self as api, event as ev};
use std::{borrow::Cow, collections::HashMap};

/// A block entity's key.
type Key = (i32, i32, i32);

/// What these events keep between ticks.
#[derive(Default)]
pub(super) struct State {
    /// The result a plugin gave a player's inventory grid, with the grid it
    /// is for: the grid is set again for each click, which works the result
    /// out again.
    craft_results: HashMap<u32, (Vec<Option<ItemStack>>, Option<ItemStack>)>,
    /// The drag (quick craft) a player is making: its type and slots.
    drags: HashMap<u32, (i8, Vec<i32>)>,
}

/// A crafting grid before a player's click or placed recipe.
pub(super) struct CraftBefore {
    grid: Vec<Option<ItemStack>>,
    /// The result a plugin gave this grid of the player's inventory, to set
    /// again after the grid.
    pub(super) kept: Option<Option<ItemStack>>,
}

/// A workstation's own slots, cost and name before a player's click.
pub(super) struct StationBefore {
    slots: Vec<Option<ItemStack>>,
    cost: i32,
    name: Option<String>,
}

/// A brewing stand before its tick.
pub(super) struct BrewBefore {
    items: [Option<ItemStack>; 5],
    /// The fuel's power and whether it is used up, as a plugin left them.
    fuel: Option<(i32, bool)>,
    /// A fuel item a plugin kept out of this tick.
    hidden: Option<ItemStack>,
}

/// A stack a plugin handed back (None, or a count of 0: nothing): the
/// game's own when it is the same, keeping its components.
fn game_stack(old: Option<&ItemStack>, new: Option<&api::ItemStack>) -> Option<ItemStack> {
    let new = new.filter(|s| s.count > 0)?;
    match old {
        Some(old) if stack_ref(old) == *new => Some(old.clone()),
        _ => Some(ItemStack::new(&new.item, new.count)),
    }
}

fn one_of(stack: &ItemStack) -> api::ItemStack {
    api::ItemStack::new(&stack.item, 1)
}

fn refs(stacks: &[Option<ItemStack>]) -> Vec<Option<api::ItemStack>> {
    stacks.iter().map(|s| s.as_ref().map(stack_ref)).collect()
}

fn count(s: Option<&ItemStack>) -> u32 {
    s.map_or(0, |s| s.count)
}

impl crate::game::Server<'_> {
    // -----------------------------------------------------------------------
    // Results menus work out.

    /// The crafting grid before the player of `conn` changes it (`grid`: the
    /// inventory's; None: the crafting table `menu`'s), when a plugin listens
    /// to `PrepareItemCraftEvent`.
    pub(super) fn plugin_craft_before(
        &self,
        conn: u32,
        menu: &Menu,
        grid: Option<&[Option<ItemStack>]>,
    ) -> Option<CraftBefore> {
        if !self.plugins_listen::<ev::PrepareItemCraftEvent>() {
            return None;
        }
        Some(match grid {
            Some(grid) => CraftBefore {
                grid: grid.to_vec(),
                kept: self
                    .plugin_inventories
                    .craft_results
                    .get(&conn)
                    .filter(|(g, _)| g == grid)
                    .map(|(_, r)| r.clone()),
            },
            None => CraftBefore {
                grid: (1..10).map(|s| menu.slot(s).cloned()).collect(),
                kept: None,
            },
        })
    }
    /// The grid of `menu` after the change: when it changed,
    /// `PrepareItemCraftEvent`, and the result a plugin set goes in slot 0.
    /// True when a plugin changed the result.
    pub(super) fn plugin_craft_after(
        &mut self,
        net: &Network,
        conn: u32,
        menu: &mut Menu,
        before: Option<CraftBefore>,
    ) -> bool {
        let Some(before) = before else {
            return false;
        };
        let n = before.grid.len();
        let inventory = n == 4;
        let grid: Vec<Option<ItemStack>> = (1..=n).map(|s| menu.slot(s).cloned()).collect();
        if grid == before.grid {
            return false;
        }
        if inventory {
            self.plugin_inventories.craft_results.remove(&conn);
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return false;
        };
        let result = menu.slot(0).cloned();
        let mut event = ev::PrepareItemCraftEvent {
            player,
            view: ev::InventoryView {
                kind: if inventory {
                    "minecraft:inventory"
                } else {
                    "minecraft:crafting"
                }
                .to_owned(),
                size: n as u32 + 1,
            },
            matrix: refs(&grid),
            result: result.as_ref().map(stack_ref),
        };
        self.plugin_fire(Some(net), &mut event);
        if event.result == result.as_ref().map(stack_ref) {
            return false;
        }
        let new = game_stack(result.as_ref(), event.result.as_ref());
        menu.set_slot(&self.menu_env(), 0, new.clone());
        if inventory {
            self.plugin_inventories
                .craft_results
                .insert(conn, (grid, new));
        }
        true
    }
    /// A workstation's slots before a player's click, when a plugin listens
    /// to its `Prepare*` event.
    pub(super) fn plugin_station_before(&self, menu: &Menu) -> Option<StationBefore> {
        let listens = match menu.kind() {
            Kind::Anvil => self.plugins_listen::<ev::PrepareAnvilEvent>(),
            Kind::Smithing => self.plugins_listen::<ev::PrepareSmithingEvent>(),
            Kind::Grindstone => self.plugins_listen::<ev::PrepareGrindstoneEvent>(),
            Kind::Loom | Kind::Stonecutter | Kind::Cartography => {
                self.plugins_listen::<ev::PrepareInventoryResultEvent>()
            }
            _ => false,
        };
        listens.then(|| StationBefore {
            slots: (0..menu.slot_count() - 36)
                .map(|s| menu.slot(s).cloned())
                .collect(),
            cost: menu.data.first().copied().unwrap_or(0),
            name: menu.anvil.name.clone(),
        })
    }
    /// The workstation `menu` (`menu_type`, its `inputs` slots, the result
    /// right after them) after a player's click: when it changed,
    /// `PrepareAnvilEvent`, `PrepareSmithingEvent`, `PrepareGrindstoneEvent`
    /// or `PrepareInventoryResultEvent`, and the result (and anvil cost) a
    /// plugin set.
    pub(super) fn plugin_station_after(
        &mut self,
        net: &Network,
        conn: u32,
        menu_type: &str,
        inputs: std::ops::Range<usize>,
        menu: &mut Menu,
        before: Option<StationBefore>,
    ) {
        let Some(before) = before else {
            return;
        };
        let own = menu.slot_count() - 36;
        let slots: Vec<Option<ItemStack>> = (0..own).map(|s| menu.slot(s).cloned()).collect();
        let cost = menu.data.first().copied().unwrap_or(0);
        if slots == before.slots && cost == before.cost && menu.anvil.name == before.name {
            return;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return;
        };
        let view = ev::InventoryView {
            kind: menu_type.to_owned(),
            size: own as u32,
        };
        let at = inputs.end;
        let ins = refs(&slots[inputs]);
        let result = slots.get(at).cloned().flatten();
        let old = result.as_ref().map(stack_ref);
        let (new, new_cost) = match menu.kind() {
            Kind::Anvil => {
                let mut event = ev::PrepareAnvilEvent {
                    player,
                    view,
                    inputs: ins,
                    rename_text: menu.anvil.name.clone(),
                    result: old.clone(),
                    repair_cost: cost,
                };
                self.plugin_fire(Some(net), &mut event);
                (event.result, event.repair_cost)
            }
            Kind::Smithing => {
                let mut event = ev::PrepareSmithingEvent {
                    player,
                    view,
                    inputs: ins,
                    result: old.clone(),
                };
                self.plugin_fire(Some(net), &mut event);
                (event.result, cost)
            }
            Kind::Grindstone => {
                let mut event = ev::PrepareGrindstoneEvent {
                    player,
                    view,
                    inputs: ins,
                    result: old.clone(),
                };
                self.plugin_fire(Some(net), &mut event);
                (event.result, cost)
            }
            kind => {
                let mut event = ev::PrepareInventoryResultEvent {
                    player,
                    view,
                    inputs: ins,
                    result: old.clone(),
                };
                self.plugin_fire(Some(net), &mut event);
                // CartographyTableMenu works its result out again when it is set.
                if kind == Kind::Cartography {
                    return;
                }
                (event.result, cost)
            }
        };
        if new_cost != cost
            && let Some(c) = menu.data.first_mut()
        {
            *c = new_cost;
        }
        if new != old {
            let stack = game_stack(result.as_ref(), new.as_ref());
            menu.set_slot(&self.menu_env(), at, stack);
        }
    }
    /// The player of `conn` takes `result` out of their smithing table:
    /// `SmithItemEvent`. False when cancelled.
    pub(super) fn plugin_smith_item(
        &mut self,
        net: &Network,
        conn: u32,
        result: &ItemStack,
    ) -> bool {
        if !self.plugins_listen::<ev::SmithItemEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let event = ev::SmithItemEvent {
            player,
            view: ev::InventoryView {
                kind: "minecraft:smithing".to_owned(),
                size: 4,
            },
            result: stack_ref(result),
            cancelled: false,
        };
        self.plugin_allows(Some(net), event)
    }

    // -----------------------------------------------------------------------
    // Furnaces, brewing stands, crafters.

    /// The furnace at `pos` before its tick, when a plugin listens to
    /// `FurnaceBurnEvent` or `FurnaceSmeltEvent`.
    pub(super) fn plugin_furnace_before(&self, pos: Key) -> Option<Furnace> {
        if !self.plugins_listen::<ev::FurnaceBurnEvent>()
            && !self.plugins_listen::<ev::FurnaceSmeltEvent>()
        {
            return None;
        }
        match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Furnace(f)) => Some(f.clone()),
            _ => None,
        }
    }
    /// What the furnace at `pos` did in its tick (`before`: as it was, `lit`:
    /// its lit state change): `FurnaceBurnEvent` for a fuel it lit and
    /// `FurnaceSmeltEvent` for an item it cooked, undone when cancelled. The
    /// lit state change after them.
    pub(super) fn plugin_furnace_ticked(
        &mut self,
        pos: Key,
        before: Option<Furnace>,
        lit: Option<bool>,
    ) -> Option<bool> {
        let Some(before) = before else {
            return lit;
        };
        let Some(BlockEntity::Furnace(after)) = self.world.block_entities.get(&pos) else {
            return lit;
        };
        let ignited = after.lit_total > 0
            && after.lit_remaining == after.lit_total
            && before.lit_remaining <= 1
            && after.items[1] != before.items[1];
        let smelted = count(after.items[0].as_ref()) < count(before.items[0].as_ref());
        let (burn_time, output) = (after.lit_total, after.items[2].clone());
        let block = self.plugin_block([pos.0, pos.1, pos.2]);
        if ignited
            && self.plugins_listen::<ev::FurnaceBurnEvent>()
            && let Some(fuel) = before.items[1].as_ref()
        {
            let mut event = ev::FurnaceBurnEvent {
                block: block.clone(),
                fuel: one_of(fuel),
                burn_time,
                cancelled: false,
            };
            self.plugin_fire(None, &mut event);
            let Some(BlockEntity::Furnace(f)) = self.world.block_entities.get_mut(&pos) else {
                return lit;
            };
            if event.cancelled {
                // The fuel stays, the furnace goes out: nothing of the tick happened.
                let was_lit = before.lit_remaining > 0;
                *f = before;
                f.lit_remaining = 0;
                f.cooking_timer = 0;
                return was_lit.then_some(false);
            }
            if event.burn_time != burn_time {
                f.lit_remaining = event.burn_time;
                f.lit_total = event.burn_time;
            }
        }
        if smelted
            && self.plugins_listen::<ev::FurnaceSmeltEvent>()
            && let Some(source) = before.items[0].as_ref()
            && let Some(made) = output.map(|mut s| {
                s.count -= count(before.items[2].as_ref());
                s
            })
        {
            let mut event = ev::FurnaceSmeltEvent {
                block,
                source: one_of(source),
                result: stack_ref(&made),
                cancelled: false,
            };
            self.plugin_fire(None, &mut event);
            let Some(BlockEntity::Furnace(f)) = self.world.block_entities.get_mut(&pos) else {
                return lit;
            };
            let mut undo = event.cancelled;
            if !undo && event.result != stack_ref(&made) {
                let new = game_stack(Some(&made), Some(&event.result));
                match (new, before.items[2].as_ref()) {
                    (None, old) => f.items[2] = old.cloned(),
                    (Some(new), None) => f.items[2] = Some(new),
                    (Some(new), Some(old)) if old.same_item_same_components(&new) => {
                        f.items[2] = Some(ItemStack {
                            count: old.count + new.count,
                            ..old.clone()
                        });
                    }
                    // Another item in the output: nothing is made.
                    _ => undo = true,
                }
            }
            if undo {
                f.items[0] = before.items[0].clone();
                f.items[2] = before.items[2].clone();
                f.recipes_used = before.recipes_used.clone();
                // A dried sponge's bucket stays empty.
                if f.items[1]
                    .as_ref()
                    .is_some_and(|s| &*s.item == "minecraft:water_bucket")
                    && before.items[1]
                        .as_ref()
                        .is_some_and(|s| &*s.item == "minecraft:bucket")
                {
                    f.items[1] = before.items[1].clone();
                }
            }
        }
        lit
    }
    /// The player of `conn` took `taken` out of the furnace at `pos`, which
    /// pops the experience of what it made at `at`: `FurnaceExtractEvent`
    /// may change how much.
    pub(super) fn plugin_furnace_extract(
        &mut self,
        net: &Network,
        conn: u32,
        pos: Key,
        at: [f64; 3],
        taken: Option<ItemStack>,
    ) {
        let player = self
            .plugins_listen::<ev::FurnaceExtractEvent>()
            .then(|| Self::plugin_player_of(net, conn))
            .flatten();
        let (Some(player), Some(taken)) = (player, taken) else {
            self.award_furnace_recipes(pos, at);
            return;
        };
        let used = match self.world.block_entities.get_mut(&pos) {
            Some(BlockEntity::Furnace(f)) => {
                let used = f.recipe_experience(self.res.recipes());
                f.recipes_used.clear();
                used
            }
            _ => return,
        };
        let orbs: Vec<u32> = used
            .into_iter()
            .map(|(count, experience)| {
                crate::hc::containers::create_experience(count, experience, &mut self.world.random)
            })
            .collect();
        let total: u32 = orbs.iter().sum();
        let mut event = ev::FurnaceExtractEvent {
            player,
            block: self.plugin_block([pos.0, pos.1, pos.2]),
            item: stack_ref(&taken),
            exp: total as i32,
        };
        self.plugin_fire(Some(net), &mut event);
        if event.exp == total as i32 {
            for xp in orbs {
                self.spawn_orbs(at, xp);
            }
        } else if event.exp > 0 {
            self.spawn_orbs(at, event.exp as u32);
        }
    }
    /// The brewing stand at `pos` before its tick, when a plugin listens to
    /// `BrewEvent` or `BrewingStandFuelEvent`; an empty stand about to use
    /// its fuel item fires `BrewingStandFuelEvent` now.
    pub(super) fn plugin_brewing_before(&mut self, pos: Key) -> Option<BrewBefore> {
        let fuel_listens = self.plugins_listen::<ev::BrewingStandFuelEvent>();
        if !fuel_listens && !self.plugins_listen::<ev::BrewEvent>() {
            return None;
        }
        let Some(BlockEntity::Brewing(b)) = self.world.block_entities.get(&pos) else {
            return None;
        };
        let mut before = BrewBefore {
            items: b.items.clone(),
            fuel: None,
            hidden: None,
        };
        let power = (fuel_listens && b.fuel <= 0)
            .then_some(b.items[4].as_ref())
            .flatten()
            .and_then(|stack| brewing_fuel(self.res.items(), stack))
            .map(|(uses, _)| self.res.providers().int_value(&uses));
        let (Some(power), Some(fuel)) = (power, before.items[4].as_ref()) else {
            return Some(before);
        };
        let mut event = ev::BrewingStandFuelEvent {
            block: self.plugin_block([pos.0, pos.1, pos.2]),
            fuel: one_of(fuel),
            fuel_power: power,
            consuming: true,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            // The stand doesn't see its fuel this tick.
            if let Some(BlockEntity::Brewing(b)) = self.world.block_entities.get_mut(&pos) {
                before.hidden = b.items[4].take();
            }
        } else {
            before.fuel = Some((event.fuel_power, event.consuming));
        }
        Some(before)
    }
    /// What the brewing stand at `pos` did in its tick (`before`: as it was):
    /// the fuel as a plugin left it, then `BrewEvent` for a finished brew,
    /// undone when cancelled.
    pub(super) fn plugin_brewing_ticked(
        &mut self,
        pos: Key,
        before: Option<BrewBefore>,
        mut out: BrewTick,
    ) -> BrewTick {
        let Some(before) = before else {
            return out;
        };
        let brew_listens = self.plugins_listen::<ev::BrewEvent>();
        let Some(BlockEntity::Brewing(b)) = self.world.block_entities.get_mut(&pos) else {
            return out;
        };
        if let Some(fuel) = before.hidden {
            b.items[4] = Some(fuel);
        }
        if let Some((power, consuming)) = before.fuel {
            b.fuel += power - b.total_fuel;
            b.total_fuel = power;
            if !consuming {
                b.items[4] = before.items[4].clone();
            }
        }
        if !out.brewed || !brew_listens {
            return out;
        }
        let Some(ingredient) = before.items[3].as_ref() else {
            return out;
        };
        let results = refs(&b.items[0..3]);
        let fuel_level = b.fuel;
        let mut event = ev::BrewEvent {
            block: self.plugin_block([pos.0, pos.1, pos.2]),
            ingredient: one_of(ingredient),
            results: results.clone(),
            fuel_level,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        let Some(BlockEntity::Brewing(b)) = self.world.block_entities.get_mut(&pos) else {
            return out;
        };
        if event.cancelled {
            // The bottles and the ingredient as they were (its remainder isn't dropped).
            b.items[..4].clone_from_slice(&before.items[..4]);
            out.brewed = false;
            out.drops.clear();
        } else if event.results != results {
            for k in 0..3 {
                let new = event.results.get(k).cloned().flatten();
                b.items[k] = game_stack(b.items[k].as_ref(), new.as_ref());
            }
        }
        let bottles = [
            b.items[0].is_some(),
            b.items[1].is_some(),
            b.items[2].is_some(),
        ];
        if b.last_bottles != Some(bottles) {
            b.last_bottles = Some(bottles);
            out.bottles = Some(bottles);
        }
        out
    }
    /// The crafter at `pos` is about to craft `recipe` into `result`:
    /// `CrafterCraftEvent`. The result to eject, or None when cancelled.
    pub(super) fn plugin_crafter_craft(
        &mut self,
        pos: Key,
        recipe: &str,
        result: ItemStack,
    ) -> Option<ItemStack> {
        if !self.plugins_listen::<ev::CrafterCraftEvent>() {
            return Some(result);
        }
        let mut event = ev::CrafterCraftEvent {
            block: self.plugin_block([pos.0, pos.1, pos.2]),
            recipe: recipe.to_owned(),
            result: stack_ref(&result),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            return None;
        }
        game_stack(Some(&result), Some(&event.result))
    }

    // -----------------------------------------------------------------------
    // Hoppers and droppers.

    /// The block or entity (a vehicle's placeholder key) holding the
    /// container at `key`.
    fn plugin_holder(&self, key: Key) -> ev::InventoryHolder {
        if key.1 == super::minecarts::CART_KEY_Y {
            ev::InventoryHolder::Entity(self.plugin_entity(None, key.2))
        } else {
            ev::InventoryHolder::Block(self.plugin_block([key.0, key.1, key.2]))
        }
    }
    /// A hopper or dropper moves `item` from the container at `source` to
    /// the one at `destination` (`pushed`: the source does it):
    /// `InventoryMoveItemEvent`. False when cancelled.
    pub(super) fn plugin_move_item(
        &mut self,
        source: Key,
        destination: Key,
        item: &ItemStack,
        pushed: bool,
    ) -> bool {
        if !self.plugins_listen::<ev::InventoryMoveItemEvent>() {
            return true;
        }
        let event = ev::InventoryMoveItemEvent {
            source: self.plugin_holder(source),
            destination: self.plugin_holder(destination),
            initiator: self.plugin_holder(if pushed { source } else { destination }),
            item: stack_ref(item),
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// The hopper (or hopper minecart) at `hopper` is about to take in the
    /// dropped item `self.dropped[index]`: `InventoryPickupItemEvent`. False
    /// when cancelled.
    pub(super) fn plugin_hopper_pickup(&mut self, hopper: Key, index: usize) -> bool {
        if !self.plugins_listen::<ev::InventoryPickupItemEvent>() {
            return true;
        }
        let Some(item) = self.dropped.get(index) else {
            return true;
        };
        let (id, stack) = (item.id, stack_ref(&item.stack));
        let event = ev::InventoryPickupItemEvent {
            inventory: self.plugin_holder(hopper),
            item: self.plugin_entity(None, id as i32),
            stack,
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }

    // -----------------------------------------------------------------------
    // A player's menus.

    /// A container click of the player of `conn`, after the click events:
    /// the drag (quick craft) it starts, adds to or ends, and at its end
    /// `InventoryDragEvent`. True when cancelled (the packet is dropped and
    /// the menu sent again).
    pub(super) fn plugin_drag(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        if !self.plugins_listen::<ev::InventoryDragEvent>() {
            return false;
        }
        let drags = &mut self.plugin_inventories.drags;
        if p.input != 5 {
            drags.remove(&conn);
            return false;
        }
        match p.button & 3 {
            0 => {
                drags.insert(conn, ((p.button >> 2) & 3, Vec::new()));
                return false;
            }
            1 => {
                if let Some((_, slots)) = drags.get_mut(&conn) {
                    slots.push(i32::from(p.slot));
                }
                return false;
            }
            2 => {}
            _ => return false,
        }
        let Some((ty, slots)) = drags.remove(&conn) else {
            return false;
        };
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return false;
        };
        if slots.is_empty() {
            return false;
        }
        let (kind, _, size) = net.hub.menu_view(conn);
        let cursor = net.hub.player(conn).and_then(|g| g.carried.clone());
        let event = ev::InventoryDragEvent {
            player,
            view: ev::InventoryView { kind, size },
            slots,
            drag_type: match ty {
                1 => ev::DragType::Single,
                2 => ev::DragType::Clone,
                _ => ev::DragType::Even,
            },
            cursor: cursor.as_ref().map(stack_ref),
            cancelled: false,
        };
        if self.plugin_allows(Some(net), event) {
            return false;
        }
        net.hub.resend_menu(conn, &self.world);
        true
    }
    /// The player of `conn` picked the offer `index` of `merchant`'s trades:
    /// `TradeSelectEvent`. False when cancelled.
    pub(super) fn plugin_trade_select(
        &mut self,
        net: &Network,
        conn: u32,
        merchant: Option<u32>,
        index: i32,
    ) -> bool {
        if !self.plugins_listen::<ev::TradeSelectEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let event = ev::TradeSelectEvent {
            player,
            merchant: merchant.map(|id| self.plugin_entity(Some(net), id as i32)),
            index,
            cancelled: false,
        };
        self.plugin_allows(Some(net), event)
    }
    /// The recipes the player whose turn it is is about to learn:
    /// `PlayerRecipeDiscoverEvent` for each new one, without those a plugin
    /// cancelled.
    pub(super) fn plugin_recipe_discover<'a>(
        &mut self,
        recipes: &'a [String],
    ) -> Cow<'a, [String]> {
        if !self.plugins_listen::<ev::PlayerRecipeDiscoverEvent>() {
            return Cow::Borrowed(recipes);
        }
        let Some(player) = self.plugin_acting() else {
            return Cow::Borrowed(recipes);
        };
        let index = &self.res.stations().recipe_index;
        let new: Vec<String> = recipes
            .iter()
            .filter(|id| !self.level.recipe_book.knows(id) && !index.special(id))
            .cloned()
            .collect();
        let mut kept: Vec<String> = Vec::new();
        for recipe in new {
            let event = ev::PlayerRecipeDiscoverEvent {
                player: player.clone(),
                recipe: recipe.clone(),
                cancelled: false,
            };
            if !self.plugin_allows(None, event) {
                kept.push(recipe);
            }
        }
        if kept.is_empty() {
            return Cow::Borrowed(recipes);
        }
        Cow::Owned(
            recipes
                .iter()
                .filter(|id| !kept.contains(id))
                .cloned()
                .collect(),
        )
    }
    /// The player of `conn` clicked `recipe` in their recipe book:
    /// `PlayerRecipeBookClickEvent`. The recipe to place, or None when
    /// cancelled.
    pub(super) fn plugin_recipe_book_click(
        &mut self,
        net: &Network,
        conn: u32,
        recipe: String,
        make_all: bool,
    ) -> Option<String> {
        if !self.plugins_listen::<ev::PlayerRecipeBookClickEvent>() {
            return Some(recipe);
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return Some(recipe);
        };
        let mut event = ev::PlayerRecipeBookClickEvent {
            player,
            recipe,
            make_all,
            cancelled: false,
        };
        self.plugin_fire(Some(net), &mut event);
        (!event.cancelled).then_some(event.recipe)
    }
    /// The player of `conn` is about to take the book from the lectern at
    /// `key`: `PlayerTakeLecternBookEvent`. False when cancelled.
    pub(super) fn plugin_take_lectern_book(&mut self, net: &Network, conn: u32, key: Key) -> bool {
        if !self.plugins_listen::<ev::PlayerTakeLecternBookEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let book = match self.world.block_entities.get(&key) {
            Some(BlockEntity::Lectern(l)) => l.book.as_ref().map(stack_ref),
            _ => None,
        };
        let event = ev::PlayerTakeLecternBookEvent {
            player,
            lectern: self.plugin_block([key.0, key.1, key.2]),
            book,
            cancelled: false,
        };
        self.plugin_allows(Some(net), event)
    }
}
