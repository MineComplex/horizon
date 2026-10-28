//! Players hitting players in an open world (`Player.attack` on a player):
//! the host hits a guest, a guest hits the host or another guest. The hit
//! is worked out as the attacker (weapon, attack strength, critical hit,
//! enchantments, a mace's smash) and lands on the victim like any player
//! attack (blocking, armour, knockback), when the `pvp` game rule allows it
//! (`ServerPlayer.canHarmPlayer`, `isPvpAllowed`). A player's arrow on
//! another player lands the same way.
use super::Game;

use horizon_core::geometry::{Aabb, Vec3};




/// A sweep attack's reach to the other players (`Player.doSweepAttack`),
/// worked out as the attacker.
#[derive(Clone)]
pub(super) struct Sweep {
    /// The target's box grown by (1, 0.25, 1).
    pub(super) hit_box: Aabb,
}


impl Game<'_> {
}
impl crate::game::Sim<'_> {
    /// The guest on the host's crosshair and how far, within reach.
    pub(super) fn guest_hit(&self) -> Option<(u32, f64)> {
        return None;
    }
    /// The host's attack on the guest under the crosshair, when no mob or
    /// block is nearer.
    pub(super) fn attack_guest(&mut self) -> bool {
        let Some((conn, d)) = self.guest_hit() else {
            return false;
        };
        if self.mob_hit().is_some_and(|(_, m)| m < d) || !self.before_block(d) {
            return false;
        }
        return false;
    }
    /// The sweep of the player in place, for the players it reaches: the
    /// damage (`1 + ratio * baseDamage`, plus the enchantments against a
    /// player, scaled by the attack strength) and who hits. The box is set
    /// by the caller.
    pub(super) fn player_sweep(&mut self, base: f32, scale: f32) -> Sweep {
        let from = self
            .player
            .as_ref()
            .map_or([0.0; 3], |p| [p.position.x, p.position.y, p.position.z]);
        let damage = (base + self.enchant_damage_bonus("minecraft:player")) * scale;
        Sweep {
            hit_box: Aabb::new(Vec3::ZERO, Vec3::ZERO),
        }
    }
    /// A sweep from a mob hit reaches the other players. Inside a guest's
    /// turn the host holds the other players (`as_guest`), so it waits for
    /// the turn to end.
    pub(super) fn sweep_other_players(&mut self, sweep: Sweep) {
    }
}
