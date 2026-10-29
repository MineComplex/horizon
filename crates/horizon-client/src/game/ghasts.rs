//! Happy ghasts on the game side: putting a harness on (and shearing it
//! off), riding, getting off on top, and the ghastling a dried ghast block
//! hatches.
use horizon_client::mobs::{MobEvent, happy_ghast};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_world::registry::BlockStateId;

impl crate::game::Sim<'_> {
    /// Client `EntityGetter.getEntityCollisions` for the player: the
    /// entities that `canBeCollidedWith` it, solid happy ghasts and boats
    /// (`AbstractBoat.canBeCollidedWith`), except the player's own vehicle.
    pub(super) fn player_entity_collisions(&self) -> Vec<Aabb> {
        let Some(p) = &self.player else {
            return Vec::new();
        };
        self.mobs
            .iter()
            .filter(|m| {
                m.species() == horizon_client::mobs::Species::HappyGhast
                    && Some(m.id) != self.riding
                    && happy_ghast::collides_with_player(m, p.position.y)
            })
            .map(|m| m.bounds())
            .chain(
                self.boats
                    .iter()
                    .filter(|b| Some(b.id) != self.riding)
                    .map(super::boats::Boat::bounds),
            )
            .collect()
    }
    /// Getting off lands on top of it.
    pub(super) fn dismount_happy_ghast(&mut self, id: u32) {
        // removePassenger: held still for half a second, and with no one
        // aboard the ghast forgets its home.
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            m.extra.counter = 10;
            m.ai.home = None;
        }
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let top = Vec3::new(
            m.position.x,
            m.position.y + f64::from(m.height()),
            m.position.z,
        );
        let up: MobEvent = happy_ghast::goggles(false, m);
        if let Some(p) = self.player.as_mut() {
            p.position = top;
            p.velocity = Vec3::ZERO;
        }
        let _ = self.handle_mob_events(vec![up]);
    }
    /// `DriedGhastBlock.spawnGhastling`: a ghastling facing the way the block did.
    pub(super) fn spawn_ghastling(&mut self, pos: [i32; 3], state: BlockStateId) {
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        ];
        let facing = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("facing").cloned());
        let yaw = match facing.as_deref() {
            Some("north") => 180.0,
            Some("east") => 270.0,
            Some("west") => 90.0,
            _ => 0.0,
        };
        if let Some(id) = self.spawn_mob("minecraft:happy_ghast", at, true)
            && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
        {
            m.extra.age = -24000;
            m.yaw = yaw;
            m.body_yaw = yaw;
            m.head_yaw = yaw;
            let at = m.center();
            self.sounds
                .play("minecraft:entity.ghastling.spawn", Some(at), 1.0, 1.0);
        }
    }
}
