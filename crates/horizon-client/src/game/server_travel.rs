//! A server's players between dimensions.
//!
//! A server holds a level per dimension (game/levels.rs). A player who steps
//! into a Nether or End portal moves to the destination's level, which opens
//! if nobody was there: the swap is the in-frame path's portal plans and
//! arrival search, and the player hears it as `ServerPlayer.teleport` into
//! another level: a Respawn packet keeping all data, then a teleport to the
//! exit once the destination has loaded around them.

use super::end::GatewayJob;
use super::nether_portal::Plan;
use super::respawn_anchor::RespawnConfig;
use super::{Dim, Game, Mob, boats, minecarts};
use horizon_client::world_spawn::PlayerSpawnFinder;
use horizon_core::geometry::Vec3;

/// A trip under way: the player is in the destination's level, whose
/// chunks are loading around `target`.
pub(super) struct Trip {
    conn: u32,
    dim: Dim,
    /// The level the player came from: the vehicle it rides waits there
    /// until the exit is found.
    source: Dim,
    target: Vec3,
    /// Through an End portal.
    end: bool,
    /// The Nether portal plan, whose exit is found or built on arrival.
    plan: Option<Plan>,
    /// The vehicle the player rides, which travels with it
    /// (`Entity.teleportCrossDimension` carries the passengers).
    vehicle: Option<u32>,
    /// Through an End gateway, within the level: its exit, found or built
    /// on arrival.
    gateway: Option<GatewayJob>,
    /// A respawn at this point outside the Overworld, resolved through its
    /// block on arrival.
    respawn: Option<RespawnConfig>,
    /// A respawn at the world spawn, moved onto a free column within the
    /// respawn radius once the chunks around it have loaded.
    world_spawn: bool,
    /// The search for that column while a candidate's chunk loads.
    finder: Option<PlayerSpawnFinder>,
    /// Sent by a command: the player is already where it goes.
    placed: bool,
}

/// A vehicle a player rides: an animal, a boat or a minecart. It travels
/// with the player through a portal (`Entity.teleportCrossDimension` carries
/// the passengers, and they ride the vehicle that is recreated in the other
/// dimension).
trait Rideable {
    fn id(&self) -> u32;
    /// The vehicle stands where its rider arrives, with no motion.
    fn arrive(&mut self, at: Vec3, yaw: f32);
}

impl Rideable for Mob {
    fn id(&self) -> u32 {
        self.id
    }
    fn arrive(&mut self, at: Vec3, yaw: f32) {
        self.position = at;
        self.previous = at;
        self.velocity = Vec3::ZERO;
        self.yaw = yaw;
        self.previous_yaw = yaw;
        self.fall_distance = 0.0;
    }
}

impl Rideable for boats::Boat {
    fn id(&self) -> u32 {
        self.id
    }
    fn arrive(&mut self, at: Vec3, yaw: f32) {
        self.position = at;
        self.previous = at;
        self.velocity = Vec3::ZERO;
        self.yaw = yaw;
        self.previous_yaw = yaw;
        self.delta_rotation = 0.0;
    }
}

impl Rideable for minecarts::Minecart {
    fn id(&self) -> u32 {
        self.id
    }
    fn arrive(&mut self, at: Vec3, yaw: f32) {
        self.position = at;
        self.previous = at;
        self.velocity = Vec3::ZERO;
        self.yaw = yaw;
        self.previous_yaw = yaw;
    }
}


/// `TeleportCommand` into another level for a server's player: where, and
/// the rotation it sets (none keeps the player's).
pub(super) struct LevelTeleport {
    pub conn: u32,
    pub dim: Dim,
    pub at: [f64; 3],
    pub rotation: Option<[f32; 2]>,
}


impl Trip {
    pub(super) fn is(&self, conn: u32) -> bool {
        self.conn == conn
    }
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// The vehicle a player rides travels with it through a portal: it
    /// leaves the level the player came from, keeps its entity id (so the
    /// ride is never broken) and stands where its rider arrives
    /// (`Entity.teleportCrossDimension` carries the passengers, and they
    /// ride the vehicle that is recreated in the other dimension).
    #[test]
    fn the_vehicle_a_player_rides_travels_with_it() {
        let mut boats = vec![boats::Boat::new(
            7,
            Arc::from("oak"),
            Vec3::new(0.0, 64.0, 0.0),
            0.0,
        )];
        let mut carts: Vec<minecarts::Minecart> = Vec::new();
        let mut mobs: Vec<Mob> = Vec::new();
        let at = Vec3::new(1.5, 65.0, -2.5);
        let boat = take_vehicle(&mut boats, 7, at, 90.0)
            .expect("the vehicle the player rides travels with it");
        assert!(
            boats.is_empty(),
            "the vehicle left the level the player came from"
        );
        assert_eq!(boat.id, 7, "the vehicle keeps its entity id");
        assert_eq!(boat.position, at, "it stands where its rider arrives");
        assert_eq!(boat.yaw, 90.0, "it looks where its rider looks");
        assert_eq!(boat.velocity, Vec3::ZERO, "it comes with no motion");
        // A vehicle that is not in the level does not travel.
        assert!(take_vehicle(&mut carts, 9, at, 0.0).is_none());
        assert!(carts.is_empty());
        assert!(take_vehicle(&mut mobs, 9, at, 0.0).is_none());
        assert!(mobs.is_empty());
    }
}
