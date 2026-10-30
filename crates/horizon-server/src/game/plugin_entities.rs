//! Where the server fires the plugins' entities events (docs/plugins.md):
//! what a mob's own tick did (its block changes, explosions, drops, shots,
//! spells, teleports and conversions, seen through the `MobEvent`s it
//! queued), what changed about mobs and players from one tick to the next
//! (fire, air, effects, wool, love, mounts, sleep, trades), and the hooks in
//! the gameplay code (lightning, totems, slimes, barters, items, potions,
//! fireworks, portals, bows, horse jumps). A cancelled change is put back.
use super::server_net::Network;
use crate::hc::items::ItemStack;
use crate::hc::mobs::{Mob, MobEvent, Species};
use horizon_core::geometry::Vec3;
use horizon_server_api::{self as api, Location, event as ev};
use std::collections::{HashMap, HashSet};

/// `DyeColor` by id.
const DYES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// What the plugins last saw of a mob or a player.
#[derive(Clone, Debug, PartialEq, Default)]
struct Seen {
    fire: i32,
    air: i32,
    effects: Vec<(&'static str, i32, i32)>,
    color: u8,
    sheared: bool,
    in_love: i32,
    mount: Option<u32>,
    resting: bool,
    glide: bool,
    swim: bool,
    /// Each offer's result and uses.
    offers: Vec<(String, i32)>,
}

/// What these events keep between ticks.
#[derive(Default)]
pub(super) struct State {
    mobs: HashMap<(super::Dim, u32), Seen>,
    players: HashMap<u32, Seen>,
}

/// What the mob ticks did, noted for the plugins while they run (the mobs
/// are out of the game's list then): which mob queued which events, the
/// mobs that teleported, and the bodies of those that went (a conversion or
/// a creeper's blast a plugin may call off).
#[derive(Default)]
pub(super) struct Noted {
    on: bool,
    was: Vec3,
    /// The end of each mob's events in the tick's list, and the mob.
    ends: Vec<(usize, u32)>,
    teleports: Vec<(u32, Vec3, Vec3)>,
    removed: Vec<Mob>,
}
impl Noted {
    /// Before the mob's tick.
    pub(super) fn before(&mut self, m: &Mob) {
        if self.on {
            self.was = m.position;
        }
    }
    /// After the mob's tick, with the tick's events so far.
    pub(super) fn after(&mut self, m: &Mob, kept: bool, events: &[MobEvent]) {
        if !self.on {
            return;
        }
        let start = self.ends.last().map_or(0, |e| e.0).min(events.len());
        self.ends.push((events.len(), m.id));
        let teleported = events[start..].iter().any(|e| {
            matches!(
                e,
                MobEvent::Sound("entity.enderman.teleport" | "entity.shulker.teleport", ..)
            )
        });
        if teleported && m.position != self.was {
            self.teleports.push((m.id, self.was, m.position));
        }
        if !kept && m.removed && m.alive() {
            self.removed.push(m.clone());
        }
    }
    fn owner(&self, k: usize) -> Option<u32> {
        self.ends.iter().find(|e| k < e.0).map(|e| e.1)
    }
}

fn effect_ref((kind, duration, amplifier): (&str, i32, i32)) -> ev::PotionEffect {
    ev::PotionEffect {
        kind: crate::hc::command::args::normalize(kind),
        duration,
        amplifier,
    }
}

fn recipe_of(o: &crate::hc::merchant::MerchantOffer) -> ev::MerchantRecipe {
    let mut ingredients = vec![super::plugin_events::stack_ref(&o.base_cost_a.stack)];
    if let Some(b) = &o.cost_b {
        ingredients.push(super::plugin_events::stack_ref(&b.stack));
    }
    ev::MerchantRecipe {
        result: super::plugin_events::stack_ref(&o.result),
        ingredients,
        uses: o.uses,
        max_uses: o.max_uses,
        villager_experience: o.xp,
    }
}

fn transform_reason(s: Species) -> ev::TransformReason {
    match s {
        Species::Skeleton => ev::TransformReason::Frozen,
        Species::Zombie | Species::Husk => ev::TransformReason::Drowned,
        Species::Piglin | Species::PiglinBrute | Species::Hoglin => {
            ev::TransformReason::PiglinZombified
        }
        Species::Tadpole => ev::TransformReason::Metamorphosis,
        _ => ev::TransformReason::Unknown,
    }
}

impl crate::game::Server<'_> {
    /// A mob as plugins see it.
    fn plugin_mob_ref(&self, m: &Mob) -> api::EntityRef {
        api::EntityRef {
            id: m.id as i32,
            kind: crate::hc::command::args::normalize(&m.kind.id),
            location: Location {
                world: self.dim.id().to_owned(),
                x: m.position.x,
                y: m.position.y,
                z: m.position.z,
                yaw: m.yaw,
                pitch: 0.0,
            },
            player: None,
        }
    }
    /// A dropped item as plugins see it.
    fn plugin_item_ref(&self, e: &crate::hc::entities::ItemEntity) -> api::EntityRef {
        api::EntityRef {
            id: e.id as i32,
            kind: "minecraft:item".into(),
            location: self.plugin_at([e.position.x, e.position.y, e.position.z]),
            player: None,
        }
    }
    fn plugin_at(&self, at: [f64; 3]) -> Location {
        Location::new(self.dim.id(), at[0], at[1], at[2])
    }
    /// The player whose turn it is, as an entity.
    fn plugin_acting_ref(&self) -> Option<api::EntityRef> {
        let player = self.plugin_acting()?;
        let id = self
            .plugin_seen_ids
            .iter()
            .find(|(_, uuid)| *uuid == player.uuid)
            .map_or(0, |(id, _)| *id);
        Some(api::EntityRef::of_player(id, player))
    }

    // -----------------------------------------------------------------------
    // What the mobs' own ticks did.

    /// Before the mob ticks: what to note of them for the plugins.
    pub(super) fn plugin_noted(&self) -> Noted {
        Noted {
            on: self.plugins_listen::<ev::EntityChangeBlockEvent>()
                || self.plugins_listen::<ev::EntityBreakDoorEvent>()
                || self.plugins_listen::<ev::EntityInteractEvent>()
                || self.plugins_listen::<ev::ExplosionPrimeEvent>()
                || self.plugins_listen::<ev::EntityDropItemEvent>()
                || self.plugins_listen::<ev::EntityShootBowEvent>()
                || self.plugins_listen::<ev::EntitySpellCastEvent>()
                || self.plugins_listen::<ev::EntityTransformEvent>()
                || self.plugins_listen::<ev::EntityTeleportEvent>(),
            ..Default::default()
        }
    }
    /// After the mob ticks, before their events are handled:
    /// `EntityTeleportEvent`, `EntityTransformEvent`, `EntityChangeBlockEvent`,
    /// `EntityInteractEvent` (farmland), `EntityBreakDoorEvent`,
    /// `ExplosionPrimeEvent`, `EntityDropItemEvent`, `EntityShootBowEvent` and
    /// `EntitySpellCastEvent` for what each mob did; cancelled events are
    /// taken out of `events`, cancelled teleports and conversions undone.
    pub(super) fn plugin_mob_events(&mut self, noted: Noted, events: &mut Vec<MobEvent>) {
        if !noted.on {
            return;
        }
        for &(id, from, to) in &noted.teleports {
            if !self.plugins_listen::<ev::EntityTeleportEvent>() {
                break;
            }
            let entity = self.plugin_entity(None, id as i32);
            let mut e = ev::EntityTeleportEvent {
                entity,
                from: self.plugin_at([from.x, from.y, from.z]),
                to: self.plugin_at([to.x, to.y, to.z]),
                cancelled: false,
            };
            self.plugin_fire(None, &mut e);
            let target = if e.cancelled {
                from
            } else {
                Vec3::new(e.to.x, e.to.y, e.to.z)
            };
            if target != to
                && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
            {
                m.position = target;
                m.previous = target;
                m.needs_sync = true;
            }
        }
        let mut keep = vec![true; events.len()];
        let mut spells: HashMap<(u32, ev::Spell), bool> = HashMap::new();
        let mut restore: Vec<Mob> = Vec::new();
        for k in 0..events.len() {
            let Some(owner) = noted.owner(k) else {
                continue;
            };
            let allowed = match &mut events[k] {
                MobEvent::SetBlock { pos, state } => {
                    let pos = *pos;
                    let to = self.state_string(*state);
                    let block = self.plugin_block(pos);
                    let mut allowed = true;
                    if block.state.starts_with("minecraft:farmland")
                        && self.plugins_listen::<ev::EntityInteractEvent>()
                    {
                        let e = ev::EntityInteractEvent {
                            entity: self.plugin_owner_ref(&noted.removed, owner),
                            block: block.clone(),
                            cancelled: false,
                        };
                        allowed = self.plugin_allows(None, e);
                    }
                    allowed
                        && self.plugin_change_block(
                            Some(self.plugin_owner_ref(&noted.removed, owner)),
                            block,
                            to,
                        )
                }
                MobEvent::SetBlockName { pos, name, .. } => {
                    let block = self.plugin_block(*pos);
                    let to = name.to_string();
                    self.plugin_change_block(
                        Some(self.plugin_owner_ref(&noted.removed, owner)),
                        block,
                        to,
                    )
                }
                MobEvent::BreakBlock { pos, .. } => {
                    let block = self.plugin_block(*pos);
                    self.plugin_change_block(
                        Some(self.plugin_owner_ref(&noted.removed, owner)),
                        block,
                        "minecraft:air".into(),
                    )
                }
                MobEvent::Infest { pos, infested, .. } => {
                    let block = self.plugin_block(*pos);
                    let to = if *infested {
                        block.state.replacen("minecraft:", "minecraft:infested_", 1)
                    } else {
                        block.state.replacen("minecraft:infested_", "minecraft:", 1)
                    };
                    self.plugin_change_block(
                        Some(self.plugin_owner_ref(&noted.removed, owner)),
                        block,
                        to,
                    )
                }
                MobEvent::BreakDoor { pos } => {
                    if !self.plugins_listen::<ev::EntityBreakDoorEvent>() {
                        true
                    } else {
                        let e = ev::EntityBreakDoorEvent {
                            entity: self.plugin_owner_ref(&noted.removed, owner),
                            block: self.plugin_block(*pos),
                            cancelled: false,
                        };
                        self.plugin_allows(None, e)
                    }
                }
                MobEvent::Explode(_, radius, _) => {
                    if !self.plugins_listen::<ev::ExplosionPrimeEvent>() {
                        true
                    } else {
                        let mut e = ev::ExplosionPrimeEvent {
                            entity: self.plugin_owner_ref(&noted.removed, owner),
                            radius: *radius,
                            fire: false,
                            cancelled: false,
                        };
                        self.plugin_fire(None, &mut e);
                        if e.cancelled {
                            // The blast's source goes too, and a creeper lives on.
                            for j in (0..k).rev() {
                                if let MobEvent::ExplosionSource { mob, .. } = &events[j]
                                    && *mob == owner
                                {
                                    keep[j] = false;
                                    break;
                                }
                            }
                            if let Some(m) = noted.removed.iter().find(|m| m.id == owner) {
                                let mut m = m.clone();
                                m.removed = false;
                                m.swell = 0;
                                m.previous_swell = 0;
                                m.extra.counter = -1;
                                restore.push(m);
                            }
                            false
                        } else {
                            if let MobEvent::Explode(_, radius, _) = &mut events[k] {
                                *radius = e.radius.max(0.0);
                            }
                            true
                        }
                    }
                }
                MobEvent::DropItem { item, count, at } => {
                    let stack = ItemStack::new(item, *count);
                    let at = *at;
                    self.plugin_entity_drop_item(owner, &stack, at)
                }
                MobEvent::ShedScute { at, .. } => {
                    let stack = ItemStack::new("minecraft:armadillo_scute", 1);
                    let at = *at;
                    self.plugin_entity_drop_item(owner, &stack, at)
                }
                MobEvent::Projectile(p)
                    if matches!(p.kind, crate::hc::projectiles::ProjectileKind::Arrow { .. }) =>
                {
                    if !self.plugins_listen::<ev::EntityShootBowEvent>() {
                        true
                    } else {
                        let projectile = p.kind.id().to_owned();
                        let bow = self
                            .mobs
                            .iter()
                            .find(|m| m.id == owner)
                            .and_then(|m| m.extra.main_hand.clone())
                            .map(|item| api::ItemStack::new(&item, 1));
                        let e = ev::EntityShootBowEvent {
                            entity: self.plugin_owner_ref(&noted.removed, owner),
                            bow,
                            consumable: Some(api::ItemStack::new("minecraft:arrow", 1)),
                            projectile,
                            force: 1.0,
                            hand: ev::Hand::Main,
                            cancelled: false,
                        };
                        self.plugin_allows(None, e)
                    }
                }
                MobEvent::SpawnKind { kind, at, setup } => {
                    let at = *at;
                    if let Some(caster) = setup.owner_mob.filter(|_| kind == "minecraft:vex") {
                        self.plugin_spell(&mut spells, caster, ev::Spell::SummonVex)
                    } else if let Some(m) = noted
                        .removed
                        .iter()
                        .find(|m| m.id == owner && [m.position.x, m.position.y, m.position.z] == at)
                    {
                        if !self.plugins_listen::<ev::EntityTransformEvent>() {
                            true
                        } else {
                            let e = ev::EntityTransformEvent {
                                entity: self.plugin_mob_ref(m),
                                transformed_kind: crate::hc::command::args::normalize(kind),
                                reason: transform_reason(m.species()),
                                cancelled: false,
                            };
                            let allowed = self.plugin_allows(None, e);
                            if !allowed && !restore.iter().any(|r| r.id == m.id) {
                                let mut m = m.clone();
                                m.removed = false;
                                // A drowning or freezing conversion starts over.
                                if matches!(
                                    m.species(),
                                    Species::Zombie | Species::Husk | Species::Skeleton
                                ) {
                                    m.extra.counter = -1;
                                    m.extra.counter2 = 0;
                                }
                                restore.push(m);
                            }
                            allowed
                        }
                    } else {
                        true
                    }
                }
                MobEvent::Fangs { owner: caster, .. } => {
                    let caster = *caster;
                    self.plugin_spell(&mut spells, caster, ev::Spell::Fangs)
                }
                MobEvent::Wololo { .. } => self.plugin_spell(&mut spells, owner, ev::Spell::Wololo),
                _ => true,
            };
            keep[k] &= allowed;
        }
        if keep.iter().any(|k| !k) {
            let mut flags = keep.into_iter();
            events.retain(|_| flags.next().unwrap_or(true));
        }
        for m in restore {
            if !self.mobs.iter().any(|o| o.id == m.id) {
                self.mobs.push(m);
            }
        }
    }
    fn plugin_mob_mut(&mut self, id: u32) -> Option<&mut Mob> {
        self.mobs.iter_mut().find(|m| m.id == id)
    }
    /// The mob that queued an event: in the list, or its body if it went.
    fn plugin_owner_ref(&self, removed: &[Mob], owner: u32) -> api::EntityRef {
        match removed.iter().find(|m| m.id == owner) {
            Some(m) => self.plugin_mob_ref(m),
            None => self.plugin_entity(None, owner as i32),
        }
    }
    /// `EntityChangeBlockEvent` for `entity` turning `block` into `to`.
    fn plugin_change_block(
        &mut self,
        entity: Option<api::EntityRef>,
        block: api::Block,
        to: String,
    ) -> bool {
        if !self.plugins_listen::<ev::EntityChangeBlockEvent>() {
            return true;
        }
        let Some(entity) = entity else {
            return true;
        };
        let e = ev::EntityChangeBlockEvent {
            entity,
            block,
            to,
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// `EntitySpellCastEvent`, once per caster and spell a tick.
    fn plugin_spell(
        &mut self,
        spells: &mut HashMap<(u32, ev::Spell), bool>,
        caster: u32,
        spell: ev::Spell,
    ) -> bool {
        if !self.plugins_listen::<ev::EntitySpellCastEvent>() {
            return true;
        }
        if let Some(&allowed) = spells.get(&(caster, spell)) {
            return allowed;
        }
        let e = ev::EntitySpellCastEvent {
            entity: self.plugin_entity(None, caster as i32),
            spell,
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, e);
        spells.insert((caster, spell), allowed);
        allowed
    }
    /// The mob `mob` is about to drop `stack` at `at` by itself:
    /// `EntityDropItemEvent`. False when cancelled.
    pub(super) fn plugin_entity_drop_item(
        &mut self,
        mob: u32,
        stack: &ItemStack,
        at: [f64; 3],
    ) -> bool {
        if !self.plugins_listen::<ev::EntityDropItemEvent>() {
            return true;
        }
        let e = ev::EntityDropItemEvent {
            entity: self.plugin_entity(None, mob as i32),
            item: super::plugin_events::stack_ref(stack),
            location: self.plugin_at(at),
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// A mob about to turn into a `kind` (`minecraft:villager`...):
    /// `EntityTransformEvent`. False when cancelled.
    pub(super) fn plugin_transform(
        &mut self,
        mob: u32,
        kind: &str,
        reason: ev::TransformReason,
    ) -> bool {
        if !self.plugins_listen::<ev::EntityTransformEvent>() {
            return true;
        }
        let e = ev::EntityTransformEvent {
            entity: self.plugin_entity(None, mob as i32),
            transformed_kind: kind.to_owned(),
            reason,
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }

    // -----------------------------------------------------------------------
    // What changed about mobs and players since the last tick.

    fn plugins_watch_bodies(&self) -> bool {
        self.plugins_listen::<ev::EntityCombustEvent>()
            || self.plugins_listen::<ev::EntityAirChangeEvent>()
            || self.plugins_listen::<ev::EntityPotionEffectEvent>()
            || self.plugins_listen::<ev::EntityToggleGlideEvent>()
            || self.plugins_listen::<ev::EntityToggleSwimEvent>()
            || self.plugins_listen::<ev::EntityMountEvent>()
            || self.plugins_listen::<ev::EntityDismountEvent>()
    }
    fn plugins_watch_entities(&self) -> bool {
        self.plugins_watch_bodies()
            || self.plugins_listen::<ev::SheepDyeWoolEvent>()
            || self.plugins_listen::<ev::SheepRegrowWoolEvent>()
            || self.plugins_listen::<ev::EntityEnterLoveModeEvent>()
            || self.plugins_listen::<ev::BatToggleSleepEvent>()
            || self.plugins_listen::<ev::VillagerAcquireTradeEvent>()
            || self.plugins_listen::<ev::VillagerReplenishTradeEvent>()
    }
    fn mob_seen(m: &Mob) -> Seen {
        Seen {
            fire: m.fire_ticks,
            air: m.air,
            effects: m.extra.effects.clone(),
            color: m.extra.color,
            sheared: m.extra.sheared,
            in_love: m.extra.in_love,
            mount: m.extra.mount,
            resting: m.species() == Species::Bat && crate::hc::mobs::bat::resting(m),
            glide: m.extra.fall_flying,
            swim: false,
            offers: m.extra.trade.as_ref().map_or_else(Vec::new, |t| {
                t.offers
                    .iter()
                    .map(|o| (o.result.item.to_string(), o.uses))
                    .collect()
            }),
        }
    }
    /// The effect changes from `before` to `now`: (old, new, action, cause).
    #[allow(clippy::type_complexity)]
    fn effect_changes(
        before: &[(&'static str, i32, i32)],
        now: &[(&'static str, i32, i32)],
    ) -> Vec<(
        Option<(&'static str, i32, i32)>,
        Option<(&'static str, i32, i32)>,
        ev::PotionAction,
        ev::PotionCause,
    )> {
        let mut out = Vec::new();
        let cleared = now.is_empty() && before.len() > 1;
        for &old in before {
            match now.iter().find(|e| e.0 == old.0) {
                None => {
                    let cause = if old.1 != -1 && old.1 <= 1 {
                        ev::PotionCause::Expiration
                    } else {
                        ev::PotionCause::Unknown
                    };
                    let action = if cleared {
                        ev::PotionAction::Cleared
                    } else {
                        ev::PotionAction::Removed
                    };
                    out.push((Some(old), None, action, cause));
                }
                Some(&new) => {
                    // A duration counts down by itself; only a longer one or
                    // a new level is a change.
                    let longer = new.1 == -1 && old.1 != -1 || old.1 != -1 && new.1 > old.1;
                    if new.2 != old.2 || longer {
                        out.push((
                            Some(old),
                            Some(new),
                            ev::PotionAction::Changed,
                            ev::PotionCause::Unknown,
                        ));
                    }
                }
            }
        }
        for &new in now {
            if !before.iter().any(|e| e.0 == new.0) {
                out.push((
                    None,
                    Some(new),
                    ev::PotionAction::Added,
                    ev::PotionCause::Unknown,
                ));
            }
        }
        out
    }
    /// Once a tick, after the mobs ticked: `EntityCombustEvent`,
    /// `EntityAirChangeEvent`, `EntityPotionEffectEvent`, `SheepDyeWoolEvent`,
    /// `SheepRegrowWoolEvent`, `EntityEnterLoveModeEvent`, `EntityMountEvent`,
    /// `EntityDismountEvent`, `BatToggleSleepEvent`, `EntityToggleGlideEvent`,
    /// `VillagerAcquireTradeEvent` and `VillagerReplenishTradeEvent` for what
    /// changed about each mob; cancelled changes go back.
    pub(super) fn plugin_watch_entities(&mut self) {
        if !self.plugins_watch_entities() {
            self.plugin_entities.mobs.clear();
            return;
        }
        let dim = self.dim;
        let live: HashSet<u32> = self.mobs.iter().map(|m| m.id).collect();
        self.plugin_entities
            .mobs
            .retain(|(d, id), _| *d != dim || live.contains(id));
        let ids: Vec<u32> = self
            .mobs
            .iter()
            .filter(|m| m.alive() && !m.removed)
            .map(|m| m.id)
            .collect();
        for id in ids {
            let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
                continue;
            };
            let now = Self::mob_seen(&self.mobs[i]);
            let Some(before) = self.plugin_entities.mobs.insert((dim, id), now.clone()) else {
                continue;
            };
            if before == now {
                continue;
            }
            self.plugin_mob_changes(id, &before, &now);
            if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
                let seen = Self::mob_seen(m);
                self.plugin_entities.mobs.insert((dim, id), seen);
            }
        }
    }
    fn plugin_mob_changes(&mut self, id: u32, before: &Seen, now: &Seen) {
        if before.fire <= 0 && now.fire > 0 && self.plugins_listen::<ev::EntityCombustEvent>() {
            let mut e = ev::EntityCombustEvent {
                entity: self.plugin_entity(None, id as i32),
                duration: now.fire as f32 / 20.0,
                cancelled: false,
            };
            self.plugin_fire(None, &mut e);
            if let Some(m) = self.plugin_mob_mut(id) {
                if e.cancelled {
                    m.fire_ticks = before.fire;
                } else if (e.duration * 20.0) as i32 != now.fire {
                    m.fire_ticks = (e.duration * 20.0) as i32;
                }
            }
        }
        if before.air != now.air && self.plugins_listen::<ev::EntityAirChangeEvent>() {
            let mut e = ev::EntityAirChangeEvent {
                entity: self.plugin_entity(None, id as i32),
                amount: now.air,
                cancelled: false,
            };
            self.plugin_fire(None, &mut e);
            if let Some(m) = self.plugin_mob_mut(id) {
                m.air = if e.cancelled { before.air } else { e.amount };
            }
        }
        if before.effects != now.effects && self.plugins_listen::<ev::EntityPotionEffectEvent>() {
            for (old, new, action, cause) in Self::effect_changes(&before.effects, &now.effects) {
                let e = ev::EntityPotionEffectEvent {
                    entity: self.plugin_entity(None, id as i32),
                    old_effect: old.map(effect_ref),
                    new_effect: new.map(effect_ref),
                    action,
                    cause,
                    cancelled: false,
                };
                if !self.plugin_allows(None, e)
                    && let Some(m) = self.plugin_mob_mut(id)
                {
                    let kind = old.or(new).map_or("", |e| e.0);
                    m.extra.effects.retain(|e| e.0 != kind);
                    if let Some(old) = old {
                        m.extra.effects.push(old);
                    }
                }
            }
        }
        if before.color != now.color && self.plugins_listen::<ev::SheepDyeWoolEvent>() {
            let mut e = ev::SheepDyeWoolEvent {
                entity: self.plugin_entity(None, id as i32),
                color: DYES[usize::from(now.color) % 16].to_owned(),
                cancelled: false,
            };
            self.plugin_fire(None, &mut e);
            if let Some(m) = self.plugin_mob_mut(id) {
                m.extra.color = if e.cancelled {
                    before.color
                } else {
                    DYES.iter()
                        .position(|d| *d == e.color.trim_start_matches("minecraft:"))
                        .map_or(now.color, |c| c as u8)
                };
                m.needs_sync = true;
            }
        }
        if before.sheared && !now.sheared && self.plugins_listen::<ev::SheepRegrowWoolEvent>() {
            let e = ev::SheepRegrowWoolEvent {
                entity: self.plugin_entity(None, id as i32),
                cancelled: false,
            };
            if !self.plugin_allows(None, e)
                && let Some(m) = self.plugin_mob_mut(id)
            {
                m.extra.sheared = true;
                m.needs_sync = true;
            }
        }
        if before.in_love <= 0
            && now.in_love > 0
            && self.plugins_listen::<ev::EntityEnterLoveModeEvent>()
        {
            let fed_by_player = self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .is_some_and(|m| m.extra.love_player);
            let mut e = ev::EntityEnterLoveModeEvent {
                entity: self.plugin_entity(None, id as i32),
                fed_by_player,
                ticks_in_love: now.in_love,
                cancelled: false,
            };
            self.plugin_fire(None, &mut e);
            if let Some(m) = self.plugin_mob_mut(id) {
                m.extra.in_love = if e.cancelled { 0 } else { e.ticks_in_love };
            }
        }
        if before.resting != now.resting && self.plugins_listen::<ev::BatToggleSleepEvent>() {
            let e = ev::BatToggleSleepEvent {
                entity: self.plugin_entity(None, id as i32),
                awake: !now.resting,
                cancelled: false,
            };
            if !self.plugin_allows(None, e)
                && let Some(m) = self.plugin_mob_mut(id)
            {
                crate::hc::mobs::bat::set_resting(m, before.resting);
            }
        }
        if before.glide != now.glide {
            let mut e = ev::EntityToggleGlideEvent {
                entity: self.plugin_entity(None, id as i32),
                gliding: now.glide,
            };
            self.plugin_fire(None, &mut e);
        }
        if before.mount != now.mount {
            if let Some(old) = before.mount
                && self.plugins_listen::<ev::EntityDismountEvent>()
            {
                let mut e = ev::EntityDismountEvent {
                    entity: self.plugin_entity(None, id as i32),
                    dismounted: self.plugin_entity(None, old as i32),
                };
                self.plugin_fire(None, &mut e);
            }
            if let Some(new) = now.mount
                && self.plugins_listen::<ev::EntityMountEvent>()
            {
                let e = ev::EntityMountEvent {
                    entity: self.plugin_entity(None, id as i32),
                    mount: self.plugin_entity(None, new as i32),
                    cancelled: false,
                };
                if !self.plugin_allows(None, e) {
                    if let Some(m) = self.plugin_mob_mut(id) {
                        m.extra.mount = None;
                        m.extra.passenger = false;
                    }
                    if let Some(v) = self.mobs.iter_mut().find(|m| m.id == new)
                        && v.extra.rider == Some(id)
                    {
                        v.extra.rider = None;
                    }
                }
            }
        }
        if before.offers != now.offers {
            self.plugin_offer_changes(id, before, now);
        }
    }
    fn plugin_offer_changes(&mut self, id: u32, before: &Seen, now: &Seen) {
        let offer = |g: &Self, k: usize| {
            g.mobs
                .iter()
                .find(|m| m.id == id)
                .and_then(|m| m.extra.trade.as_ref())
                .and_then(|t| t.offers.get(k))
                .map(recipe_of)
        };
        let mut gone = Vec::new();
        if self.plugins_listen::<ev::VillagerReplenishTradeEvent>() {
            for k in 0..before.offers.len().min(now.offers.len()) {
                if before.offers[k].0 != now.offers[k].0
                    || before.offers[k].1 <= 0
                    || now.offers[k].1 != 0
                {
                    continue;
                }
                let Some(recipe) = offer(self, k) else {
                    continue;
                };
                let e = ev::VillagerReplenishTradeEvent {
                    entity: self.plugin_entity(None, id as i32),
                    recipe,
                    cancelled: false,
                };
                if !self.plugin_allows(None, e)
                    && let Some(o) = self
                        .mobs
                        .iter_mut()
                        .find(|m| m.id == id)
                        .and_then(|m| m.extra.trade.as_mut())
                        .and_then(|t| t.offers.get_mut(k))
                {
                    o.uses = before.offers[k].1;
                }
            }
        }
        if self.plugins_listen::<ev::VillagerAcquireTradeEvent>() {
            for k in before.offers.len()..now.offers.len() {
                let Some(recipe) = offer(self, k) else {
                    continue;
                };
                let mut e = ev::VillagerAcquireTradeEvent {
                    entity: self.plugin_entity(None, id as i32),
                    recipe: recipe.clone(),
                    cancelled: false,
                };
                self.plugin_fire(None, &mut e);
                if e.cancelled {
                    gone.push(k);
                    continue;
                }
                if e.recipe != recipe
                    && let Some(o) = self
                        .mobs
                        .iter_mut()
                        .find(|m| m.id == id)
                        .and_then(|m| m.extra.trade.as_mut())
                        .and_then(|t| t.offers.get_mut(k))
                {
                    o.max_uses = e.recipe.max_uses;
                    o.result.item = e.recipe.result.item.as_str().into();
                    o.result.count = e.recipe.result.count;
                }
            }
        }
        if let Some(t) = self
            .mobs
            .iter_mut()
            .find(|m| m.id == id)
            .and_then(|m| m.extra.trade.as_mut())
        {
            for k in gone.into_iter().rev() {
                if k < t.offers.len() {
                    t.offers.remove(k);
                }
            }
        }
    }
    /// Once a tick, for every player: `EntityCombustEvent`,
    /// `EntityAirChangeEvent`, `EntityPotionEffectEvent` (cancelled: back as
    /// they were), `EntityToggleGlideEvent`, `EntityToggleSwimEvent`,
    /// `EntityMountEvent` and `EntityDismountEvent`.
    pub(super) fn plugin_watch_player_bodies(&mut self, net: &mut Network) {
        if !self.plugins_watch_bodies() {
            self.plugin_entities.players.clear();
            return;
        }
        let live: HashSet<u32> = net
            .hub
            .players
            .iter()
            .filter(|g| g.placed)
            .map(|g| g.conn)
            .collect();
        self.plugin_entities.players.retain(|c, _| live.contains(c));
        for i in 0..net.hub.players.len() {
            let g = &net.hub.players[i];
            let Some(body) = g.motion.as_ref().filter(|_| g.placed) else {
                continue;
            };
            let conn = g.conn;
            let now = Seen {
                fire: body.fire_ticks,
                air: body.air,
                effects: g
                    .effects
                    .list
                    .iter()
                    .map(|e| (e.id, e.duration, e.amplifier))
                    .collect(),
                mount: g.vehicle,
                glide: g.fall_flying,
                swim: body.swimming,
                ..Default::default()
            };
            let Some(before) = self.plugin_entities.players.insert(conn, now.clone()) else {
                continue;
            };
            if before == now {
                continue;
            }
            let entity =
                api::EntityRef::of_player(g.entity_id, crate::game::Server::plugin_player(g));
            self.plugin_player_changes(net, i, entity, &before, &now);
            let g = &net.hub.players[i];
            if let Some(seen) = self.plugin_entities.players.get_mut(&conn) {
                if let Some(body) = g.motion.as_ref() {
                    seen.fire = body.fire_ticks;
                    seen.air = body.air;
                }
                seen.effects = g
                    .effects
                    .list
                    .iter()
                    .map(|e| (e.id, e.duration, e.amplifier))
                    .collect();
            }
        }
    }
    fn plugin_player_changes(
        &mut self,
        net: &mut Network,
        i: usize,
        entity: api::EntityRef,
        before: &Seen,
        now: &Seen,
    ) {
        if before.fire <= 0 && now.fire > 0 && self.plugins_listen::<ev::EntityCombustEvent>() {
            let mut e = ev::EntityCombustEvent {
                entity: entity.clone(),
                duration: now.fire as f32 / 20.0,
                cancelled: false,
            };
            self.plugin_fire(Some(net), &mut e);
            if let Some(body) = net.hub.players[i].motion.as_mut() {
                if e.cancelled {
                    body.fire_ticks = before.fire;
                } else if (e.duration * 20.0) as i32 != now.fire {
                    body.fire_ticks = (e.duration * 20.0) as i32;
                }
            }
        }
        if before.air != now.air && self.plugins_listen::<ev::EntityAirChangeEvent>() {
            let mut e = ev::EntityAirChangeEvent {
                entity: entity.clone(),
                amount: now.air,
                cancelled: false,
            };
            self.plugin_fire(Some(net), &mut e);
            if let Some(body) = net.hub.players[i].motion.as_mut() {
                body.air = if e.cancelled { before.air } else { e.amount };
            }
        }
        if before.effects != now.effects && self.plugins_listen::<ev::EntityPotionEffectEvent>() {
            for (old, new, action, cause) in Self::effect_changes(&before.effects, &now.effects) {
                let e = ev::EntityPotionEffectEvent {
                    entity: entity.clone(),
                    old_effect: old.map(effect_ref),
                    new_effect: new.map(effect_ref),
                    action,
                    cause,
                    cancelled: false,
                };
                if !self.plugin_allows(Some(net), e) {
                    let list = &mut net.hub.players[i].effects.list;
                    let kind = old.or(new).map_or("", |e| e.0);
                    list.retain(|e| e.id != kind);
                    if let Some((id, duration, amplifier)) = old {
                        list.push(crate::hc::effects::EffectInstance::new(
                            id, duration, amplifier,
                        ));
                    }
                }
            }
        }
        if before.glide != now.glide {
            let mut e = ev::EntityToggleGlideEvent {
                entity: entity.clone(),
                gliding: now.glide,
            };
            self.plugin_fire(Some(net), &mut e);
        }
        if before.swim != now.swim {
            let mut e = ev::EntityToggleSwimEvent {
                entity: entity.clone(),
                swimming: now.swim,
            };
            self.plugin_fire(Some(net), &mut e);
        }
        if before.mount != now.mount {
            if let Some(old) = before.mount {
                let mut e = ev::EntityDismountEvent {
                    entity: entity.clone(),
                    dismounted: self.plugin_entity(Some(net), old as i32),
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if let Some(new) = now.mount {
                let mut e = ev::EntityMountEvent {
                    entity,
                    mount: self.plugin_entity(Some(net), new as i32),
                    cancelled: false,
                };
                self.plugin_fire(Some(net), &mut e);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Hooks in the gameplay code.

    /// A bolt about to strike `at`: `LightningStrikeEvent`. False when cancelled.
    pub(super) fn plugin_lightning(&mut self, at: [f64; 3], visual_only: bool) -> bool {
        if !self.plugins_listen::<ev::LightningStrikeEvent>() {
            return true;
        }
        let e = ev::LightningStrikeEvent {
            world: self.dim.id().to_owned(),
            location: self.plugin_at(at),
            effect: visual_only,
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// The mobs `struck` by a bolt that plugins keep as they are:
    /// `PigZapEvent` and `EntityTransformEvent` for pigs, `EntityTransformEvent`
    /// for villagers, `CreeperPowerEvent` for creepers.
    pub(super) fn plugin_thunder_vetoes(&mut self, struck: &[u32], peaceful: bool) -> HashSet<u32> {
        let mut vetoed = HashSet::new();
        if !self.plugins_listen::<ev::PigZapEvent>()
            && !self.plugins_listen::<ev::EntityTransformEvent>()
            && !self.plugins_listen::<ev::CreeperPowerEvent>()
        {
            return vetoed;
        }
        for &id in struck {
            let Some(species) = self.mobs.iter().find(|m| m.id == id).map(|m| m.species()) else {
                continue;
            };
            let allowed = match species {
                Species::Pig if !peaceful => {
                    let e = ev::PigZapEvent {
                        entity: self.plugin_entity(None, id as i32),
                        cancelled: false,
                    };
                    self.plugin_allows(None, e)
                        && self.plugin_transform(
                            id,
                            "minecraft:zombified_piglin",
                            ev::TransformReason::Lightning,
                        )
                }
                Species::Villager if !peaceful => {
                    self.plugin_transform(id, "minecraft:witch", ev::TransformReason::Lightning)
                }
                Species::Creeper if self.plugins_listen::<ev::CreeperPowerEvent>() => {
                    let e = ev::CreeperPowerEvent {
                        entity: self.plugin_entity(None, id as i32),
                        cause: ev::PowerCause::Lightning,
                        cancelled: false,
                    };
                    self.plugin_allows(None, e)
                }
                _ => true,
            };
            if !allowed {
                vetoed.insert(id);
            }
        }
        vetoed
    }
    /// The player whose turn it is is about to be saved by a totem:
    /// `EntityResurrectEvent`. False when cancelled.
    pub(super) fn plugin_resurrect_acting(&mut self, hand: ev::Hand) -> bool {
        if !self.plugins_listen::<ev::EntityResurrectEvent>() {
            return true;
        }
        let Some(entity) = self.plugin_acting_ref() else {
            return true;
        };
        let e = ev::EntityResurrectEvent {
            entity,
            hand: Some(hand),
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// The dead slime `m` is about to split into `count`: `SlimeSplitEvent`.
    /// How many it splits into (none when cancelled).
    pub(super) fn plugin_slime_split(&mut self, m: &Mob, count: i32) -> i32 {
        if !self.plugins_listen::<ev::SlimeSplitEvent>() {
            return count;
        }
        let mut e = ev::SlimeSplitEvent {
            entity: self.plugin_mob_ref(m),
            count,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled { 0 } else { e.count.max(0) }
    }
    /// The piglin `mob` is about to throw `outcome` for gold: `PiglinBarterEvent`.
    /// What it throws, or None when cancelled.
    pub(super) fn plugin_barter(
        &mut self,
        mob: u32,
        input: &str,
        outcome: Vec<ItemStack>,
    ) -> Option<Vec<ItemStack>> {
        if !self.plugins_listen::<ev::PiglinBarterEvent>()
            || self.mobs.iter().any(|m| m.id == mob && m.is_baby())
        {
            return Some(outcome);
        }
        let before: Vec<api::ItemStack> = outcome
            .iter()
            .map(super::plugin_events::stack_ref)
            .collect();
        let mut e = ev::PiglinBarterEvent {
            entity: self.plugin_entity(None, mob as i32),
            input: api::ItemStack::new(input, 1),
            outcome: before.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled {
            return None;
        }
        if e.outcome == before {
            return Some(outcome);
        }
        Some(
            e.outcome
                .iter()
                .map(|s| ItemStack::new(&s.item, s.count))
                .collect(),
        )
    }
    /// A primed TNT (or another entity `kind`) about to blow up at `at`:
    /// `ExplosionPrimeEvent`. The radius and fire to blast with, or None.
    pub(super) fn plugin_explosion_prime(
        &mut self,
        id: u32,
        kind: &str,
        at: [f64; 3],
        radius: f32,
        fire: bool,
    ) -> Option<(f32, bool)> {
        if !self.plugins_listen::<ev::ExplosionPrimeEvent>() {
            return Some((radius, fire));
        }
        let mut e = ev::ExplosionPrimeEvent {
            entity: api::EntityRef {
                id: id as i32,
                kind: kind.to_owned(),
                location: self.plugin_at(at),
                player: None,
            },
            radius,
            fire,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        (!e.cancelled).then_some((e.radius.max(0.0), e.fire))
    }
    /// A firework rocket about to burst at `at`: `FireworkExplodeEvent`.
    /// False when cancelled.
    pub(super) fn plugin_firework_explode(&mut self, id: u32, at: [f64; 3]) -> bool {
        if !self.plugins_listen::<ev::FireworkExplodeEvent>() {
            return true;
        }
        let e = ev::FireworkExplodeEvent {
            entity: api::EntityRef {
                id: id as i32,
                kind: "minecraft:firework_rocket".into(),
                location: self.plugin_at(at),
                player: None,
            },
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// A falling block of `state` about to land at `pos`:
    /// `EntityChangeBlockEvent`. False when cancelled (it goes without a block).
    pub(super) fn plugin_falling_lands(
        &mut self,
        id: u32,
        state: horizon_world::registry::BlockStateId,
        at: Vec3,
        pos: [i32; 3],
    ) -> bool {
        if !self.plugins_listen::<ev::EntityChangeBlockEvent>() {
            return true;
        }
        let e = ev::EntityChangeBlockEvent {
            entity: api::EntityRef {
                id: id as i32,
                kind: "minecraft:falling_block".into(),
                location: self.plugin_at([at.x, at.y, at.z]),
                player: None,
            },
            block: self.plugin_block(pos),
            to: self.state_string(state),
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// The entity `id` is about to go through a portal: `EntityPortalEvent`.
    /// False when cancelled.
    pub(super) fn plugin_entity_portal(&mut self, id: u32) -> bool {
        if !self.plugins_listen::<ev::EntityPortalEvent>() {
            return true;
        }
        let mut entity = self.plugin_entity(None, id as i32);
        if let Some(e) = self.dropped.iter().find(|e| e.id == id) {
            entity = self.plugin_item_ref(e);
        }
        let e = ev::EntityPortalEvent {
            from: entity.location.clone(),
            entity,
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// The mob `mob` is about to pick up the dropped item `item` (an index
    /// into the dropped items), taking `taking` of it: `EntityPickupItemEvent`.
    /// False when cancelled.
    pub(super) fn plugin_mob_pickup(&mut self, mob: u32, item: usize, taking: u32) -> bool {
        if !self.plugins_listen::<ev::EntityPickupItemEvent>() {
            return true;
        }
        let Some(e) = self.dropped.get(item) else {
            return true;
        };
        let (stack, location) = (
            super::plugin_events::stack_ref(&e.stack),
            self.plugin_at([e.position.x, e.position.y, e.position.z]),
        );
        let remaining = stack.count.saturating_sub(taking) as i32;
        let e = ev::EntityPickupItemEvent {
            entity: self.plugin_entity(None, mob as i32),
            item: stack,
            location,
            remaining,
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// The player whose turn it is is about to shoot `bow` with `ammo` at
    /// `force`: `EntityShootBowEvent`. False when cancelled (their client gets
    /// their inventory again).
    pub(super) fn plugin_shoot_bow_acting(
        &mut self,
        bow: Option<&ItemStack>,
        ammo: Option<&ItemStack>,
        force: f32,
    ) -> bool {
        if !self.plugins_listen::<ev::EntityShootBowEvent>() {
            return true;
        }
        let Some(entity) = self.plugin_acting_ref() else {
            return true;
        };
        let e = ev::EntityShootBowEvent {
            entity,
            bow: bow.map(super::plugin_events::stack_ref),
            consumable: ammo.map(super::plugin_events::stack_ref),
            projectile: "minecraft:arrow".into(),
            force,
            hand: ev::Hand::Main,
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, e);
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }
    /// The mount `index` a player rides is about to jump with `power`:
    /// `HorseJumpEvent`. The power to jump with (0: none).
    pub(super) fn plugin_horse_jump(&mut self, index: usize, power: f32) -> f32 {
        if power <= 0.0 || !self.plugins_listen::<ev::HorseJumpEvent>() {
            return power;
        }
        let Some(m) = self.mobs.get(index) else {
            return power;
        };
        if !crate::hc::mobs::equine::is_equine(m.species()) {
            return power;
        }
        let mut e = ev::HorseJumpEvent {
            entity: self.plugin_mob_ref(m),
            power,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled {
            0.0
        } else {
            e.power.clamp(0.0, 1.0)
        }
    }

    // -----------------------------------------------------------------------
    // Dropped items and potions.

    /// Whether the dropped items merge after their ticks, for `ItemMergeEvent`.
    pub(super) fn plugin_merges_later(&self) -> bool {
        self.plugins_listen::<ev::ItemMergeEvent>()
    }
    /// The merges `due` held back from the item ticks, each pair asked with
    /// `ItemMergeEvent` first; a cancelled pair stays apart.
    pub(super) fn plugin_merge_items(
        &mut self,
        items: &mut [crate::hc::entities::ItemEntity],
        removed: &mut [bool],
        cells: &mut crate::hc::entities::ItemCells,
        due: Vec<usize>,
    ) {
        for i in due {
            if removed[i] {
                continue;
            }
            let catalog = self.res.items();
            let max = |s: &ItemStack| catalog.get(&s.item).map_or(64, |i| i.max_stack).max(1);
            let mergeable = |e: &crate::hc::entities::ItemEntity, age: u32| {
                e.pickup_delay != u32::MAX && age < 6000 && e.stack.count < max(&e.stack)
            };
            if !mergeable(&items[i], items[i].age.saturating_sub(1)) {
                continue;
            }
            let area = items[i].bounds().inflate(Vec3::new(0.5, 0.0, 0.5));
            let pairs: Vec<usize> = (0..items.len())
                .filter(|&j| {
                    let (a, b) = (&items[i], &items[j]);
                    j != i
                        && !removed[j]
                        && b.bounds().intersects(area)
                        && mergeable(b, b.age)
                        && a.target == b.target
                        && a.stack.count + b.stack.count <= max(&b.stack)
                        && a.stack.same_item_same_components(&b.stack)
                })
                .collect();
            let mut denied = Vec::new();
            for j in pairs {
                // The smaller stack goes into the larger.
                let (from, to) = if items[j].stack.count < items[i].stack.count {
                    (j, i)
                } else {
                    (i, j)
                };
                let e = ev::ItemMergeEvent {
                    entity: self.plugin_item_ref(&items[from]),
                    item: super::plugin_events::stack_ref(&items[from].stack),
                    target: self.plugin_item_ref(&items[to]),
                    target_item: super::plugin_events::stack_ref(&items[to].stack),
                    cancelled: false,
                };
                if !self.plugin_allows(None, e) {
                    denied.push(j);
                }
            }
            // A denied neighbour sits out the merge.
            for &j in &denied {
                removed[j] = true;
            }
            let catalog = self.res.items();
            let max_stack = |s: &ItemStack| catalog.get(&s.item).map_or(64, |i| i.max_stack).max(1);
            crate::hc::entities::merge_with_neighbours(items, removed, cells, i, &max_stack);
            for j in denied {
                removed[j] = false;
            }
        }
    }
    /// The dropped items at the end of their life (`removed` by the item
    /// ticks): `ItemDespawnEvent`; a cancelled one lives five more minutes.
    pub(super) fn plugin_items_despawn(
        &mut self,
        items: &mut [crate::hc::entities::ItemEntity],
        removed: &mut [bool],
    ) {
        if !self.plugins_listen::<ev::ItemDespawnEvent>() {
            return;
        }
        for i in 0..items.len() {
            let e = &items[i];
            if !removed[i] || e.age < 6000 || e.position.y <= -128.0 || e.stack.count == 0 {
                continue;
            }
            let event = ev::ItemDespawnEvent {
                entity: self.plugin_item_ref(e),
                item: super::plugin_events::stack_ref(&e.stack),
                location: self.plugin_at([e.position.x, e.position.y, e.position.z]),
                cancelled: false,
            };
            if !self.plugin_allows(None, event) {
                items[i].age = 0;
                removed[i] = false;
            }
        }
    }
    /// A splash potion broke at `at`: `PotionSplashEvent`. The mobs spared,
    /// or None when cancelled.
    pub(super) fn plugin_potion_splash(
        &mut self,
        stack: &ItemStack,
        at: Vec3,
        age: u32,
    ) -> Option<Vec<u32>> {
        if !self.plugins_listen::<ev::PotionSplashEvent>() {
            return Some(Vec::new());
        }
        let intensity = |d2: f64| 1.0 - d2.sqrt() / 4.0;
        let mut affected = Vec::new();
        let mut mobs = Vec::new();
        for m in self.mobs.iter().filter(|m| m.alive()) {
            if let Some(d2) = super::potions::splash_distance_sqr(at, age, m.bounds()) {
                affected.push((self.plugin_mob_ref(m), intensity(d2)));
                mobs.push(m.id);
            }
        }
        if let Some(net) = self.network.as_ref() {
            for g in net
                .hub
                .players
                .iter()
                .filter(|g| net.hub.here(g) && g.loaded && !g.dead)
            {
                let bounds = super::net_mobs::player_bounds(g);
                if let Some(d2) = super::potions::splash_distance_sqr(at, age, bounds) {
                    let player = crate::game::Server::plugin_player(g);
                    affected.push((
                        api::EntityRef::of_player(g.entity_id, player),
                        intensity(d2),
                    ));
                }
            }
        }
        let mut e = ev::PotionSplashEvent {
            potion: super::plugin_events::stack_ref(stack),
            location: self.plugin_at([at.x, at.y, at.z]),
            affected,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled {
            return None;
        }
        let left: HashSet<i32> = e.affected.iter().map(|(r, _)| r.id).collect();
        Some(
            mobs.into_iter()
                .filter(|id| !left.contains(&(*id as i32)))
                .collect(),
        )
    }
    /// A lingering potion broke at `at`: `LingeringPotionSplashEvent`. False
    /// when cancelled.
    pub(super) fn plugin_lingering_splash(&mut self, stack: &ItemStack, at: Vec3) -> bool {
        if !self.plugins_listen::<ev::LingeringPotionSplashEvent>() {
            return true;
        }
        let e = ev::LingeringPotionSplashEvent {
            potion: super::plugin_events::stack_ref(stack),
            location: self.plugin_at([at.x, at.y, at.z]),
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// A lingering potion's cloud is about to affect those `inside` it that
    /// are not among its `victims` and stand within `radius`:
    /// `AreaEffectCloudApplyEvent`. Those the plugins removed leave `inside`;
    /// false when cancelled.
    pub(super) fn plugin_cloud_apply(
        &mut self,
        at: [f64; 3],
        radius: f32,
        victims: &[(super::potions::Victim, i32)],
        inside: &mut Vec<(super::potions::Victim, f64, f64)>,
    ) -> bool {
        use super::{dragon::FightWho, potions::Victim};
        if !self.plugins_listen::<ev::AreaEffectCloudApplyEvent>() {
            return true;
        }
        let mut affected = Vec::new();
        let mut listed = Vec::new();
        for (k, (victim, x, z)) in inside.iter().enumerate() {
            let (dx, dz) = (x - at[0], z - at[2]);
            if victims.iter().any(|(v, _)| v == victim)
                || dx * dx + dz * dz > f64::from(radius * radius)
            {
                continue;
            }
            let entity = match *victim {
                Victim::Mob(id) => Some(self.plugin_entity(None, id as i32)),
                Victim::Player(FightWho::ServerPlayer(conn)) => {
                    self.network.as_ref().and_then(|net| {
                        let g = net.hub.player(conn)?;
                        Some(api::EntityRef::of_player(
                            g.entity_id,
                            crate::game::Server::plugin_player(g),
                        ))
                    })
                }
                Victim::Player(FightWho::Host) => self.plugin_acting_ref(),
            };
            if let Some(entity) = entity {
                affected.push(entity);
                listed.push(k);
            }
        }
        if affected.is_empty() {
            return true;
        }
        let ids: Vec<i32> = affected.iter().map(|e| e.id).collect();
        let mut e = ev::AreaEffectCloudApplyEvent {
            location: self.plugin_at(at),
            radius,
            affected,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled {
            return false;
        }
        let left: HashSet<i32> = e.affected.iter().map(|r| r.id).collect();
        let spared: HashSet<usize> = listed
            .into_iter()
            .zip(ids)
            .filter(|(_, id)| !left.contains(id))
            .map(|(k, _)| k)
            .collect();
        if !spared.is_empty() {
            let mut k = 0;
            inside.retain(|_| {
                k += 1;
                !spared.contains(&(k - 1))
            });
        }
        true
    }
}
