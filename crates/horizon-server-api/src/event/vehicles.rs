//! More of Bukkit's events (group: vehicles). Uses the `event!` and
//! `cancellable!` macros of `event.rs`.
#[allow(unused_imports)]
use super::{BlockFace, Event, Hand};
#[allow(unused_imports)]
use crate::server::{Block, BlockPos, EntityRef, ItemStack, Location, PlayerRef, World};

// ---------------------------------------------------------------------------
// org.bukkit.event.hanging

/// `HangingPlaceEvent`: a player hangs `entity` (a painting, an item frame,
/// a glow item frame or a leash knot) on `block`'s `face` (None for a knot,
/// Bukkit's `SELF`). Cancelling hangs nothing; the item stays in hand.
#[derive(Clone, Debug)]
pub struct HangingPlaceEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub block: Block,
    pub face: Option<BlockFace>,
    /// The item it was hung from (None for a knot).
    pub item: Option<ItemStack>,
    pub cancelled: bool,
}
cancellable!(HangingPlaceEvent, "HangingPlaceEvent");

/// `HangingBreakEvent.RemoveCause`: why a hanging entity is coming down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemoveCause {
    /// An entity (a player) broke it.
    Entity,
    Explosion,
    /// A block was placed where it hangs.
    Obstruction,
    /// The block it hangs on is gone.
    Physics,
    Default,
}

/// `HangingBreakEvent`: `entity` (a painting or an item frame) is about to
/// break. `remover` is set when an entity broke it (then a
/// `HangingBreakByEntityEvent` fires first, with the same fields).
/// Cancelling keeps it hanging.
#[derive(Clone, Debug)]
pub struct HangingBreakEvent {
    pub entity: EntityRef,
    pub cause: RemoveCause,
    pub remover: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(HangingBreakEvent, "HangingBreakEvent");

/// `HangingBreakByEntityEvent`: `remover` is about to break `entity`.
/// Cancelling keeps it hanging (and the `HangingBreakEvent` after it starts
/// cancelled).
#[derive(Clone, Debug)]
pub struct HangingBreakByEntityEvent {
    pub entity: EntityRef,
    pub remover: EntityRef,
    pub cause: RemoveCause,
    pub cancelled: bool,
}
cancellable!(HangingBreakByEntityEvent, "HangingBreakByEntityEvent");

// ---------------------------------------------------------------------------
// org.bukkit.event.vehicle and entity placing

/// `VehicleCreateEvent`: a boat or a minecart comes into the world (a
/// player places it, a dispenser shoots it, `/summon`). Cancelling leaves it
/// out; a placed or dispensed item stays.
#[derive(Clone, Debug)]
pub struct VehicleCreateEvent {
    pub vehicle: EntityRef,
    pub cancelled: bool,
}
cancellable!(VehicleCreateEvent, "VehicleCreateEvent");

/// `VehicleDamageEvent`: `vehicle` is about to take `damage` (`attacker`:
/// the player who hit it; None for lava, fire, explosions). Changing
/// `damage` deals that instead; cancelling deals none (no wobble).
#[derive(Clone, Debug)]
pub struct VehicleDamageEvent {
    pub vehicle: EntityRef,
    pub attacker: Option<EntityRef>,
    pub damage: f64,
    pub cancelled: bool,
}
cancellable!(VehicleDamageEvent, "VehicleDamageEvent");

/// `VehicleDestroyEvent`: the damage breaks `vehicle`. Cancelling keeps it
/// (at the breaking point of its damage, as in CraftBukkit).
#[derive(Clone, Debug)]
pub struct VehicleDestroyEvent {
    pub vehicle: EntityRef,
    pub attacker: Option<EntityRef>,
    pub cancelled: bool,
}
cancellable!(VehicleDestroyEvent, "VehicleDestroyEvent");

/// `VehicleMoveEvent`: `vehicle` moved or turned this tick, from `from` to
/// `to` (seen once a tick).
#[derive(Clone, Debug)]
pub struct VehicleMoveEvent {
    pub vehicle: EntityRef,
    pub from: Location,
    pub to: Location,
}
event!(VehicleMoveEvent, "VehicleMoveEvent");

/// `VehicleUpdateEvent`: once a tick for each boat and minecart that ticks.
#[derive(Clone, Debug)]
pub struct VehicleUpdateEvent {
    pub vehicle: EntityRef,
}
event!(VehicleUpdateEvent, "VehicleUpdateEvent");

/// `VehicleBlockCollisionEvent`: `vehicle` ran sideways into `block`
/// (`velocity`: its motion before the bump, Paper's `getVelocity`).
#[derive(Clone, Debug)]
pub struct VehicleBlockCollisionEvent {
    pub vehicle: EntityRef,
    pub block: Block,
    pub velocity: [f64; 3],
}
event!(VehicleBlockCollisionEvent, "VehicleBlockCollisionEvent");

/// `VehicleEntityCollisionEvent`: `vehicle` touches `entity` (a mob, a
/// player, another boat or minecart). Cancelling, or `collision_cancelled`,
/// stops the push between them; `pickup_cancelled` keeps a boat from taking
/// the mob on board (it is pushed instead).
#[derive(Clone, Debug)]
pub struct VehicleEntityCollisionEvent {
    pub vehicle: EntityRef,
    pub entity: EntityRef,
    pub pickup_cancelled: bool,
    pub collision_cancelled: bool,
    pub cancelled: bool,
}
cancellable!(VehicleEntityCollisionEvent, "VehicleEntityCollisionEvent");

/// `EntityPlaceEvent`: a player places `entity` (a boat, a minecart, an
/// armor stand or an end crystal) against `block`'s `face`. Cancelling
/// places nothing; the item stays in hand.
#[derive(Clone, Debug)]
pub struct EntityPlaceEvent {
    pub player: PlayerRef,
    pub entity: EntityRef,
    pub block: Block,
    pub face: BlockFace,
    pub cancelled: bool,
}
cancellable!(EntityPlaceEvent, "EntityPlaceEvent");

/// `EntityUnleashEvent.UnleashReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnleashReason {
    /// The holder got too far away (over 12 blocks).
    Distance,
    /// The holder (a knot, a mob) is gone, or the mob died.
    HolderGone,
    /// A player took it off (after `PlayerUnleashEntityEvent`).
    PlayerUnleash,
    /// Anything else: shears, boarding a boat, a bucket...
    Unknown,
}

/// `EntityUnleashEvent`: the lead comes off `entity`. `drop_leash`
/// (Paper's `setDropLeash`): whether the lead falls as an item.
#[derive(Clone, Debug)]
pub struct EntityUnleashEvent {
    pub entity: EntityRef,
    pub reason: UnleashReason,
    pub drop_leash: bool,
}
event!(EntityUnleashEvent, "EntityUnleashEvent");
