//! Where the server fires the plugins' mobs events (docs/plugins.md): the
//! hooks in the gameplay code (knockback, exhaustion, portals, spawns, block
//! damage, bees, snow trails and Frost Walker, chunks' entities, the dragon's
//! phases) and what changed about mobs and players from one tick to the
//! next (poses, anger, professions, striders' cold, targets, fire, the
//! entities that left). A cancelled change is put back.
use super::server_net::Network;
use crate::hc::mobs::{Mob, MobEvent, MobType, Species, Who};
use horizon_core::geometry::Vec3;
use horizon_server_api::{self as api, Location, event as ev};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Bukkit's `Pose` names by vanilla's `Pose` ids.
const POSES: [&str; 18] = [
    "STANDING",
    "FALL_FLYING",
    "SLEEPING",
    "SWIMMING",
    "SPIN_ATTACK",
    "SNEAKING",
    "LONG_JUMPING",
    "DYING",
    "CROAKING",
    "USING_TONGUE",
    "SITTING",
    "ROARING",
    "SNIFFING",
    "EMERGING",
    "DIGGING",
    "SLIDING",
    "SHOOTING",
    "INHALING",
];

/// What the plugins last saw of a mob.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    pose: u8,
    anger: i32,
    profession: u8,
    cold: bool,
    target: Option<Who>,
    fire: i32,
}

/// A mob or an item as it was last seen, for `EntityRemoveEvent`.
#[derive(Clone)]
struct Present {
    kind: Option<Arc<MobType>>,
    at: Vec3,
    yaw: f32,
    alive: bool,
    /// An item that lay for five minutes.
    expiring: bool,
}

/// Who a hint of fire is about: a mob, or a player by UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Burnt {
    Mob(u32),
    Player(u128),
}

/// What these events keep between ticks.
#[derive(Default)]
pub(super) struct State {
    mobs: HashMap<(super::Dim, u32), Seen>,
    /// Each player's pose and fire.
    players: HashMap<u32, (u8, i32)>,
    present: HashMap<(super::Dim, u32), Present>,
    /// Why entities that are about to go will have gone.
    why: HashMap<(super::Dim, u32), ev::EntityRemoveCause>,
    /// The cause and source of the next push of the player whose turn it is.
    pub(super) knock: Option<(ev::KnockbackCause, Option<api::EntityRef>)>,
    /// The entities that set something alight since the last look.
    burnt_by: HashMap<Burnt, api::EntityRef>,
}

/// The mobs that leave in their own tick, noted while the mobs tick (they
/// are out of the game's list then) for `EntityRemoveEvent`.
#[derive(Default)]
pub(super) struct Leaving {
    on: bool,
    was_removed: bool,
    list: Vec<(u32, ev::EntityRemoveCause)>,
}
impl Leaving {
    /// A mob outside the ticking chunks despawned.
    pub(super) fn despawned(&mut self, m: &Mob) {
        if self.on {
            self.list.push((m.id, ev::EntityRemoveCause::Despawn));
        }
    }
    /// Before the mob's tick.
    pub(super) fn before(&mut self, m: &Mob) {
        self.was_removed = m.removed;
    }
    /// After the mob's tick, with the events it queued.
    pub(super) fn after(&mut self, m: &Mob, kept: bool, events: &[MobEvent]) {
        if !self.on || kept || !m.alive() || self.was_removed {
            return;
        }
        let at = [m.position.x, m.position.y, m.position.z];
        let converted = events
            .iter()
            .any(|e| matches!(e, MobEvent::SpawnKind { at: a, .. } if *a == at));
        let cause = if converted {
            ev::EntityRemoveCause::Transformation
        } else if events.is_empty() {
            // Mob.checkDespawn, at the start of the tick.
            ev::EntityRemoveCause::Despawn
        } else {
            ev::EntityRemoveCause::Discard
        };
        self.list.push((m.id, cause));
    }
}

/// The blocks a damage type comes from, and whether it is the one under the
/// entity's feet.
fn damage_blocks(cause: &str) -> Option<(&'static [&'static str], bool)> {
    Some(match cause {
        "minecraft:cactus" => (&["minecraft:cactus"], false),
        "minecraft:sweet_berry_bush" => (&["minecraft:sweet_berry_bush"], false),
        "minecraft:campfire" => (&["minecraft:campfire", "minecraft:soul_campfire"], false),
        "minecraft:in_fire" => (
            &[
                "minecraft:fire",
                "minecraft:soul_fire",
                "minecraft:campfire",
                "minecraft:soul_campfire",
            ],
            false,
        ),
        "minecraft:lava" => (&["minecraft:lava"], false),
        "minecraft:hot_floor" => (&["minecraft:magma_block"], true),
        "minecraft:stalagmite" => (&["minecraft:pointed_dripstone"], true),
        _ => return None,
    })
}

fn add(a: Vec3, k: [f64; 3]) -> Vec3 {
    Vec3::new(a.x + k[0], a.y + k[1], a.z + k[2])
}

impl crate::game::Server<'_> {
    fn plugin_here(&self, at: [f64; 3]) -> Location {
        Location::new(self.dim.id(), at[0], at[1], at[2])
    }
    /// The player whose turn it is, as an entity.
    fn plugin_mobs_acting(&self) -> Option<api::EntityRef> {
        let player = self.plugin_acting()?;
        let id = self
            .plugin_seen_ids
            .iter()
            .find(|(_, uuid)| *uuid == player.uuid)
            .map_or(0, |(id, _)| *id);
        Some(api::EntityRef::of_player(id, player))
    }
    fn plugin_who_ref(&self, who: Who) -> Option<api::EntityRef> {
        match who {
            Who::Mob(id) => Some(self.plugin_entity(None, id as i32)),
            Who::Player => self.plugin_mobs_acting(),
        }
    }
    /// The first block named in `names` the box (`at`, `width`, `height`)
    /// touches, or the one under it with `below`.
    fn plugin_touched_block(
        &self,
        at: Vec3,
        width: f64,
        height: f64,
        names: &[&str],
        below: bool,
    ) -> Option<api::Block> {
        let half = width / 2.0 + 0.001;
        let (x0, x1) = ((at.x - half).floor() as i32, (at.x + half).floor() as i32);
        let (z0, z1) = ((at.z - half).floor() as i32, (at.z + half).floor() as i32);
        let (y0, y1) = if below {
            let y = (at.y - 0.2).floor() as i32;
            (y, y.max(at.y.floor() as i32))
        } else {
            (
                (at.y - 0.001).floor() as i32,
                (at.y + height + 0.001).floor() as i32,
            )
        };
        let facts = self.res.facts();
        for y in y0..=y1 {
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let name = &facts.get(self.world.block(x, y, z)).name;
                    if names.iter().any(|n| **n == **name) {
                        return Some(self.plugin_block([x, y, z]));
                    }
                }
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Knockback.

    /// `EntityKnockbackEvent` (and `EntityKnockbackByEntityEvent` with a
    /// source) for a push from `before` to `now`: the velocity to keep.
    fn plugin_knockback(
        &mut self,
        net: Option<&Network>,
        entity: api::EntityRef,
        before: Vec3,
        now: Vec3,
        cause: ev::KnockbackCause,
        source: Option<api::EntityRef>,
    ) -> Vec3 {
        let push = [now.x - before.x, now.y - before.y, now.z - before.z];
        if push == [0.0; 3] {
            return now;
        }
        let force = (push[0] * push[0] + push[2] * push[2]).sqrt();
        let mut e = ev::EntityKnockbackEvent {
            entity: entity.clone(),
            cause,
            force,
            knockback: push,
            cancelled: false,
        };
        if self.plugins_listen::<ev::EntityKnockbackEvent>() {
            self.plugin_fire(net, &mut e);
        }
        if let Some(source_entity) = source
            && !e.cancelled
            && self.plugins_listen::<ev::EntityKnockbackByEntityEvent>()
        {
            let mut by = ev::EntityKnockbackByEntityEvent {
                entity,
                source_entity,
                cause,
                force,
                knockback: e.knockback,
                cancelled: false,
            };
            self.plugin_fire(net, &mut by);
            e.knockback = by.knockback;
            e.cancelled = by.cancelled;
        }
        if e.cancelled || !e.knockback.iter().all(|k| k.is_finite()) {
            before
        } else if e.knockback == push {
            now
        } else {
            add(before, e.knockback)
        }
    }
    pub(super) fn plugins_hear_knockback(&self) -> bool {
        self.plugins_listen::<ev::EntityKnockbackEvent>()
            || self.plugins_listen::<ev::EntityKnockbackByEntityEvent>()
    }
    /// The mob `id` was pushed from velocity `before`: the knockback events.
    pub(super) fn plugin_mob_knockback(
        &mut self,
        id: u32,
        before: Vec3,
        cause: ev::KnockbackCause,
        source: Option<Who>,
    ) {
        if !self.plugins_hear_knockback() {
            return;
        }
        let Some(now) = self.mobs.iter().find(|m| m.id == id).map(|m| m.velocity) else {
            return;
        };
        let entity = self.plugin_entity(None, id as i32);
        let source = source.and_then(|s| self.plugin_who_ref(s));
        let kept = self.plugin_knockback(None, entity, before, now, cause, source);
        if kept != now
            && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
        {
            m.velocity = kept;
            m.needs_sync = true;
        }
    }
    /// Velocities of the mobs `ids` before a push, while plugins hear of
    /// knockback.
    pub(super) fn plugin_knock_before(&self, index: usize) -> Option<Vec3> {
        self.plugins_hear_knockback()
            .then(|| self.mobs.get(index).map(|m| m.velocity))
            .flatten()
    }
    /// After a push of the mob at `index` from `before` (taken by
    /// `plugin_knock_before`) by the player whose turn it is.
    pub(super) fn plugin_mob_knocked(
        &mut self,
        index: usize,
        before: Option<Vec3>,
        cause: ev::KnockbackCause,
    ) {
        let (Some(before), Some(id)) = (before, self.mobs.get(index).map(|m| m.id)) else {
            return;
        };
        self.plugin_mob_knockback(id, before, cause, Some(Who::Player));
    }
    /// The player whose turn it is was pushed from velocity `before`:
    /// the knockback events, with the cause the caller left in
    /// `plugin_mobs.knock` (else `Damage`).
    pub(super) fn plugin_player_knockback(&mut self, before: Vec3) {
        let (cause, source) = self
            .plugin_mobs
            .knock
            .take()
            .unwrap_or((ev::KnockbackCause::Damage, None));
        if !self.plugins_hear_knockback() {
            return;
        }
        let (Some(now), Some(entity)) = (
            self.player.as_ref().map(|p| p.velocity),
            self.plugin_mobs_acting(),
        ) else {
            return;
        };
        let kept = self.plugin_knockback(None, entity, before, now, cause, source);
        if kept != now
            && let Some(p) = self.player.as_mut()
        {
            p.velocity = kept;
        }
    }
    /// The cause and source of the next push of the player whose turn it is
    /// (`knock_back_player`), when plugins hear of it.
    pub(super) fn plugin_knock_cause(&mut self, cause: ev::KnockbackCause, source: Option<Who>) {
        if !self.plugins_hear_knockback() {
            return;
        }
        let source = source.and_then(|s| self.plugin_who_ref(s));
        self.plugin_mobs.knock = Some((cause, source));
    }
    /// The same with the source given as an entity.
    pub(super) fn plugin_knock_cause_ref(
        &mut self,
        cause: ev::KnockbackCause,
        source: Option<api::EntityRef>,
    ) {
        if self.plugins_hear_knockback() {
            self.plugin_mobs.knock = Some((cause, source));
        }
    }
    /// The acting player as a knockback source (a PvP hit).
    pub(super) fn plugin_knock_source(&self) -> Option<api::EntityRef> {
        if self.plugins_hear_knockback() {
            self.plugin_mobs_acting()
        } else {
            None
        }
    }

    // -----------------------------------------------------------------------
    // Exhaustion.

    /// The exhaustion the player whose turn it is is about to gain:
    /// `EntityExhaustionEvent`. The amount to add (0 when cancelled).
    pub(super) fn plugin_exhaustion(&mut self, amount: f32, reason: ev::ExhaustionReason) -> f32 {
        if !self.plugins_listen::<ev::EntityExhaustionEvent>() || amount <= 0.0 {
            return amount;
        }
        if self
            .player
            .as_ref()
            .is_none_or(|p| p.invulnerable || p.spectator)
        {
            return amount;
        }
        let Some(entity) = self.plugin_mobs_acting() else {
            return amount;
        };
        let mut e = ev::EntityExhaustionEvent {
            entity,
            reason,
            exhaustion: amount,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled || !e.exhaustion.is_finite() {
            0.0
        } else {
            e.exhaustion.max(0.0)
        }
    }
    /// The exhaustion the acting player's tick already added, as `parts`
    /// (each amount and its reason): `EntityExhaustionEvent` for each, the
    /// cancelled parts taken back and the changed ones as changed.
    pub(super) fn plugin_exhausted(&mut self, parts: &[(f32, ev::ExhaustionReason)]) {
        if !self.plugins_listen::<ev::EntityExhaustionEvent>()
            || parts.iter().all(|(a, _)| *a <= 0.0)
        {
            return;
        }
        let mut delta = 0.0;
        for &(amount, reason) in parts {
            if amount > 0.0 {
                delta += self.plugin_exhaustion(amount, reason) - amount;
            }
        }
        if delta != 0.0
            && let Some(p) = self.player.as_mut()
        {
            let food = &mut p.health.food;
            food.exhaustion = (food.exhaustion + delta).clamp(0.0, 40.0);
        }
    }

    // -----------------------------------------------------------------------
    // Portals.

    /// The entity `id` (a mob, an item, a vehicle) is in the portal block
    /// `pos`: `EntityPortalEnterEvent`. False when cancelled.
    pub(super) fn plugin_portal_enter(
        &mut self,
        net: Option<&Network>,
        entity: Option<api::EntityRef>,
        id: u32,
        pos: [i32; 3],
    ) -> bool {
        if !self.plugins_listen::<ev::EntityPortalEnterEvent>() {
            return true;
        }
        let entity = entity.unwrap_or_else(|| {
            let mut e = self.plugin_entity(net, id as i32);
            if let Some(item) = self.dropped.iter().find(|i| i.id == id) {
                e.kind = "minecraft:item".into();
                e.location = self.plugin_here([item.position.x, item.position.y, item.position.z]);
            }
            e
        });
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let portal_type = if self.res.behaviours().portal_axis(state).is_some() {
            ev::PortalType::Nether
        } else {
            ev::PortalType::Ender
        };
        let e = ev::EntityPortalEnterEvent {
            entity,
            location: self.plugin_here(pos.map(f64::from)),
            portal_type,
            cancelled: false,
        };
        self.plugin_allows(net, e)
    }
    /// The player `conn` is in the portal block `pos`.
    pub(super) fn plugin_player_portal_enter(
        &mut self,
        net: &Network,
        conn: u32,
        pos: [i32; 3],
    ) -> bool {
        if !self.plugins_listen::<ev::EntityPortalEnterEvent>() {
            return true;
        }
        let Some(g) = net.hub.player(conn) else {
            return true;
        };
        let entity = api::EntityRef::of_player(g.entity_id, crate::game::Server::plugin_player(g));
        self.plugin_portal_enter(Some(net), Some(entity), 0, pos)
    }
    /// The entity saved in `tag` comes out of a portal at `at` with
    /// `motion` (`before`: as it went in): `EntityPortalExitEvent`. Where it
    /// comes out and its motion.
    pub(super) fn plugin_portal_exit(
        &mut self,
        tag: &horizon_world::nbt::Tag,
        at: Vec3,
        before: Vec3,
        motion: Vec3,
    ) -> (Vec3, Vec3) {
        if !self.plugins_listen::<ev::EntityPortalExitEvent>() {
            return (at, motion);
        }
        let to = self.plugin_here([at.x, at.y, at.z]);
        let mut e = ev::EntityPortalExitEvent {
            entity: api::EntityRef {
                id: 0,
                kind: crate::hc::entity_nbt::kind(tag).unwrap_or_default(),
                location: to.clone(),
                player: None,
            },
            to,
            before: [before.x, before.y, before.z],
            after: [motion.x, motion.y, motion.z],
        };
        self.plugin_fire(None, &mut e);
        let at = if [e.to.x, e.to.y, e.to.z].iter().all(|v| v.is_finite()) {
            Vec3::new(e.to.x, e.to.y, e.to.z)
        } else {
            at
        };
        let motion = if e.after.iter().all(|v| v.is_finite()) {
            Vec3::new(e.after[0], e.after[1], e.after[2])
        } else {
            motion
        };
        (at, motion)
    }

    // -----------------------------------------------------------------------
    // Coming into the world, blocks.

    /// An entity of `kind` that isn't a mob is about to come into the world
    /// at `at`: `EntitySpawnEvent`. False when cancelled.
    pub(super) fn plugin_entity_spawn(&mut self, kind: &str, id: u32, at: [f64; 3]) -> bool {
        if !self.plugins_listen::<ev::EntitySpawnEvent>() {
            return true;
        }
        let location = self.plugin_here(at);
        let e = ev::EntitySpawnEvent {
            entity: api::EntityRef {
                id: id as i32,
                kind: kind.to_owned(),
                location: location.clone(),
                player: None,
            },
            location,
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// The bee `mob` is about to go into the hive at `at`:
    /// `EntityEnterBlockEvent`. False when cancelled.
    pub(super) fn plugin_enter_block(&mut self, mob: u32, at: [i32; 3]) -> bool {
        if !self.plugins_listen::<ev::EntityEnterBlockEvent>() {
            return true;
        }
        let e = ev::EntityEnterBlockEvent {
            entity: self.plugin_entity(None, mob as i32),
            block: self.plugin_block(at),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, e);
        if allowed {
            self.plugin_mob_leaving(mob, ev::EntityRemoveCause::EnterBlock);
        }
        allowed
    }
    /// A snow golem about to leave snow at `pos` (a `SetBlockName` its
    /// tick queued): `EntityBlockFormEvent`. False when cancelled.
    pub(super) fn plugin_snow_trail(&mut self, pos: [i32; 3], name: &str) -> bool {
        if !self.plugins_listen::<ev::EntityBlockFormEvent>() || name != "minecraft:snow" {
            return true;
        }
        let Some(golem) = self.mobs.iter().find(|m| {
            m.species() == Species::SnowGolem
                && m.position.y.floor() as i32 == pos[1]
                && (m.position.x - (f64::from(pos[0]) + 0.5)).abs() <= 1.0
                && (m.position.z - (f64::from(pos[2]) + 0.5)).abs() <= 1.0
        }) else {
            return true;
        };
        let entity = self.plugin_entity(None, golem.id as i32);
        let e = ev::EntityBlockFormEvent {
            entity,
            block: self.plugin_block(pos),
            new_state: name.to_owned(),
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }
    /// The player whose turn it is is about to form `state` at `pos`
    /// (Frost Walker): `EntityBlockFormEvent`. False when cancelled.
    pub(super) fn plugin_player_block_form(
        &mut self,
        pos: [i32; 3],
        state: horizon_world::registry::BlockStateId,
    ) -> bool {
        if !self.plugins_listen::<ev::EntityBlockFormEvent>() {
            return true;
        }
        let Some(entity) = self.plugin_mobs_acting() else {
            return true;
        };
        let e = ev::EntityBlockFormEvent {
            entity,
            block: self.plugin_block(pos),
            new_state: self.state_string(state),
            cancelled: false,
        };
        self.plugin_allows(None, e)
    }

    // -----------------------------------------------------------------------
    // Damage from blocks.

    /// Whether the surroundings' damage waits for `EntityDamageByBlockEvent`.
    pub(super) fn plugins_hear_block_damage(&self) -> bool {
        self.plugins_listen::<ev::EntityDamageByBlockEvent>()
    }
    /// `EntityDamageByBlockEvent` for a hit of `cause` on `entity` (whose box
    /// is `at`, `width`, `height`): the amount, None when cancelled.
    fn plugin_block_damage(
        &mut self,
        entity: api::EntityRef,
        box_: (Vec3, f64, f64),
        cause: &str,
        amount: f32,
    ) -> Option<f32> {
        let Some((names, below)) = damage_blocks(cause) else {
            return Some(amount);
        };
        let damager = self.plugin_touched_block(box_.0, box_.1, box_.2, names, below);
        let mut e = ev::EntityDamageByBlockEvent {
            entity,
            damager,
            cause: cause.to_owned(),
            damage: f64::from(amount),
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled {
            None
        } else if e.damage.is_finite() {
            Some(e.damage.max(0.0) as f32)
        } else {
            Some(amount)
        }
    }
    /// Before `plugin_player_env_damage`: `EntityDamageByBlockEvent` for the
    /// held hits of blocks on the player whose turn it is.
    pub(super) fn plugin_player_block_damage(&mut self) {
        if !self.plugins_hear_block_damage() {
            return;
        }
        let Some(p) = self.player.as_mut() else {
            return;
        };
        if p.env_damage.is_empty() {
            return;
        }
        let held = std::mem::take(&mut p.env_damage);
        let at = p.position;
        let Some(entity) = self.plugin_mobs_acting() else {
            if let Some(p) = self.player.as_mut() {
                p.env_damage = held;
            }
            return;
        };
        let mut kept = Vec::with_capacity(held.len());
        for (cause, amount) in held {
            if let Some(amount) =
                self.plugin_block_damage(entity.clone(), (at, 0.6, 1.8), cause, amount)
            {
                kept.push((cause, amount));
            }
        }
        if let Some(p) = self.player.as_mut() {
            kept.append(&mut p.env_damage);
            p.env_damage = kept;
        }
    }
    /// Before `plugin_mob_env_damage`: `EntityDamageByBlockEvent` for the
    /// held hits of blocks on the mobs.
    pub(super) fn plugin_mobs_block_damage(&mut self) {
        if !self.plugins_hear_block_damage() {
            return;
        }
        let held: Vec<_> = self
            .mobs
            .iter_mut()
            .filter(|m| !m.env_damage.is_empty())
            .map(|m| {
                let b = (m.position, f64::from(m.width()), f64::from(m.height()));
                (m.id, b, std::mem::take(&mut m.env_damage))
            })
            .collect();
        for (id, b, hits) in held {
            let mut kept = Vec::with_capacity(hits.len());
            for (amount, kind) in hits {
                let cause = super::creatures::damage_type(kind, false);
                let entity = self.plugin_entity(None, id as i32);
                if let Some(amount) = self.plugin_block_damage(entity, b, cause, amount) {
                    kept.push((amount, kind));
                }
            }
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                m.env_damage = kept;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Fire from an entity.

    fn plugins_hear_combust_by(&self) -> bool {
        self.plugins_listen::<ev::EntityCombustByEntityEvent>()
            || self.plugins_listen::<ev::EntityCombustByBlockEvent>()
    }
    /// `who` is about to be set alight by the entity `id` of `kind` at
    /// `at` (an arrow, a fireball, a bolt): noted for the next look.
    pub(super) fn plugin_combust_by(&mut self, who: Who, id: i32, kind: &str, at: [f64; 3]) {
        if !self.plugins_listen::<ev::EntityCombustByEntityEvent>() {
            return;
        }
        let combuster = api::EntityRef {
            id,
            kind: kind.to_owned(),
            location: self.plugin_here(at),
            player: None,
        };
        self.plugin_burnt_by(who, combuster);
    }
    /// The mob at `index` is about to be set alight by the player whose
    /// turn it is (Fire Aspect).
    pub(super) fn plugin_combust_by_acting(&mut self, index: usize) {
        if !self.plugins_listen::<ev::EntityCombustByEntityEvent>() {
            return;
        }
        let (Some(id), Some(by)) = (
            self.mobs.get(index).map(|m| m.id),
            self.plugin_mobs_acting(),
        ) else {
            return;
        };
        self.plugin_burnt_by(Who::Mob(id), by);
    }
    fn plugin_burnt_by(&mut self, who: Who, by: api::EntityRef) {
        let key = match who {
            Who::Mob(id) => Burnt::Mob(id),
            Who::Player => match self.net_acting_uuid {
                Some(uuid) => Burnt::Player(uuid),
                None => return,
            },
        };
        self.plugin_mobs.burnt_by.insert(key, by);
    }
    /// A mob or player that caught fire (from `before` to `now` fire ticks):
    /// `EntityCombustByEntityEvent` when an entity was noted setting it
    /// alight, else `EntityCombustByBlockEvent` when it is in fire or lava.
    /// The fire ticks to keep.
    fn plugin_combusted(
        &mut self,
        net: Option<&Network>,
        key: Burnt,
        entity: api::EntityRef,
        box_: (Vec3, f64, f64),
        before: i32,
        now: i32,
    ) -> i32 {
        let duration = now as f32 / 20.0;
        let (cancelled, duration) = if let Some(by) = self.plugin_mobs.burnt_by.remove(&key) {
            if !self.plugins_listen::<ev::EntityCombustByEntityEvent>() {
                return now;
            }
            let mut e = ev::EntityCombustByEntityEvent {
                entity,
                combuster: by,
                duration,
                cancelled: false,
            };
            self.plugin_fire(net, &mut e);
            (e.cancelled, e.duration)
        } else {
            if !self.plugins_listen::<ev::EntityCombustByBlockEvent>() {
                return now;
            }
            let names = ["minecraft:fire", "minecraft:soul_fire", "minecraft:lava"];
            let Some(block) = self.plugin_touched_block(box_.0, box_.1, box_.2, &names, false)
            else {
                return now;
            };
            let mut e = ev::EntityCombustByBlockEvent {
                entity,
                combuster: Some(block),
                duration,
                cancelled: false,
            };
            self.plugin_fire(net, &mut e);
            (e.cancelled, e.duration)
        };
        if cancelled {
            before
        } else if duration.is_finite() && (duration * 20.0) as i32 != now {
            (duration * 20.0) as i32
        } else {
            now
        }
    }

    // -----------------------------------------------------------------------
    // What changed about the mobs since the last tick.

    fn plugins_watch_mob_changes(&self) -> bool {
        self.plugins_listen::<ev::EntityPoseChangeEvent>()
            || self.plugins_listen::<ev::PigZombieAngerEvent>()
            || self.plugins_listen::<ev::VillagerCareerChangeEvent>()
            || self.plugins_listen::<ev::StriderTemperatureChangeEvent>()
            || self.plugins_listen::<ev::EntityTargetEvent>()
            || self.plugins_hear_combust_by()
    }
    fn mobs_group_seen(m: &Mob) -> Seen {
        Seen {
            pose: crate::hc::mobs::sync::pose_id(m),
            anger: m.extra.anger,
            profession: m.extra.variant2,
            cold: crate::hc::mobs::strider::suffocating(m),
            target: m.ai.target,
            fire: m.fire_ticks,
        }
    }
    /// Before the mob ticks: what to note of those that leave in them.
    pub(super) fn plugin_leaving(&self) -> Leaving {
        Leaving {
            on: self.plugins_listen::<ev::EntityRemoveEvent>(),
            ..Default::default()
        }
    }
    /// Why the mob `id` is about to leave, for `EntityRemoveEvent`.
    pub(super) fn plugin_mob_leaving(&mut self, id: u32, cause: ev::EntityRemoveCause) {
        if self.plugins_listen::<ev::EntityRemoveEvent>() {
            self.plugin_mobs.why.insert((self.dim, id), cause);
        }
    }
    /// Once a tick, after the mobs ticked and before the entities group's
    /// watchers: `EntityRemoveEvent` for the mobs and items gone since the
    /// last tick, then `EntityPoseChangeEvent`, `PigZombieAngerEvent`,
    /// `VillagerCareerChangeEvent`, `StriderTemperatureChangeEvent`,
    /// `EntityTargetEvent`, `EntityCombustByEntityEvent` and
    /// `EntityCombustByBlockEvent` for what changed about each mob;
    /// cancelled changes go back.
    pub(super) fn plugin_watch_mob_changes(&mut self, leaving: Leaving) {
        let dim = self.dim;
        for (id, cause) in leaving.list {
            self.plugin_mobs.why.insert((dim, id), cause);
        }
        self.plugin_watch_removed();
        if !self.plugins_watch_mob_changes() {
            self.plugin_mobs.mobs.clear();
            self.plugin_mobs.burnt_by.clear();
            return;
        }
        let live: HashSet<u32> = self.mobs.iter().map(|m| m.id).collect();
        self.plugin_mobs
            .mobs
            .retain(|(d, id), _| *d != dim || live.contains(id));
        let ids: Vec<u32> = self
            .mobs
            .iter()
            .filter(|m| m.alive() && !m.removed)
            .map(|m| m.id)
            .collect();
        for id in ids {
            let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
                continue;
            };
            let now = Self::mobs_group_seen(m);
            let Some(before) = self.plugin_mobs.mobs.insert((dim, id), now) else {
                continue;
            };
            if before == now {
                continue;
            }
            self.plugin_mob_seen_changes(id, before, now);
            if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
                let seen = Self::mobs_group_seen(m);
                self.plugin_mobs.mobs.insert((dim, id), seen);
            }
        }
        self.plugin_mobs
            .burnt_by
            .retain(|k, _| matches!(k, Burnt::Player(_)));
    }
    fn plugin_mob_seen_changes(&mut self, id: u32, before: Seen, now: Seen) {
        let Some(species) = self.mobs.iter().find(|m| m.id == id).map(|m| m.species()) else {
            return;
        };
        if before.pose != now.pose && self.plugins_listen::<ev::EntityPoseChangeEvent>() {
            let mut e = ev::EntityPoseChangeEvent {
                entity: self.plugin_entity(None, id as i32),
                pose: POSES[usize::from(now.pose) % POSES.len()].to_owned(),
            };
            self.plugin_fire(None, &mut e);
        }
        if species == Species::ZombifiedPiglin
            && now.anger > before.anger
            && self.plugins_listen::<ev::PigZombieAngerEvent>()
        {
            let mut e = ev::PigZombieAngerEvent {
                entity: self.plugin_entity(None, id as i32),
                target: now.target.and_then(|t| self.plugin_who(id, t)),
                new_anger: now.anger,
                cancelled: false,
            };
            self.plugin_fire(None, &mut e);
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                if e.cancelled {
                    m.extra.anger = before.anger;
                    if before.anger <= 0 {
                        m.ai.target = before.target;
                    }
                } else {
                    m.extra.anger = e.new_anger.max(0);
                }
            }
        }
        if species == Species::Villager
            && before.profession != now.profession
            && self.plugins_listen::<ev::VillagerCareerChangeEvent>()
        {
            let names = crate::hc::mobs::render::PROFESSIONS;
            let name = names[usize::from(now.profession) % names.len()];
            let e = ev::VillagerCareerChangeEvent {
                entity: self.plugin_entity(None, id as i32),
                profession: format!("minecraft:{name}"),
                reason: if now.profession == 0 {
                    ev::CareerChangeReason::LosingJob
                } else {
                    ev::CareerChangeReason::Employed
                },
                cancelled: false,
            };
            if !self.plugin_allows(None, e)
                && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
            {
                m.extra.variant2 = before.profession;
                m.needs_sync = true;
            }
        }
        if species == Species::Strider
            && before.cold != now.cold
            && self.plugins_listen::<ev::StriderTemperatureChangeEvent>()
        {
            let e = ev::StriderTemperatureChangeEvent {
                entity: self.plugin_entity(None, id as i32),
                shivering: now.cold,
                cancelled: false,
            };
            if !self.plugin_allows(None, e)
                && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
            {
                m.extra.flag = before.cold;
                m.needs_sync = true;
            }
        }
        let target = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.ai.target);
        if before.target != target && self.plugins_listen::<ev::EntityTargetEvent>() {
            self.plugin_target_changed(id, species, before.target, target);
        }
        if before.fire <= 0 && now.fire > 0 && self.plugins_hear_combust_by() {
            let Some(b) = self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map(|m| (m.position, f64::from(m.width()), f64::from(m.height())))
            else {
                return;
            };
            let entity = self.plugin_entity(None, id as i32);
            let fire =
                self.plugin_combusted(None, Burnt::Mob(id), entity, b, before.fire, now.fire);
            if fire != now.fire
                && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
            {
                m.fire_ticks = fire;
            }
        }
    }
    /// `EntityTargetEvent` for the mob `id`, whose target went from `old` to
    /// `new`.
    fn plugin_target_changed(
        &mut self,
        id: u32,
        species: Species,
        old: Option<Who>,
        new: Option<Who>,
    ) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let reason = match new {
            None => {
                let gone = match old {
                    Some(Who::Mob(o)) => !self.mobs.iter().any(|t| t.id == o && t.alive()),
                    _ => false,
                };
                if gone {
                    ev::TargetReason::TargetDied
                } else {
                    ev::TargetReason::ForgotTarget
                }
            }
            Some(t) if m.ai.last_hurt_by == Some(t) => ev::TargetReason::TargetAttackedEntity,
            Some(_) if species == Species::ZombifiedPiglin && m.extra.anger > 0 => {
                ev::TargetReason::PigZombieTarget
            }
            Some(Who::Player) => ev::TargetReason::ClosestPlayer,
            Some(Who::Mob(_)) => ev::TargetReason::ClosestEntity,
        };
        let target = new.and_then(|t| self.plugin_who(id, t));
        let mut e = ev::EntityTargetEvent {
            entity: self.plugin_entity(None, id as i32),
            target: target.clone(),
            reason,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        let set = if e.cancelled {
            Some(old)
        } else if e.target.as_ref().map(|t| t.id) != target.as_ref().map(|t| t.id) {
            match &e.target {
                None => Some(None),
                Some(t) if t.player.is_some() => Some(Some(Who::Player)),
                Some(t) => u32::try_from(t.id)
                    .ok()
                    .filter(|&o| o != id && self.mobs.iter().any(|m| m.id == o))
                    .map(|o| Some(Who::Mob(o))),
            }
        } else {
            None
        };
        if let Some(to) = set
            && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
        {
            m.ai.target = to;
        }
    }
    /// `EntityRemoveEvent` for the mobs and dropped items of this level
    /// gone since the last look.
    fn plugin_watch_removed(&mut self) {
        let dim = self.dim;
        if !self.plugins_listen::<ev::EntityRemoveEvent>() {
            self.plugin_mobs.present.clear();
            self.plugin_mobs.why.clear();
            return;
        }
        let mut now: HashMap<(super::Dim, u32), Present> =
            HashMap::with_capacity(self.mobs.len() + self.dropped.len());
        for m in &self.mobs {
            now.insert(
                (dim, m.id),
                Present {
                    kind: Some(m.kind.clone()),
                    at: m.position,
                    yaw: m.yaw,
                    alive: m.alive(),
                    expiring: false,
                },
            );
        }
        for e in &self.dropped {
            now.insert(
                (dim, e.id),
                Present {
                    kind: None,
                    at: e.position,
                    yaw: 0.0,
                    alive: true,
                    expiring: e.age >= 5999,
                },
            );
        }
        let before = std::mem::take(&mut self.plugin_mobs.present);
        let mut kept = HashMap::with_capacity(before.len());
        for (key, p) in before {
            if key.0 != dim {
                kept.insert(key, p);
                continue;
            }
            if now.contains_key(&key) {
                continue;
            }
            let why = self.plugin_mobs.why.remove(&key);
            let cause = why.unwrap_or(if !p.alive {
                ev::EntityRemoveCause::Death
            } else if p.expiring {
                ev::EntityRemoveCause::Despawn
            } else {
                ev::EntityRemoveCause::Discard
            });
            let kind = p.kind.as_ref().map_or_else(
                || "minecraft:item".to_owned(),
                |k| crate::hc::command::args::normalize(&k.id),
            );
            let mut location = self.plugin_here([p.at.x, p.at.y, p.at.z]);
            location.yaw = p.yaw;
            let mut e = ev::EntityRemoveEvent {
                entity: api::EntityRef {
                    id: key.1 as i32,
                    kind,
                    location,
                    player: None,
                },
                cause,
            };
            self.plugin_fire(None, &mut e);
        }
        kept.extend(now);
        self.plugin_mobs.present = kept;
        let present = &self.plugin_mobs.present;
        self.plugin_mobs.why.retain(|k, _| present.contains_key(k));
    }

    // -----------------------------------------------------------------------
    // Players.

    /// Once a tick, before the entities group's watcher of the players'
    /// bodies: `EntityPoseChangeEvent`, `EntityCombustByEntityEvent` and
    /// `EntityCombustByBlockEvent` for each player.
    pub(super) fn plugin_watch_player_mobs(&mut self, net: &mut Network) {
        let poses = self.plugins_listen::<ev::EntityPoseChangeEvent>();
        if !poses && !self.plugins_hear_combust_by() {
            self.plugin_mobs.players.clear();
            self.plugin_mobs.burnt_by.clear();
            return;
        }
        let live: HashSet<u32> = net
            .hub
            .players
            .iter()
            .filter(|g| g.placed)
            .map(|g| g.conn)
            .collect();
        self.plugin_mobs.players.retain(|c, _| live.contains(c));
        for i in 0..net.hub.players.len() {
            let g = &net.hub.players[i];
            let Some(body) = g.motion.as_ref().filter(|_| g.placed) else {
                continue;
            };
            let pose = match g.pose() {
                crate::hc::pose::Pose::Standing => 0,
                crate::hc::pose::Pose::FallFlying => 1,
                crate::hc::pose::Pose::Sleeping => 2,
                crate::hc::pose::Pose::Swimming => 3,
                crate::hc::pose::Pose::SpinAttack => 4,
                crate::hc::pose::Pose::Crouching => 5,
            };
            let (conn, fire, at, uuid) = (g.conn, body.fire_ticks, body.position, g.uuid);
            let Some((old_pose, old_fire)) = self.plugin_mobs.players.insert(conn, (pose, fire))
            else {
                continue;
            };
            if old_pose == pose && (old_fire > 0 || fire <= 0) {
                continue;
            }
            let entity =
                api::EntityRef::of_player(g.entity_id, crate::game::Server::plugin_player(g));
            if old_pose != pose && poses {
                let mut e = ev::EntityPoseChangeEvent {
                    entity: entity.clone(),
                    pose: POSES[usize::from(pose)].to_owned(),
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if old_fire <= 0 && fire > 0 {
                let kept = self.plugin_combusted(
                    Some(net),
                    Burnt::Player(uuid),
                    entity,
                    (at, 0.6, 1.8),
                    old_fire,
                    fire,
                );
                if kept != fire
                    && let Some(body) = net.hub.players[i].motion.as_mut()
                {
                    body.fire_ticks = kept;
                    self.plugin_mobs.players.insert(conn, (pose, kept));
                }
            }
        }
        self.plugin_mobs
            .burnt_by
            .retain(|k, _| matches!(k, Burnt::Mob(_)));
    }

    // -----------------------------------------------------------------------
    // The dragon, chunks' entities.

    /// The dragon `id` at `at` is about to go from phase `from` to `to`:
    /// `EnderDragonChangePhaseEvent`. None when cancelled, else the phase.
    pub(super) fn plugin_dragon_phase(
        &mut self,
        id: u32,
        at: [f64; 3],
        from: i32,
        to: i32,
    ) -> Option<i32> {
        if !self.plugins_listen::<ev::EnderDragonChangePhaseEvent>() {
            return Some(to);
        }
        let mut e = ev::EnderDragonChangePhaseEvent {
            entity: api::EntityRef {
                id: id as i32,
                kind: "minecraft:ender_dragon".into(),
                location: self.plugin_here(at),
                player: None,
            },
            current_phase: Some(ev::DragonPhase::from_id(from)),
            new_phase: ev::DragonPhase::from_id(to),
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        (!e.cancelled).then_some(e.new_phase.id())
    }
    /// Whether plugins hear of the dragon's phases.
    pub(super) fn plugins_hear_dragon_phase(&self) -> bool {
        self.plugins_listen::<ev::EnderDragonChangePhaseEvent>()
    }
    /// The entities of this level in chunk `key`, as plugins see them.
    fn plugin_chunk_entities(&self, key: (i32, i32), since: u32) -> Vec<api::EntityRef> {
        let inside = |p: Vec3, id: u32| {
            id > since && ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4) == key
        };
        let mut out: Vec<api::EntityRef> = self
            .mobs
            .iter()
            .filter(|m| inside(m.position, m.id))
            .map(|m| self.plugin_entity(None, m.id as i32))
            .collect();
        let item = |id: u32, kind: &str, at: Vec3| api::EntityRef {
            id: id as i32,
            kind: kind.to_owned(),
            location: self.plugin_here([at.x, at.y, at.z]),
            player: None,
        };
        out.extend(
            self.dropped
                .iter()
                .filter(|e| inside(e.position, e.id))
                .map(|e| item(e.id, "minecraft:item", e.position)),
        );
        out.extend(
            self.orbs
                .iter()
                .filter(|o| inside(o.position, o.id))
                .map(|o| item(o.id, "minecraft:experience_orb", o.position)),
        );
        out.extend(
            self.boats
                .iter()
                .filter(|b| inside(b.position, b.id))
                .map(|b| item(b.id, &b.entity_type(), b.position)),
        );
        out.extend(
            self.carts
                .iter()
                .filter(|c| inside(c.position, c.id))
                .map(|c| item(c.id, "minecraft:minecart", c.position)),
        );
        out.extend(
            self.armor_stands
                .iter()
                .filter(|a| inside(a.position, a.id))
                .map(|a| item(a.id, "minecraft:armor_stand", a.position)),
        );
        out
    }
    /// Before a chunk's saved entities join the world: the entity counter,
    /// for `plugin_entities_loaded`.
    pub(super) fn plugin_load_mark(&self) -> Option<u32> {
        self.plugins_listen::<ev::EntitiesLoadEvent>()
            .then_some(self.next_entity)
    }
    /// The saved entities of chunk `key` joined the world (those with ids
    /// after `mark`): `EntitiesLoadEvent`.
    pub(super) fn plugin_entities_loaded(&mut self, key: (i32, i32), mark: Option<u32>) {
        let Some(mark) = mark else {
            return;
        };
        let mut e = ev::EntitiesLoadEvent {
            world: self.dim.id().to_owned(),
            chunk_x: key.0,
            chunk_z: key.1,
            entities: self.plugin_chunk_entities(key, mark),
        };
        self.plugin_fire(None, &mut e);
    }
    /// The entities of the chunks `keys` are about to leave with them:
    /// `EntitiesUnloadEvent` for each chunk with entities, and the
    /// `EntityRemoveEvent` cause to come.
    pub(super) fn plugin_entities_unloading(&mut self, keys: &HashSet<(i32, i32)>) {
        let unload = self.plugins_listen::<ev::EntitiesUnloadEvent>();
        let remove = self.plugins_listen::<ev::EntityRemoveEvent>();
        if !unload && !remove {
            return;
        }
        let dim = self.dim;
        for &key in keys {
            let entities = self.plugin_chunk_entities(key, 0);
            if entities.is_empty() {
                continue;
            }
            if remove {
                for e in &entities {
                    self.plugin_mobs
                        .why
                        .insert((dim, e.id as u32), ev::EntityRemoveCause::Unload);
                }
            }
            if unload {
                let mut e = ev::EntitiesUnloadEvent {
                    world: dim.id().to_owned(),
                    chunk_x: key.0,
                    chunk_z: key.1,
                    entities,
                };
                self.plugin_fire(None, &mut e);
            }
        }
    }
}
