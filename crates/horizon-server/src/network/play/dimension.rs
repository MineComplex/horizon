//! The packets of a player changing level, in the order Leaf sends them (`ServerPlayer.teleport(
//! TeleportTransition)` when the new level is another dimension; derived from the sources, the
//! pieces are the join's and are checked there).

use super::chunk_sending::PlayerDistances;
use super::clientbound::{self as cb, Abilities, SpawnInfo, keep};
use super::join::{JoinStep, LevelInfo, level_info};
use crate::core::position::ChunkPos;

/// What the change reads from the player and the new level.
#[derive(Clone, Copy, Debug)]
pub struct ChangeInputs<'a> {
    /// `createCommonSpawnInfo(newLevel)`.
    pub spawn: SpawnInfo<'a>,
    /// The new level's `getLevelData()`.
    pub difficulty: u8,
    pub difficulty_locked: bool,
    pub entity_id: i32,
    /// `getProfilePermissions`: `sendPlayerPermissionLevel` sends entity event 24 + level.
    pub permission_level: u8,
    /// The teleport id the client accepts (`internalTeleport` on the connection).
    pub teleport_id: i32,
    pub position: [f64; 3],
    pub y_rot: f32,
    pub x_rot: f32,
    pub abilities: Abilities,
    pub level: LevelInfo<'a>,
    pub selected_slot: i32,
    /// The new level's `reducedDebugInfo` game rule.
    pub reduced_debug_info: bool,
    /// The new level's `immediateRespawn` game rule.
    pub immediate_respawn: bool,
    /// The player's Moonrise distances in the new level's loader, and its chunk.
    pub distances: PlayerDistances,
    pub chunk: ChunkPos,
}

/// The change, in Leaf's order: `ServerPlayer.teleport`'s sends, with `addDuringTeleport`'s loader
/// and tracker steps where the entity enters the new level (see the module docs of
/// [`super`]). Leaving the old level sends nothing.
pub fn dimension_change_sequence(i: &ChangeInputs) -> Vec<JoinStep> {
    use JoinStep::*;
    let mut s = vec![
        Packet(cb::respawn(&i.spawn, keep::ALL_DATA)),
        Packet(cb::change_difficulty(i.difficulty, i.difficulty_locked)),
        // `sendPlayerPermissionLevel`.
        Packet(cb::entity_event(
            i.entity_id,
            24 + i.permission_level.min(4) as i8,
        )),
        // `internalTeleport`.
        Packet(cb::player_position(
            i.teleport_id,
            &cb::PositionMoveRotation {
                position: i.position,
                delta: [0.0; 3],
                y_rot: i.y_rot,
                x_rot: i.x_rot,
            },
            0,
        )),
        // `addDuringTeleport` -> `ChunkMap.addEntity` -> Moonrise's `PlayerChunkLoaderData.add`.
        Packet(cb::set_chunk_cache_radius(i.distances.send)),
        Packet(cb::set_simulation_distance(i.distances.tick)),
        Packet(cb::set_chunk_cache_center(i.chunk.x, i.chunk.z)),
        TrackEntities,
        Packet(cb::player_abilities(&i.abilities)),
    ];
    level_info(&i.level, &mut s);
    // `sendAllPlayerInfo`.
    s.extend([
        InventoryMenu,
        Attributes,
        EntityData,
        Packet(cb::set_held_slot(i.selected_slot)),
        Packet(cb::entity_event(
            i.entity_id,
            if i.reduced_debug_info { 22 } else { 23 },
        )),
        Packet(cb::game_event(
            cb::game_event::IMMEDIATE_RESPAWN,
            if i.immediate_respawn { 1.0 } else { 0.0 },
        )),
        // `sendActivePlayerEffects`, `sendPostEffects`.
        Effects,
    ]);
    s
}
