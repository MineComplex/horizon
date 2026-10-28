//! A server's players between dimensions.
//!
//! A server holds a level per dimension (game/levels.rs). A player who steps
//! into a Nether or End portal moves to the destination's level, which opens
//! if nobody was there: the swap is the in-frame path's portal plans and
//! arrival search, and the player hears it as `ServerPlayer.teleport` into
//! another level: a Respawn packet keeping all data, then a teleport to the
//! exit once the destination has loaded around them.

use super::{ Game, Mob, boats, minecarts};
use horizon_core::geometry::Vec3;


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
}



impl Game<'_> {}
impl crate::game::Sim<'_> {
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

}
