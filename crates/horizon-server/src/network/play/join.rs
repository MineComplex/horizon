//! The packets of a join, in the order Leaf sends them (`PlayerList.placeNewPlayer` with Paper's,
//! Moonrise's and Leaf's patches; the order is written out in [`super`]'s docs). Packets other
//! systems build appear as [`JoinStep`] markers at their place.

use super::chunk_sending::PlayerDistances;
use super::clientbound::{self as cb, Abilities, Border, ClockUpdate, Login, PlayerInfoEntry};
use crate::azalea::chat::FormattedText;
use crate::core::position::{BlockPos, ChunkPos};
use crate::nbt;

/// What `sendLevelInfo` reads from the level.
#[derive(Clone, Copy, Debug)]
pub struct LevelInfo<'a> {
    pub border: Border,
    /// The level clock manager's full sync for this player (Paper: per-world and per-player time).
    pub game_time: i64,
    pub clocks: &'a [ClockUpdate],
    pub spawn_dimension: &'a str,
    pub spawn_pos: BlockPos,
    pub spawn_yaw: f32,
    pub spawn_pitch: f32,
    pub raining: bool,
    /// The level's `rainLevel`/`thunderLevel` fields.
    pub rain_level: f32,
    pub thunder_level: f32,
    /// `ServerTickRateManager`: `tickrate`, `isFrozen`, `frozenTicksToRun`.
    pub tick_rate: f32,
    pub frozen: bool,
    pub frozen_ticks_to_run: i32,
}

/// Everything a join sends that this module encodes.
#[derive(Clone, Copy, Debug)]
pub struct JoinInputs<'a> {
    pub login: Login<'a>,
    pub difficulty: u8,
    pub difficulty_locked: bool,
    pub abilities: Abilities,
    pub selected_slot: i32,
    /// The permission level (0..=4): `sendPlayerPermissionLevel` sends entity event 24 + level.
    pub permission_level: u8,
    /// The first `awaitingTeleport` of a new connection: 1.
    pub teleport_id: i32,
    pub position: [f64; 3],
    pub y_rot: f32,
    pub x_rot: f32,
    pub level: LevelInfo<'a>,
    /// The joining player's own entry (`createPlayerInitializing(List.of(player))`).
    pub own_info: PlayerInfoEntry<'a>,
    /// The players the joining one can see, in the player list's order, without itself.
    pub others: &'a [PlayerInfoEntry<'a>],
    /// `PlayerJoinEvent`'s message (Leaf's configurable join message); `None` sends nothing.
    pub join_message: Option<&'a FormattedText>,
    /// The player's Moonrise distances when its chunk loader is added.
    pub distances: PlayerDistances,
    pub chunk: ChunkPos,
}

/// One step of the join, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinStep {
    /// A packet for the joining player.
    Packet(Vec<u8>),
    /// A packet for every other player who can see the joining one.
    ToOthers(Vec<u8>),
    /// `ClientboundUpdateRecipesPacket` (item properties, stonecutter recipes).
    UpdateRecipes,
    /// `Commands.sendCommands`: the commands packet. Leaf builds it off the main thread, so it may
    /// reach the client later than this point.
    Commands,
    /// `ServerRecipeBook.sendInitialRecipeBook`: recipe book settings and the known recipes.
    RecipeBook,
    /// `updateEntireScoreboard`: teams, then each displayed objective with its scores.
    Scoreboard,
    /// `initInventoryMenu`: the inventory menu's full content.
    InventoryMenu,
    /// `addEntity` after the join event: the entity tracker pairs the player with the entities in
    /// range (spawn packets both ways); runs right after the chunk loader steps before it.
    TrackEntities,
    /// `sendActivePlayerEffects`, then `sendPostEffects`.
    Effects,
    /// `ClientboundUpdateAttributesPacket` with all the syncable attributes (Paper's
    /// `sendAllPlayerInfo`).
    Attributes,
    /// `refreshEntityData`: the player's full entity data (`ClientboundSetEntityDataPacket`).
    EntityData,
}

/// `sendLevelInfo` with Paper's weather (CraftBukkit's `setPlayerWeather`/`updateWeather`, which
/// sends `STOP_RAINING` when raining and `START_RAINING` otherwise, then the levels it changed) and
/// without the distances Moonrise removed.
pub(super) fn level_info(l: &LevelInfo, out: &mut Vec<JoinStep>) {
    use cb::game_event::*;
    let mut p = |bytes| out.push(JoinStep::Packet(bytes));
    p(cb::initialize_border(&l.border));
    p(cb::set_time(l.game_time, l.clocks));
    p(cb::set_default_spawn_position(
        l.spawn_dimension,
        l.spawn_pos,
        l.spawn_yaw,
        l.spawn_pitch,
    ));
    let (old_rain, old_thunder) = if l.raining {
        p(cb::game_event(STOP_RAINING, 0.0));
        (-l.rain_level, -l.thunder_level)
    } else {
        p(cb::game_event(START_RAINING, 0.0));
        (-1.0, -1.0)
    };
    if old_rain != l.rain_level {
        p(cb::game_event(RAIN_LEVEL_CHANGE, l.rain_level));
    }
    if old_thunder != l.thunder_level {
        p(cb::game_event(THUNDER_LEVEL_CHANGE, l.thunder_level));
    }
    p(cb::game_event(LEVEL_CHUNKS_LOAD_START, 0.0));
    p(cb::ticking_state(l.tick_rate, l.frozen));
    p(cb::ticking_step(l.frozen_ticks_to_run));
}

/// The join, in Leaf's order. Errors only if a component does not fit the NBT writer's limits.
pub fn join_sequence(j: &JoinInputs) -> Result<Vec<JoinStep>, nbt::Error> {
    use JoinStep::*;
    let mut s = vec![
        Packet(cb::login(&j.login)),
        Packet(cb::change_difficulty(j.difficulty, j.difficulty_locked)),
        Packet(cb::player_abilities(&j.abilities)),
        Packet(cb::set_held_slot(j.selected_slot)),
        UpdateRecipes,
        Packet(cb::entity_event(
            j.login.player_id,
            24 + j.permission_level.min(4) as i8,
        )),
        Commands,
        RecipeBook,
        Scoreboard,
        Packet(cb::player_position(
            j.teleport_id,
            &cb::PositionMoveRotation {
                position: j.position,
                delta: [0.0; 3],
                y_rot: j.y_rot,
                x_rot: j.x_rot,
            },
            0,
        )),
    ];
    level_info(&j.level, &mut s);
    s.push(InventoryMenu);
    if let Some(message) = j.join_message {
        // `broadcastSystemMessage`: everyone, the joining player last (it is last in the list).
        let chat = cb::system_chat(message, false)?;
        s.push(ToOthers(chat.clone()));
        s.push(Packet(chat));
    }
    let own = cb::player_info_update(cb::info_action::INITIALIZING, &[j.own_info])?;
    s.push(ToOthers(own.clone()));
    s.push(Packet(own));
    if !j.others.is_empty() {
        s.push(Packet(cb::player_info_update(
            cb::info_action::INITIALIZING,
            j.others,
        )?));
    }
    // `addEntity` → `updatePlayerStatus` → Moonrise's `PlayerChunkLoaderData.add`.
    s.push(Packet(cb::set_chunk_cache_radius(j.distances.send)));
    s.push(Packet(cb::set_simulation_distance(j.distances.tick)));
    s.push(Packet(cb::set_chunk_cache_center(j.chunk.x, j.chunk.z)));
    s.push(TrackEntities);
    level_info(&j.level, &mut s);
    s.push(Effects);
    s.push(InventoryMenu);
    Ok(s)
}
