//! Moonrise's chunk system as far as holding chunks goes: tickets and their levels, chunk holders
//! and their full status, unloading with saving, autosave, and the players' chunk loaders. Plain
//! data owned by the tick thread, one [`ChunkHolderManager`] per level; the region workers only do
//! I/O, by message ([`crate::world::chunk_io::ChunkIo`]). Moonrise's locks, sections and threaded
//! propagator exist for its concurrency and are not ported: the results are.
//!
//! What was compared before writing this: horizon-server had a per-level count of holders (the
//! stand-in this replaces) and Moonrise's distances and rate limiter
//! (`network::play::chunk_sending`, reused here); Horizon's `horizon-game::ticket_levels` computes
//! vanilla's ticket levels by rebuilding a grid on every change (not Moonrise's tickets, and
//! horizon-server depends on no other Horizon crate). No maintained crate covers Minecraft's
//! ticket propagation. The propagator is the usual two-phase incremental update over a map.

mod manager;
pub mod player_loader;
mod propagator;
pub mod ticket;

pub use manager::{ChunkHolderManager, ChunkSettings, ChunkStorage};
pub use player_loader::{ChunkSink, PlayerChunkLoader};
pub use ticket::{FullChunkStatus, TicketType};

#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod tests;
