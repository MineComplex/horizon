//! More of Bukkit's events (group: mobs). Uses the `event!` and
//! `cancellable!` macros of `event.rs`.
#[allow(unused_imports)]
use super::{BlockFace, Event, Hand};
#[allow(unused_imports)]
use crate::server::{Block, BlockPos, EntityRef, ItemStack, Location, PlayerRef, World};

/// `EntityKnockbackEvent.Cause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KnockbackCause {
    /// The push of any damage (`dealDefaultKnockback`).
    Damage,
    /// A melee hit's push, its own and the weapon's Knockback.
    EntityAttack,
    Explosion,
    /// A shield that took a hit pushes its holder back.
    ShieldBlock,
    /// A sword's sweep pushes those around the target.
    SweepAttack,
    Unknown,
}

/// `EntityKnockbackEvent`: a mob or a player is about to be pushed by
/// `knockback` (the velocity it adds; `force` the push's strength).
/// Changing `knockback` pushes them by that instead; cancelling: no push.
#[derive(Clone, Debug)]
pub struct EntityKnockbackEvent {
    pub entity: EntityRef,
    pub cause: KnockbackCause,
    pub force: f64,
    pub knockback: [f64; 3],
    pub cancelled: bool,
}
cancellable!(EntityKnockbackEvent, "EntityKnockbackEvent");

/// `EntityKnockbackByEntityEvent`: the same push when an entity caused it
/// (`source_entity`: the attacker, the exploding mob). Fired after
/// `EntityKnockbackEvent`, with its outcome.
#[derive(Clone, Debug)]
pub struct EntityKnockbackByEntityEvent {
    pub entity: EntityRef,
    pub source_entity: EntityRef,
    pub cause: KnockbackCause,
    pub force: f64,
    pub knockback: [f64; 3],
    pub cancelled: bool,
}
cancellable!(EntityKnockbackByEntityEvent, "EntityKnockbackByEntityEvent");

/// `EntityExhaustionEvent.ExhaustionReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExhaustionReason {
    Attack,
    BlockMined,
    HungerEffect,
    Jump,
    JumpSprint,
    Regen,
    Sprint,
    Swim,
    Walk,
    Unknown,
}

/// `EntityExhaustionEvent`: a player is about to gain `exhaustion` (food
/// exhaustion, 4 of which cost a saturation point or a food point).
/// Changing it adds that instead; cancelling adds none.
#[derive(Clone, Debug)]
pub struct EntityExhaustionEvent {
    pub entity: EntityRef,
    pub reason: ExhaustionReason,
    pub exhaustion: f32,
    pub cancelled: bool,
}
cancellable!(EntityExhaustionEvent, "EntityExhaustionEvent");

/// `EntityPoseChangeEvent`: a mob's or a player's pose changed (seen once a
/// tick). `pose` is Bukkit's `Pose` name: `STANDING`, `SNEAKING`,
/// `SWIMMING`, `FALL_FLYING`, `SLEEPING`, `SPIN_ATTACK`, `SITTING`,
/// `ROARING`, `SNIFFING`, `EMERGING`, `DIGGING`, `CROAKING`...
#[derive(Clone, Debug)]
pub struct EntityPoseChangeEvent {
    pub entity: EntityRef,
    pub pose: String,
}
event!(EntityPoseChangeEvent, "EntityPoseChangeEvent");

/// `PortalType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortalType {
    Nether,
    Ender,
}

/// `EntityPortalEnterEvent`: an entity (a mob, an item, a vehicle or a
/// player) is in the portal block at `location`, each tick it is.
/// Cancelling (Paper) keeps the portal from counting it this tick.
#[derive(Clone, Debug)]
pub struct EntityPortalEnterEvent {
    pub entity: EntityRef,
    pub location: Location,
    pub portal_type: PortalType,
    pub cancelled: bool,
}
cancellable!(EntityPortalEnterEvent, "EntityPortalEnterEvent");

/// `EntityPortalExitEvent`: a mob, an item or a vehicle comes out of a
/// portal at `to` (its id is new: it is not in the world yet). `before` is
/// its motion as it went in, `after` the motion it comes out with; changing
/// `to` or `after` puts it there or sends it that way.
#[derive(Clone, Debug)]
pub struct EntityPortalExitEvent {
    pub entity: EntityRef,
    pub to: Location,
    pub before: [f64; 3],
    pub after: [f64; 3],
}
event!(EntityPortalExitEvent, "EntityPortalExitEvent");

/// `EntityRemoveEvent.Cause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityRemoveCause {
    /// A mob's body went (twenty ticks after it died).
    Death,
    /// Too far from every player, or not allowed in peaceful; an item that
    /// lay for five minutes.
    Despawn,
    /// Taken out by the game otherwise.
    Discard,
    /// Its chunk unloaded (saved with it).
    Unload,
    /// It turned into another mob.
    Transformation,
    /// A bee went into its hive.
    EnterBlock,
    /// It went through a portal to another level.
    ChangedDimension,
}

/// `EntityRemoveEvent`: a mob or a dropped item left the world (seen once a
/// tick; `entity` as it was last).
#[derive(Clone, Debug)]
pub struct EntityRemoveEvent {
    pub entity: EntityRef,
    pub cause: EntityRemoveCause,
}
event!(EntityRemoveEvent, "EntityRemoveEvent");

/// `EnderDragon.Phase`, by the vanilla phase ids 0 to 10.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragonPhase {
    Circling,
    Strafing,
    FlyToPortal,
    LandOnPortal,
    LeavePortal,
    BreathAttack,
    SearchForBreathAttackTarget,
    RoarBeforeAttack,
    ChargePlayer,
    Dying,
    Hover,
}
impl DragonPhase {
    /// The phase of a vanilla `EnderDragonPhase` id.
    pub fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Strafing,
            2 => Self::FlyToPortal,
            3 => Self::LandOnPortal,
            4 => Self::LeavePortal,
            5 => Self::BreathAttack,
            6 => Self::SearchForBreathAttackTarget,
            7 => Self::RoarBeforeAttack,
            8 => Self::ChargePlayer,
            9 => Self::Dying,
            10 => Self::Hover,
            _ => Self::Circling,
        }
    }
    /// Its vanilla id.
    pub fn id(self) -> i32 {
        self as i32
    }
}

/// `EnderDragonChangePhaseEvent`: the dragon is about to go from
/// `current_phase` to `new_phase` (seen at the end of its tick). Changing
/// `new_phase` starts that one instead; cancelling keeps the current one.
#[derive(Clone, Debug)]
pub struct EnderDragonChangePhaseEvent {
    pub entity: EntityRef,
    pub current_phase: Option<DragonPhase>,
    pub new_phase: DragonPhase,
    pub cancelled: bool,
}
cancellable!(EnderDragonChangePhaseEvent, "EnderDragonChangePhaseEvent");

/// `PigZombieAngerEvent`: a zombified piglin got angry (seen once a tick),
/// at `target` if it has one, for `new_anger` ticks. Changing it keeps it
/// angry that long; cancelling calms it (its old anger and target back).
#[derive(Clone, Debug)]
pub struct PigZombieAngerEvent {
    pub entity: EntityRef,
    pub target: Option<EntityRef>,
    pub new_anger: i32,
    pub cancelled: bool,
}
cancellable!(PigZombieAngerEvent, "PigZombieAngerEvent");

/// `VillagerCareerChangeEvent.ChangeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CareerChangeReason {
    /// It took a job site.
    Employed,
    /// It lost its job site before it traded.
    LosingJob,
}

/// `VillagerCareerChangeEvent`: a villager's profession changed to
/// `profession` (`minecraft:farmer`...; `minecraft:none` when it lost its
/// job). Seen once a tick; cancelling gives it its old profession back.
#[derive(Clone, Debug)]
pub struct VillagerCareerChangeEvent {
    pub entity: EntityRef,
    pub profession: String,
    pub reason: CareerChangeReason,
    pub cancelled: bool,
}
cancellable!(VillagerCareerChangeEvent, "VillagerCareerChangeEvent");

/// `StriderTemperatureChangeEvent`: a strider started (`shivering`) or
/// stopped being cold, out of or back in lava (seen once a tick).
/// Cancelling keeps it as it was.
#[derive(Clone, Debug)]
pub struct StriderTemperatureChangeEvent {
    pub entity: EntityRef,
    pub shivering: bool,
    pub cancelled: bool,
}
cancellable!(
    StriderTemperatureChangeEvent,
    "StriderTemperatureChangeEvent"
);

/// `EntitiesLoadEvent`: the saved entities of chunk (`chunk_x`, `chunk_z`)
/// came into the world.
#[derive(Clone, Debug)]
pub struct EntitiesLoadEvent {
    pub world: World,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub entities: Vec<EntityRef>,
}
event!(EntitiesLoadEvent, "EntitiesLoadEvent");

/// `EntitiesUnloadEvent`: the entities of chunk (`chunk_x`, `chunk_z`) are
/// about to be saved and leave the world with it.
#[derive(Clone, Debug)]
pub struct EntitiesUnloadEvent {
    pub world: World,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub entities: Vec<EntityRef>,
}
event!(EntitiesUnloadEvent, "EntitiesUnloadEvent");

/// `EntityEnterBlockEvent`: a bee is about to go into the hive or nest
/// `block`. Cancelling keeps it out.
#[derive(Clone, Debug)]
pub struct EntityEnterBlockEvent {
    pub entity: EntityRef,
    pub block: Block,
    pub cancelled: bool,
}
cancellable!(EntityEnterBlockEvent, "EntityEnterBlockEvent");

/// `EntityBlockFormEvent`: an entity is about to form `new_state` at
/// `block`: a snow golem's snow trail, a player's Frost Walker ice.
/// Cancelling leaves the block as it is.
#[derive(Clone, Debug)]
pub struct EntityBlockFormEvent {
    pub entity: EntityRef,
    pub block: Block,
    pub new_state: String,
    pub cancelled: bool,
}
cancellable!(EntityBlockFormEvent, "EntityBlockFormEvent");

/// `EntitySpawnEvent`: an entity that isn't a mob (mobs:
/// `CreatureSpawnEvent`; dropped items: `ItemSpawnEvent`) is about to come
/// into the world at `location`: an experience orb, primed TNT, a falling
/// block, an area effect cloud, a lightning bolt. `entity.id` is 0 when it
/// has no id yet. Cancelling keeps it out.
#[derive(Clone, Debug)]
pub struct EntitySpawnEvent {
    pub entity: EntityRef,
    pub location: Location,
    pub cancelled: bool,
}
cancellable!(EntitySpawnEvent, "EntitySpawnEvent");

/// `EntityTargetEvent.TargetReason`, as far as the server tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetReason {
    /// The old target died or left.
    TargetDied,
    /// The mob forgot its target.
    ForgotTarget,
    /// It turned on the entity that hurt it.
    TargetAttackedEntity,
    /// An angry zombified piglin.
    PigZombieTarget,
    /// It chose the nearest player.
    ClosestPlayer,
    /// It chose another entity.
    ClosestEntity,
}

/// `EntityTargetEvent`: a mob's target changed to `target` (None: it has
/// none now), seen once a tick, before `EntityTargetLivingEntityEvent`.
/// Changing `target` (to None, a mob or the player it sees) sets that one;
/// cancelling puts the old target back.
#[derive(Clone, Debug)]
pub struct EntityTargetEvent {
    pub entity: EntityRef,
    pub target: Option<EntityRef>,
    pub reason: TargetReason,
    pub cancelled: bool,
}
cancellable!(EntityTargetEvent, "EntityTargetEvent");

/// `EntityCombustByBlockEvent`: a mob or a player caught fire from
/// `combuster` (fire, soul fire or lava), for `duration` seconds. Seen once
/// a tick, before `EntityCombustEvent`; changing `duration` burns them that
/// long, cancelling puts the fire out.
#[derive(Clone, Debug)]
pub struct EntityCombustByBlockEvent {
    pub entity: EntityRef,
    pub combuster: Option<Block>,
    pub duration: f32,
    pub cancelled: bool,
}
cancellable!(EntityCombustByBlockEvent, "EntityCombustByBlockEvent");

/// `EntityCombustByEntityEvent`: a mob or a player caught fire from
/// `combuster`: a burning arrow, a blaze's fireball, a player's Fire Aspect
/// hit, a lightning bolt. Seen once a tick, before `EntityCombustEvent`;
/// changing `duration` burns them that long, cancelling puts the fire out.
#[derive(Clone, Debug)]
pub struct EntityCombustByEntityEvent {
    pub entity: EntityRef,
    pub combuster: EntityRef,
    pub duration: f32,
    pub cancelled: bool,
}
cancellable!(EntityCombustByEntityEvent, "EntityCombustByEntityEvent");

/// `EntityDamageByBlockEvent`: a mob or a player is about to be hurt by the
/// block `damager`: a cactus, a magma block, a sweet berry bush, a campfire,
/// fire, lava, a stalagmite (None when the block wasn't found). `cause` is
/// the damage type, as in `EntityDamageEvent`, which follows with this
/// event's outcome. Changing `damage` changes the hit; cancelling stops it.
#[derive(Clone, Debug)]
pub struct EntityDamageByBlockEvent {
    pub entity: EntityRef,
    pub damager: Option<Block>,
    pub cause: String,
    pub damage: f64,
    pub cancelled: bool,
}
cancellable!(EntityDamageByBlockEvent, "EntityDamageByBlockEvent");
