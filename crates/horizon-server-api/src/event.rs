//! Events (`org.bukkit.event`): what happened, what a listener may change,
//! and `cancelled` where Bukkit's event is `Cancellable`.
use crate::server::{Block, BlockPos, EntityRef, ItemStack, Location, PlayerRef, World};
use std::any::Any;

/// `EventPriority`: listeners run from `Lowest` to `Monitor`; `Monitor`
/// only watches the outcome and should change nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventPriority {
    Lowest,
    Low,
    Normal,
    High,
    Highest,
    Monitor,
}

/// An event a listener can be registered for.
pub trait Event: Any {
    /// The event's class name, as Bukkit names it.
    const NAME: &'static str;
    /// Whether a listener cancelled it (`Cancellable.isCancelled`); false
    /// for events that can't be.
    fn cancelled(&self) -> bool {
        false
    }
}

macro_rules! cancellable {
    ($t:ty, $name:literal) => {
        impl Event for $t {
            const NAME: &'static str = $name;
            fn cancelled(&self) -> bool {
                self.cancelled
            }
        }
    };
}
macro_rules! event {
    ($t:ty, $name:literal) => {
        impl Event for $t {
            const NAME: &'static str = $name;
        }
    };
}

/// `PlayerJoinEvent`: a player is in the world. `join_message` starts as
/// vanilla's (`§e<name> joined the game`); `None` sends none.
#[derive(Clone, Debug)]
pub struct PlayerJoinEvent {
    pub player: PlayerRef,
    pub join_message: Option<String>,
    /// Whether this is the player's first time on the server.
    pub first_join: bool,
}
event!(PlayerJoinEvent, "PlayerJoinEvent");

/// `PlayerQuitEvent`: a player left. `quit_message` starts as vanilla's
/// (`§e<name> left the game`); `None` sends none.
#[derive(Clone, Debug)]
pub struct PlayerQuitEvent {
    pub player: PlayerRef,
    pub quit_message: Option<String>,
    /// Why the connection ended, as the server logs it.
    pub reason: String,
}
event!(PlayerQuitEvent, "PlayerQuitEvent");

/// `AsyncPlayerChatEvent` (run in the tick here): a chat message. `format`
/// is `<{name}> {message}`; changing the message, the format or the
/// recipients sends it as a system message instead of the player's signed one.
#[derive(Clone, Debug)]
pub struct PlayerChatEvent {
    pub player: PlayerRef,
    pub message: String,
    pub format: String,
    /// Who sees it (UUIDs).
    pub recipients: Vec<u128>,
    pub cancelled: bool,
}
cancellable!(PlayerChatEvent, "AsyncPlayerChatEvent");
impl PlayerChatEvent {
    /// The line as `format` shows it.
    pub fn formatted(&self) -> String {
        self.format
            .replace("{name}", &self.player.name)
            .replace("{message}", &self.message)
    }
}

/// `PlayerCommandPreprocessEvent`: a command a player typed, with its
/// slash. Changing `message` runs the new line.
#[derive(Clone, Debug)]
pub struct PlayerCommandPreprocessEvent {
    pub player: PlayerRef,
    pub message: String,
    pub cancelled: bool,
}
cancellable!(PlayerCommandPreprocessEvent, "PlayerCommandPreprocessEvent");

/// `ServerCommandEvent`: a console command, without a slash.
#[derive(Clone, Debug)]
pub struct ServerCommandEvent {
    pub command: String,
    pub cancelled: bool,
}
cancellable!(ServerCommandEvent, "ServerCommandEvent");

/// `BlockBreakEvent`: a player breaks a block.
#[derive(Clone, Debug)]
pub struct BlockBreakEvent {
    pub player: PlayerRef,
    pub world: World,
    pub pos: BlockPos,
    /// The block state broken.
    pub block: String,
    /// `setDropItems(false)` breaks it without drops.
    pub drop_items: bool,
    pub cancelled: bool,
}
cancellable!(BlockBreakEvent, "BlockBreakEvent");

/// `BlockPlaceEvent`: a player placed a block (`block` is the new state,
/// `replaced` what was there). Cancelling puts the old one back.
#[derive(Clone, Debug)]
pub struct BlockPlaceEvent {
    pub player: PlayerRef,
    pub world: World,
    pub pos: BlockPos,
    pub block: String,
    pub replaced: String,
    /// The item it was placed from.
    pub item: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(BlockPlaceEvent, "BlockPlaceEvent");

/// `PlayerDeathEvent`: `death_message` starts as vanilla's (plain text);
/// `None` sends none.
#[derive(Clone, Debug)]
pub struct PlayerDeathEvent {
    pub player: PlayerRef,
    pub death_message: Option<String>,
    /// `setKeepInventory`: the inventory stays instead of dropping.
    pub keep_inventory: bool,
}
event!(PlayerDeathEvent, "PlayerDeathEvent");

/// `PlayerMoveEvent`: a player moved or turned this tick, from `from` to
/// `to`. Cancelling puts the player back at `from`; changing `to` moves them
/// there instead.
#[derive(Clone, Debug)]
pub struct PlayerMoveEvent {
    pub player: PlayerRef,
    pub from: Location,
    pub to: Location,
    pub cancelled: bool,
}
cancellable!(PlayerMoveEvent, "PlayerMoveEvent");

/// `PlayerRespawnEvent`: a dead player comes back. `respawn_location` starts
/// where the server would put them (their bed or anchor, else the world
/// spawn); changing it puts them there.
#[derive(Clone, Debug)]
pub struct PlayerRespawnEvent {
    pub player: PlayerRef,
    pub respawn_location: Location,
    /// `isBedSpawn` / `isAnchorSpawn`: the point is the player's own.
    pub own_spawn: bool,
}
event!(PlayerRespawnEvent, "PlayerRespawnEvent");

/// `org.bukkit.event.block.Action`: what a `PlayerInteractEvent` was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InteractAction {
    LeftClickBlock,
    RightClickBlock,
    RightClickAir,
}

/// `BlockFace` of the six directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockFace {
    Down,
    Up,
    North,
    South,
    West,
    East,
}
impl BlockFace {
    /// The face of vanilla's `Direction` id (0 down .. 5 east).
    pub fn from_id(id: i32) -> Self {
        match id {
            0 => Self::Down,
            1 => Self::Up,
            2 => Self::North,
            3 => Self::South,
            4 => Self::West,
            _ => Self::East,
        }
    }
    /// The face a step of one block towards `d` leaves by.
    pub fn from_offset(d: [i32; 3]) -> Self {
        match d {
            [0, -1, 0] => Self::Down,
            [0, 1, 0] => Self::Up,
            [0, 0, -1] => Self::North,
            [0, 0, 1] => Self::South,
            [-1, 0, 0] => Self::West,
            _ => Self::East,
        }
    }
}

/// `EquipmentSlot.HAND` / `OFF_HAND`: the hand an interaction uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hand {
    Main,
    Off,
}

/// `PlayerInteractEvent`: a player starts breaking a block (left click),
/// uses an item on a block or uses an item in the air (right click).
/// Cancelling stops it: no block is used or placed, no item used, no dig
/// started; the player's client gets the blocks and the inventory back.
#[derive(Clone, Debug)]
pub struct PlayerInteractEvent {
    pub player: PlayerRef,
    pub action: InteractAction,
    pub hand: Hand,
    /// The item in that hand.
    pub item: Option<ItemStack>,
    pub world: World,
    /// The block clicked (None in the air) and the face.
    pub block: Option<(BlockPos, BlockFace)>,
    pub cancelled: bool,
}
cancellable!(PlayerInteractEvent, "PlayerInteractEvent");

/// `PlayerDropItemEvent`: a player throws an item out of their hand (the
/// drop key). Cancelling keeps it in the hand.
#[derive(Clone, Debug)]
pub struct PlayerDropItemEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
    pub cancelled: bool,
}
cancellable!(PlayerDropItemEvent, "PlayerDropItemEvent");

/// `PlayerGameModeChangeEvent`: a player's game mode changes (0 survival,
/// 1 creative, 2 adventure, 3 spectator). Cancelling keeps the old one.
#[derive(Clone, Debug)]
pub struct PlayerGameModeChangeEvent {
    pub player: PlayerRef,
    pub new_game_mode: u8,
    pub cancelled: bool,
}
cancellable!(PlayerGameModeChangeEvent, "PlayerGameModeChangeEvent");

/// `EntityDamageEvent`, and `EntityDamageByEntityEvent` when `damager` is
/// set: an entity is about to be hurt. `cause` is vanilla's damage type
/// (`minecraft:fall`, `minecraft:lava`, `minecraft:player_attack`,
/// `minecraft:mob_attack`, `minecraft:explosion`...). Changing `damage`
/// changes the hit, before armour and effects; cancelling stops it.
#[derive(Clone, Debug)]
pub struct EntityDamageEvent {
    pub entity: EntityRef,
    pub cause: String,
    pub damage: f64,
    /// The entity that caused it: the attacker, a projectile's shooter, the
    /// creeper that exploded.
    pub damager: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(EntityDamageEvent, "EntityDamageEvent");

// ---------------------------------------------------------------------------
// org.bukkit.event.player

/// `PlayerLoginEvent`: a player is about to join, before the join message.
/// Setting `allowed` to false (`disallow`) turns them away with
/// `kick_message`.
#[derive(Clone, Debug)]
pub struct PlayerLoginEvent {
    pub player: PlayerRef,
    pub allowed: bool,
    pub kick_message: String,
}
event!(PlayerLoginEvent, "PlayerLoginEvent");

/// `PlayerInteractEntityEvent`: a player uses an entity (a right click: a
/// villager's trades, a saddle, a name tag...). Attacks are
/// `EntityDamageEvent`. Cancelling stops the use.
#[derive(Clone, Debug)]
pub struct PlayerInteractEntityEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub hand: Hand,
    pub cancelled: bool,
}
cancellable!(PlayerInteractEntityEvent, "PlayerInteractEntityEvent");

/// `PlayerToggleSneakEvent`: a player starts or stops sneaking (seen once a
/// tick; the client has done it already).
#[derive(Clone, Debug)]
pub struct PlayerToggleSneakEvent {
    pub player: PlayerRef,
    pub sneaking: bool,
}
event!(PlayerToggleSneakEvent, "PlayerToggleSneakEvent");

/// `PlayerToggleSprintEvent`: a player starts or stops sprinting (seen once
/// a tick).
#[derive(Clone, Debug)]
pub struct PlayerToggleSprintEvent {
    pub player: PlayerRef,
    pub sprinting: bool,
}
event!(PlayerToggleSprintEvent, "PlayerToggleSprintEvent");

/// `PlayerToggleFlightEvent`: a player who may fly starts or stops flying.
/// Cancelling puts them back as they were.
#[derive(Clone, Debug)]
pub struct PlayerToggleFlightEvent {
    pub player: PlayerRef,
    pub flying: bool,
    pub cancelled: bool,
}
cancellable!(PlayerToggleFlightEvent, "PlayerToggleFlightEvent");

/// `PlayerItemHeldEvent`: a player picks another hotbar slot (0..9).
/// Cancelling keeps the old one.
#[derive(Clone, Debug)]
pub struct PlayerItemHeldEvent {
    pub player: PlayerRef,
    pub previous_slot: u8,
    pub new_slot: u8,
    pub cancelled: bool,
}
cancellable!(PlayerItemHeldEvent, "PlayerItemHeldEvent");

/// `PlayerSwapHandItemsEvent`: a player swaps the items of their hands (F).
/// `main_hand_item` and `off_hand_item` are what each hand would hold.
/// Cancelling leaves them as they were.
#[derive(Clone, Debug)]
pub struct PlayerSwapHandItemsEvent {
    pub player: PlayerRef,
    pub main_hand_item: Option<ItemStack>,
    pub off_hand_item: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(PlayerSwapHandItemsEvent, "PlayerSwapHandItemsEvent");

/// `PlayerChangedWorldEvent`: a player arrived in another level from `from`.
#[derive(Clone, Debug)]
pub struct PlayerChangedWorldEvent {
    pub player: PlayerRef,
    pub from: World,
}
event!(PlayerChangedWorldEvent, "PlayerChangedWorldEvent");

/// `PlayerTeleportEvent.TeleportCause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TeleportCause {
    Command,
    EnderPearl,
    ChorusFruit,
    Plugin,
    Unknown,
}

/// `PlayerTeleportEvent`: a player is about to be moved within their level.
/// Changing `to` sends them there; cancelling keeps them where they are.
#[derive(Clone, Debug)]
pub struct PlayerTeleportEvent {
    pub player: PlayerRef,
    pub from: Location,
    pub to: Location,
    pub cause: TeleportCause,
    pub cancelled: bool,
}
cancellable!(PlayerTeleportEvent, "PlayerTeleportEvent");

/// `PlayerBedEnterEvent`: a player is about to lie down in `bed` (its head).
/// Cancelling leaves the bed unused (and the spawn point unset).
#[derive(Clone, Debug)]
pub struct PlayerBedEnterEvent {
    pub player: PlayerRef,
    pub bed: Block,
    pub cancelled: bool,
}
cancellable!(PlayerBedEnterEvent, "PlayerBedEnterEvent");

/// `PlayerBedLeaveEvent`: a player got out of `bed`.
#[derive(Clone, Debug)]
pub struct PlayerBedLeaveEvent {
    pub player: PlayerRef,
    pub bed: Block,
}
event!(PlayerBedLeaveEvent, "PlayerBedLeaveEvent");

/// `PlayerBucketFillEvent`: a player is about to fill `bucket` from the
/// fluid (or powder snow) `block`. Cancelling leaves both as they were.
#[derive(Clone, Debug)]
pub struct PlayerBucketFillEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub bucket: ItemStack,
    pub cancelled: bool,
}
cancellable!(PlayerBucketFillEvent, "PlayerBucketFillEvent");

/// `PlayerBucketEmptyEvent`: a player is about to empty `bucket` into
/// `block`. Cancelling leaves both as they were.
#[derive(Clone, Debug)]
pub struct PlayerBucketEmptyEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub bucket: ItemStack,
    pub cancelled: bool,
}
cancellable!(PlayerBucketEmptyEvent, "PlayerBucketEmptyEvent");

/// `PlayerItemConsumeEvent`: a player finished eating or drinking `item`.
/// Cancelling keeps the item and gives none of its effects.
#[derive(Clone, Debug)]
pub struct PlayerItemConsumeEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
    pub cancelled: bool,
}
cancellable!(PlayerItemConsumeEvent, "PlayerItemConsumeEvent");

/// `PlayerPickupItemEvent` (`EntityPickupItemEvent` for a player): a player
/// is about to pick up the dropped `item` (the entity `entity`). Cancelling
/// leaves it on the ground.
#[derive(Clone, Debug)]
pub struct PlayerPickupItemEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
    pub entity: EntityRef,
    pub cancelled: bool,
}
cancellable!(PlayerPickupItemEvent, "PlayerPickupItemEvent");

/// `PlayerResourcePackStatusEvent.Status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourcePackStatus {
    SuccessfullyLoaded,
    Declined,
    FailedDownload,
    Accepted,
    Downloaded,
    InvalidUrl,
    FailedReload,
    Discarded,
}
impl ResourcePackStatus {
    /// The status of `ServerboundResourcePackPacket.Action`'s id.
    pub fn from_id(id: i32) -> Self {
        match id {
            0 => Self::SuccessfullyLoaded,
            1 => Self::Declined,
            2 => Self::FailedDownload,
            3 => Self::Accepted,
            4 => Self::Downloaded,
            5 => Self::InvalidUrl,
            6 => Self::FailedReload,
            _ => Self::Discarded,
        }
    }
}

/// `PlayerResourcePackStatusEvent`: a player's client answered a resource pack.
#[derive(Clone, Debug)]
pub struct PlayerResourcePackStatusEvent {
    pub player: PlayerRef,
    pub status: ResourcePackStatus,
}
event!(
    PlayerResourcePackStatusEvent,
    "PlayerResourcePackStatusEvent"
);

/// `PlayerKickEvent`: a plugin is about to kick a player with `reason`.
/// Changing it changes the message; cancelling keeps them.
#[derive(Clone, Debug)]
pub struct PlayerKickEvent {
    pub player: PlayerRef,
    pub reason: String,
    pub cancelled: bool,
}
cancellable!(PlayerKickEvent, "PlayerKickEvent");

/// `PlayerExpChangeEvent`: a player's experience points changed by `amount`
/// (seen once a tick).
#[derive(Clone, Debug)]
pub struct PlayerExpChangeEvent {
    pub player: PlayerRef,
    pub amount: i32,
}
event!(PlayerExpChangeEvent, "PlayerExpChangeEvent");

/// `PlayerAdvancementDoneEvent`: a player completed `advancement`
/// (`minecraft:story/mine_stone`...).
#[derive(Clone, Debug)]
pub struct PlayerAdvancementDoneEvent {
    pub player: PlayerRef,
    pub advancement: String,
}
event!(PlayerAdvancementDoneEvent, "PlayerAdvancementDoneEvent");

/// `PlayerLevelChangeEvent`: a player's experience level changed.
#[derive(Clone, Debug)]
pub struct PlayerLevelChangeEvent {
    pub player: PlayerRef,
    pub old_level: i32,
    pub new_level: i32,
}
event!(PlayerLevelChangeEvent, "PlayerLevelChangeEvent");

// ---------------------------------------------------------------------------
// org.bukkit.event.block

/// `SignChangeEvent`: a player finished editing a sign's `front` or back.
/// Changing `lines` writes those; cancelling keeps the sign as it was.
#[derive(Clone, Debug)]
pub struct SignChangeEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub lines: Vec<String>,
    pub front: bool,
    pub cancelled: bool,
}
cancellable!(SignChangeEvent, "SignChangeEvent");

/// `BlockPistonExtendEvent`: the piston `block` is about to push `blocks`
/// towards `direction`. Cancelling keeps it in.
#[derive(Clone, Debug)]
pub struct BlockPistonExtendEvent {
    pub block: Block,
    pub direction: BlockFace,
    pub sticky: bool,
    pub blocks: Vec<BlockPos>,
    pub cancelled: bool,
}
cancellable!(BlockPistonExtendEvent, "BlockPistonExtendEvent");

/// `BlockPistonRetractEvent`: the piston `block` is about to retract (a
/// sticky one pulling `blocks`). Cancelling keeps it out.
#[derive(Clone, Debug)]
pub struct BlockPistonRetractEvent {
    pub block: Block,
    pub direction: BlockFace,
    pub sticky: bool,
    pub blocks: Vec<BlockPos>,
    pub cancelled: bool,
}
cancellable!(BlockPistonRetractEvent, "BlockPistonRetractEvent");

/// `BlockFromToEvent`: the fluid at `block` is about to flow into `to`
/// (towards `face`). Cancelling keeps it out of there (a cancelled flow down
/// spreads no further this tick).
#[derive(Clone, Debug)]
pub struct BlockFromToEvent {
    pub block: Block,
    pub to: Block,
    pub face: BlockFace,
    pub cancelled: bool,
}
cancellable!(BlockFromToEvent, "BlockFromToEvent");

/// `BlockGrowEvent`: a plant grew into `new_state` (crops, saplings into
/// trees, sugar cane...). Cancelling puts back what it changed.
#[derive(Clone, Debug)]
pub struct BlockGrowEvent {
    pub block: Block,
    pub new_state: String,
    pub cancelled: bool,
}
cancellable!(BlockGrowEvent, "BlockGrowEvent");

/// `BlockFormEvent`: snow or ice formed by the weather, or stone,
/// cobblestone, obsidian or basalt where lava met water. Cancelling stops it.
#[derive(Clone, Debug)]
pub struct BlockFormEvent {
    pub block: Block,
    pub new_state: String,
    pub cancelled: bool,
}
cancellable!(BlockFormEvent, "BlockFormEvent");

/// `BlockFadeEvent`: ice or snow melting, coral dying, fire going out,
/// grass and farmland turning to dirt. Cancelling stops it.
#[derive(Clone, Debug)]
pub struct BlockFadeEvent {
    pub block: Block,
    pub new_state: String,
    pub cancelled: bool,
}
cancellable!(BlockFadeEvent, "BlockFadeEvent");

/// `BlockSpreadEvent`: grass, mycelium, vines, mushrooms or fire spread
/// from `source` into `block`. Cancelling stops it.
#[derive(Clone, Debug)]
pub struct BlockSpreadEvent {
    pub block: Block,
    pub source: Block,
    pub new_state: String,
    pub cancelled: bool,
}
cancellable!(BlockSpreadEvent, "BlockSpreadEvent");

/// `BlockFertilizeEvent`: bone meal is about to be used on `block`, by
/// `player` or a dispenser (None). Cancelling grows nothing and keeps the meal.
#[derive(Clone, Debug)]
pub struct BlockFertilizeEvent {
    pub player: Option<PlayerRef>,
    pub block: Block,
    pub cancelled: bool,
}
cancellable!(BlockFertilizeEvent, "BlockFertilizeEvent");

/// `LeavesDecayEvent`: leaves too far from a log are about to drop and go.
#[derive(Clone, Debug)]
pub struct LeavesDecayEvent {
    pub block: Block,
    pub cancelled: bool,
}
cancellable!(LeavesDecayEvent, "LeavesDecayEvent");

/// `BlockBurnEvent`: the fire at `igniting` is about to burn `block` away.
#[derive(Clone, Debug)]
pub struct BlockBurnEvent {
    pub block: Block,
    pub igniting: Block,
    pub cancelled: bool,
}
cancellable!(BlockBurnEvent, "BlockBurnEvent");

// ---------------------------------------------------------------------------
// org.bukkit.event.entity

/// `CreatureSpawnEvent.SpawnReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpawnReason {
    Natural,
    ChunkGen,
    Spawner,
    SpawnerEgg,
    Command,
    Raid,
    Patrol,
    BuildIronGolem,
    BuildSnowman,
    BuildWither,
    Breeding,
    Default,
}

/// `CreatureSpawnEvent`: a mob is about to come into the world at
/// `location`. Cancelling keeps it out.
#[derive(Clone, Debug)]
pub struct CreatureSpawnEvent {
    pub entity: EntityRef,
    pub location: Location,
    pub reason: SpawnReason,
    pub cancelled: bool,
}
cancellable!(CreatureSpawnEvent, "CreatureSpawnEvent");

/// `EntityDeathEvent`: a mob died at `location` (players: `PlayerDeathEvent`).
#[derive(Clone, Debug)]
pub struct EntityDeathEvent {
    pub entity: EntityRef,
    pub location: Location,
    /// The player who killed it, if one did.
    pub killer: Option<PlayerRef>,
}
event!(EntityDeathEvent, "EntityDeathEvent");

/// `EntityExplodeEvent`: an explosion at `location` (of `entity`: a creeper,
/// TNT...) is about to destroy `blocks`. Removing blocks spares them;
/// cancelling stops the explosion's damage to the world and the entities.
#[derive(Clone, Debug)]
pub struct EntityExplodeEvent {
    pub entity: Option<EntityRef>,
    pub location: Location,
    pub blocks: Vec<BlockPos>,
    pub cancelled: bool,
}
cancellable!(EntityExplodeEvent, "EntityExplodeEvent");

// ---------------------------------------------------------------------------
// org.bukkit.event.inventory

/// `InventoryView`: the menu a player has open: its `minecraft:menu` type
/// (`minecraft:generic_9x3`, `minecraft:crafting`...; empty for their own
/// inventory) and the slots of the container itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InventoryView {
    pub kind: String,
    pub size: u32,
}

/// `ClickType`, from the click's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClickType {
    Pickup,
    QuickMove,
    Swap,
    Clone,
    Throw,
    QuickCraft,
    PickupAll,
}
impl ClickType {
    /// `ContainerInput`'s id.
    pub fn from_id(id: i32) -> Self {
        match id {
            1 => Self::QuickMove,
            2 => Self::Swap,
            3 => Self::Clone,
            4 => Self::Throw,
            5 => Self::QuickCraft,
            6 => Self::PickupAll,
            _ => Self::Pickup,
        }
    }
}

/// `InventoryOpenEvent`: a player is about to open a menu. Cancelling
/// opens nothing.
#[derive(Clone, Debug)]
pub struct InventoryOpenEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub cancelled: bool,
}
cancellable!(InventoryOpenEvent, "InventoryOpenEvent");

/// `InventoryClickEvent`: a player clicked `slot` (the menu's numbering,
/// -999 outside) with `button`. `current` is the item in the slot, `cursor`
/// the one carried. Cancelling undoes the click on their client.
#[derive(Clone, Debug)]
pub struct InventoryClickEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub slot: i32,
    pub button: u8,
    pub click: ClickType,
    pub current: Option<ItemStack>,
    pub cursor: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(InventoryClickEvent, "InventoryClickEvent");

/// `InventoryCloseEvent`: a player closed a menu.
#[derive(Clone, Debug)]
pub struct InventoryCloseEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
}
event!(InventoryCloseEvent, "InventoryCloseEvent");

// ---------------------------------------------------------------------------
// More of org.bukkit.event.player

/// `FoodLevelChangeEvent`: a player's food level changed to `food_level`
/// (eating, hunger; seen once a tick). Cancelling puts the old one back.
#[derive(Clone, Debug)]
pub struct FoodLevelChangeEvent {
    pub player: PlayerRef,
    pub food_level: i32,
    pub cancelled: bool,
}
cancellable!(FoodLevelChangeEvent, "FoodLevelChangeEvent");

/// `PlayerFishEvent.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FishState {
    /// The rod was cast.
    Fishing,
    /// A fish bit and was pulled in.
    CaughtFish,
    /// An entity on the hook was pulled.
    CaughtEntity,
    /// Reeled in with nothing.
    ReelIn,
}

/// `PlayerFishEvent`: a player casts or reels in a fishing rod. Cancelling
/// keeps the hook as it was (no cast, no catch).
#[derive(Clone, Debug)]
pub struct PlayerFishEvent {
    pub player: PlayerRef,
    pub state: FishState,
    /// The entity on the hook (`CaughtEntity`).
    pub caught: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(PlayerFishEvent, "PlayerFishEvent");

/// `PlayerEditBookEvent`: a player saved a book and quill (`title` set: they
/// signed it). Changing `pages` or `title` writes those; cancelling keeps
/// the book as it was.
#[derive(Clone, Debug)]
pub struct PlayerEditBookEvent {
    pub player: PlayerRef,
    pub pages: Vec<String>,
    pub title: Option<String>,
    pub cancelled: bool,
}
cancellable!(PlayerEditBookEvent, "PlayerEditBookEvent");

/// `PlayerPortalEvent`: a player is about to go through a portal from
/// `from` to the level `to_world`. Cancelling keeps them here.
#[derive(Clone, Debug)]
pub struct PlayerPortalEvent {
    pub player: PlayerRef,
    pub from: Location,
    pub to_world: World,
    pub cancelled: bool,
}
cancellable!(PlayerPortalEvent, "PlayerPortalEvent");

/// `PlayerShearEntityEvent`: a player is about to shear a sheep, a
/// mooshroom, a snow golem or a bogged. Cancelling keeps its wool.
#[derive(Clone, Debug)]
pub struct PlayerShearEntityEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub cancelled: bool,
}
cancellable!(PlayerShearEntityEvent, "PlayerShearEntityEvent");

// ---------------------------------------------------------------------------
// More of org.bukkit.event.block

/// `BlockDamageEvent`: a player starts digging `block` with `item`.
/// Cancelling stops the dig.
#[derive(Clone, Debug)]
pub struct BlockDamageEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub item: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(BlockDamageEvent, "BlockDamageEvent");

/// `BlockDispenseEvent`: the dispenser or dropper `block` is about to
/// dispense `item`. Cancelling keeps it inside.
#[derive(Clone, Debug)]
pub struct BlockDispenseEvent {
    pub block: Block,
    pub item: ItemStack,
    pub cancelled: bool,
}
cancellable!(BlockDispenseEvent, "BlockDispenseEvent");

// ---------------------------------------------------------------------------
// More of org.bukkit.event.entity

/// `EntityRegainHealthEvent`: a player's health went up by `amount`
/// (regeneration, food, potions; seen once a tick). Cancelling takes it back.
#[derive(Clone, Debug)]
pub struct EntityRegainHealthEvent {
    pub entity: EntityRef,
    pub amount: f64,
    pub cancelled: bool,
}
cancellable!(EntityRegainHealthEvent, "EntityRegainHealthEvent");

/// `EntityTargetLivingEntityEvent` (`EntityTargetEvent`): a mob chose
/// `target` (None: it forgot its target). Seen once a tick; cancelling puts
/// the old target back.
#[derive(Clone, Debug)]
pub struct EntityTargetLivingEntityEvent {
    pub entity: EntityRef,
    pub target: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(
    EntityTargetLivingEntityEvent,
    "EntityTargetLivingEntityEvent"
);

/// `EntityTameEvent`: a wolf, cat, parrot, horse... was tamed by the player
/// `owner` (seen once a tick). Cancelling makes it wild again.
#[derive(Clone, Debug)]
pub struct EntityTameEvent {
    pub entity: EntityRef,
    pub owner: Option<PlayerRef>,
    pub cancelled: bool,
}
cancellable!(EntityTameEvent, "EntityTameEvent");

/// `EntityBreedEvent`: two animals in love are about to make a baby
/// (`breeder`: the player who fed them). Cancelling makes none.
#[derive(Clone, Debug)]
pub struct EntityBreedEvent {
    pub mother: EntityRef,
    pub father: EntityRef,
    pub breeder: Option<PlayerRef>,
    pub cancelled: bool,
}
cancellable!(EntityBreedEvent, "EntityBreedEvent");

/// `ProjectileLaunchEvent`: an arrow, trident, snowball, egg, pearl, potion,
/// wind charge or fireball came into the world (`shooter`: who threw or shot
/// it). Cancelling removes it.
#[derive(Clone, Debug)]
pub struct ProjectileLaunchEvent {
    pub entity: EntityRef,
    pub shooter: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(ProjectileLaunchEvent, "ProjectileLaunchEvent");

/// `ProjectileHitEvent`: a projectile hit a block or an entity. Cancelling
/// skips what the hit would do (no damage, no pearl teleport, no splash).
#[derive(Clone, Debug)]
pub struct ProjectileHitEvent {
    pub entity: EntityRef,
    pub shooter: Option<EntityRef>,
    pub hit_block: Option<Block>,
    pub hit_entity: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(ProjectileHitEvent, "ProjectileHitEvent");

/// `ItemSpawnEvent`: a dropped item is about to come into the world.
/// Cancelling keeps it out.
#[derive(Clone, Debug)]
pub struct ItemSpawnEvent {
    pub item: ItemStack,
    pub location: Location,
    pub cancelled: bool,
}
cancellable!(ItemSpawnEvent, "ItemSpawnEvent");

// ---------------------------------------------------------------------------
// More of org.bukkit.event.inventory and enchantment

/// `CraftItemEvent`: a player takes `result` out of a crafting grid.
/// Cancelling leaves it there.
#[derive(Clone, Debug)]
pub struct CraftItemEvent {
    pub player: PlayerRef,
    pub view: InventoryView,
    pub result: ItemStack,
    pub cancelled: bool,
}
cancellable!(CraftItemEvent, "CraftItemEvent");

/// `EnchantItemEvent`: a player is about to enchant `item` at an enchanting
/// table with the offer `button` (0..3), costing `level_cost` levels.
/// Cancelling enchants nothing.
#[derive(Clone, Debug)]
pub struct EnchantItemEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
    pub button: u8,
    pub level_cost: i32,
    pub cancelled: bool,
}
cancellable!(EnchantItemEvent, "EnchantItemEvent");

// ---------------------------------------------------------------------------
// org.bukkit.event.vehicle, weather, world, server

/// `VehicleEnterEvent`: a player got on `vehicle` (a boat, a minecart, a
/// horse...; seen once a tick).
#[derive(Clone, Debug)]
pub struct VehicleEnterEvent {
    pub vehicle: EntityRef,
    pub entered: PlayerRef,
}
event!(VehicleEnterEvent, "VehicleEnterEvent");

/// `VehicleExitEvent`: a player got off `vehicle` (seen once a tick).
#[derive(Clone, Debug)]
pub struct VehicleExitEvent {
    pub vehicle: EntityRef,
    pub exited: PlayerRef,
}
event!(VehicleExitEvent, "VehicleExitEvent");

/// `WeatherChangeEvent`: rain starts (`to_raining`) or stops in `world`.
/// Cancelling keeps the weather as it was.
#[derive(Clone, Debug)]
pub struct WeatherChangeEvent {
    pub world: World,
    pub to_raining: bool,
    pub cancelled: bool,
}
cancellable!(WeatherChangeEvent, "WeatherChangeEvent");

/// `ThunderChangeEvent`: a thunderstorm starts or stops in `world`.
/// Cancelling keeps the weather as it was.
#[derive(Clone, Debug)]
pub struct ThunderChangeEvent {
    pub world: World,
    pub to_thundering: bool,
    pub cancelled: bool,
}
cancellable!(ThunderChangeEvent, "ThunderChangeEvent");

/// `WorldSaveEvent`: `world` is being saved.
#[derive(Clone, Debug)]
pub struct WorldSaveEvent {
    pub world: World,
}
event!(WorldSaveEvent, "WorldSaveEvent");

/// `ServerLoadEvent`: the server finished starting (the plugins are enabled,
/// the worlds loaded, the players may join).
#[derive(Clone, Debug, Default)]
pub struct ServerLoadEvent;
event!(ServerLoadEvent, "ServerLoadEvent");

mod blocks;
mod entities;
mod inventories;
mod world;
pub use blocks::*;
pub use entities::*;
pub use inventories::*;
pub use world::*;
