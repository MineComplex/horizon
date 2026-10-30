//! Where the server fires the plugins' stations events (docs/plugins.md):
//! furnaces, brewing stands and campfires starting their work, what a
//! player's break drops, digging stopped, dispensers putting armour on,
//! vaults and spawners, sculk, hoppers looking for containers, creative
//! slots, enchanting offers, placements, generated structures and new maps.
//! Each one returns the vanilla answer at once when no plugin listens.
use super::plugin_events::stack_ref;
use super::{Dim, server_net::Network};
use crate::hc::{containers::BlockEntity, items::ItemStack, mobs::Who};
use horizon_server_api::{self as api, Location, event as ev};
use horizon_world::{level::PlaceContext, nbt::Tag, registry::BlockStateId};
use std::collections::{HashMap, HashSet};

type Pos = (i32, i32, i32);

/// What these events keep between ticks.
#[derive(Default)]
pub(super) struct State {
    /// The spawner whose planned mobs are being added (`add_planned`), and
    /// whether it is a trial spawner.
    pub(super) spawning: Option<(Pos, bool)>,
    /// The chunks of each level the structure events last saw, with the
    /// chunk map's revision then.
    chunks: HashMap<Dim, (u64, HashSet<(i32, i32)>)>,
}

/// The game's stacks for the ones a plugin handed back: an unchanged one
/// keeps its components (the first of `old` that matches).
fn game_stacks(old: &[ItemStack], new: &[api::ItemStack]) -> Vec<ItemStack> {
    let mut left: Vec<Option<&ItemStack>> = old.iter().map(Some).collect();
    new.iter()
        .filter(|s| s.count > 0)
        .map(|s| {
            let same = left
                .iter_mut()
                .find(|o| o.is_some_and(|o| stack_ref(o) == *s))
                .and_then(Option::take);
            same.cloned()
                .unwrap_or_else(|| ItemStack::new(&s.item, s.count))
        })
        .collect()
}

fn one_of(stack: &ItemStack) -> api::ItemStack {
    api::ItemStack::new(&stack.item, 1)
}

/// The box of a saved structure start (`BB`).
fn start_box(tag: Option<Tag>) -> ev::BoundingBox {
    match tag.as_ref().and_then(|t| t.get("BB")) {
        Some(Tag::IntArray(v)) if v.len() == 6 => ev::BoundingBox {
            min: [v[0], v[1], v[2]],
            max: [v[3], v[4], v[5]],
        },
        _ => ev::BoundingBox::default(),
    }
}

impl crate::game::Server<'_> {
    fn station_block(&self, pos: Pos) -> api::Block {
        self.plugin_block([pos.0, pos.1, pos.2])
    }
    /// The player whose turn it is, as an entity.
    fn station_acting_entity(&self) -> Option<api::EntityRef> {
        let player = self.plugin_acting()?;
        let id = self
            .net_acting
            .and_then(|conn| self.network.as_ref()?.hub.player(conn))
            .map(|g| g.entity_id)
            .or_else(|| {
                self.plugin_seen_ids
                    .iter()
                    .find(|(_, uuid)| *uuid == player.uuid)
                    .map(|(id, _)| *id)
            })
            .unwrap_or(-1);
        Some(api::EntityRef::of_player(id, player))
    }
    fn station_who(&self, who: Option<Who>) -> Option<api::EntityRef> {
        match who? {
            Who::Mob(id) => Some(self.plugin_entity(None, id as i32)),
            Who::Player => self.station_acting_entity(),
        }
    }

    // -----------------------------------------------------------------------
    // Workstations starting their work.

    /// Before the furnace at `pos` ticks: whether it cooks nothing yet, for
    /// `FurnaceStartSmeltEvent`.
    pub(super) fn plugin_furnace_idle(&self, pos: Pos) -> bool {
        self.plugins_listen::<ev::FurnaceStartSmeltEvent>()
            && matches!(self.world.block_entities.get(&pos),
                Some(BlockEntity::Furnace(f)) if f.cooking_timer == 0)
    }
    /// After the tick of a furnace that was `idle`: `FurnaceStartSmeltEvent`
    /// when it started cooking, with the cook time a plugin leaves.
    pub(super) fn plugin_furnace_start(&mut self, pos: Pos, idle: bool) {
        if !idle {
            return;
        }
        let Some(BlockEntity::Furnace(f)) = self.world.block_entities.get(&pos) else {
            return;
        };
        let Some(source) = f.items[0].as_ref().filter(|_| f.cooking_timer > 0) else {
            return;
        };
        let recipe = self
            .res
            .recipes()
            .cook(f.kind.recipe_type(), source)
            .map(|r| format!("minecraft:{}", r.id))
            .unwrap_or_default();
        let mut event = ev::FurnaceStartSmeltEvent {
            block: self.station_block(pos),
            source: one_of(source),
            recipe,
            total_cook_time: f.cooking_total,
        };
        self.plugin_fire(None, &mut event);
        if let Some(BlockEntity::Furnace(f)) = self.world.block_entities.get_mut(&pos) {
            f.cooking_total = event.total_cook_time.max(1);
        }
    }
    /// Before the brewing stand at `pos` ticks: whether it brews nothing yet,
    /// for `BrewingStartEvent`.
    pub(super) fn plugin_brewing_idle(&self, pos: Pos) -> bool {
        self.plugins_listen::<ev::BrewingStartEvent>()
            && matches!(self.world.block_entities.get(&pos),
                Some(BlockEntity::Brewing(b)) if b.brew_time == 0)
    }
    /// After the tick of a brewing stand that was `idle`: `BrewingStartEvent`
    /// when a brew started, with the time a plugin leaves.
    pub(super) fn plugin_brewing_start(&mut self, pos: Pos, idle: bool) {
        if !idle {
            return;
        }
        let Some(BlockEntity::Brewing(b)) = self.world.block_entities.get(&pos) else {
            return;
        };
        let Some(ingredient) = b.items[3].as_ref().filter(|_| b.brew_time > 0) else {
            return;
        };
        let mut event = ev::BrewingStartEvent {
            block: self.station_block(pos),
            ingredient: one_of(ingredient),
            brewing_time: b.brew_time,
        };
        self.plugin_fire(None, &mut event);
        if let Some(BlockEntity::Brewing(b)) = self.world.block_entities.get_mut(&pos) {
            b.brew_time = event.brewing_time.max(1);
            b.total_brew_time = b.brew_time;
        }
    }
    /// `held` is about to go on the campfire at `pos` for `time` ticks:
    /// `CampfireStartEvent` (when it has room). The cook time to use.
    pub(super) fn plugin_campfire_start(
        &mut self,
        pos: [i32; 3],
        held: &ItemStack,
        time: i32,
    ) -> i32 {
        if !self.plugins_listen::<ev::CampfireStartEvent>() {
            return time;
        }
        let room = matches!(self.world.block_entities.get(&(pos[0], pos[1], pos[2])),
            Some(BlockEntity::Campfire(c)) if c.items.iter().any(Option::is_none));
        if !room {
            return time;
        }
        let recipe = self
            .res
            .recipes()
            .cook("minecraft:campfire_cooking", held)
            .map(|r| format!("minecraft:{}", r.id))
            .unwrap_or_default();
        let mut event = ev::CampfireStartEvent {
            block: self.plugin_block(pos),
            source: one_of(held),
            recipe,
            total_cook_time: time,
        };
        self.plugin_fire(None, &mut event);
        event.total_cook_time.max(1)
    }

    // -----------------------------------------------------------------------
    // What a broken block gives; digging.

    /// The player whose turn it is broke `broken` at `pos`, which is about to
    /// drop `drops`: `BlockDropItemEvent`. What to drop.
    pub(super) fn plugin_block_drops(
        &mut self,
        pos: [i32; 3],
        broken: BlockStateId,
        drops: Vec<ItemStack>,
    ) -> Vec<ItemStack> {
        if !self.plugins_listen::<ev::BlockDropItemEvent>()
            || !self.world_data.rules.bool("block_drops")
        {
            return drops;
        }
        let drops: Vec<ItemStack> = drops
            .into_iter()
            .filter(|d| &*d.item != "minecraft:air")
            .collect();
        let Some(player) = self.plugin_acting().filter(|_| !drops.is_empty()) else {
            return drops;
        };
        let items: Vec<api::ItemStack> = drops.iter().map(stack_ref).collect();
        let mut event = ev::BlockDropItemEvent {
            player,
            block: self.plugin_block(pos),
            block_state: self.state_string(broken),
            items: items.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            return Vec::new();
        }
        if event.items == items {
            return drops;
        }
        game_stacks(&drops, &event.items)
    }
    /// The block a player broke at `pos` is about to pop `xp` experience:
    /// `BlockExpEvent`. How much.
    pub(super) fn plugin_block_exp(&mut self, pos: [i32; 3], xp: u32) -> u32 {
        if !self.plugins_listen::<ev::BlockExpEvent>() {
            return xp;
        }
        let mut event = ev::BlockExpEvent {
            block: self.plugin_block(pos),
            exp_to_drop: xp as i32,
        };
        self.plugin_fire(None, &mut event);
        event.exp_to_drop.max(0) as u32
    }
    /// The player of `conn` stopped digging `pos` (`ABORT_DESTROY_BLOCK`)
    /// before it broke: `BlockDamageAbortEvent`.
    pub(super) fn plugin_dig_abort(&mut self, net: &Network, conn: u32, pos: [i32; 3]) {
        if !self.plugins_listen::<ev::BlockDamageAbortEvent>() {
            return;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return;
        };
        let item = net
            .hub
            .player(conn)
            .and_then(|g| g.inventory.get(g.selected).cloned().flatten());
        let mut event = ev::BlockDamageAbortEvent {
            block: self.plugin_block_in(&player.location.world, pos),
            player,
            item: item.as_ref().map(stack_ref),
        };
        self.plugin_fire(Some(net), &mut event);
    }

    // -----------------------------------------------------------------------
    // Dispensers, vaults and spawners.

    /// The dispenser at `pos` is about to put `item` on its target (`None`:
    /// the player whose turn it is; else the mob or, with `stand`, the armour
    /// stand of that id): `BlockDispenseArmorEvent`. False when cancelled.
    pub(super) fn plugin_dispense_armor(
        &mut self,
        pos: Pos,
        item: &ItemStack,
        target: Option<(u32, bool)>,
    ) -> bool {
        if !self.plugins_listen::<ev::BlockDispenseArmorEvent>() {
            return true;
        }
        let target = match target {
            None => self.station_acting_entity(),
            Some((id, true)) => {
                self.armor_stands
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| api::EntityRef {
                        id: id as i32,
                        kind: "minecraft:armor_stand".into(),
                        location: Location {
                            world: self.dim.id().to_owned(),
                            x: s.position.x,
                            y: s.position.y,
                            z: s.position.z,
                            yaw: 0.0,
                            pitch: 0.0,
                        },
                        player: None,
                    })
            }
            Some((id, false)) => Some(self.plugin_entity(None, id as i32)),
        };
        let Some(target) = target else {
            return true;
        };
        let event = ev::BlockDispenseArmorEvent {
            block: self.station_block(pos),
            item: stack_ref(item),
            target,
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// A key unlocked `loot` from the vault at `key`: `BlockDispenseLootEvent`.
    /// The loot to eject (none when cancelled: the vault stays locked).
    pub(super) fn plugin_vault_loot(&mut self, key: Pos, loot: Vec<ItemStack>) -> Vec<ItemStack> {
        if !self.plugins_listen::<ev::BlockDispenseLootEvent>() || loot.is_empty() {
            return loot;
        }
        let items: Vec<api::ItemStack> = loot.iter().map(stack_ref).collect();
        let mut event = ev::BlockDispenseLootEvent {
            player: self.plugin_acting(),
            block: self.station_block(key),
            dispensed_loot: items.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            self.plugin_resync_acting();
            return Vec::new();
        }
        if event.dispensed_loot == items {
            return loot;
        }
        game_stacks(&loot, &event.dispensed_loot)
    }
    /// Before the vault at `key` ticks: the item it shows, for
    /// `VaultDisplayItemEvent`.
    pub(super) fn plugin_vault_shown(&self, key: Pos) -> Option<Option<ItemStack>> {
        if !self.plugins_listen::<ev::VaultDisplayItemEvent>() {
            return None;
        }
        match self.world.block_entities.get(&key) {
            Some(BlockEntity::Vault(v)) => Some(v.display_item.clone()),
            _ => None,
        }
    }
    /// After the tick of the vault at `key` that showed `before`: an active
    /// vault that cycled to another item fires `VaultDisplayItemEvent`.
    pub(super) fn plugin_vault_display(&mut self, key: Pos, before: Option<Option<ItemStack>>) {
        let Some(before) = before else {
            return;
        };
        let state = self.world.block(key.0, key.1, key.2);
        let active = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("vault_state"))
            .is_some_and(|v| v == "active");
        let Some(BlockEntity::Vault(v)) = self.world.block_entities.get(&key) else {
            return;
        };
        let Some(shown) = v
            .display_item
            .clone()
            .filter(|s| active && before.as_ref() != Some(s))
        else {
            return;
        };
        let mut event = ev::VaultDisplayItemEvent {
            block: self.station_block(key),
            display_item: Some(stack_ref(&shown)),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        let new = if event.cancelled {
            before
        } else if event.display_item == Some(stack_ref(&shown)) {
            return;
        } else {
            event
                .display_item
                .filter(|s| s.count > 0)
                .map(|s| ItemStack::new(&s.item, s.count))
        };
        if let Some(BlockEntity::Vault(v)) = self.world.block_entities.get_mut(&key) {
            v.display_item = new;
        }
        self.mark_block_entity_updated(key);
    }
    fn spawner_event(&mut self, entity: api::EntityRef) -> bool {
        let Some((pos, trial)) = self.plugin_stations.spawning else {
            return true;
        };
        let spawner = self.station_block(pos);
        if trial {
            if !self.plugins_listen::<ev::TrialSpawnerSpawnEvent>() {
                return true;
            }
            let event = ev::TrialSpawnerSpawnEvent {
                entity,
                spawner,
                cancelled: false,
            };
            self.plugin_allows(None, event)
        } else {
            if !self.plugins_listen::<ev::SpawnerSpawnEvent>() {
                return true;
            }
            let event = ev::SpawnerSpawnEvent {
                entity,
                spawner,
                cancelled: false,
            };
            self.plugin_allows(None, event)
        }
    }
    fn spawner_listened(&self) -> bool {
        match self.plugin_stations.spawning {
            Some((_, true)) => self.plugins_listen::<ev::TrialSpawnerSpawnEvent>(),
            Some((_, false)) => self.plugins_listen::<ev::SpawnerSpawnEvent>(),
            None => false,
        }
    }
    /// A spawner's fresh `mob` about to be added: `SpawnerSpawnEvent` or
    /// `TrialSpawnerSpawnEvent`. False when cancelled.
    pub(super) fn plugin_spawner_spawn(&mut self, mob: &crate::hc::mobs::Mob) -> bool {
        if !self.spawner_listened() {
            return true;
        }
        let entity = api::EntityRef {
            id: mob.id as i32,
            kind: crate::hc::command::args::normalize(&mob.kind.id),
            location: Location {
                world: self.dim.id().to_owned(),
                x: mob.position.x,
                y: mob.position.y,
                z: mob.position.z,
                yaw: mob.yaw,
                pitch: 0.0,
            },
            player: None,
        };
        self.spawner_event(entity)
    }
    /// A spawner's mob loaded from its spawn data as `id`: the same events;
    /// a cancelled one is taken out again. Whether it stays.
    pub(super) fn plugin_spawner_spawned(&mut self, id: u32) -> bool {
        if !self.spawner_listened() {
            return true;
        }
        let entity = self.plugin_entity(None, id as i32);
        if self.spawner_event(entity) {
            return true;
        }
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            m.removed = true;
        }
        false
    }

    // -----------------------------------------------------------------------
    // Sculk.

    /// The catalyst at `key` is about to spread `charge`: `SculkBloomEvent`.
    /// The charge to spread, None when cancelled.
    pub(super) fn plugin_sculk_bloom(&mut self, key: Pos, charge: i32) -> Option<i32> {
        if !self.plugins_listen::<ev::SculkBloomEvent>() {
            return Some(charge);
        }
        let mut event = ev::SculkBloomEvent {
            block: self.station_block(key),
            charge,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        (!event.cancelled && event.charge > 0).then_some(event.charge)
    }
    /// The sculk block at `key` hears `event` from `source`, and `can`
    /// (vanilla) says whether it takes it: `BlockReceiveGameEvent`, which
    /// starts cancelled when it can't. Whether it takes it.
    pub(super) fn plugin_receive_game_event(
        &mut self,
        key: Pos,
        event: &str,
        source: Option<Who>,
        can: bool,
    ) -> bool {
        if !self.plugins_listen::<ev::BlockReceiveGameEvent>() {
            return can;
        }
        let e = ev::BlockReceiveGameEvent {
            event: event.to_owned(),
            block: self.station_block(key),
            entity: self.station_who(source),
            cancelled: !can,
        };
        self.plugin_allows(None, e)
    }

    // -----------------------------------------------------------------------
    // Hoppers, creative slots, enchanting tables, placement.

    /// The hopper at `hopper` found a block container of `size` slots at
    /// `search` (its `destination`, or its source): `HopperInventorySearchEvent`.
    /// False when a plugin hid it.
    pub(super) fn plugin_hopper_search(
        &mut self,
        hopper: Pos,
        search: Pos,
        size: usize,
        destination: bool,
    ) -> bool {
        if !self.plugins_listen::<ev::HopperInventorySearchEvent>() {
            return true;
        }
        let search_block = self.station_block(search);
        let mut event = ev::HopperInventorySearchEvent {
            block: self.station_block(hopper),
            inventory: Some(ev::InventoryView {
                kind: search_block.kind().to_owned(),
                size: size as u32,
            }),
            search_block,
            container_type: if destination {
                ev::HopperContainerType::Destination
            } else {
                ev::HopperContainerType::Source
            },
        };
        self.plugin_fire(None, &mut event);
        event.inventory.is_some()
    }
    /// The creative player of `conn` sets `slot` to `item`:
    /// `InventoryCreativeEvent`. The item to set, None when cancelled (their
    /// inventory is sent again).
    pub(super) fn plugin_creative_slot(
        &mut self,
        net: &Network,
        conn: u32,
        slot: i32,
        item: Option<ItemStack>,
    ) -> Option<Option<ItemStack>> {
        if !self.plugins_listen::<ev::InventoryCreativeEvent>() {
            return Some(item);
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return Some(item);
        };
        let shown = item.as_ref().map(stack_ref);
        let mut event = ev::InventoryCreativeEvent {
            player,
            slot,
            item: shown.clone(),
            cancelled: false,
        };
        self.plugin_fire(Some(net), &mut event);
        if event.cancelled {
            self.plugin_resync.push(conn);
            return None;
        }
        if event.item == shown {
            return Some(item);
        }
        self.plugin_resync.push(conn);
        let old: Vec<ItemStack> = item.into_iter().collect();
        let new: Vec<api::ItemStack> = event.item.into_iter().collect();
        Some(game_stacks(&old, &new).pop())
    }
    /// The enchanting table at `pos` offers `offers` to the player of `conn`
    /// for `item`, with `bookcases` shelves: `PrepareItemEnchantEvent`. The
    /// offers to show (none when cancelled).
    pub(super) fn plugin_prepare_enchant(
        &mut self,
        net: &Network,
        conn: u32,
        pos: [i32; 3],
        item: Option<&ItemStack>,
        bookcases: i32,
        offers: crate::hc::stations::EnchantOffers,
    ) -> crate::hc::stations::EnchantOffers {
        if !self.plugins_listen::<ev::PrepareItemEnchantEvent>()
            || offers.costs.iter().all(|c| *c <= 0)
        {
            return offers;
        }
        let (Some(item), Some(player)) = (item, Self::plugin_player_of(net, conn)) else {
            return offers;
        };
        let shown: Vec<Option<ev::EnchantmentOffer>> = (0..3)
            .map(|i| {
                (offers.costs[i] > 0).then(|| ev::EnchantmentOffer {
                    enchantment: offers.clues[i].as_ref().map(|(e, _)| e.to_string()),
                    level: offers.clues[i].as_ref().map_or(0, |(_, l)| *l),
                    cost: offers.costs[i],
                })
            })
            .collect();
        let mut event = ev::PrepareItemEnchantEvent {
            player,
            block: self.plugin_block_in(self.dim.id(), pos),
            item: stack_ref(item),
            enchantment_bonus: bookcases,
            offers: shown.clone(),
            cancelled: false,
        };
        self.plugin_fire(Some(net), &mut event);
        if event.cancelled {
            return Default::default();
        }
        if event.offers == shown {
            return offers;
        }
        let mut out = crate::hc::stations::EnchantOffers::default();
        for (i, offer) in event.offers.into_iter().take(3).enumerate() {
            let Some(offer) = offer.filter(|o| o.cost > 0) else {
                continue;
            };
            out.costs[i] = offer.cost;
            out.clues[i] = offer
                .enchantment
                .map(|e| (std::sync::Arc::from(e.as_str()), offer.level));
        }
        out
    }
    /// The player whose turn it is is about to place `item` as `ctx` says:
    /// `BlockCanBuildEvent` for the block and state vanilla would place (a
    /// placement vanilla refuses isn't asked about). False when a plugin
    /// refused it.
    pub(super) fn plugin_can_build(&mut self, ctx: &PlaceContext, item: &ItemStack) -> bool {
        if !self.plugins_listen::<ev::BlockCanBuildEvent>() {
            return true;
        }
        let Some((at, state)) =
            self.with_level(|b, level, ticks| b.placement_state(level, ticks, ctx))
        else {
            return true;
        };
        let pos = [at.x, at.y, at.z];
        let mut event = ev::BlockCanBuildEvent {
            player: self.plugin_acting(),
            block: self.plugin_block(pos),
            block_data: self.state_string(state),
            item: Some(one_of(item)),
            buildable: true,
        };
        self.plugin_fire(None, &mut event);
        if !event.buildable {
            // The client placed it already.
            self.plugin_resend_acting(pos);
            self.plugin_resync_acting();
        }
        event.buildable
    }

    // -----------------------------------------------------------------------
    // Structures and maps.

    /// After the level's chunks streamed: for each new chunk that came,
    /// `AsyncStructureGenerateEvent` for the structures it starts and
    /// `AsyncStructureSpawnEvent` for those placed into it.
    pub(super) fn plugin_watch_structures(&mut self) {
        let listens = self.plugins_listen::<ev::AsyncStructureGenerateEvent>()
            || self.plugins_listen::<ev::AsyncStructureSpawnEvent>();
        let dim = self.dim;
        if !listens {
            self.plugin_stations.chunks.remove(&dim);
            return;
        }
        let revision = self.world.chunks.membership_revision();
        if self
            .plugin_stations
            .chunks
            .get(&dim)
            .is_some_and(|(r, _)| *r == revision)
        {
            return;
        }
        let now: HashSet<(i32, i32)> = self.world.chunks.keys().copied().collect();
        let before = self
            .plugin_stations
            .chunks
            .insert(dim, (revision, now.clone()))
            .map(|(_, set)| set)
            .unwrap_or_default();
        let mut came: Vec<(i32, i32)> = now.difference(&before).copied().collect();
        came.sort_unstable();
        let world = dim.id().to_owned();
        let mut events: Vec<(
            Option<ev::AsyncStructureGenerateEvent>,
            Vec<ev::AsyncStructureSpawnEvent>,
        )> = Vec::new();
        for (x, z) in came {
            let Some(chunk) = self.world.chunks.get(&(x, z)).filter(|c| c.fresh) else {
                continue;
            };
            for start in &chunk.structure_starts {
                let e = ev::AsyncStructureGenerateEvent {
                    world: world.clone(),
                    structure: start.name().to_owned(),
                    bounding_box: start_box(start.save()),
                    chunk_x: x,
                    chunk_z: z,
                };
                events.push((Some(e), Vec::new()));
            }
            let column = ev::BoundingBox {
                min: [x * 16, crate::hc::chunk::MIN_Y, z * 16],
                max: [
                    x * 16 + 15,
                    crate::hc::chunk::MIN_Y + crate::hc::chunk::HEIGHT - 1,
                    z * 16 + 15,
                ],
            };
            let spawns = chunk
                .structure_refs
                .iter()
                .map(|(id, (sx, sz))| ev::AsyncStructureSpawnEvent {
                    world: world.clone(),
                    structure: id.to_string(),
                    start_x: *sx,
                    start_z: *sz,
                    bounding_box: column,
                    chunk_x: x,
                    chunk_z: z,
                })
                .collect();
            events.push((None, spawns));
        }
        for (generate, spawns) in events {
            if let Some(mut e) = generate
                && self.plugins_listen::<ev::AsyncStructureGenerateEvent>()
            {
                self.plugin_fire(None, &mut e);
            }
            if self.plugins_listen::<ev::AsyncStructureSpawnEvent>() {
                for mut e in spawns {
                    self.plugin_fire(None, &mut e);
                }
            }
        }
    }
    /// The new map `id` was made: `MapInitializeEvent`, its view as a plugin
    /// leaves it.
    pub(super) fn plugin_map_init(&mut self, id: i32) {
        if !self.plugins_listen::<ev::MapInitializeEvent>() {
            return;
        }
        let Some(m) = self.maps.data.get(&id) else {
            return;
        };
        let mut event = ev::MapInitializeEvent {
            map_id: id,
            world: m.dimension.clone(),
            center_x: m.center[0],
            center_z: m.center[1],
            scale: m.scale,
            tracking_position: m.tracking,
            unlimited_tracking: m.unlimited,
            locked: m.locked,
        };
        self.plugin_fire(None, &mut event);
        if let Some(m) = self.maps.data.get_mut(&id) {
            m.center = [event.center_x, event.center_z];
            m.scale = event.scale.min(4);
            m.tracking = event.tracking_position;
            m.unlimited = event.unlimited_tracking;
            m.locked = event.locked;
        }
    }
}
