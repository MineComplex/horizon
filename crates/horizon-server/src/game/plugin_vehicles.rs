//! Where the server fires the plugins' vehicles events (docs/plugins.md):
//! hanging entities (paintings, item frames, leash knots) going up and
//! coming down, boats and minecarts coming into the world, hurt, broken,
//! moving and bumping into things, entities a player places, and leads
//! coming off.
use super::boats::Boat;
use super::minecarts::Minecart;
use crate::hc::entities::{ItemFrame, Painting};
use crate::hc::items::ItemStack;
use horizon_core::geometry::Vec3;
use horizon_server_api::{self as api, BlockPos, Location, event as ev};
use std::collections::HashMap;

/// What these events keep between ticks.
#[derive(Default)]
pub(super) struct State {
    /// Where each boat and minecart was at the last tick, by level and id
    /// (`VehicleMoveEvent`).
    seen: HashMap<(String, u32), Location>,
    /// Who is hitting a vehicle: set by a player's attack, read by the
    /// `VehicleDamageEvent` it causes.
    attacker: Option<api::EntityRef>,
    /// The attacker of the vehicle being hurt, for its `VehicleDestroyEvent`.
    hurting: Option<api::EntityRef>,
    /// Why the next lead comes off (`EntityUnleashEvent`).
    unleash: Option<ev::UnleashReason>,
}

fn face_of(d: [i32; 3]) -> ev::BlockFace {
    ev::BlockFace::from_offset(d)
}

impl crate::game::Server<'_> {
    fn pv_location(&self, at: [f64; 3], yaw: f32) -> Location {
        let mut l = Location::new(self.dim.id(), at[0], at[1], at[2]);
        l.yaw = yaw;
        l
    }
    fn pv_ref(&self, id: u32, kind: &str, at: [f64; 3], yaw: f32) -> api::EntityRef {
        api::EntityRef {
            id: id as i32,
            kind: kind.to_owned(),
            location: self.pv_location(at, yaw),
            player: None,
        }
    }
    fn plugin_boat_ref(&self, b: &Boat) -> api::EntityRef {
        let p = b.position;
        self.pv_ref(b.id, &b.entity_type(), [p.x, p.y, p.z], b.yaw)
    }
    fn plugin_cart_ref(&self, c: &Minecart) -> api::EntityRef {
        let p = c.position;
        self.pv_ref(c.id, c.kind.id(), [p.x, p.y, p.z], c.yaw)
    }
    /// A boat or minecart of this level by id, else any other entity.
    fn plugin_vehicle_or_entity(&self, id: u32) -> api::EntityRef {
        if let Some(b) = self.boats.iter().find(|b| b.id == id) {
            return self.plugin_boat_ref(b);
        }
        if let Some(c) = self.carts.iter().find(|c| c.id == id) {
            return self.plugin_cart_ref(c);
        }
        self.plugin_entity(None, id as i32)
    }
    fn plugin_painting_ref(&self, p: &Painting) -> api::EntityRef {
        let (lo, hi) = p.bounds();
        let at = std::array::from_fn(|i| (lo[i] + hi[i]) / 2.0);
        self.pv_ref(p.id, "minecraft:painting", at, 0.0)
    }
    fn plugin_frame_ref(&self, f: &ItemFrame) -> api::EntityRef {
        let (lo, hi) = f.bounds();
        let at = std::array::from_fn(|i| (lo[i] + hi[i]) / 2.0);
        self.pv_ref(f.id, f.kind(), at, 0.0)
    }
    /// The entity of the player whose turn it is.
    fn plugin_acting_player_entity(&self) -> Option<api::EntityRef> {
        let player = self.plugin_acting()?;
        let id = self
            .plugin_seen_ids
            .iter()
            .find(|(_, uuid)| *uuid == player.uuid)
            .map_or(0, |(id, _)| *id);
        Some(api::EntityRef::of_player(id, player))
    }

    // -----------------------------------------------------------------------
    // Hanging entities.

    /// The player whose turn it is hangs `entity` on the block `on` (`face`:
    /// the face clicked, None for a knot), from `item`: `HangingPlaceEvent`.
    /// False when cancelled.
    fn plugin_hanging_place(
        &mut self,
        entity: api::EntityRef,
        on: [i32; 3],
        face: Option<[i32; 3]>,
        item: Option<&ItemStack>,
    ) -> bool {
        if !self.plugins_listen::<ev::HangingPlaceEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::HangingPlaceEvent {
            player,
            entity,
            block: self.plugin_block(on),
            face: face.map(face_of),
            item: item.map(super::plugin_events::stack_ref),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }
    /// `HangingPlaceEvent` for the painting `p`, hung from `stack`.
    pub(super) fn plugin_painting_place(&mut self, p: &Painting, stack: &ItemStack) -> bool {
        if !self.plugins_listen::<ev::HangingPlaceEvent>() {
            return true;
        }
        let s = p.step();
        let on = std::array::from_fn(|i| p.pos[i] - s[i]);
        let entity = self.plugin_painting_ref(p);
        self.plugin_hanging_place(entity, on, Some(s), Some(stack))
    }
    /// `HangingPlaceEvent` for the item frame `f`, hung from `stack`.
    pub(super) fn plugin_frame_place(&mut self, f: &ItemFrame, stack: &ItemStack) -> bool {
        if !self.plugins_listen::<ev::HangingPlaceEvent>() {
            return true;
        }
        let s = f.step();
        let on = std::array::from_fn(|i| f.pos[i] - s[i]);
        let entity = self.plugin_frame_ref(f);
        self.plugin_hanging_place(entity, on, Some(s), Some(stack))
    }
    /// `HangingPlaceEvent` for a new leash knot on the fence at `pos`.
    pub(super) fn plugin_knot_place(&mut self, pos: [i32; 3]) -> bool {
        if !self.plugins_listen::<ev::HangingPlaceEvent>() {
            return true;
        }
        let id = self.knot_ids.get(&pos).copied().unwrap_or(0);
        let c = crate::hc::mobs::leash::knot_state(pos).center;
        let entity = self.pv_ref(id, "minecraft:leash_knot", [c.x, c.y, c.z], 0.0);
        self.plugin_hanging_place(entity, pos, None, None)
    }
    /// `HangingBreakByEntityEvent` (with a remover) and `HangingBreakEvent`.
    /// False when cancelled.
    fn plugin_hanging_break(
        &mut self,
        entity: api::EntityRef,
        cause: ev::RemoveCause,
        remover: Option<api::EntityRef>,
    ) -> bool {
        let mut cancelled = false;
        if let Some(r) = remover.clone()
            && self.plugins_listen::<ev::HangingBreakByEntityEvent>()
        {
            let mut e = ev::HangingBreakByEntityEvent {
                entity: entity.clone(),
                remover: r,
                cause,
                cancelled: false,
            };
            self.plugin_fire(None, &mut e);
            cancelled = e.cancelled;
        }
        if self.plugins_listen::<ev::HangingBreakEvent>() {
            let mut e = ev::HangingBreakEvent {
                entity,
                cause,
                remover,
                cancelled,
            };
            self.plugin_fire(None, &mut e);
            cancelled = e.cancelled;
        }
        !cancelled
    }
    fn plugins_hear_hanging_break(&self) -> bool {
        self.plugins_listen::<ev::HangingBreakEvent>()
            || self.plugins_listen::<ev::HangingBreakByEntityEvent>()
    }
    /// Why a hanging entity at `pos` that no longer survives comes down:
    /// something in its block (`OBSTRUCTION`), else its support (`PHYSICS`).
    fn plugin_hanging_cause(&self, pos: [i32; 3]) -> ev::RemoveCause {
        if self
            .res
            .facts()
            .is_air(self.world.block(pos[0], pos[1], pos[2]))
        {
            ev::RemoveCause::Physics
        } else {
            ev::RemoveCause::Obstruction
        }
    }
    /// The painting at `index` is about to break (`by_player`: the player
    /// whose turn it is hit it, else it no longer survives). False when
    /// cancelled.
    pub(super) fn plugin_painting_break(&mut self, index: usize, by_player: bool) -> bool {
        if !self.plugins_hear_hanging_break() {
            return true;
        }
        let Some(p) = self.paintings.get(index).cloned() else {
            return true;
        };
        let entity = self.plugin_painting_ref(&p);
        let (cause, remover) = if by_player {
            (ev::RemoveCause::Entity, self.plugin_acting_player_entity())
        } else {
            (self.plugin_hanging_cause(p.pos), None)
        };
        self.plugin_hanging_break(entity, cause, remover)
    }
    /// The item frame `f` is about to break (`by_player`: the player whose
    /// turn it is hit it). False when cancelled.
    pub(super) fn plugin_frame_break(&mut self, f: &ItemFrame, by_player: bool) -> bool {
        if !self.plugins_hear_hanging_break() {
            return true;
        }
        let entity = self.plugin_frame_ref(f);
        let (cause, remover) = if by_player {
            (ev::RemoveCause::Entity, self.plugin_acting_player_entity())
        } else {
            (self.plugin_hanging_cause(f.pos), None)
        };
        self.plugin_hanging_break(entity, cause, remover)
    }

    // -----------------------------------------------------------------------
    // Placing entities, vehicles coming into the world.

    /// The player whose turn it is places the entity `kind` (`id`, at `at`
    /// facing `yaw`) against `on`'s `face`: `EntityPlaceEvent`. False when
    /// cancelled.
    pub(super) fn plugin_entity_place(
        &mut self,
        (id, kind, at, yaw): (u32, &str, Vec3, f32),
        on: [i32; 3],
        face: [i32; 3],
    ) -> bool {
        if !self.plugins_listen::<ev::EntityPlaceEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::EntityPlaceEvent {
            player,
            entity: self.pv_ref(id, kind, [at.x, at.y, at.z], yaw),
            block: self.plugin_block(on),
            face: face_of(face),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }
    /// The boat `b` is about to come into the world: `VehicleCreateEvent`.
    /// False when cancelled.
    pub(super) fn plugin_boat_create(&mut self, b: &Boat) -> bool {
        if !self.plugins_listen::<ev::VehicleCreateEvent>() {
            return true;
        }
        let vehicle = self.plugin_boat_ref(b);
        self.plugin_vehicle_create(vehicle)
    }
    /// The minecart `c` is about to come into the world: `VehicleCreateEvent`.
    /// False when cancelled.
    pub(super) fn plugin_cart_create(&mut self, c: &Minecart) -> bool {
        if !self.plugins_listen::<ev::VehicleCreateEvent>() {
            return true;
        }
        let vehicle = self.plugin_cart_ref(c);
        self.plugin_vehicle_create(vehicle)
    }
    fn plugin_vehicle_create(&mut self, vehicle: api::EntityRef) -> bool {
        let allowed = self.plugin_allows(
            None,
            ev::VehicleCreateEvent {
                vehicle,
                cancelled: false,
            },
        );
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }

    // -----------------------------------------------------------------------
    // Hurting and breaking vehicles.

    /// The player whose turn it is hits a vehicle: the attacker of the
    /// `VehicleDamageEvent` that follows.
    pub(super) fn plugin_vehicle_attacked(&mut self) {
        if self.plugins_listen::<ev::VehicleDamageEvent>()
            || self.plugins_listen::<ev::VehicleDestroyEvent>()
        {
            self.plugin_vehicles.attacker = self.plugin_acting_player_entity();
        }
    }
    /// The vehicle `id` is about to take `damage`: `VehicleDamageEvent`. The
    /// damage to deal, or None when cancelled.
    pub(super) fn plugin_vehicle_damage(&mut self, id: u32, damage: f32) -> Option<f32> {
        let attacker = self.plugin_vehicles.attacker.take();
        self.plugin_vehicles.hurting = attacker.clone();
        if !self.plugins_listen::<ev::VehicleDamageEvent>() {
            return Some(damage);
        }
        let mut e = ev::VehicleDamageEvent {
            vehicle: self.plugin_vehicle_or_entity(id),
            attacker,
            damage: f64::from(damage),
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled {
            self.plugin_vehicles.hurting = None;
            return None;
        }
        Some(e.damage as f32)
    }
    /// The damage is breaking the vehicle `id`: `VehicleDestroyEvent`. False
    /// when cancelled.
    pub(super) fn plugin_vehicle_destroy(&mut self, id: u32) -> bool {
        let attacker = self.plugin_vehicles.hurting.take();
        if !self.plugins_listen::<ev::VehicleDestroyEvent>() {
            return true;
        }
        let event = ev::VehicleDestroyEvent {
            vehicle: self.plugin_vehicle_or_entity(id),
            attacker,
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }

    // -----------------------------------------------------------------------
    // Moving and bumping.

    /// After the vehicles' tick: `VehicleUpdateEvent` for each, and
    /// `VehicleMoveEvent` for each that moved or turned since the last one.
    pub(super) fn plugin_watch_vehicles(&mut self) {
        let update = self.plugins_listen::<ev::VehicleUpdateEvent>();
        let moves = self.plugins_listen::<ev::VehicleMoveEvent>();
        if !moves {
            self.plugin_vehicles.seen.clear();
        }
        if !update && !moves {
            return;
        }
        let world = self.dim.id().to_owned();
        let mut now: Vec<api::EntityRef> =
            self.boats.iter().map(|b| self.plugin_boat_ref(b)).collect();
        now.extend(self.carts.iter().map(|c| self.plugin_cart_ref(c)));
        for vehicle in &now {
            if update {
                let mut e = ev::VehicleUpdateEvent {
                    vehicle: vehicle.clone(),
                };
                self.plugin_fire(None, &mut e);
            }
            if !moves {
                continue;
            }
            let to = vehicle.location.clone();
            let key = (world.clone(), vehicle.id as u32);
            if let Some(from) = self.plugin_vehicles.seen.insert(key, to.clone())
                && from != to
            {
                let mut e = ev::VehicleMoveEvent {
                    vehicle: vehicle.clone(),
                    from,
                    to,
                };
                self.plugin_fire(None, &mut e);
            }
        }
        if moves {
            self.plugin_vehicles
                .seen
                .retain(|(w, id), _| *w != world || now.iter().any(|v| v.id as u32 == *id));
        }
    }
    /// A boat ran sideways into a block, wanting to move by `wanted`:
    /// `VehicleBlockCollisionEvent`.
    pub(super) fn plugin_boat_bump(&mut self, b: &Boat, wanted: Vec3) {
        if !self.plugins_listen::<ev::VehicleBlockCollisionEvent>() {
            return;
        }
        let vehicle = self.plugin_boat_ref(b);
        self.plugin_vehicle_bump(vehicle, wanted);
    }
    /// A minecart ran sideways into a block: `VehicleBlockCollisionEvent`.
    pub(super) fn plugin_cart_bump(&mut self, c: &Minecart, wanted: Vec3) {
        if !self.plugins_listen::<ev::VehicleBlockCollisionEvent>() {
            return;
        }
        let vehicle = self.plugin_cart_ref(c);
        self.plugin_vehicle_bump(vehicle, wanted);
    }
    fn plugin_vehicle_bump(&mut self, vehicle: api::EntityRef, wanted: Vec3) {
        // CraftBukkit: the vehicle's block, one step the way it was going.
        let BlockPos { mut x, y, mut z } = vehicle.location.block();
        if wanted.x.abs() >= wanted.z.abs() {
            x += if wanted.x > 0.0 { 1 } else { -1 };
        } else {
            z += if wanted.z > 0.0 { 1 } else { -1 };
        }
        let mut e = ev::VehicleBlockCollisionEvent {
            vehicle,
            block: self.plugin_block([x, y, z]),
            velocity: [wanted.x, wanted.y, wanted.z],
        };
        self.plugin_fire(None, &mut e);
    }
    /// The vehicle `vehicle` touches the entity `other`:
    /// `VehicleEntityCollisionEvent`. Whether a boat may take it on board,
    /// and whether they push each other.
    pub(super) fn plugin_vehicle_touch(&mut self, vehicle: u32, other: i32) -> (bool, bool) {
        if !self.plugins_listen::<ev::VehicleEntityCollisionEvent>() {
            return (true, true);
        }
        let entity = match u32::try_from(other) {
            Ok(o) => self.plugin_vehicle_or_entity(o),
            Err(_) => self.plugin_entity(None, other),
        };
        let mut e = ev::VehicleEntityCollisionEvent {
            vehicle: self.plugin_vehicle_or_entity(vehicle),
            entity,
            pickup_cancelled: false,
            collision_cancelled: false,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        (
            !e.cancelled && !e.pickup_cancelled,
            !e.cancelled && !e.collision_cancelled,
        )
    }

    // -----------------------------------------------------------------------
    // Leads.

    /// Why the next lead to come off does (`EntityUnleashEvent`).
    pub(super) fn plugin_unleash_reason(&mut self, reason: ev::UnleashReason) {
        if self.plugins_listen::<ev::EntityUnleashEvent>() {
            self.plugin_vehicles.unleash = Some(reason);
        }
    }
    /// The lead is about to come off the mob `id`: `EntityUnleashEvent`.
    /// Whether it drops as an item.
    pub(super) fn plugin_unleash(&mut self, id: u32, drop: bool) -> bool {
        let reason = self.plugin_vehicles.unleash.take();
        if !self.plugins_listen::<ev::EntityUnleashEvent>()
            || !self
                .mobs
                .iter()
                .any(|m| m.id == id && m.extra.leash.is_some())
        {
            return drop;
        }
        let mut e = ev::EntityUnleashEvent {
            entity: self.plugin_entity(None, id as i32),
            reason: reason.unwrap_or(ev::UnleashReason::Unknown),
            drop_leash: drop,
        };
        self.plugin_fire(None, &mut e);
        e.drop_leash
    }
}
