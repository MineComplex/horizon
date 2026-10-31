//! The play state's packets as Leaf 26.3 sends and reads them: ids ([`ids`]), encoders for the
//! clientbound packets of a join ([`clientbound`], [`chunk`]), decoders for the serverbound ones
//! ([`serverbound`]), the join sequence ([`join`]) and Moonrise's chunk-sending math
//! ([`chunk_sending`]). Everything takes plain values: no dependency on the level, entities or
//! registries, whose network ids the caller passes. Checked byte for byte against the 26.3
//! classes (`testdata/play_oracle.txt`, printed by a Java oracle kept outside Git).
//!
//! # The join (`PlayerList.placeNewPlayer`)
//!
//! Vanilla 26.3 plus Paper, Moonrise and Leaf ([`join::join_sequence`] builds it):
//!
//! 1. `login` with Moonrise's distances: chunk radius = the level's load distance - 1, simulation
//!    distance = its tick distance ([`chunk_sending::api_distances`]).
//! 2. CraftBukkit's plugin channel `minecraft:register`: only when plugins listen on channels
//!    (never here: no plugins).
//! 3. `change_difficulty`, `player_abilities`, `set_held_slot`, `update_recipes`.
//! 4. `entity_event` 24 + permission level, then the commands packet (Leaf builds it off the main
//!    thread, so it can come later).
//! 5. The recipe book (settings, known recipes), the scoreboard (teams, displayed objectives).
//! 6. Vanilla broadcasts the join message here; Paper moves it after `PlayerJoinEvent`.
//! 7. `player_position` (the join teleport, id 1, no relatives, zero motion).
//! 8. `server_data` (MOTD, icon): removed by Leaf's Cleanup.
//! 9. Vanilla's player info packets here: CraftBukkit moves them after the join event.
//! 10. `sendLevelInfo`: `initialize_border`, `set_time` (Paper: the level's clocks for this
//!     player), `set_default_spawn_position`; Paper's `set_chunk_cache_radius`/
//!     `set_simulation_distance` are removed again by Moonrise; weather the CraftBukkit way
//!     (`STOP_RAINING` if raining, else `START_RAINING`; then `RAIN_LEVEL_CHANGE` and
//!     `THUNDER_LEVEL_CHANGE` with the level's fields, each only if it differs from the "old"
//!     value: the negated level when raining, -1 when not); `LEVEL_CHUNKS_LOAD_START`;
//!     `ticking_state`, `ticking_step`.
//! 11. `level.addNewPlayer`: Paper suppresses entity tracking until after the player list packets,
//!     so nothing is sent; boss bars (`CustomBossEvents.onPlayerConnect`); `initInventoryMenu`.
//! 12. `PlayerJoinEvent`, then its message (Leaf's configurable join message) to everyone.
//! 13. Player info (`createPlayerInitializing`, every action): the new player's entry to everyone
//!     who can see it, the new player included; then, if any, one packet with every other visible
//!     player to the new one (Paper).
//! 14. `addEntity` now runs: the tracker, and `updatePlayerStatus` adds the player to Moonrise's
//!     chunk loader, which sends `set_chunk_cache_radius` (send distance), `set_simulation_distance`
//!     (tick distance) and `set_chunk_cache_center`.
//! 15. `sendLevelInfo` again (CraftBukkit), then `sendActivePlayerEffects`, `sendPostEffects`,
//!     `initInventoryMenu` again. Vanilla's notification service and Paper's collide-rule team are
//!     removed by Leaf; Purpur's TPS bar is added only for players who turned it on.
//! 16. A dead player gets an empty chunk at its position (Paper), since Moonrise sends no chunks
//!     to the dead.
//!
//! # Changing level (`ServerPlayer.teleport(TeleportTransition)` into another dimension)
//!
//! [`dimension::dimension_change_sequence`]: `respawn` (the new level's spawn info, keep all
//! data), `change_difficulty` (the new level's), `entity_event` 24 + permission level; the server
//! side leaves the old level (its chunk loader's tickets become delayed ones, no packet); then
//! `player_position` (a new teleport id), the new loader's `set_chunk_cache_radius`,
//! `set_simulation_distance`, `set_chunk_cache_center` and the entity tracker, `player_abilities`,
//! `sendLevelInfo` (the same as the join's), `sendAllPlayerInfo` (the inventory menu, all
//! attributes, entity data, `set_held_slot`, entity event 22/23 for reduced debug info,
//! `IMMEDIATE_RESPAWN`), the active effects and post effects.
//!
//! # Chunk sending (Moonrise's `RegionizedPlayerChunkLoader`)
//!
//! Moonrise replaces vanilla's `ChunkMap` tracking views and `PlayerChunkSender` batching:
//! `markChunkPendingToSend` is gone, so the vanilla sender's set stays empty and Leaf **never
//! sends `chunk_batch_start`/`chunk_batch_finished`**; `chunk_batch_received` is still read
//! (it sets a desired rate nobody uses). Each tick, per player (`PlayerChunkLoaderData`):
//!
//! - Distances ([`chunk_sending::player_distances`]): tick = min(tick, load - 1),
//!   load = max(tick + 1, load), send = min(load - 1, player send ?: client view distance + 1
//!   when `auto-config-send-distance` (default) and known ?: level send ?: load - 1). The level's
//!   load distance is `view-distance` (2..=32) + 1, its tick distance `simulation-distance`.
//! - `update()` (when the player's chunk, distances or spectator-generation changed): unloads
//!   chunks no longer wanted (`forget_level_chunk`), resends radius/simulation if changed, then
//!   walks every chunk within load + 1 in Euclidean order (Moonrise's precomputed
//!   `ParallelSearchRadiusIteration`) sorting each into the load, generate, tick and send queues
//!   by its ticket stage, and sends the new center last so the client keeps the unloads.
//! - A chunk is sent when it is FULL with its 8 neighbours generated, within the square of
//!   send + 1 and vanilla's view circle ([`chunk_sending::want_chunk_sent`]), closest first (a
//!   heap by squared distance to the player's chunk); post-processing runs just before sending.
//! - Rates ([`chunk_sending::ChunkRateLimiter`]): each queue has a staggered limiter allowing 4x
//!   the rate over 100 ms, 2x over 500 ms, 1.25x over 1 s, 1x over 3 s; Paper's defaults are
//!   75 sends/s, 100 loads/s, unlimited generation ([`chunk_sending::MAX_RATE`]). Concurrent loads
//!   and generations are capped at max(5, (2 load + 1)^2 / 5) unless configured.
//! - Each sent chunk is one `level_chunk_with_light` built at once (light from Starlight, see
//!   [`chunk`]), followed by Paper's extra block entity packets past 750.

pub mod chunk;
pub mod chunk_sending;
pub mod clientbound;
pub mod dimension;
pub mod ids;
pub mod join;
pub mod serverbound;
#[cfg(test)]
mod tests;

use bytes::BufMut;

use crate::network::codec::put_var_int;

/// A packet body starting with its VarInt id.
fn start(id: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    put_var_int(&mut out, id.into());
    out
}

/// `writeUtf`.
fn put_utf(out: &mut Vec<u8>, s: &str) {
    put_var_int(out, s.len() as i32);
    out.put_slice(s.as_bytes());
}

/// `Identifier.STREAM_CODEC` / `ResourceKey.streamCodec`: the identifier's text.
fn put_identifier(out: &mut Vec<u8>, id: &str) {
    put_utf(out, id);
}

/// `VarLong.write`.
fn put_var_long(out: &mut Vec<u8>, value: i64) {
    let mut v = value as u64;
    while v & !0x7f != 0 {
        out.put_u8((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.put_u8(v as u8);
}
