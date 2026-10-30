//! More of Bukkit's events (group: players): the rest of the player events,
//! the server's own (plugins, ticks, reloads, the whitelist, unknown
//! commands) and Paper's most used player events. Uses the `event!` and
//! `cancellable!` macros of `event.rs`.
#[allow(unused_imports)]
use super::{BlockFace, Event, Hand, MainHand, PlayerChatEvent, TimeSkipReason};
use crate::command::CommandSender;
#[allow(unused_imports)]
use crate::server::{Block, BlockPos, EntityRef, ItemStack, Location, PlayerRef, World};

// ---------------------------------------------------------------------------
// Items a player picks up.

/// `PlayerAttemptPickupItemEvent`: a player touches a dropped item they
/// could pick up, before `PlayerPickupItemEvent` and even when their
/// inventory has no room. `remaining` is what would stay on the ground.
/// Cancelling leaves it there; with `fly_at_player` the player is still
/// shown taking it.
#[derive(Clone, Debug)]
pub struct PlayerAttemptPickupItemEvent {
    pub player: PlayerRef,
    pub item: EntityRef,
    pub stack: ItemStack,
    pub remaining: u32,
    pub fly_at_player: bool,
    pub cancelled: bool,
}
cancellable!(PlayerAttemptPickupItemEvent, "PlayerAttemptPickupItemEvent");

/// `PlayerBucketFishEvent` (deprecated in Bukkit for
/// `PlayerBucketEntityEvent`, which fires first): a player scoops a fish.
/// Cancelling leaves it in the water.
#[derive(Clone, Debug)]
pub struct PlayerBucketFishEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub original_bucket: ItemStack,
    pub entity_bucket: ItemStack,
    pub cancelled: bool,
}
cancellable!(PlayerBucketFishEvent, "PlayerBucketFishEvent");

// ---------------------------------------------------------------------------
// What a player's client tells.

/// `org.bukkit.Input`: the movement keys a player holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Input {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}
impl Input {
    /// The flags of `ServerboundPlayerInputPacket` (forward 1 .. sprint 64).
    pub fn from_flags(flags: u8) -> Self {
        Self {
            forward: flags & 1 != 0,
            backward: flags & 2 != 0,
            left: flags & 4 != 0,
            right: flags & 8 != 0,
            jump: flags & 16 != 0,
            sneak: flags & 32 != 0,
            sprint: flags & 64 != 0,
        }
    }
}

/// `PlayerInputEvent`: the keys a player holds changed (once a tick at most).
#[derive(Clone, Debug)]
pub struct PlayerInputEvent {
    pub player: PlayerRef,
    pub input: Input,
}
event!(PlayerInputEvent, "PlayerInputEvent");

/// `PlayerRecipeBookSettingsChangeEvent.RecipeBookType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecipeBookType {
    Crafting,
    Furnace,
    BlastFurnace,
    Smoker,
}

/// `PlayerRecipeBookSettingsChangeEvent`: a player opened, closed or
/// filtered one of their recipe books.
#[derive(Clone, Debug)]
pub struct PlayerRecipeBookSettingsChangeEvent {
    pub player: PlayerRef,
    pub recipe_book_type: RecipeBookType,
    pub open: bool,
    pub filtering: bool,
}
event!(
    PlayerRecipeBookSettingsChangeEvent,
    "PlayerRecipeBookSettingsChangeEvent"
);

/// `PlayerRegisterChannelEvent`: a player's client registered a plugin
/// channel (`minecraft:register`).
#[derive(Clone, Debug)]
pub struct PlayerRegisterChannelEvent {
    pub player: PlayerRef,
    pub channel: String,
}
event!(PlayerRegisterChannelEvent, "PlayerRegisterChannelEvent");

/// `PlayerUnregisterChannelEvent`: a player's client dropped a plugin
/// channel (`minecraft:unregister`).
#[derive(Clone, Debug)]
pub struct PlayerUnregisterChannelEvent {
    pub player: PlayerRef,
    pub channel: String,
}
event!(PlayerUnregisterChannelEvent, "PlayerUnregisterChannelEvent");

/// `ClientOption.ChatVisibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatVisibility {
    Full,
    System,
    Hidden,
}

/// `ClientOption.ParticleVisibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleVisibility {
    All,
    Decreased,
    Minimal,
}

/// A player's client options (`ClientInformation`).
#[derive(Clone, Debug, PartialEq)]
pub struct ClientOptions {
    pub locale: String,
    pub view_distance: i32,
    pub chat_visibility: ChatVisibility,
    pub chat_colors: bool,
    /// The skin layers shown (`SkinParts`' bits: cape 1 .. hat 64).
    pub skin_parts: u8,
    pub main_hand: MainHand,
    pub text_filtering: bool,
    pub allows_server_listings: bool,
    pub particle_visibility: ParticleVisibility,
}

/// Paper's `PlayerClientOptionsChangeEvent`: a player's client sent its
/// options and some changed (`previous` is what they were).
#[derive(Clone, Debug)]
pub struct PlayerClientOptionsChangeEvent {
    pub player: PlayerRef,
    pub options: ClientOptions,
    pub previous: ClientOptions,
}
event!(
    PlayerClientOptionsChangeEvent,
    "PlayerClientOptionsChangeEvent"
);
impl PlayerClientOptionsChangeEvent {
    pub fn locale_changed(&self) -> bool {
        self.options.locale != self.previous.locale
    }
    pub fn view_distance_changed(&self) -> bool {
        self.options.view_distance != self.previous.view_distance
    }
    pub fn skin_parts_changed(&self) -> bool {
        self.options.skin_parts != self.previous.skin_parts
    }
    pub fn main_hand_changed(&self) -> bool {
        self.options.main_hand != self.previous.main_hand
    }
}

/// `ServerLinks.Type`: a link the client labels itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServerLinkType {
    ReportBug,
    CommunityGuidelines,
    Support,
    Status,
    Feedback,
    Community,
    Website,
    Forums,
    News,
    Announcements,
}
impl ServerLinkType {
    /// By `ServerLinks.KnownLinkType` id.
    pub const ALL: [Self; 10] = [
        Self::ReportBug,
        Self::CommunityGuidelines,
        Self::Support,
        Self::Status,
        Self::Feedback,
        Self::Community,
        Self::Website,
        Self::Forums,
        Self::News,
        Self::Announcements,
    ];
}

/// `ServerLinks.ServerLink`: a known kind of link or one with its own
/// label (legacy-coded text), and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerLink {
    pub kind: Option<ServerLinkType>,
    /// The label when `kind` is None.
    pub label: String,
    pub url: String,
}

/// `PlayerLinksSendEvent`: the server links (the pause menu's "Server
/// Links...") a player joining is given. Changing the list sends that one.
#[derive(Clone, Debug)]
pub struct PlayerLinksSendEvent {
    pub player: PlayerRef,
    pub links: Vec<ServerLink>,
}
event!(PlayerLinksSendEvent, "PlayerLinksSendEvent");

// ---------------------------------------------------------------------------
// A player's body.

/// `PlayerSpawnChangeEvent.Cause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpawnChangeCause {
    Command,
    Bed,
    RespawnAnchor,
    Plugin,
    /// The bed or anchor was gone when the player respawned.
    Reset,
    Unknown,
}

/// `PlayerSpawnChangeEvent`: a player's respawn point is about to be
/// `new_spawn` (None: cleared, the world spawn again). A plugin may move or
/// clear it, or make it `forced` (no bed or anchor needed); cancelling keeps
/// the old one.
#[derive(Clone, Debug)]
pub struct PlayerSpawnChangeEvent {
    pub player: PlayerRef,
    pub new_spawn: Option<Location>,
    pub cause: SpawnChangeCause,
    pub forced: bool,
    pub cancelled: bool,
}
cancellable!(PlayerSpawnChangeEvent, "PlayerSpawnChangeEvent");

/// `PlayerHealthChangeEvent` (Leaf's): a player's health changed (seen once
/// a tick).
#[derive(Clone, Debug)]
pub struct PlayerHealthChangeEvent {
    pub player: PlayerRef,
    pub previous_health: f64,
    pub new_health: f64,
}
event!(PlayerHealthChangeEvent, "PlayerHealthChangeEvent");

/// Paper's `PlayerJumpEvent`: a player jumped from `from` to `to`.
/// Cancelling puts them back at `from` (which a plugin may change).
#[derive(Clone, Debug)]
pub struct PlayerJumpEvent {
    pub player: PlayerRef,
    pub from: Location,
    pub to: Location,
    pub cancelled: bool,
}
cancellable!(PlayerJumpEvent, "PlayerJumpEvent");

/// `PlayerArmorChangeEvent.SlotType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArmorSlot {
    Head,
    Chest,
    Legs,
    Feet,
}

/// Paper's `PlayerArmorChangeEvent`: what a player wears in `slot_type`
/// changed (None: nothing).
#[derive(Clone, Debug)]
pub struct PlayerArmorChangeEvent {
    pub player: PlayerRef,
    pub slot_type: ArmorSlot,
    pub old_item: Option<ItemStack>,
    pub new_item: Option<ItemStack>,
}
event!(PlayerArmorChangeEvent, "PlayerArmorChangeEvent");

/// Paper's `PlayerInventorySlotChangeEvent`: a slot of a player's own
/// inventory changed. `raw_slot` is the inventory menu's (armor 5-8, the
/// rest 9-44, off hand 45), `slot` Bukkit's (hotbar 0-8, 9-35, armor
/// 36-39 feet first, off hand 40).
#[derive(Clone, Debug)]
pub struct PlayerInventorySlotChangeEvent {
    pub player: PlayerRef,
    pub raw_slot: i32,
    pub slot: i32,
    pub old_item_stack: Option<ItemStack>,
    pub new_item_stack: Option<ItemStack>,
}
event!(
    PlayerInventorySlotChangeEvent,
    "PlayerInventorySlotChangeEvent"
);

/// Paper's `PlayerPostRespawnEvent`: a player is back in the world after
/// respawning, at `respawned_location`.
#[derive(Clone, Debug)]
pub struct PlayerPostRespawnEvent {
    pub player: PlayerRef,
    pub respawned_location: Location,
}
event!(PlayerPostRespawnEvent, "PlayerPostRespawnEvent");

// ---------------------------------------------------------------------------
// Items a player uses.

/// Paper's `PlayerReadyArrowEvent`: the server picks `arrow` for a player's
/// `bow` (or crossbow). Cancelling skips it for the next one.
#[derive(Clone, Debug)]
pub struct PlayerReadyArrowEvent {
    pub player: PlayerRef,
    pub bow: ItemStack,
    pub arrow: ItemStack,
    pub cancelled: bool,
}
cancellable!(PlayerReadyArrowEvent, "PlayerReadyArrowEvent");

/// Paper's `PlayerLaunchProjectileEvent`: a player threw `projectile` from
/// `item_stack` (a snowball, an egg, an ender pearl). Cancelling: nothing is
/// thrown or used; `should_consume` false keeps the item.
#[derive(Clone, Debug)]
pub struct PlayerLaunchProjectileEvent {
    pub player: PlayerRef,
    pub item_stack: ItemStack,
    pub projectile: EntityRef,
    pub should_consume: bool,
    pub cancelled: bool,
}
cancellable!(PlayerLaunchProjectileEvent, "PlayerLaunchProjectileEvent");

/// Paper's `PlayerStopUsingItemEvent`: a player let go of the item they
/// used (a bow, a trident, a spyglass...) after `ticks_held_for` ticks.
#[derive(Clone, Debug)]
pub struct PlayerStopUsingItemEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
    pub ticks_held_for: u32,
}
event!(PlayerStopUsingItemEvent, "PlayerStopUsingItemEvent");

/// Paper's `PlayerFlowerPotManipulateEvent`: a player puts `item` in a
/// flower pot (`placing`) or takes it out. Cancelling leaves the pot.
#[derive(Clone, Debug)]
pub struct PlayerFlowerPotManipulateEvent {
    pub player: PlayerRef,
    pub flowerpot: Block,
    pub item: ItemStack,
    pub placing: bool,
    pub cancelled: bool,
}
cancellable!(
    PlayerFlowerPotManipulateEvent,
    "PlayerFlowerPotManipulateEvent"
);

/// Paper's `PlayerPickBlockEvent`: a player picks `block` (middle click)
/// for `item`; `source_slot` is where they already have it (-1: nowhere).
/// Cancelling picks nothing.
#[derive(Clone, Debug)]
pub struct PlayerPickBlockEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub item: ItemStack,
    pub include_data: bool,
    pub source_slot: i32,
    pub cancelled: bool,
}
cancellable!(PlayerPickBlockEvent, "PlayerPickBlockEvent");

/// Paper's `PlayerPickEntityEvent`: as `PlayerPickBlockEvent`, for an entity.
#[derive(Clone, Debug)]
pub struct PlayerPickEntityEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub item: ItemStack,
    pub include_data: bool,
    pub source_slot: i32,
    pub cancelled: bool,
}
cancellable!(PlayerPickEntityEvent, "PlayerPickEntityEvent");

/// Paper's `AsyncChatEvent` (run in the tick here, after
/// `AsyncPlayerChatEvent`, with what that left): `viewers` are who see it
/// (UUIDs). Changing the message or the viewers sends it as a system
/// message.
#[derive(Clone, Debug)]
pub struct AsyncChatEvent {
    pub player: PlayerRef,
    pub message: String,
    pub viewers: Vec<u128>,
    pub cancelled: bool,
}
cancellable!(AsyncChatEvent, "AsyncChatEvent");
impl AsyncChatEvent {
    /// The event for what `AsyncPlayerChatEvent` left.
    pub fn after(chat: &PlayerChatEvent) -> Self {
        Self {
            player: chat.player.clone(),
            message: chat.message.clone(),
            viewers: chat.recipients.clone(),
            cancelled: chat.cancelled,
        }
    }
}

// ---------------------------------------------------------------------------
// The server.

/// `UnknownCommandEvent`: a command line (without its slash) that the
/// dispatcher could not parse. `message` starts as vanilla's error
/// (legacy-coded); a plugin may change it, None says nothing.
#[derive(Clone, Debug)]
pub struct UnknownCommandEvent {
    pub sender: CommandSender,
    pub command_line: String,
    pub message: Option<String>,
}
event!(UnknownCommandEvent, "UnknownCommandEvent");

/// `PluginEnableEvent`: `plugin` (its name) was just enabled.
#[derive(Clone, Debug)]
pub struct PluginEnableEvent {
    pub plugin: String,
}
event!(PluginEnableEvent, "PluginEnableEvent");

/// `PluginDisableEvent`: `plugin` is about to be disabled.
#[derive(Clone, Debug)]
pub struct PluginDisableEvent {
    pub plugin: String,
}
event!(PluginDisableEvent, "PluginDisableEvent");

/// `ClockTimeSkipEvent` (26.1's server-wide clocks): the server's clock is
/// about to jump by `skip_amount` ticks (`/time`). Changing it jumps that
/// far; cancelling keeps the time. `TimeSkipEvent` follows for the world.
#[derive(Clone, Debug)]
pub struct ClockTimeSkipEvent {
    pub skip_reason: TimeSkipReason,
    pub skip_amount: i64,
    pub cancelled: bool,
}
cancellable!(ClockTimeSkipEvent, "ClockTimeSkipEvent");

/// Paper's `ServerTickStartEvent`: tick `tick_number` begins.
#[derive(Clone, Debug)]
pub struct ServerTickStartEvent {
    pub tick_number: u64,
}
event!(ServerTickStartEvent, "ServerTickStartEvent");

/// Paper's `ServerTickEndEvent`: tick `tick_number` took `tick_duration`
/// milliseconds; `time_remaining` nanoseconds are left before the next one
/// (negative when late).
#[derive(Clone, Debug)]
pub struct ServerTickEndEvent {
    pub tick_number: u64,
    pub tick_duration: f64,
    pub time_remaining: i64,
}
event!(ServerTickEndEvent, "ServerTickEndEvent");

/// Paper's `WhitelistToggleEvent`: the whitelist was turned on or off.
#[derive(Clone, Debug)]
pub struct WhitelistToggleEvent {
    pub enabled: bool,
}
event!(WhitelistToggleEvent, "WhitelistToggleEvent");

/// `ServerResourcesReloadedEvent.Cause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReloadCause {
    Command,
    Plugin,
}

/// Paper's `ServerResourcesReloadedEvent`: the data packs were reloaded
/// (`/reload`).
#[derive(Clone, Debug)]
pub struct ServerResourcesReloadedEvent {
    pub cause: ReloadCause,
}
event!(ServerResourcesReloadedEvent, "ServerResourcesReloadedEvent");
