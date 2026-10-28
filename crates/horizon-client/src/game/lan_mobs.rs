//! Mobs and the guests of an open world. The mob code sees one player
//! (`MobWorld.player`, `Who::Player`); with guests on, each mob sees the
//! player vanilla's `Level.getNearestPlayer` would give it, and keeps the
//! guest it targets until it lets go. What it then does to `Who::Player`
//! (hits, pushes, fire, its projectiles) runs with that guest swapped in.
use super::{Game, lan::LanHost};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_protocol::packets::play::{self, Clientbound as C};





impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// Runs `f` as each guest in play here, for what strikes every player in
    /// an area (`Level.getEntities`), not only the window's own.
    pub(super) fn each_guest_hit(&mut self, f: impl FnMut(&mut Self)) {
        self.each_guest_hit_except(None, f);
    }
    /// [`Self::each_guest_hit`] leaving out the guest `except` (the one a
    /// firework rocket is attached to, which it hurts on its own).
    pub(super) fn each_guest_hit_except(
        &mut self,
        except: Option<u32>,
        mut f: impl FnMut(&mut Self),
    ) {
        let Some(mut host) = self.lan.take() else {
            return;
        };
        let conns: Vec<u32> = host
            .hub
            .guests
            .iter()
            .filter(|g| host.hub.here(g) && g.loaded && !g.dead && Some(g.conn) != except)
            .map(|g| g.conn)
            .collect();
        for conn in conns {
            self.guest_hit_by(&mut host, conn, &mut f);
        }
        self.lan = Some(host);
    }
    /// The guests' boxes for the blocks entities trigger (pressure plates,
    /// tripwires, detector rails): `Entity.checkInsideBlocks` and
    /// `Level.getEntities` see every player in the level, but not spectators
    /// (`Entity.isIgnoringBlockTriggers`).
    pub(super) fn lan_block_trigger_boxes(&self) -> Vec<([f64; 3], [f64; 3])> {
        let Some(host) = self.lan.as_ref() else {
            return Vec::new();
        };
        host.hub
            .guests
            .iter()
            .filter(|g| host.hub.here(g) && g.loaded && !g.dead && !g.spectator())
            .map(|g| {
                let b = g.bounds();
                let (lo, hi) = (b.min_position(), b.max_position());
                ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z])
            })
            .collect()
    }
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// Hurt a guest: `f` runs as them, and the knockback it gave reaches
    /// their client. A hurt marks the velocity (Entity.markHurt), which goes
    /// out once the guest's server copy has travelled (`guest_server_tick`);
    /// a push with no hurt goes out at once.
    pub(super) fn guest_hit_by<R>(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let (result, pushed) = self.as_guest(host, conn, |g| {
            let before = g.player.as_ref().map(|p| p.velocity);
            let result = f(g);
            let pushed = g
                .player
                .as_ref()
                .filter(|p| !p.health.sync_velocity && Some(p.velocity) != before)
                .map(|p| p.velocity);
            (result, pushed)
        })?;
        self.send_guest_motion(host, conn, pushed);
        Some(result)
    }
    /// A player's melee hit on a guest (Player.causeExtraKnockback on a
    /// ServerPlayer): a hurt that marked the velocity sends it at once, and
    /// the server's copy goes back to the velocity it had before the hit.
    pub(super) fn guest_struck_by<R>(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let (result, sent) = self.as_guest(host, conn, |g| {
            let before = g.player.as_ref().map(|p| p.velocity);
            let result = f(g);
            let sent = g.player.as_mut().zip(before).and_then(|(p, before)| {
                std::mem::take(&mut p.health.sync_velocity)
                    .then(|| std::mem::replace(&mut p.velocity, before))
            });
            (result, sent)
        })?;
        self.send_guest_motion(host, conn, sent);
        Some(result)
    }
    /// ClientboundSetEntityMotionPacket to a guest about themselves.
    pub(super) fn send_guest_motion(&self, host: &LanHost, conn: u32, velocity: Option<Vec3>) {
        if let (Some(v), Some(g)) = (velocity, host.hub.guest(conn)) {
            g.send(&C::SetEntityMotion(play::SetEntityMotion {
                id: g.entity_id,
                velocity: [v.x, v.y, v.z],
            }));
        }
    }
}

/// A guest as the blocks that look for players see them
/// (`PlayerDetector`, `level.getPlayers`: the dead ones too).
pub(super) struct Present {
    pub uuid: [i32; 4],
    pub position: [f64; 3],
    pub eye: [f64; 3],
    pub height: f32,
    pub creative: bool,
    pub spectator: bool,
    pub alive: bool,
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// Every guest in the world (`ServerLevel.getPlayers` past the host).
    pub(super) fn lan_present(&self) -> Vec<Present> {
        let guests = self
            .lan
            .iter()
            .flat_map(|h| h.hub.guests.iter().filter(|g| h.hub.here(g)));
        guests
            .filter(|g| g.loaded)
            .map(|g| {
                let [x, y, z] = g.position;
                Present {
                    uuid: horizon_client::lan::playerdata::uuid_ints(g.uuid),
                    position: g.position,
                    eye: [x, y + g.eye_height(), z],
                    height: g.dimensions().height,
                    creative: g.creative(),
                    spectator: g.spectator(),
                    alive: !g.dead,
                }
            })
            .collect()
    }
    /// The UUID of the player acting: the guest `as_guest` runs, else the host.
    pub(super) fn acting_uuid(&self) -> [i32; 4] {
        self.lan_acting_uuid
            .map(horizon_client::lan::playerdata::uuid_ints)
            .or(self.level.player_uuid)
            .unwrap_or_default()
    }
}

/// A guest's bounding box, by its pose (`Avatar.POSES`).
pub(super) fn guest_bounds(g: &horizon_client::lan::guest::Guest) -> Aabb {
    g.bounds()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(x: f64, creative: bool) -> PlayerView {
        PlayerView {
            position: Vec3::new(x, 64.0, 0.0),
            bounds: Aabb::new(Vec3::new(x, 64.0, 0.0), Vec3::new(x, 65.8, 0.0)),
            eye: [x, 65.62, 0.0],
            alive: true,
            creative,
            sneaking: false,
            swimming: false,
            look: [0.0; 3],
            main_hand: None,
            off_hand: None,
            head: None,
            armor_cover: 0.0,
            invisible: false,
            spectator: false,
            gold_armor: false,
            hurt_by: None,
            last_hurt: None,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            sprinting: false,
            riding: false,
            hero: false,
            effects: Vec::new(),
            uuid: [0; 4],
            allies: Default::default(),
            remote: false,
            sleeping: false,
            sleep_timer: 0,
            passenger: false,
        }
    }

    const MOB: Vec3 = Vec3::new(0.0, 64.0, 0.0);

    #[test]
    fn the_nearest_survival_player_is_seen() {
        let host = view(10.0, false);
        let guests = vec![(7, view(3.0, false)), (8, view(-5.0, false))];
        assert_eq!(choose(MOB, false, Some(&host), &guests, None), Some(7));
        // A creative guest is passed over for the host further away.
        let guests = vec![(7, view(3.0, true))];
        assert_eq!(choose(MOB, false, Some(&host), &guests, None), None);
        // Nobody targetable: the nearest one is still seen.
        let host = view(10.0, true);
        assert_eq!(choose(MOB, false, Some(&host), &guests, None), Some(7));
        assert_eq!(choose(MOB, false, None, &Vec::new(), None), None);
    }

    #[test]
    fn a_targeted_guest_stays_the_target() {
        let host = view(1.0, false);
        let guests = vec![(7, view(12.0, false))];
        assert_eq!(choose(MOB, true, Some(&host), &guests, Some(7)), Some(7));
        // Gone creative: the mob looks again.
        let guests = vec![(7, view(12.0, true))];
        assert_eq!(choose(MOB, true, Some(&host), &guests, Some(7)), None);
        // Without a target the nearest player wins.
        let guests = vec![(7, view(12.0, false))];
        assert_eq!(choose(MOB, false, Some(&host), &guests, Some(7)), None);
    }
}
