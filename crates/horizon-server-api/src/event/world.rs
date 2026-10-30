//! More of Bukkit's events (group: world): the rest of the player's events,
//! chunks and worlds, growth and portals, time, raids, game events and
//! loot. Uses the `event!` and `cancellable!` macros of `event.rs`.
#[allow(unused_imports)]
use super::{BlockFace, Event, Hand};
#[allow(unused_imports)]
use crate::server::{Block, BlockPos, EntityRef, ItemStack, Location, PlayerRef, World};

// ---------------------------------------------------------------------------
// The player.

/// `PlayerStatisticIncrementEvent`: one of a player's statistics is about
/// to go up. `statistic` is its type (`minecraft:custom`, `minecraft:mined`,
/// `minecraft:used`...), `key` what it counts (`minecraft:jump`,
/// `minecraft:stone`...). Cancelling keeps `previous_value`. The counters
/// that go up every tick (play time, time since death or rest, total world
/// time, sneak time) fire none, as in CraftBukkit.
#[derive(Clone, Debug)]
pub struct PlayerStatisticIncrementEvent {
    pub player: PlayerRef,
    pub statistic: String,
    pub key: String,
    pub previous_value: i32,
    pub new_value: i32,
    pub cancelled: bool,
}
cancellable!(
    PlayerStatisticIncrementEvent,
    "PlayerStatisticIncrementEvent"
);

/// `PlayerItemDamageEvent`: an item of the player's is about to lose
/// `damage` points of durability (after Unbreaking; `original_damage`
/// before it). Changing `damage` loses that many instead; cancelling loses
/// none.
#[derive(Clone, Debug)]
pub struct PlayerItemDamageEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
    pub damage: u32,
    pub original_damage: u32,
    pub cancelled: bool,
}
cancellable!(PlayerItemDamageEvent, "PlayerItemDamageEvent");

/// `PlayerItemBreakEvent`: an item of the player's wore out and breaks.
#[derive(Clone, Debug)]
pub struct PlayerItemBreakEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
}
event!(PlayerItemBreakEvent, "PlayerItemBreakEvent");

/// `PlayerItemMendEvent`: an experience orb's Mending is about to repair
/// `repair_amount` points of `item`. Changing it repairs that many;
/// cancelling repairs nothing and the orb's experience goes to the player.
#[derive(Clone, Debug)]
pub struct PlayerItemMendEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
    pub repair_amount: u32,
    pub cancelled: bool,
}
cancellable!(PlayerItemMendEvent, "PlayerItemMendEvent");

/// `PlayerRiptideEvent`: a player launched themselves with a Riptide
/// trident.
#[derive(Clone, Debug)]
pub struct PlayerRiptideEvent {
    pub player: PlayerRef,
    pub item: ItemStack,
}
event!(PlayerRiptideEvent, "PlayerRiptideEvent");

/// `PlayerHarvestBlockEvent`: a player picked the berries of a sweet berry
/// bush or of cave vines. `items` are what drops; changing the list drops
/// those instead; cancelling drops nothing and leaves the plant as it was.
#[derive(Clone, Debug)]
pub struct PlayerHarvestBlockEvent {
    pub player: PlayerRef,
    pub block: Block,
    pub items: Vec<ItemStack>,
    pub cancelled: bool,
}
cancellable!(PlayerHarvestBlockEvent, "PlayerHarvestBlockEvent");

/// `PlayerArmorStandManipulateEvent`: a player is about to swap
/// `player_item` (their hand's) with `armor_stand_item` in the stand's
/// `slot` (`HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST` or `HEAD`).
/// Cancelling leaves both where they are.
#[derive(Clone, Debug)]
pub struct PlayerArmorStandManipulateEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub player_item: Option<ItemStack>,
    pub armor_stand_item: Option<ItemStack>,
    pub slot: String,
    pub cancelled: bool,
}
cancellable!(
    PlayerArmorStandManipulateEvent,
    "PlayerArmorStandManipulateEvent"
);

/// `PlayerLeashEntityEvent`: a player is about to put a lead on `entity`
/// (the player holds it). Cancelling keeps the lead in hand.
#[derive(Clone, Debug)]
pub struct PlayerLeashEntityEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub cancelled: bool,
}
cancellable!(PlayerLeashEntityEvent, "PlayerLeashEntityEvent");

/// `PlayerUnleashEntityEvent`: a player is about to take the lead off
/// `entity`. `drop_leash`: the lead falls as an item. Cancelling keeps it on.
#[derive(Clone, Debug)]
pub struct PlayerUnleashEntityEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub drop_leash: bool,
    pub cancelled: bool,
}
cancellable!(PlayerUnleashEntityEvent, "PlayerUnleashEntityEvent");

/// `PlayerBucketEntityEvent`: a player is about to scoop `entity` (a fish,
/// an axolotl, a tadpole) with `original_bucket`, making `entity_bucket`.
/// Cancelling leaves it in the water.
#[derive(Clone, Debug)]
pub struct PlayerBucketEntityEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub original_bucket: ItemStack,
    pub entity_bucket: ItemStack,
    pub cancelled: bool,
}
cancellable!(PlayerBucketEntityEvent, "PlayerBucketEntityEvent");

/// `PlayerSignOpenEvent`: a sign's editor is about to open for a player
/// (they placed it or clicked it); `front` is the side. Cancelling opens
/// nothing.
#[derive(Clone, Debug)]
pub struct PlayerSignOpenEvent {
    pub player: PlayerRef,
    pub sign: Block,
    pub front: bool,
    pub cancelled: bool,
}
cancellable!(PlayerSignOpenEvent, "PlayerSignOpenEvent");

/// `PlayerInteractAtEntityEvent`: a player right-clicked `entity` at
/// `position` (relative to the entity's feet). Cancelling drops the click.
#[derive(Clone, Debug)]
pub struct PlayerInteractAtEntityEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub hand: Hand,
    pub position: [f64; 3],
    pub cancelled: bool,
}
cancellable!(PlayerInteractAtEntityEvent, "PlayerInteractAtEntityEvent");

/// `MainHand`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MainHand {
    Left,
    Right,
}

/// `PlayerChangedMainHandEvent`: a player's options switched their main
/// hand to `main_hand`.
#[derive(Clone, Debug)]
pub struct PlayerChangedMainHandEvent {
    pub player: PlayerRef,
    pub main_hand: MainHand,
}
event!(PlayerChangedMainHandEvent, "PlayerChangedMainHandEvent");

/// `PlayerLocaleChangeEvent`: a player's language is now `locale`
/// (`en_us`...).
#[derive(Clone, Debug)]
pub struct PlayerLocaleChangeEvent {
    pub player: PlayerRef,
    pub locale: String,
}
event!(PlayerLocaleChangeEvent, "PlayerLocaleChangeEvent");

/// `PlayerCommandSendEvent`: the command tree a player was sent (as they
/// join, or as their permissions or the commands change), by its top-level
/// names. Removing names sends the tree without them.
#[derive(Clone, Debug)]
pub struct PlayerCommandSendEvent {
    pub player: PlayerRef,
    pub commands: Vec<String>,
}
event!(PlayerCommandSendEvent, "PlayerCommandSendEvent");

/// `PlayerVelocityEvent`: the server is about to set a player's velocity
/// (knockback, explosions, wind charges...). Changing it sends that one;
/// cancelling leaves their client's motion alone.
#[derive(Clone, Debug)]
pub struct PlayerVelocityEvent {
    pub player: PlayerRef,
    pub velocity: [f64; 3],
    pub cancelled: bool,
}
cancellable!(PlayerVelocityEvent, "PlayerVelocityEvent");

/// `PlayerEggThrowEvent`: a player's egg broke. `hatching` and
/// `num_hatches` start as the draw; `hatching_type` is what hatches
/// (`minecraft:chicken`). A plugin may change all three.
#[derive(Clone, Debug)]
pub struct PlayerEggThrowEvent {
    pub player: PlayerRef,
    pub egg: EntityRef,
    pub hatching: bool,
    pub num_hatches: u8,
    pub hatching_type: String,
}
event!(PlayerEggThrowEvent, "PlayerEggThrowEvent");

/// `PlayerExpCooldownChangeEvent.ChangeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpCooldownReason {
    PickupOrb,
    Plugin,
}

/// `PlayerExpCooldownChangeEvent`: a player's experience pickup cooldown
/// (ticks before the next orb) is about to be `new_cooldown`; a plugin may
/// change it.
#[derive(Clone, Debug)]
pub struct PlayerExpCooldownChangeEvent {
    pub player: PlayerRef,
    pub new_cooldown: u32,
    pub reason: ExpCooldownReason,
}
event!(PlayerExpCooldownChangeEvent, "PlayerExpCooldownChangeEvent");

/// `PlayerAnimationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlayerAnimationType {
    ArmSwing,
}

/// `PlayerAnimationEvent`: a player swung their arm (the swing packet).
/// Cancelling: the others don't see it, and the attack strength doesn't
/// reset.
#[derive(Clone, Debug)]
pub struct PlayerAnimationEvent {
    pub player: PlayerRef,
    pub animation: PlayerAnimationType,
    pub cancelled: bool,
}
cancellable!(PlayerAnimationEvent, "PlayerAnimationEvent");

/// `AsyncPlayerPreLoginEvent` (run in the tick here, before the player is
/// placed): `allowed` false turns them away with `kick_message`.
#[derive(Clone, Debug)]
pub struct AsyncPlayerPreLoginEvent {
    pub name: String,
    pub uuid: u128,
    /// The address they connect from.
    pub address: String,
    pub allowed: bool,
    pub kick_message: String,
}
event!(AsyncPlayerPreLoginEvent, "AsyncPlayerPreLoginEvent");

// ---------------------------------------------------------------------------
// Commands and messages.

/// `AsyncTabCompleteEvent` (run in the tick here): a player's client asks
/// for the completions of `buffer` (the line typed so far, with its slash).
/// `completions` start as the server's; a plugin may change them.
/// `handled`: the plugin's completions are the answer (`TabCompleteEvent`
/// is not fired). Cancelling sends none.
#[derive(Clone, Debug)]
pub struct AsyncTabCompleteEvent {
    pub sender: PlayerRef,
    pub buffer: String,
    pub completions: Vec<String>,
    pub handled: bool,
    pub cancelled: bool,
}
cancellable!(AsyncTabCompleteEvent, "AsyncTabCompleteEvent");

/// `TabCompleteEvent`: the completions of `buffer` about to go to the
/// player; a plugin may change them. Cancelling sends none.
#[derive(Clone, Debug)]
pub struct TabCompleteEvent {
    pub sender: PlayerRef,
    pub buffer: String,
    pub completions: Vec<String>,
    pub cancelled: bool,
}
cancellable!(TabCompleteEvent, "TabCompleteEvent");

/// `BroadcastMessageEvent`: a message to every player and the console (the
/// console's `say`). Changing `message` sends that; cancelling sends none.
#[derive(Clone, Debug)]
pub struct BroadcastMessageEvent {
    pub message: String,
    pub cancelled: bool,
}
cancellable!(BroadcastMessageEvent, "BroadcastMessageEvent");

// ---------------------------------------------------------------------------
// Chunks and worlds.

/// `ChunkLoadEvent`: the chunk `x`, `z` of `world` is loaded;
/// `is_new_chunk` when it was generated just now.
#[derive(Clone, Debug)]
pub struct ChunkLoadEvent {
    pub world: World,
    pub x: i32,
    pub z: i32,
    pub is_new_chunk: bool,
}
event!(ChunkLoadEvent, "ChunkLoadEvent");

/// `ChunkUnloadEvent`: the chunk `x`, `z` of `world` was unloaded (saved
/// first when it changed).
#[derive(Clone, Debug)]
pub struct ChunkUnloadEvent {
    pub world: World,
    pub x: i32,
    pub z: i32,
}
event!(ChunkUnloadEvent, "ChunkUnloadEvent");

/// `ChunkPopulateEvent`: the new chunk `x`, `z` of `world` has its
/// features, structures and first mobs.
#[derive(Clone, Debug)]
pub struct ChunkPopulateEvent {
    pub world: World,
    pub x: i32,
    pub z: i32,
}
event!(ChunkPopulateEvent, "ChunkPopulateEvent");

/// `WorldInitEvent`: the level `world` is opened, before it loads.
#[derive(Clone, Debug)]
pub struct WorldInitEvent {
    pub world: World,
}
event!(WorldInitEvent, "WorldInitEvent");

/// `WorldLoadEvent`: the level `world` is loaded.
#[derive(Clone, Debug)]
pub struct WorldLoadEvent {
    pub world: World,
}
event!(WorldLoadEvent, "WorldLoadEvent");

/// `WorldUnloadEvent`: the level `world`, empty for a while, is about to be
/// saved and closed. Cancelling keeps it open.
#[derive(Clone, Debug)]
pub struct WorldUnloadEvent {
    pub world: World,
    pub cancelled: bool,
}
cancellable!(WorldUnloadEvent, "WorldUnloadEvent");

// ---------------------------------------------------------------------------
// Growth, portals, the spawn and time.

/// `StructureGrowEvent`: bone meal grew a tree, a huge mushroom or a huge
/// fungus from `species` (the sapling's block) at `location`. `blocks` are
/// the blocks it set; removing some takes them back, cancelling takes the
/// whole growth back (the sapling stays).
#[derive(Clone, Debug)]
pub struct StructureGrowEvent {
    pub location: Location,
    pub species: String,
    pub bonemeal: bool,
    pub player: Option<PlayerRef>,
    pub blocks: Vec<Block>,
    pub cancelled: bool,
}
cancellable!(StructureGrowEvent, "StructureGrowEvent");

/// `PortalCreateEvent.CreateReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortalCreateReason {
    /// Fire lit an obsidian frame.
    Fire,
    /// A traveller arrived where no portal was.
    NetherPair,
}

/// `PortalCreateEvent`: a nether portal is about to be lit (`blocks` are
/// the portal blocks) or was made for an arrival (`blocks` are what it
/// set). Cancelling lights nothing or takes the new portal back.
#[derive(Clone, Debug)]
pub struct PortalCreateEvent {
    pub world: World,
    pub blocks: Vec<Block>,
    pub entity: Option<EntityRef>,
    pub reason: PortalCreateReason,
    pub cancelled: bool,
}
cancellable!(PortalCreateEvent, "PortalCreateEvent");

/// `SpawnChangeEvent`: the world spawn of `world` moved from
/// `previous_location` to `location` (`/setworldspawn`).
#[derive(Clone, Debug)]
pub struct SpawnChangeEvent {
    pub world: World,
    pub previous_location: Option<Location>,
    pub location: Location,
}
event!(SpawnChangeEvent, "SpawnChangeEvent");

/// `TimeSkipEvent.SkipReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeSkipReason {
    /// `/time set` or `/time add`.
    Command,
    /// The players slept through the night.
    NightSkip,
}

/// `TimeSkipEvent`: the time of day of `world` is about to jump by
/// `skip_amount` ticks (negative back). Changing it jumps that far;
/// cancelling keeps the time.
#[derive(Clone, Debug)]
pub struct TimeSkipEvent {
    pub world: World,
    pub skip_reason: TimeSkipReason,
    pub skip_amount: i64,
    pub cancelled: bool,
}
cancellable!(TimeSkipEvent, "TimeSkipEvent");

// ---------------------------------------------------------------------------
// Raids.

/// `org.bukkit.Raid`: what a raid event names.
#[derive(Clone, Debug, Default)]
pub struct RaidInfo {
    /// The raid's id in its level (0 for one not made yet).
    pub id: i32,
    pub center: BlockPos,
    /// `getBadOmenLevel`.
    pub omen_level: i32,
    pub spawned_groups: i32,
    pub total_groups: i32,
    pub started: bool,
}

/// `RaidTriggerEvent`: a player's Raid Omen is about to start a raid (or
/// make one stronger) at `raid.center`. Cancelling starts none (the omen is
/// used up).
#[derive(Clone, Debug)]
pub struct RaidTriggerEvent {
    pub raid: RaidInfo,
    pub world: World,
    pub player: Option<PlayerRef>,
    pub cancelled: bool,
}
cancellable!(RaidTriggerEvent, "RaidTriggerEvent");

/// `RaidSpawnWaveEvent`: a raid's wave came: its `raiders` and `leader`.
#[derive(Clone, Debug)]
pub struct RaidSpawnWaveEvent {
    pub raid: RaidInfo,
    pub world: World,
    pub leader: Option<EntityRef>,
    pub raiders: Vec<EntityRef>,
}
event!(RaidSpawnWaveEvent, "RaidSpawnWaveEvent");

/// `RaidFinishEvent`: a raid was won; `winners` are the heroes still here.
#[derive(Clone, Debug)]
pub struct RaidFinishEvent {
    pub raid: RaidInfo,
    pub world: World,
    pub winners: Vec<PlayerRef>,
}
event!(RaidFinishEvent, "RaidFinishEvent");

/// `RaidStopEvent.Reason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RaidStopReason {
    /// The difficulty is peaceful.
    Peace,
    /// It ran 48000 ticks.
    Timeout,
    /// It was won or lost and its celebration is over.
    Finished,
    /// No village left around it before its first wave.
    NotInVillage,
    /// `/raid stop`, the `raids` rule turned off, or anything else.
    Other,
}

/// `RaidStopEvent`: a raid ended.
#[derive(Clone, Debug)]
pub struct RaidStopEvent {
    pub raid: RaidInfo,
    pub world: World,
    pub reason: RaidStopReason,
}
event!(RaidStopEvent, "RaidStopEvent");

// ---------------------------------------------------------------------------
// Game events and loot.

/// `GenericGameEvent`: a vanilla game event (`minecraft:step`,
/// `minecraft:block_place`...: what sculk sensors and wardens hear) at
/// `location`, by `entity`. Changing `radius` is how far it reaches (16 at
/// most); cancelling: nothing hears it.
#[derive(Clone, Debug)]
pub struct GenericGameEvent {
    pub event: String,
    pub location: Location,
    pub entity: Option<EntityRef>,
    pub radius: i32,
    pub cancelled: bool,
}
cancellable!(GenericGameEvent, "GenericGameEvent");

/// `LootGenerateEvent`: `loot_table` rolled `loot` for a container
/// (`block`, opened for the first time by `player`) or a mob that died
/// (`entity`). Changing the list puts those instead; cancelling: none.
#[derive(Clone, Debug)]
pub struct LootGenerateEvent {
    pub world: World,
    pub loot_table: String,
    pub block: Option<Block>,
    pub entity: Option<EntityRef>,
    pub player: Option<PlayerRef>,
    pub loot: Vec<ItemStack>,
    pub cancelled: bool,
}
cancellable!(LootGenerateEvent, "LootGenerateEvent");
