//! More of Bukkit's events (group: blocks). Uses the `event!` and
//! `cancellable!` macros of `event.rs`.
use super::{BlockFace, Event};
use crate::server::{Block, BlockPos, EntityRef, ItemStack, PlayerRef, World};

/// `BlockRedstoneEvent`: the power of a redstone component at `block`
/// (wire, lever, button, torch, lamp) went from `old_current` to
/// `new_current` (0 to 15). Heard after the change: changing `new_current`
/// changes nothing here.
#[derive(Clone, Debug)]
pub struct BlockRedstoneEvent {
    pub block: Block,
    pub old_current: i32,
    pub new_current: i32,
}
event!(BlockRedstoneEvent, "BlockRedstoneEvent");

/// `BlockIgniteEvent.IgniteCause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IgniteCause {
    Lava,
    FlintAndSteel,
    Spread,
    Lightning,
    Fireball,
    EnderCrystal,
    Explosion,
    Arrow,
}

/// `BlockIgniteEvent`: `block` is about to be set on fire (or a campfire or
/// candle lit by flint and steel or a fire charge), by `igniting_entity`
/// (the player, the fireball's shooter...) or from `igniting_block` (the
/// lava, the fire it spreads from). Cancelling keeps the fire out.
#[derive(Clone, Debug)]
pub struct BlockIgniteEvent {
    pub block: Block,
    pub cause: IgniteCause,
    pub igniting_entity: Option<EntityRef>,
    pub igniting_block: Option<Block>,
    pub cancelled: bool,
}
cancellable!(BlockIgniteEvent, "BlockIgniteEvent");
impl BlockIgniteEvent {
    /// `getPlayer`: the player who lit it, if one did.
    pub fn player(&self) -> Option<&PlayerRef> {
        self.igniting_entity.as_ref()?.player.as_ref()
    }
}

/// `BlockExplodeEvent`: a block explodes (a bed or a respawn anchor used
/// where it can't be), about to destroy `blocks`. `block` is the block as it
/// was before it went (`getExplodedBlockState`). Removing blocks spares
/// them; cancelling stops the explosion's damage to the world and the
/// entities.
#[derive(Clone, Debug)]
pub struct BlockExplodeEvent {
    pub block: Block,
    pub blocks: Vec<BlockPos>,
    pub cancelled: bool,
}
cancellable!(BlockExplodeEvent, "BlockExplodeEvent");

/// `NotePlayEvent`: the note block at `block` is about to play `note` (0 to
/// 24) on `instrument` (`harp`, `bass`, `zombie`...). Cancelling keeps it
/// silent; the note and instrument are what the block holds.
#[derive(Clone, Debug)]
pub struct NotePlayEvent {
    pub block: Block,
    pub instrument: String,
    pub note: u8,
    pub cancelled: bool,
}
cancellable!(NotePlayEvent, "NotePlayEvent");

/// `BellRingEvent`: the bell at `block` is about to ring, struck towards
/// `direction`, by `entity` when one rang it (the player whose turn it is).
/// Cancelling keeps it silent.
#[derive(Clone, Debug)]
pub struct BellRingEvent {
    pub block: Block,
    pub direction: BlockFace,
    pub entity: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(BellRingEvent, "BellRingEvent");

/// `TNTPrimeEvent.PrimeCause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrimeCause {
    Fire,
    Redstone,
    Player,
    Explosion,
    Projectile,
    BlockBreak,
    Dispenser,
}

/// `TNTPrimeEvent`: the TNT at `block` is about to be lit, by
/// `priming_entity` (a player, the projectile's shooter) or `priming_block`
/// (the dispenser). A TNT item a dispenser fires names the block in front of
/// it. Cancelling keeps it from being lit: TNT lit by a player or redstone,
/// or fired by a dispenser, stays a block (or an item); TNT burnt, broken or
/// blown up goes all the same.
#[derive(Clone, Debug)]
pub struct TntPrimeEvent {
    pub block: Block,
    pub cause: PrimeCause,
    pub priming_entity: Option<EntityRef>,
    pub priming_block: Option<Block>,
    pub cancelled: bool,
}
cancellable!(TntPrimeEvent, "TNTPrimeEvent");

/// `SpongeAbsorbEvent`: the sponge at `block` is about to soak up the water
/// of `blocks`. Removing blocks leaves their water; cancelling soaks up none
/// and the sponge stays dry.
#[derive(Clone, Debug)]
pub struct SpongeAbsorbEvent {
    pub block: Block,
    pub blocks: Vec<BlockPos>,
    pub cancelled: bool,
}
cancellable!(SpongeAbsorbEvent, "SpongeAbsorbEvent");

/// `BlockMultiPlaceEvent`: a player placed a block of more than one part (a
/// bed, a door, a tall flower...), after its `BlockPlaceEvent`: `blocks` as
/// placed, `replaced` what was there. Cancelling takes them all back.
#[derive(Clone, Debug)]
pub struct BlockMultiPlaceEvent {
    pub player: PlayerRef,
    pub world: World,
    pub blocks: Vec<Block>,
    pub replaced: Vec<Block>,
    /// The item it was placed from.
    pub item: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(BlockMultiPlaceEvent, "BlockMultiPlaceEvent");

/// `CauldronLevelChangeEvent.ChangeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CauldronChangeReason {
    BucketFill,
    BucketEmpty,
    BottleFill,
    BottleEmpty,
    BannerWash,
    ArmorWash,
    ShulkerWash,
    Extinguish,
    NaturalFill,
    Unknown,
}

/// `CauldronLevelChangeEvent`: the cauldron at `block` is about to become
/// `new_state` (`minecraft:water_cauldron[level=2]`, `minecraft:cauldron`...),
/// by `entity` when one changes it. Changing `new_state` (to another
/// cauldron) sets that instead; cancelling keeps it as it is (and the item
/// in the player's hand).
#[derive(Clone, Debug)]
pub struct CauldronLevelChangeEvent {
    pub block: Block,
    pub entity: Option<EntityRef>,
    pub reason: CauldronChangeReason,
    pub new_state: String,
    pub cancelled: bool,
}
cancellable!(CauldronLevelChangeEvent, "CauldronLevelChangeEvent");

/// `MoistureChangeEvent`: the farmland at `block` is about to dry a little
/// or get wet: `new_state` (`minecraft:farmland[moisture=7]`). Changing
/// `new_state` (to other farmland) sets that instead; cancelling keeps it.
#[derive(Clone, Debug)]
pub struct MoistureChangeEvent {
    pub block: Block,
    pub new_state: String,
    pub cancelled: bool,
}
cancellable!(MoistureChangeEvent, "MoistureChangeEvent");

/// `FluidLevelChangeEvent`: the flowing water or lava at `block` is about to
/// change level: `new_state` (`minecraft:water[level=3]`, or air when it
/// dries up). Changing `new_state` (to the same fluid or air) sets that
/// instead; cancelling keeps it as it is, and it doesn't spread this tick.
#[derive(Clone, Debug)]
pub struct FluidLevelChangeEvent {
    pub block: Block,
    pub new_state: String,
    pub cancelled: bool,
}
cancellable!(FluidLevelChangeEvent, "FluidLevelChangeEvent");

/// `BlockShearEntityEvent`: the dispenser at `block` is about to shear
/// `entity` (a sheep, a snow golem, a mooshroom...) with `tool`. Cancelling
/// leaves it as it is (the dispenser tries the next one).
#[derive(Clone, Debug)]
pub struct BlockShearEntityEvent {
    pub block: Block,
    pub entity: EntityRef,
    pub tool: ItemStack,
    pub cancelled: bool,
}
cancellable!(BlockShearEntityEvent, "BlockShearEntityEvent");

/// `BlockCookEvent`: the campfire at `block` finished cooking `source` into
/// `result`, about to drop it. Changing `result` drops that instead;
/// cancelling keeps the item on the fire (it is asked again next tick).
#[derive(Clone, Debug)]
pub struct BlockCookEvent {
    pub block: Block,
    pub source: ItemStack,
    pub result: ItemStack,
    pub cancelled: bool,
}
cancellable!(BlockCookEvent, "BlockCookEvent");
