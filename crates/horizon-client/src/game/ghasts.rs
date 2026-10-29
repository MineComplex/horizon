//! Happy ghasts on the game side: putting a harness on (and shearing it
//! off), riding, getting off on top, and the ghastling a dried ghast block
//! hatches.
use horizon_client::mobs::happy_ghast;
use horizon_core::geometry::Aabb;

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
}
