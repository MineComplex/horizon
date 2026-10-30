//! More of Bukkit's events (group: inventories). Uses the `event!` and
//! `cancellable!` macros of `event.rs`.
#[allow(unused_imports)]
use super::{BlockFace, Event, Hand, InventoryView};
#[allow(unused_imports)]
use crate::server::{Block, BlockPos, EntityRef, ItemStack, Location, PlayerRef, World};

// ---------------------------------------------------------------------------
// Results a menu prepares (org.bukkit.event.inventory.Prepare*)

/// `PrepareItemCraftEvent`: the crafting grid of a player's inventory
/// (`view.kind` `minecraft:inventory`) or crafting table (`minecraft:crafting`)
/// changed and its result was worked out. `matrix` is the grid, row by row.
/// Changing `result` (None: nothing) shows and gives that instead, until the
/// grid changes again.
#[derive(Clone, Debug)]
pub struct PrepareItemCraftEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub matrix: Vec<Option<ItemStack>>,
    pub result: Option<ItemStack>,
}
event!(PrepareItemCraftEvent, "PrepareItemCraftEvent");

/// `PrepareAnvilEvent`: an anvil's inputs or name changed and its result was
/// worked out. `result` (None: nothing) and `repair_cost` (in levels) may be
/// changed.
#[derive(Clone, Debug)]
pub struct PrepareAnvilEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    /// The item to repair or rename, and the one combined with it.
    pub inputs: Vec<Option<ItemStack>>,
    /// The rename box's text.
    pub rename_text: Option<String>,
    pub result: Option<ItemStack>,
    pub repair_cost: i32,
}
event!(PrepareAnvilEvent, "PrepareAnvilEvent");

/// `PrepareSmithingEvent`: a smithing table's inputs (template, base,
/// addition) changed and its result was worked out. `result` may be changed.
#[derive(Clone, Debug)]
pub struct PrepareSmithingEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub inputs: Vec<Option<ItemStack>>,
    pub result: Option<ItemStack>,
}
event!(PrepareSmithingEvent, "PrepareSmithingEvent");

/// `PrepareGrindstoneEvent`: a grindstone's two inputs changed and its result
/// was worked out. `result` may be changed.
#[derive(Clone, Debug)]
pub struct PrepareGrindstoneEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub inputs: Vec<Option<ItemStack>>,
    pub result: Option<ItemStack>,
}
event!(PrepareGrindstoneEvent, "PrepareGrindstoneEvent");

/// `PrepareInventoryResultEvent` (Paper's `PrepareResultEvent`): the result of
/// a loom, a stonecutter or a cartography table, worked out after its inputs
/// changed (the anvil, the smithing table and the grindstone have their own
/// events). `result` may be changed, except at the cartography table, which
/// works its result out again.
#[derive(Clone, Debug)]
pub struct PrepareInventoryResultEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub inputs: Vec<Option<ItemStack>>,
    pub result: Option<ItemStack>,
}
event!(PrepareInventoryResultEvent, "PrepareInventoryResultEvent");

/// `SmithItemEvent`: a player takes `result` out of a smithing table.
/// Cancelling leaves it there.
#[derive(Clone, Debug)]
pub struct SmithItemEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub result: ItemStack,
    pub cancelled: bool,
}
cancellable!(SmithItemEvent, "SmithItemEvent");

// ---------------------------------------------------------------------------
// Furnaces, brewing stands, crafters

/// `FurnaceSmeltEvent`: a furnace, smoker or blast furnace finished cooking
/// one `source` into `result`. Changing `result` puts that in the output
/// instead (nothing is made when the output holds another item); cancelling
/// keeps the input and the output as they were.
#[derive(Clone, Debug)]
pub struct FurnaceSmeltEvent {
    pub block: Block,
    pub source: ItemStack,
    pub result: ItemStack,
    pub cancelled: bool,
}
cancellable!(FurnaceSmeltEvent, "FurnaceSmeltEvent");

/// `FurnaceBurnEvent`: a furnace, smoker or blast furnace lit a new `fuel`
/// item, for `burn_time` ticks (which may be changed). Cancelling keeps the
/// fuel and leaves the furnace out.
#[derive(Clone, Debug)]
pub struct FurnaceBurnEvent {
    pub block: Block,
    pub fuel: ItemStack,
    pub burn_time: i32,
    pub cancelled: bool,
}
cancellable!(FurnaceBurnEvent, "FurnaceBurnEvent");

/// `FurnaceExtractEvent`: a player took `item` out of a furnace's output, and
/// gets `exp` experience points for what it cooked (which may be changed).
#[derive(Clone, Debug)]
pub struct FurnaceExtractEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub item: ItemStack,
    pub exp: i32,
}
event!(FurnaceExtractEvent, "FurnaceExtractEvent");

/// `BrewEvent`: a brewing stand finished brewing with `ingredient`. `results`
/// are the three bottle slots after it, and may be changed; cancelling keeps
/// the bottles and the ingredient as they were.
#[derive(Clone, Debug)]
pub struct BrewEvent {
    pub block: Block,
    pub ingredient: ItemStack,
    pub results: Vec<Option<ItemStack>>,
    /// The fuel left (brews).
    pub fuel_level: i32,
    pub cancelled: bool,
}
cancellable!(BrewEvent, "BrewEvent");

/// `BrewingStandFuelEvent`: a brewing stand is about to use a `fuel` item
/// (blaze powder) for `fuel_power` brews (which may be changed). With
/// `consuming` false the item stays; cancelling uses nothing.
#[derive(Clone, Debug)]
pub struct BrewingStandFuelEvent {
    pub block: Block,
    pub fuel: ItemStack,
    pub fuel_power: i32,
    pub consuming: bool,
    pub cancelled: bool,
}
cancellable!(BrewingStandFuelEvent, "BrewingStandFuelEvent");

/// `CrafterCraftEvent`: a crafter is about to craft `recipe`, ejecting
/// `result` (which may be changed). Cancelling crafts nothing.
#[derive(Clone, Debug)]
pub struct CrafterCraftEvent {
    pub block: Block,
    pub recipe: String,
    pub result: ItemStack,
    pub cancelled: bool,
}
cancellable!(CrafterCraftEvent, "CrafterCraftEvent");

// ---------------------------------------------------------------------------
// Hoppers and droppers

/// What holds an inventory items move between: a block (a hopper, a chest,
/// a furnace...) or an entity (a chest or hopper minecart, a chest boat).
#[derive(Clone, Debug)]
pub enum InventoryHolder {
    Block(Block),
    Entity(EntityRef),
}

/// `InventoryMoveItemEvent`: a hopper (or hopper minecart) pushes or pulls
/// one `item`, or a dropper drops it into a container. `initiator` is the
/// hopper or dropper doing it. Cancelling leaves the item where it was.
#[derive(Clone, Debug)]
pub struct InventoryMoveItemEvent {
    pub source: InventoryHolder,
    pub destination: InventoryHolder,
    pub initiator: InventoryHolder,
    pub item: ItemStack,
    pub cancelled: bool,
}
cancellable!(InventoryMoveItemEvent, "InventoryMoveItemEvent");

/// `InventoryPickupItemEvent`: a hopper (or hopper minecart) is about to
/// take in the dropped item `item` holding `stack`. Cancelling leaves it on
/// the ground.
#[derive(Clone, Debug)]
pub struct InventoryPickupItemEvent {
    pub inventory: InventoryHolder,
    pub item: EntityRef,
    pub stack: ItemStack,
    pub cancelled: bool,
}
cancellable!(InventoryPickupItemEvent, "InventoryPickupItemEvent");

// ---------------------------------------------------------------------------
// A player's menus

/// `DragType`: how a drag spreads the carried stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragType {
    /// Left button: split evenly.
    Even,
    /// Right button: one item in each slot.
    Single,
    /// Middle button (creative): a full stack in each slot.
    Clone,
}

/// `InventoryDragEvent`: a player let go of a drag (quick craft) over
/// `slots` (the menu's numbering), carrying `cursor`. Cancelling undoes the
/// drag on their client.
#[derive(Clone, Debug)]
pub struct InventoryDragEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub slots: Vec<i32>,
    pub drag_type: DragType,
    pub cursor: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(InventoryDragEvent, "InventoryDragEvent");

/// `TradeSelectEvent`: a player picked the offer `index` in a trading
/// screen with `merchant` (None: a plugin's merchant). Cancelling keeps the
/// trade slots as they were.
#[derive(Clone, Debug)]
pub struct TradeSelectEvent {
    pub player: PlayerRef,
    pub merchant: Option<EntityRef>,
    pub index: i32,
    pub cancelled: bool,
}
cancellable!(TradeSelectEvent, "TradeSelectEvent");

/// `PlayerRecipeDiscoverEvent`: a player is about to learn `recipe` (by
/// crafting it, a `/recipe give`, a knowledge book, an advancement).
/// Cancelling keeps it out of their recipe book.
#[derive(Clone, Debug)]
pub struct PlayerRecipeDiscoverEvent {
    pub player: PlayerRef,
    pub recipe: String,
    pub cancelled: bool,
}
cancellable!(PlayerRecipeDiscoverEvent, "PlayerRecipeDiscoverEvent");

/// `PlayerRecipeBookClickEvent`: a player clicked `recipe` in their recipe
/// book to fill the crafting grid (`make_all`: shift, as many as they can).
/// Changing `recipe` places that one (if they know it); cancelling places
/// nothing.
#[derive(Clone, Debug)]
pub struct PlayerRecipeBookClickEvent {
    pub player: PlayerRef,
    pub recipe: String,
    pub make_all: bool,
    pub cancelled: bool,
}
cancellable!(PlayerRecipeBookClickEvent, "PlayerRecipeBookClickEvent");

/// `PlayerTakeLecternBookEvent`: a player is about to take `book` from the
/// lectern `lectern`. Cancelling leaves it there.
#[derive(Clone, Debug)]
pub struct PlayerTakeLecternBookEvent {
    pub player: PlayerRef,
    pub lectern: Block,
    pub book: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(PlayerTakeLecternBookEvent, "PlayerTakeLecternBookEvent");
