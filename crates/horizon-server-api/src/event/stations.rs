//! More of Bukkit's events (group: stations): workstations starting their
//! work, what a broken block gives, dispensers, vaults and spawners, sculk,
//! hoppers, creative slots, enchanting offers, placement checks, structures
//! and maps. Uses the `event!` and `cancellable!` macros of `event.rs`.
use super::{Event, InventoryView};
use crate::server::{Block, EntityRef, ItemStack, PlayerRef, World};

// ---------------------------------------------------------------------------
// Workstations starting their work.

/// `FurnaceStartSmeltEvent`: the furnace, smoker or blast furnace at `block`
/// starts cooking `source` (`recipe`), for `total_cook_time` ticks, which
/// may be changed.
#[derive(Clone, Debug)]
pub struct FurnaceStartSmeltEvent {
    pub block: Block,
    pub source: ItemStack,
    pub recipe: String,
    pub total_cook_time: i32,
}
event!(FurnaceStartSmeltEvent, "FurnaceStartSmeltEvent");

/// `BrewingStartEvent`: the brewing stand at `block` starts a brew with
/// `ingredient`, taking `brewing_time` ticks (400 at vanilla's speed), which
/// may be changed.
#[derive(Clone, Debug)]
pub struct BrewingStartEvent {
    pub block: Block,
    pub ingredient: ItemStack,
    pub brewing_time: i32,
}
event!(BrewingStartEvent, "BrewingStartEvent");

/// `CampfireStartEvent`: `source` was put on the campfire at `block` and
/// cooks for `total_cook_time` ticks (its recipe's), which may be changed.
#[derive(Clone, Debug)]
pub struct CampfireStartEvent {
    pub block: Block,
    pub source: ItemStack,
    pub recipe: String,
    pub total_cook_time: i32,
}
event!(CampfireStartEvent, "CampfireStartEvent");

// ---------------------------------------------------------------------------
// What a broken block gives; digging.

/// `BlockDropItemEvent`: after `BlockBreakEvent`, the items `player`'s break
/// of `block` (`block_state`: the state broken; `block.state` is what is
/// there now) is about to drop. `items` may be changed (removed, edited,
/// added); cancelling drops none.
#[derive(Clone, Debug)]
pub struct BlockDropItemEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub block_state: String,
    pub items: Vec<ItemStack>,
    pub cancelled: bool,
}
cancellable!(BlockDropItemEvent, "BlockDropItemEvent");

/// `BlockExpEvent`: the experience a block a player broke pops (ores,
/// spawners, sculk...: `exp_to_drop`, 0 with silk touch), which may be
/// changed. A furnace's experience is `FurnaceExtractEvent`'s.
#[derive(Clone, Debug)]
pub struct BlockExpEvent {
    pub block: Block,
    pub exp_to_drop: i32,
}
event!(BlockExpEvent, "BlockExpEvent");

/// `BlockDamageAbortEvent`: `player` stopped digging `block` before it broke
/// (`ABORT_DESTROY_BLOCK`), holding `item`.
#[derive(Clone, Debug)]
pub struct BlockDamageAbortEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub item: Option<ItemStack>,
}
event!(BlockDamageAbortEvent, "BlockDamageAbortEvent");

// ---------------------------------------------------------------------------
// Dispensers, vaults and spawners.

/// `BlockDispenseArmorEvent`: the dispenser at `block` is about to put
/// `item` on `target` (a player, an armor stand or a mob). Cancelling puts
/// nothing on: the dispenser drops the item instead, as with no one there.
#[derive(Clone, Debug)]
pub struct BlockDispenseArmorEvent {
    pub block: Block,
    pub item: ItemStack,
    pub target: EntityRef,
    pub cancelled: bool,
}
cancellable!(BlockDispenseArmorEvent, "BlockDispenseArmorEvent");

/// `BlockDispenseLootEvent`: a key `player` put in the vault at `block`
/// unlocked `dispensed_loot`, which it is about to eject one by one. The
/// list may be changed; cancelling (or emptying it) keeps the vault locked
/// and the key in the player's hand.
#[derive(Clone, Debug)]
pub struct BlockDispenseLootEvent {
    pub player: Option<PlayerRef>,
    pub block: Block,
    pub dispensed_loot: Vec<ItemStack>,
    pub cancelled: bool,
}
cancellable!(BlockDispenseLootEvent, "BlockDispenseLootEvent");

/// `VaultDisplayItemEvent`: the active vault at `block` cycled the item it
/// shows to `display_item` (from its loot table). Changing it shows that
/// instead (`None` shows nothing); cancelling keeps the one shown before.
#[derive(Clone, Debug)]
pub struct VaultDisplayItemEvent {
    pub block: Block,
    pub display_item: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(VaultDisplayItemEvent, "VaultDisplayItemEvent");

/// `SpawnerSpawnEvent`: the monster spawner at `spawner` spawned `entity`
/// (before `CreatureSpawnEvent` with reason `Spawner`). Cancelling takes the
/// mob back out (the spawner's delay is reset all the same).
#[derive(Clone, Debug)]
pub struct SpawnerSpawnEvent {
    pub entity: EntityRef,
    pub spawner: Block,
    pub cancelled: bool,
}
cancellable!(SpawnerSpawnEvent, "SpawnerSpawnEvent");

/// `TrialSpawnerSpawnEvent`: the trial spawner at `spawner` spawned `entity`
/// for its wave. Cancelling takes the mob back out (it still counts as one
/// of the wave's spawns).
#[derive(Clone, Debug)]
pub struct TrialSpawnerSpawnEvent {
    pub entity: EntityRef,
    pub spawner: Block,
    pub cancelled: bool,
}
cancellable!(TrialSpawnerSpawnEvent, "TrialSpawnerSpawnEvent");

// ---------------------------------------------------------------------------
// Sculk.

/// `SculkBloomEvent`: the sculk catalyst at `block` took a dying mob's
/// experience and is about to spread `charge` of it as sculk. Changing
/// `charge` spreads that much; cancelling spreads none (the catalyst still
/// blooms).
#[derive(Clone, Debug)]
pub struct SculkBloomEvent {
    pub block: Block,
    pub charge: i32,
    pub cancelled: bool,
}
cancellable!(SculkBloomEvent, "SculkBloomEvent");

/// `BlockReceiveGameEvent`: the sculk sensor, calibrated sensor or shrieker
/// at `block` is about to take the vibration of `event`
/// (`minecraft:step`...) caused by `entity`. It starts cancelled when the
/// block can't take it now (vanilla's answer); a plugin may cancel it, or
/// let a refused one through (the vibration must still reach it unblocked).
#[derive(Clone, Debug)]
pub struct BlockReceiveGameEvent {
    pub event: String,
    pub block: Block,
    pub entity: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(BlockReceiveGameEvent, "BlockReceiveGameEvent");

// ---------------------------------------------------------------------------
// Hoppers, creative slots, enchanting tables, placement.

/// `HopperInventorySearchEvent.ContainerType`: the hopper looks for where
/// to push (`Destination`) or what to pull from (`Source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HopperContainerType {
    Source,
    Destination,
}

/// `HopperInventorySearchEvent`: the hopper (or hopper minecart) at `block`
/// found the block container `inventory` at `search_block`. Setting
/// `inventory` to `None` makes it see no container there (it then pulls
/// item entities, or pushes nowhere); another container can't be put in.
#[derive(Clone, Debug)]
pub struct HopperInventorySearchEvent {
    pub block: Block,
    pub search_block: Block,
    pub container_type: HopperContainerType,
    pub inventory: Option<InventoryView>,
}
event!(HopperInventorySearchEvent, "HopperInventorySearchEvent");

/// `InventoryCreativeEvent`: a creative player set `slot` of their own
/// inventory (the inventory menu's numbering, -999 for an item thrown out of
/// the screen) to `item` (`setCursor`). Changing `item` puts that there;
/// cancelling leaves the slot (or drops nothing) and resends the inventory.
#[derive(Clone, Debug)]
pub struct InventoryCreativeEvent {
    pub player: PlayerRef,
    pub slot: i32,
    pub item: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(InventoryCreativeEvent, "InventoryCreativeEvent");

/// One offer of an enchanting table (`EnchantmentOffer`): the shown clue
/// (`enchantment`, `level`, None when hidden) and the levels it costs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnchantmentOffer {
    pub enchantment: Option<String>,
    pub level: u32,
    pub cost: i32,
}

/// `PrepareItemEnchantEvent`: the enchanting table at `block` worked out
/// `player`'s three `offers` for `item` with `enchantment_bonus` bookshelves.
/// The offers may be changed (a cost picks what the button enchants with, as
/// in vanilla; a clue only shows); cancelling shows none.
#[derive(Clone, Debug)]
pub struct PrepareItemEnchantEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub item: ItemStack,
    pub enchantment_bonus: i32,
    pub offers: Vec<Option<EnchantmentOffer>>,
    pub cancelled: bool,
}
cancellable!(PrepareItemEnchantEvent, "PrepareItemEnchantEvent");

/// `BlockCanBuildEvent`: `player` is about to place `block_data` (the state
/// worked out) at `block` from `item`, and vanilla allows it (`buildable`).
/// Setting `buildable` to false refuses the placement. A placement vanilla
/// refuses isn't asked about, so it can't be forced through.
#[derive(Clone, Debug)]
pub struct BlockCanBuildEvent {
    pub player: Option<PlayerRef>,
    pub block: Block,
    pub block_data: String,
    pub item: Option<ItemStack>,
    pub buildable: bool,
}
event!(BlockCanBuildEvent, "BlockCanBuildEvent");

// ---------------------------------------------------------------------------
// Structures and maps.

/// A structure's bounding box (`BoundingBox`), block coordinates inclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BoundingBox {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

/// `AsyncStructureGenerateEvent` (cause `GENERATION`): world generation
/// started `structure` (`minecraft:village_plains`...) in the new chunk
/// `chunk_x`, `chunk_z`. Generation runs on worker threads, so this is told
/// on the server's thread once the chunk arrives: it only informs.
#[derive(Clone, Debug)]
pub struct AsyncStructureGenerateEvent {
    pub world: World,
    pub structure: String,
    pub bounding_box: BoundingBox,
    pub chunk_x: i32,
    pub chunk_z: i32,
}
event!(AsyncStructureGenerateEvent, "AsyncStructureGenerateEvent");

/// `AsyncStructureSpawnEvent`: world generation placed the pieces of
/// `structure` (started in chunk `start_x`, `start_z`) that reach into the
/// new chunk `chunk_x`, `chunk_z`; `bounding_box` is that chunk's column.
/// Told on the server's thread once the chunk arrives (the blocks are placed
/// by then), so it only informs.
#[derive(Clone, Debug)]
pub struct AsyncStructureSpawnEvent {
    pub world: World,
    pub structure: String,
    pub start_x: i32,
    pub start_z: i32,
    pub bounding_box: BoundingBox,
    pub chunk_x: i32,
    pub chunk_z: i32,
}
event!(AsyncStructureSpawnEvent, "AsyncStructureSpawnEvent");

/// `MapInitializeEvent`: a new map got the id `map_id` (an empty map used, a
/// map locked at a cartography table, an explorer map). Its view may be
/// changed: the centre, the scale (0..=4), tracking, unlimited tracking and
/// locked.
#[derive(Clone, Debug)]
pub struct MapInitializeEvent {
    pub map_id: i32,
    pub world: World,
    pub center_x: i32,
    pub center_z: i32,
    pub scale: u8,
    pub tracking_position: bool,
    pub unlimited_tracking: bool,
    pub locked: bool,
}
event!(MapInitializeEvent, "MapInitializeEvent");
