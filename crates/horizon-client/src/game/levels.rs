//! A server's levels, one per dimension (`MinecraftServer.levels`).
//!
//! The game's tick code reads one level: the one in place, held in `Game`'s
//! own fields (the world, its streaming and save, block ticks, entities).
//! A server (the dedicated server and single player's server thread) keeps
//! the other dimensions' levels here and ticks each one with players or forced
//! chunks by swapping it into place (`enter_level`), the way `as_guest` swaps a player
//! in, so the tick code runs unchanged, one level at a time. The in-frame
//! game holds one level, which follows its player (`enter_dimension`).
//!
//! Shared by every level, as vanilla's derived levels share the
//! Overworld's level data: the game time, the day time and the weather, the
//! game rules, the scoreboard, the maps and the entities waiting to arrive
//! through a portal.

use super::Dim;
use std::collections::HashMap;

/// A level that is not in place: every per-level field of `Game`.
pub(super) struct Parked {}

/// The levels not in place, by dimension.
#[derive(Default)]
pub(super) struct Levels {
    parked: HashMap<Dim, Parked>,
}

impl Levels {
    /// The levels that are not in place, by dimension.
    pub(super) fn parked_dims(&self) -> Vec<Dim> {
        self.parked.keys().copied().collect()
    }
}

impl crate::game::Sim<'_> {
    /// The level a command's source is in: a client holds only its own, so
    /// nothing is swapped in (the server runs the commands of other levels).
    pub(super) fn enter_command_level(&mut self, _dim: Dim) -> Option<Dim> {
        None
    }
}
