//! More of Bukkit's events (group: entities). Uses the `event!` and
//! `cancellable!` macros of `event.rs`.
#[allow(unused_imports)]
use super::{BlockFace, Event, Hand};
#[allow(unused_imports)]
use crate::server::{Block, BlockPos, EntityRef, ItemStack, Location, PlayerRef, World};

/// `EntityPickupItemEvent`: a mob (a piglin, a villager, an allay, a panda,
/// a sulfur cube) is about to pick up the dropped `item` lying at
/// `location`. Cancelling leaves it on the ground.
#[derive(Clone, Debug)]
pub struct EntityPickupItemEvent {
    pub entity: EntityRef,
    pub item: ItemStack,
    pub location: Location,
    /// What stays on the ground once it took its share.
    pub remaining: i32,
    pub cancelled: bool,
}
cancellable!(EntityPickupItemEvent, "EntityPickupItemEvent");

/// `EntityChangeBlockEvent`: a mob is about to change `block` to `to` (an
/// enderman taking or placing one, a sheep eating grass, a silverfish, a
/// ravager or a wither breaking blocks, a villager farming...), or a falling
/// block lands. Cancelling leaves the block as it is.
#[derive(Clone, Debug)]
pub struct EntityChangeBlockEvent {
    pub entity: EntityRef,
    pub block: Block,
    /// The block state it would become.
    pub to: String,
    pub cancelled: bool,
}
cancellable!(EntityChangeBlockEvent, "EntityChangeBlockEvent");

/// `EntityBreakDoorEvent`: a zombie or a vindicator is about to break the
/// door `block`. Cancelling keeps the door.
#[derive(Clone, Debug)]
pub struct EntityBreakDoorEvent {
    pub entity: EntityRef,
    pub block: Block,
    pub cancelled: bool,
}
cancellable!(EntityBreakDoorEvent, "EntityBreakDoorEvent");

/// `EntityInteractEvent`: a mob trampled the farmland `block` (seen as it
/// turns to dirt). Cancelling keeps the farmland.
#[derive(Clone, Debug)]
pub struct EntityInteractEvent {
    pub entity: EntityRef,
    pub block: Block,
    pub cancelled: bool,
}
cancellable!(EntityInteractEvent, "EntityInteractEvent");

/// `EntityCombustEvent`: a mob or a player caught fire (seen once a tick),
/// burning for `duration` seconds. Changing it burns them that long;
/// cancelling puts the fire out.
#[derive(Clone, Debug)]
pub struct EntityCombustEvent {
    pub entity: EntityRef,
    pub duration: f32,
    pub cancelled: bool,
}
cancellable!(EntityCombustEvent, "EntityCombustEvent");

/// `ExplosionPrimeEvent`: a creeper (or another mob) or a primed TNT is about
/// to explode with `radius`. Changing it changes the blast; `fire` sets the
/// blocks around alight (TNT only). Cancelling: no blast, and a creeper lives on.
#[derive(Clone, Debug)]
pub struct ExplosionPrimeEvent {
    pub entity: EntityRef,
    pub radius: f32,
    pub fire: bool,
    pub cancelled: bool,
}
cancellable!(ExplosionPrimeEvent, "ExplosionPrimeEvent");

/// `EntityShootBowEvent`: a mob or a player shoots `bow` (a bow or a
/// crossbow; None for a mob's own) with `force` (0..1, the bow's draw).
/// Cancelling shoots nothing and uses no ammo.
#[derive(Clone, Debug)]
pub struct EntityShootBowEvent {
    pub entity: EntityRef,
    pub bow: Option<ItemStack>,
    /// The ammunition it would use.
    pub consumable: Option<ItemStack>,
    /// The type of the projectile (`minecraft:arrow`...).
    pub projectile: String,
    pub force: f32,
    pub hand: Hand,
    pub cancelled: bool,
}
cancellable!(EntityShootBowEvent, "EntityShootBowEvent");

/// `PotionEffect`: an effect as plugins see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PotionEffect {
    /// `minecraft:speed`...
    pub kind: String,
    /// Ticks; -1 lasts forever.
    pub duration: i32,
    pub amplifier: i32,
}

/// `EntityPotionEffectEvent.Action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PotionAction {
    Added,
    Changed,
    Cleared,
    Removed,
}

/// `EntityPotionEffectEvent.Cause`, as far as the server tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PotionCause {
    /// The effect ran out.
    Expiration,
    Unknown,
}

/// `EntityPotionEffectEvent`: an effect was added to, changed on or removed
/// from a mob or a player (seen once a tick). Cancelling puts it back as it was.
#[derive(Clone, Debug)]
pub struct EntityPotionEffectEvent {
    pub entity: EntityRef,
    pub old_effect: Option<PotionEffect>,
    pub new_effect: Option<PotionEffect>,
    pub action: PotionAction,
    pub cause: PotionCause,
    pub cancelled: bool,
}
cancellable!(EntityPotionEffectEvent, "EntityPotionEffectEvent");

/// `EntityAirChangeEvent`: a mob's or a player's air changed to `amount`
/// (seen once a tick). Changing it sets their air; cancelling keeps the old.
#[derive(Clone, Debug)]
pub struct EntityAirChangeEvent {
    pub entity: EntityRef,
    pub amount: i32,
    pub cancelled: bool,
}
cancellable!(EntityAirChangeEvent, "EntityAirChangeEvent");

/// `EntityTeleportEvent`: an enderman or a shulker teleported itself from
/// `from` to `to`. Changing `to` puts it there; cancelling puts it back.
#[derive(Clone, Debug)]
pub struct EntityTeleportEvent {
    pub entity: EntityRef,
    pub from: Location,
    pub to: Location,
    pub cancelled: bool,
}
cancellable!(EntityTeleportEvent, "EntityTeleportEvent");

/// `EntityTransformEvent.TransformReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransformReason {
    /// A zombie villager cured.
    Cured,
    /// A zombie or a husk under water.
    Drowned,
    /// A skeleton in powder snow.
    Frozen,
    /// A villager killed by a zombie.
    Infection,
    /// A pig or a villager struck by lightning.
    Lightning,
    /// A tadpole grown up.
    Metamorphosis,
    /// A piglin or a hoglin out of the Nether.
    PiglinZombified,
    Unknown,
}

/// `EntityTransformEvent`: a mob is about to turn into a `transformed_kind`
/// (`minecraft:villager`...). Cancelling keeps it as it is.
#[derive(Clone, Debug)]
pub struct EntityTransformEvent {
    pub entity: EntityRef,
    pub transformed_kind: String,
    pub reason: TransformReason,
    pub cancelled: bool,
}
cancellable!(EntityTransformEvent, "EntityTransformEvent");

/// `EntityResurrectEvent`: a totem of undying in `hand` is about to save a
/// player from death. Cancelling keeps the totem and the player dies.
#[derive(Clone, Debug)]
pub struct EntityResurrectEvent {
    pub entity: EntityRef,
    pub hand: Option<Hand>,
    pub cancelled: bool,
}
cancellable!(EntityResurrectEvent, "EntityResurrectEvent");

/// `EntityToggleGlideEvent`: a player or a mob started or stopped gliding
/// with elytra (seen once a tick).
#[derive(Clone, Debug)]
pub struct EntityToggleGlideEvent {
    pub entity: EntityRef,
    pub gliding: bool,
}
event!(EntityToggleGlideEvent, "EntityToggleGlideEvent");

/// `EntityToggleSwimEvent`: a player started or stopped swimming (seen once
/// a tick).
#[derive(Clone, Debug)]
pub struct EntityToggleSwimEvent {
    pub entity: EntityRef,
    pub swimming: bool,
}
event!(EntityToggleSwimEvent, "EntityToggleSwimEvent");

/// `EntityMountEvent`: `entity` got on `mount` (a mob on a mob or a boat, a
/// player on anything; seen once a tick). Cancelling gets a mob off again
/// (players: see `VehicleEnterEvent`, they stay on).
#[derive(Clone, Debug)]
pub struct EntityMountEvent {
    pub entity: EntityRef,
    pub mount: EntityRef,
    pub cancelled: bool,
}
cancellable!(EntityMountEvent, "EntityMountEvent");

/// `EntityDismountEvent`: `entity` got off `dismounted` (seen once a tick).
#[derive(Clone, Debug)]
pub struct EntityDismountEvent {
    pub entity: EntityRef,
    pub dismounted: EntityRef,
}
event!(EntityDismountEvent, "EntityDismountEvent");

/// `CreeperPowerEvent.PowerCause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PowerCause {
    Lightning,
}

/// `CreeperPowerEvent`: a creeper is about to be charged. Cancelling keeps
/// it plain (it still burns and takes the bolt's damage).
#[derive(Clone, Debug)]
pub struct CreeperPowerEvent {
    pub entity: EntityRef,
    pub cause: PowerCause,
    pub cancelled: bool,
}
cancellable!(CreeperPowerEvent, "CreeperPowerEvent");

/// `PigZapEvent`: lightning is about to turn a pig into a zombified piglin.
/// Cancelling keeps the pig (it burns and is hurt instead).
#[derive(Clone, Debug)]
pub struct PigZapEvent {
    pub entity: EntityRef,
    pub cancelled: bool,
}
cancellable!(PigZapEvent, "PigZapEvent");

/// `SheepDyeWoolEvent`: a sheep's wool turned `color` (`white`, `red`...;
/// seen once a tick: a dye, an evoker's wololo). Changing it dyes it that
/// colour instead; cancelling keeps the old one.
#[derive(Clone, Debug)]
pub struct SheepDyeWoolEvent {
    pub entity: EntityRef,
    pub color: String,
    pub cancelled: bool,
}
cancellable!(SheepDyeWoolEvent, "SheepDyeWoolEvent");

/// `SheepRegrowWoolEvent`: a sheared sheep grew its wool back (seen once a
/// tick). Cancelling keeps it sheared.
#[derive(Clone, Debug)]
pub struct SheepRegrowWoolEvent {
    pub entity: EntityRef,
    pub cancelled: bool,
}
cancellable!(SheepRegrowWoolEvent, "SheepRegrowWoolEvent");

/// `SlimeSplitEvent`: a dead slime or magma cube is about to split into
/// `count` smaller ones. Changing it changes how many; cancelling makes none.
#[derive(Clone, Debug)]
pub struct SlimeSplitEvent {
    pub entity: EntityRef,
    pub count: i32,
    pub cancelled: bool,
}
cancellable!(SlimeSplitEvent, "SlimeSplitEvent");

/// `MerchantRecipe`: one of a villager's or wandering trader's offers.
#[derive(Clone, Debug, PartialEq)]
pub struct MerchantRecipe {
    pub result: ItemStack,
    /// What it costs: one or two stacks.
    pub ingredients: Vec<ItemStack>,
    pub uses: i32,
    pub max_uses: i32,
    /// The experience the villager gets from it.
    pub villager_experience: i32,
}

/// `VillagerAcquireTradeEvent`: a villager or a wandering trader got a new
/// offer (seen once a tick). Changing the recipe's `result` or `max_uses`
/// changes the offer; cancelling takes it away.
#[derive(Clone, Debug)]
pub struct VillagerAcquireTradeEvent {
    pub entity: EntityRef,
    pub recipe: MerchantRecipe,
    pub cancelled: bool,
}
cancellable!(VillagerAcquireTradeEvent, "VillagerAcquireTradeEvent");

/// `VillagerReplenishTradeEvent`: a villager restocked an offer that had been
/// used (seen once a tick). Cancelling leaves it used up.
#[derive(Clone, Debug)]
pub struct VillagerReplenishTradeEvent {
    pub entity: EntityRef,
    pub recipe: MerchantRecipe,
    pub cancelled: bool,
}
cancellable!(VillagerReplenishTradeEvent, "VillagerReplenishTradeEvent");

/// `PiglinBarterEvent`: a piglin is done admiring the gold `input` and is
/// about to throw `outcome`. Changing the outcome throws that instead;
/// cancelling throws nothing.
#[derive(Clone, Debug)]
pub struct PiglinBarterEvent {
    pub entity: EntityRef,
    pub input: ItemStack,
    pub outcome: Vec<ItemStack>,
    pub cancelled: bool,
}
cancellable!(PiglinBarterEvent, "PiglinBarterEvent");

/// `ItemDespawnEvent`: a dropped item has lain five minutes and is about to
/// go. Cancelling gives it five more.
#[derive(Clone, Debug)]
pub struct ItemDespawnEvent {
    pub entity: EntityRef,
    pub item: ItemStack,
    pub location: Location,
    pub cancelled: bool,
}
cancellable!(ItemDespawnEvent, "ItemDespawnEvent");

/// `ItemMergeEvent`: the dropped item `entity` is about to merge with
/// `target`. Cancelling keeps them apart.
#[derive(Clone, Debug)]
pub struct ItemMergeEvent {
    pub entity: EntityRef,
    pub item: ItemStack,
    pub target: EntityRef,
    pub target_item: ItemStack,
    pub cancelled: bool,
}
cancellable!(ItemMergeEvent, "ItemMergeEvent");

/// `PotionSplashEvent`: a splash potion broke at `location`. `affected` are
/// the mobs and players it reaches, with their intensity (0..1); removing a
/// mob spares it. Cancelling gives nobody anything.
#[derive(Clone, Debug)]
pub struct PotionSplashEvent {
    pub potion: ItemStack,
    pub location: Location,
    pub affected: Vec<(EntityRef, f64)>,
    pub cancelled: bool,
}
cancellable!(PotionSplashEvent, "PotionSplashEvent");

/// `LingeringPotionSplashEvent`: a lingering potion broke at `location` and
/// is about to leave its cloud. Cancelling leaves none.
#[derive(Clone, Debug)]
pub struct LingeringPotionSplashEvent {
    pub potion: ItemStack,
    pub location: Location,
    pub cancelled: bool,
}
cancellable!(LingeringPotionSplashEvent, "LingeringPotionSplashEvent");

/// `AreaEffectCloudApplyEvent`: a lingering potion's cloud at `location` is
/// about to affect `affected`. Removing one spares it; cancelling spares all.
#[derive(Clone, Debug)]
pub struct AreaEffectCloudApplyEvent {
    pub location: Location,
    pub radius: f32,
    pub affected: Vec<EntityRef>,
    pub cancelled: bool,
}
cancellable!(AreaEffectCloudApplyEvent, "AreaEffectCloudApplyEvent");

/// `FireworkExplodeEvent`: a firework rocket is about to burst. Cancelling:
/// it goes without bursting or hurting anyone.
#[derive(Clone, Debug)]
pub struct FireworkExplodeEvent {
    pub entity: EntityRef,
    pub cancelled: bool,
}
cancellable!(FireworkExplodeEvent, "FireworkExplodeEvent");

/// `HorseJumpEvent`: a horse (donkey, mule, camel...) a player rides is about
/// to jump with `power` (0..1). Changing it changes the jump; cancelling
/// makes none.
#[derive(Clone, Debug)]
pub struct HorseJumpEvent {
    pub entity: EntityRef,
    pub power: f32,
    pub cancelled: bool,
}
cancellable!(HorseJumpEvent, "HorseJumpEvent");

/// `EntityEnterLoveModeEvent`: an animal fell in love for `ticks_in_love`
/// (seen once a tick; `fed_by_player`: a player's food did it). Changing the
/// ticks changes how long; cancelling takes it out of love.
#[derive(Clone, Debug)]
pub struct EntityEnterLoveModeEvent {
    pub entity: EntityRef,
    pub fed_by_player: bool,
    pub ticks_in_love: i32,
    pub cancelled: bool,
}
cancellable!(EntityEnterLoveModeEvent, "EntityEnterLoveModeEvent");

/// `BatToggleSleepEvent`: a bat woke up (`awake`) or hung itself up to rest
/// (seen once a tick). Cancelling keeps it as it was.
#[derive(Clone, Debug)]
pub struct BatToggleSleepEvent {
    pub entity: EntityRef,
    pub awake: bool,
    pub cancelled: bool,
}
cancellable!(BatToggleSleepEvent, "BatToggleSleepEvent");

/// `EntityPortalEvent`: a mob, a dropped item, a boat or a minecart is about
/// to go through the nether or end portal it stands in at `from`.
/// Cancelling keeps it here.
#[derive(Clone, Debug)]
pub struct EntityPortalEvent {
    pub entity: EntityRef,
    pub from: Location,
    pub cancelled: bool,
}
cancellable!(EntityPortalEvent, "EntityPortalEvent");

/// `LightningStrikeEvent`: a bolt is about to strike `location` of `world`
/// (`effect`: one that only flashes and harms nothing). Cancelling: no bolt.
#[derive(Clone, Debug)]
pub struct LightningStrikeEvent {
    pub world: World,
    pub location: Location,
    pub effect: bool,
    pub cancelled: bool,
}
cancellable!(LightningStrikeEvent, "LightningStrikeEvent");

/// `EntityDropItemEvent`: a mob is about to drop `item` by itself (an
/// armadillo's scute, a chicken's egg, a sniffer's seed...). Cancelling
/// drops nothing.
#[derive(Clone, Debug)]
pub struct EntityDropItemEvent {
    pub entity: EntityRef,
    pub item: ItemStack,
    pub location: Location,
    pub cancelled: bool,
}
cancellable!(EntityDropItemEvent, "EntityDropItemEvent");

/// `Spellcaster.Spell`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Spell {
    SummonVex,
    Fangs,
    Wololo,
}

/// `EntitySpellCastEvent`: an evoker cast `spell` (seen as its effects
/// come). Cancelling undoes them: no vexes, no fangs, no red sheep.
#[derive(Clone, Debug)]
pub struct EntitySpellCastEvent {
    pub entity: EntityRef,
    pub spell: Spell,
    pub cancelled: bool,
}
cancellable!(EntitySpellCastEvent, "EntitySpellCastEvent");
