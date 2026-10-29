//! `RespawnAnchorBlock` in the game: glowstone charges it (`useItemOn`), a
//! right click on a charged anchor sets the respawn point in the Nether and
//! blows it up anywhere else (`useWithoutItem`), and respawning goes to it
//! (`ServerPlayer.findRespawnAndUseSpawnBlock`, `PlayerList.respawn`),
//! using a charge.
use super::{Dim, Game};

/// `ServerPlayer.RespawnConfig`: a respawn point other than the world spawn
/// that the game resolves through its block (a respawn anchor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RespawnConfig {
    pub dim: Dim,
    pub pos: [i32; 3],
    pub yaw: f32,
    pub forced: bool,
}

/// A respawn point as a player's save keeps it (`respawn`): dimension,
/// block, yaw and whether it is forced.
pub(super) type SavedRespawn = (String, [i32; 3], f32, bool);

impl RespawnConfig {
    pub(super) fn saved(&self) -> SavedRespawn {
        (self.dim.id().to_owned(), self.pos, self.yaw, self.forced)
    }
}

impl Game<'_> {
    // ---- respawning -------------------------------------------------------
}
impl crate::game::Sim<'_> {
    pub(super) fn block_in_tag(&self, tag: &str, block: &str) -> bool {
        self.res
            .items()
            .block_tags
            .get(tag)
            .is_some_and(|s| s.contains(block))
    }
}
