//! Mobs riding mobs: boarding and dismounting, keeping riders on their
//! vehicles, the jockeys that spawn mounted (chicken jockeys, spider
//! jockeys, strider jockeys, zombie horsemen) and the skeleton horse trap.
//! Saving riders under `Passengers` lives in `persist.rs`.
use horizon_client::mobs::riders;
use horizon_core::geometry::Vec3;
use std::collections::HashMap;

impl crate::game::Game<'_> {
    /// `Entity.positionRider` for every mob passenger: in its boat, then on
    /// its mob. A server's client places them itself too, as the server sends
    /// no passenger's position.
    pub(super) fn position_riders(&mut self) {
        self.seat_boat_mobs();
        let index: HashMap<u32, usize> = self
            .mobs
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id, i))
            .collect();
        // Riders of riders settle over a few passes.
        for _ in 0..3 {
            for i in 0..self.mobs.len() {
                let Some(v) = self.mobs[i].extra.mount else {
                    continue;
                };
                let Some(&j) = index.get(&v) else { continue };
                let vehicle = &self.mobs[j];
                let attach = riders::passenger_attachment(vehicle);
                let (pos, prev) = (vehicle.position, vehicle.previous);
                let (yaw, prev_yaw) = (vehicle.yaw, vehicle.previous_yaw);
                let (body, prev_body) = (vehicle.body_yaw, vehicle.previous_body_yaw);
                let m = &mut self.mobs[i];
                let offset = riders::vehicle_attachment(m);
                m.position = riders::seat(attach, offset, pos, yaw);
                m.previous = riders::seat(attach, offset, prev, prev_yaw);
                m.velocity = Vec3::ZERO;
                m.fall_distance = 0.0;
                m.on_ground = false;
                m.body_yaw = body;
                m.previous_body_yaw = prev_body;
                m.extra.passenger = true;
                // A rider never walks off on its own.
                m.ai.nav.stop();
            }
        }
    }
}
