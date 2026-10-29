//! The players in the client's level, as the mob code sees them. Spawning
//! is the server's.
use horizon_core::geometry::Vec3;

/// A player in the level in place, as spawning and despawning see it.
pub(super) struct LevelPlayer {
    pub(super) position: Vec3,
}

impl<'a> crate::game::Game<'a> {
    /// The players in the level: the client's own.
    pub(super) fn level_players(&self) -> Vec<LevelPlayer> {
        let mut players: Vec<LevelPlayer> = (Vec::new)();
        if let Some(p) = self.player_view() {
            players.insert(
                0,
                LevelPlayer {
                    position: p.position,
                },
            );
        }
        players
    }
}
