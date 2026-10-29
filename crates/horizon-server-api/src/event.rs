//! Events (`org.bukkit.event`): what happened, what a listener may change,
//! and `cancelled` where Bukkit's event is `Cancellable`.
use crate::server::{BlockPos, ItemStack, Location, PlayerRef, World};
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
