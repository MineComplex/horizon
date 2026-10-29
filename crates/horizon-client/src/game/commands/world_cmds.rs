//! `/recipe`, `/damage`, `/rotate`, `/swing`, `/playsound`, `/stopsound`,
//! `/particle`, `/attribute`, `/ride` and `/spreadplayers`.
use super::Game;

impl Game<'_> {
    // ---- /recipe -----------------------------------------------------------

    // ---- /damage ----------------------------------------------------------------

    // ---- /rotate and /swing -----------------------------------------------------------

    // ---- sounds -----------------------------------------------------------------------

    // ---- /particle ----------------------------------------------------------------------

    // ---- /attribute ---------------------------------------------------------------------

    // ---- /ride ----------------------------------------------------------------------------

    // ---- /spreadplayers -----------------------------------------------------------------------
}
impl crate::game::Sim<'_> {
    /// LivingEntity.stopRiding for a mob: off its vehicle
    /// (Entity.removePassenger), then to where the vehicle lets it out
    /// (LivingEntity.dismountVehicle).
    pub(in crate::game) fn dismount_mob(&mut self, rider: u32) {
        let Some(i) = self.mobs.iter().position(|m| m.id == rider) else {
            return;
        };
        let Some(v) = self.mobs[i].extra.mount.take() else {
            return;
        };
        let (width, yaw) = (f64::from(self.mobs[i].width()), self.mobs[i].yaw);
        let at = if let Some(b) = self.boats.iter().position(|b| b.id == v) {
            self.boat_dismount_target(b, width, yaw)
        } else if let Some(m) = self.mobs.iter_mut().find(|m| m.id == v) {
            m.extra.rider = None;
            // Entity.getDismountLocationForPassenger: on top of the vehicle.
            let p = m.position;
            horizon_core::geometry::Vec3::new(p.x, p.y + f64::from(m.height()), p.z)
        } else {
            self.mobs[i].position
        };
        let m = &mut self.mobs[i];
        m.extra.passenger = false;
        m.extra.boarding_cooldown = super::super::boat_passengers::BOARDING_COOLDOWN;
        m.position = at;
        m.previous = at;
    }
}
