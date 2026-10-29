//! Mobs and the guests of an open world. The mob code sees one player
//! (`MobWorld.player`, `Who::Player`); with guests on, each mob sees the
//! player vanilla's `Level.getNearestPlayer` would give it, and keeps the
//! guest it targets until it lets go. What it then does to `Who::Player`
//! (hits, pushes, fire, its projectiles) runs with that guest swapped in.
use super::Game;





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
        return;
    }
    /// The guests' boxes for the blocks entities trigger (pressure plates,
    /// tripwires, detector rails): `Entity.checkInsideBlocks` and
    /// `Level.getEntities` see every player in the level, but not spectators
    /// (`Entity.isIgnoringBlockTriggers`).
    pub(super) fn lan_block_trigger_boxes(&self) -> Vec<([f64; 3], [f64; 3])> {
        return Vec::new();
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

impl crate::game::Sim<'_> {
    /// Every guest in the world (`ServerLevel.getPlayers` past the host).
    pub(super) fn lan_present(&self) -> Vec<Present> {
        Vec::new()
    }
    /// The UUID of the player acting: the guest `as_guest` runs, else the host.
    pub(super) fn acting_uuid(&self) -> [i32; 4] {
        None::<u128>
            .map(horizon_client::lan::playerdata::uuid_ints)
            .or(self.level.player_uuid)
            .unwrap_or_default()
    }
}


