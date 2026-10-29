//! Pick Block on an entity (`Minecraft.pickBlockOrEntity`'s entity case and
//! `ServerGamePacketListenerImpl.handlePickItemFromEntity`): the client aims,
//! the server answers with `Entity.getPickResult`.
use super::Game;

impl crate::game::Sim<'_> {
    /// The entity the crosshair hits before any block, as `Entity.getId`; an
    /// entity with nothing to pick (a projectile, a player, the dragon) is
    /// `Some(None)`. `None` when the view hits a block or nothing.
    pub(super) fn pick_entity_hit(&self) -> Option<Option<i32>> {
        let mut best: Option<(f64, Option<i32>)> = None;
        let mut offer = |distance: f64, id: Option<i32>| {
            if self.before_block(distance) && best.is_none_or(|b| distance < b.0) {
                best = Some((distance, id));
            }
        };
        if let Some(hit) = self.remote_object_hit(true) {
            offer(hit.distance, Some(hit.id));
        }
        if let Some((i, d)) = self.mob_hit() {
            offer(d, Some(self.mobs[i].id as i32));
        }
        if let Some((_, d)) = self.guest_hit() {
            offer(d, None);
        }
        if let Some((_, d)) = self.redirectable_hit() {
            offer(d, None);
        }
        if let Some((_, d)) = self.dragon_hit() {
            offer(d, None);
        }
        best.map(|b| b.1)
    }
}
