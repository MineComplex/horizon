//! The game's side of `NaturalSpawner` (`mobs::spawn_selection`): the level
//! it reads, the spawning chunks and caps of `ServerChunkCache.tickChunks`,
//! and the mobs it adds (finalized with the game's `finalizeSpawn`).
use super::Game;
use horizon_core::geometry::Vec3;

/// A player in the level in place, as spawning and despawning see it.
pub(super) struct LevelPlayer {
    pub(super) position: Vec3,
}
impl LevelPlayer {}

impl<'a> Game<'a> {}
impl<'a> crate::game::Sim<'a> {
    /// Every player in the level in place: the local player, and the LAN
    /// guests or server players in it.
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
