//! Events (`org.bukkit.event`): what happened, what a listener may change,
//! and `cancelled` where Bukkit's event is `Cancellable`.
use crate::server::{BlockPos, ItemStack, PlayerRef, World};
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
