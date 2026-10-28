//! Nether portals for the player and the other entities:
//! `Entity.setAsInsidePortal` / `handlePortal` with `PortalProcessor`,
//! `NetherPortalBlock.getPortalTransitionTime` and `getPortalDestination`
//! (`PortalForcer` through `horizon_world::level::portal_travel`), and
//! `LocalPlayer.handlePortalTransitionEffect`. Items and mobs also go through
//! End portals here (`EndPortalBlock.entityInside` / `getPortalDestination`);
//! the player's End portals are game/end.rs's.
//!
//! Only one dimension is loaded at a time. The player's destination is found
//! once the other dimension has loaded around it. An item or mob that goes
//! through waits as an `Arrival` (its saved compound and where it stood in its
//! portal) until the player is in that dimension and its exit is loaded.
use super::{Dim, Game, Screen};
use horizon_client::nether_portal::PortalState;
use horizon_core::{
    geometry::Vec3,
    position::BlockPos,
    random::RandomSource};
use horizon_world::nbt::Tag;
use std::collections::HashMap;

/// `Entity.getDimensionChangingDelay`.
const ENTITY_DELAY: i32 = 300;

/// Where in its portal a traveller stood, and where its exit is to be found.
#[derive(Clone, Copy, Debug)]
pub(super) struct Plan {
    pub(super) dim: Dim,
    /// The border-clamped, scaled position (`approximateExitPos`).
    pub(super) approximate: BlockPos,
    entry_x_axis: bool,
    offset: Vec3,
    width: f32,
    height: f32,
    /// Through an End portal: `approximate` is the End's spawn point, or the
    /// Overworld's shared spawn.
    pub(super) end: bool,
}

impl Plan {
}

/// An entity on its way to a dimension that isn't loaded.
#[derive(Clone, Debug)]
struct Arrival {
    plan: Plan,
    tag: Tag,
}

/// The nether portal state of the world (`Game::portals`).
#[derive(Default)]
pub(super) struct Portals {
    /// The player's.
    pub(super) player: PortalState,
    /// `LocalPlayer.portalProcess.isInsidePortalThisTick`.
    client_inside: bool,
    /// Items and mobs, by entity id.
    entities: HashMap<u32, PortalState>,
    /// Positions before this tick's moves, by entity id.
    before: HashMap<u32, Vec3>,
    arrivals: Vec<Arrival>,
    /// The vehicles server players ride that went through a portal this
    /// tick, in their level, with their entry: they carry their riders
    /// (`tick_server_travel`).
    carrying: Vec<(Dim, u32, BlockPos)>,
    loaded: bool,
    dirty: bool,
    /// Per-chunk portal palette masks, kept across ticks (portal_sections).
    pub(super) sections: horizon_client::portal_sections::PortalSections,
}

/// A waiting entity as `horizon_portal_arrivals.dat` keeps it.
fn arrival_tag(a: &Arrival) -> Tag {
    let p = &a.plan;
    Tag::compound([
        ("dimension", Tag::String(p.dim.id().into())),
        ("x", Tag::Int(p.approximate.x)),
        ("y", Tag::Int(p.approximate.y)),
        ("z", Tag::Int(p.approximate.z)),
        ("x_axis", Tag::Int(i32::from(p.entry_x_axis))),
        ("right", Tag::Double(p.offset.x.to_bits())),
        ("up", Tag::Double(p.offset.y.to_bits())),
        ("forward", Tag::Double(p.offset.z.to_bits())),
        ("width", Tag::Float(p.width.to_bits())),
        ("height", Tag::Float(p.height.to_bits())),
        ("end", Tag::Int(i32::from(p.end))),
        ("entity", a.tag.clone()),
    ])
}

impl Game<'_> {
    // ---- the player ----

    /// `LocalPlayer.handlePortalTransitionEffect`: whether the confusion
    /// effect grows this tick. Entering a portal closes screens other than
    /// the chat and death screens and plays `block.portal.trigger` when the
    /// effect starts from nothing.
    pub(super) fn client_portal_effect(&mut self) -> bool {
        if !std::mem::take(&mut self.portals.client_inside) {
            return false;
        }
        match self.screen {
            Screen::Playing | Screen::Chat | Screen::Dead => {}
            Screen::Inventory | Screen::Creative => {
                self.close_container();
                self.carried = None;
                self.screen = Screen::Playing;
                self.capture(true);
            }
            _ => {
                self.screen = Screen::Playing;
                self.capture(true);
            }
        }
        if self.front.screen_fx.portal == 0.0 {
            let pitch = self.world.random.next_float() * 0.4 + 0.8;
            self.sounds
                .play("minecraft:block.portal.trigger", None, 0.25, pitch);
        }
        true
    }

    // ---- other entities ----

    // ---- persistence ----
}
impl crate::game::Sim<'_> {
    /// The same on a server's client: `Entity.canUsePortal` (alive, not a
    /// passenger), then only the client's `setAsInsidePortal`, which drives
    /// the confusion effect (`client_portal_effect`).
    pub(super) fn client_inside_nether_portal(&mut self) {
        let alive = self
            .player
            .as_ref()
            .is_some_and(|p| !p.health.dead_or_dying());
        if alive && self.riding.is_none() {
            self.portals.client_inside = true;
        }
    }
    /// `Entity.load`: the saved `PortalCooldown` (an entity that just came
    /// through stands in its exit portal for 300 ticks without going back).
    pub(super) fn restore_portal_cooldown(&mut self, id: u32, tag: &Tag) {
        if let Some(Tag::Int(cooldown)) = tag.get("PortalCooldown")
            && *cooldown > 0
        {
            self.portals.entities.insert(
                id,
                PortalState {
                    cooldown: *cooldown,
                    process: None,
                },
            );
        }
    }
    /// `Entity.setPortalCooldown` on an entity that has just spawned in a
    /// portal: it stands in the portal for 300 ticks without going through.
    pub(super) fn set_portal_cooldown(&mut self, id: u32) {
        self.portals.entities.insert(
            id,
            PortalState {
                cooldown: ENTITY_DELAY,
                process: None,
            },
        );
    }
    fn arrivals_path(&self) -> std::path::PathBuf {
        self.world_root
            .join("data")
            .join("horizon_portal_arrivals.dat")
    }
    /// Write the waiting entities when they changed.
    pub(super) fn save_portal_arrivals(&mut self) {
        if !self.portals.dirty || self.remote.is_some() {
            return;
        }
        let list = self.portals.arrivals.iter().map(arrival_tag).collect();
        let root = Tag::compound([("arrivals", Tag::List(list))]);
        if horizon_client::save::write_dat(&self.arrivals_path(), root).is_ok() {
            self.portals.dirty = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_portals_lead_to_the_end_spawn_and_back_to_the_world_spawn() {
        let spawn = BlockPos::new(-7, 70, 12);
        for from in [Dim::Overworld, Dim::Nether] {
            let plan = Plan::end_portal(from, spawn, 0.9, 1.4);
            assert_eq!(plan.dim, Dim::End);
            assert_eq!(plan.approximate, BlockPos::new(100, 50, 0));
            assert!(plan.end);
        }
        let plan = Plan::end_portal(Dim::End, spawn, 0.25, 0.25);
        assert_eq!(plan.dim, Dim::Overworld);
        assert_eq!(plan.approximate, spawn);
    }

    #[test]
    fn the_end_portal_shape_takes_falling_items_and_standing_mobs() {
        let portal = BlockPos::new(0, 100, 3);
        let at = |y| Vec3::new(0.5, y, 3.5);
        // A dropped item falling through the slab, or in it.
        assert!(touches_end_portal(
            at(100.6),
            at(100.56),
            0.25,
            0.25,
            portal
        ));
        // One tick's fall across the whole slab, in it at neither end.
        assert!(touches_end_portal(at(101.0), at(99.9), 0.25, 0.25, portal));
        // Resting below the slab (on the floor) or above it.
        assert!(!touches_end_portal(
            at(100.0),
            at(100.0),
            0.25,
            0.25,
            portal
        ));
        assert!(!touches_end_portal(
            at(100.8),
            at(100.8),
            0.25,
            0.25,
            portal
        ));
        // A cow standing in the portal block.
        assert!(touches_end_portal(at(100.0), at(100.0), 0.9, 1.4, portal));
        // Beside it.
        let beside = Vec3::new(2.0, 100.0, 3.5);
        assert!(!touches_end_portal(beside, beside, 0.9, 1.4, portal));
    }

    #[test]
    fn end_portals_turn_entities_like_their_teleport_transition() {
        // Into the End: facing west, pitch kept.
        assert_eq!(
            end_portal_rotation(true, [-30.0, 20.0], [45.0, 10.0]),
            [90.0, 20.0]
        );
        // Out of it: turned by the world spawn's yaw and pitch, pitch clamped.
        assert_eq!(
            end_portal_rotation(false, [-30.0, 20.0], [45.0, 10.0]),
            [15.0, 30.0]
        );
        assert_eq!(
            end_portal_rotation(false, [0.0, 80.0], [0.0, 30.0]),
            [0.0, 90.0]
        );
    }

    #[test]
    fn waiting_arrivals_keep_their_end_destination_when_saved() {
        let tag = Tag::compound([("id", Tag::String("minecraft:cow".into()))]);
        for (from, dim) in [(Dim::Overworld, Dim::End), (Dim::End, Dim::Overworld)] {
            let arrival = Arrival {
                plan: Plan::end_portal(from, BlockPos::new(3, 64, -5), 0.9, 1.4),
                tag: tag.clone(),
            };
            let back = arrival_from_tag(&arrival_tag(&arrival)).unwrap();
            assert_eq!(back.plan.dim, dim);
            assert_eq!(back.plan.approximate, arrival.plan.approximate);
            assert!(back.plan.end);
            assert_eq!(back.tag, tag);
        }
        // Arrivals saved before End portals were Nether portal plans.
        let mut old = arrival_tag(&Arrival {
            plan: Plan::end_portal(Dim::End, BlockPos::new(0, 64, 0), 0.25, 0.25),
            tag,
        });
        if let Tag::Compound(c) = &mut old {
            c.remove(&"end".into());
        }
        assert!(!arrival_from_tag(&old).unwrap().plan.end);
    }
}
