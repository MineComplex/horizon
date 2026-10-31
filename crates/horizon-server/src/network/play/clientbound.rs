//! Encoders for the clientbound play packets a join needs. Each returns the packet body as the
//! byte pipeline takes it (VarInt id, then the fields), from plain values: registry entries are
//! their network ids, resource keys their identifiers, components [`FormattedText`].

use bytes::BufMut;

use super::ids::cb;
use super::{put_identifier, put_utf, put_var_long, start};
use crate::azalea::chat::{FormattedText, codec::write_component};
use crate::core::position::BlockPos;
use crate::nbt;
use crate::network::codec::put_var_int;
use crate::network::login::GameProfile;

/// `GameType` ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GameType {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}

/// `CommonPlayerSpawnInfo`. `seed` is the obfuscated seed (`BiomeManager.obfuscateSeed`), as
/// `createCommonSpawnInfo` sends it.
#[derive(Clone, Copy, Debug)]
pub struct SpawnInfo<'a> {
    /// Id in the `minecraft:dimension_type` registry sent during configuration.
    pub dimension_type: i32,
    pub dimension: &'a str,
    pub seed: i64,
    pub game_type: GameType,
    pub previous_game_type: Option<GameType>,
    pub is_debug: bool,
    pub is_flat: bool,
    pub last_death_location: Option<(&'a str, BlockPos)>,
    pub portal_cooldown: i32,
    pub sea_level: i32,
}

impl SpawnInfo<'_> {
    fn write(&self, out: &mut Vec<u8>) {
        put_var_int(out, self.dimension_type);
        put_identifier(out, self.dimension);
        out.put_i64(self.seed);
        put_var_int(out, self.game_type as i32);
        // `GameType.OPTIONAL_STREAM_CODEC`: id + 1, 0 when absent.
        put_var_int(out, self.previous_game_type.map_or(0, |g| g as i32 + 1));
        out.put_u8(self.is_debug.into());
        out.put_u8(self.is_flat.into());
        out.put_u8(self.last_death_location.is_some().into());
        if let Some((dimension, pos)) = self.last_death_location {
            put_identifier(out, dimension);
            out.put_i64(pos.pack());
        }
        put_var_int(out, self.portal_cooldown);
        put_var_int(out, self.sea_level);
    }
}

/// `ClientboundLoginPacket`. Leaf sends `chunk_radius`/`simulation_distance` from the level's
/// Moonrise distances (`FeatureHooks.getViewDistance`/`getSimulationDistance`, see
/// [`super::chunk_sending::api_distances`]); `levels` in the server's level order.
#[derive(Clone, Copy, Debug)]
pub struct Login<'a> {
    pub player_id: i32,
    pub hardcore: bool,
    pub levels: &'a [&'a str],
    pub max_players: i32,
    pub chunk_radius: i32,
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    /// `!immediateRespawn`.
    pub show_death_screen: bool,
    pub do_limited_crafting: bool,
    pub spawn: SpawnInfo<'a>,
    pub online_mode: bool,
    pub enforces_secure_chat: bool,
}

pub fn login(p: &Login) -> Vec<u8> {
    let mut out = start(cb::LOGIN);
    out.put_i32(p.player_id);
    out.put_u8(p.hardcore.into());
    put_var_int(&mut out, p.levels.len() as i32);
    for level in p.levels {
        put_identifier(&mut out, level);
    }
    put_var_int(&mut out, p.max_players);
    put_var_int(&mut out, p.chunk_radius);
    put_var_int(&mut out, p.simulation_distance);
    out.put_u8(p.reduced_debug_info.into());
    out.put_u8(p.show_death_screen.into());
    out.put_u8(p.do_limited_crafting.into());
    p.spawn.write(&mut out);
    out.put_u8(p.online_mode.into());
    out.put_u8(p.enforces_secure_chat.into());
    out
}

/// `ClientboundRespawnPacket.dataToKeep`.
pub mod keep {
    pub const ATTRIBUTE_MODIFIERS: u8 = 1;
    pub const ENTITY_DATA: u8 = 2;
    pub const ALL_DATA: u8 = 3;
}

/// `ClientboundRespawnPacket`: the new level's `CommonPlayerSpawnInfo` (as in the login packet),
/// then `dataToKeep` (a [`keep`] mask).
pub fn respawn(spawn: &SpawnInfo, data_to_keep: u8) -> Vec<u8> {
    let mut out = start(cb::RESPAWN);
    spawn.write(&mut out);
    out.put_u8(data_to_keep);
    out
}

/// `PositionMoveRotation`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PositionMoveRotation {
    pub position: [f64; 3],
    pub delta: [f64; 3],
    pub y_rot: f32,
    pub x_rot: f32,
}

/// `Relative` bits (`Relative.pack`).
pub mod relative {
    pub const X: i32 = 1;
    pub const Y: i32 = 2;
    pub const Z: i32 = 4;
    pub const Y_ROT: i32 = 8;
    pub const X_ROT: i32 = 16;
    pub const DELTA_X: i32 = 32;
    pub const DELTA_Y: i32 = 64;
    pub const DELTA_Z: i32 = 128;
    pub const ROTATE_DELTA: i32 = 256;
}

/// `ClientboundPlayerPositionPacket`: `id` is the teleport id the client accepts
/// (`ServerGamePacketListenerImpl.awaitingTeleport`, 1 for the join teleport, wrapping to 0 at
/// `i32::MAX`); `relatives` a set of [`relative`] bits.
pub fn player_position(id: i32, change: &PositionMoveRotation, relatives: i32) -> Vec<u8> {
    let mut out = start(cb::PLAYER_POSITION);
    put_var_int(&mut out, id);
    for v in change.position.iter().chain(&change.delta) {
        out.put_f64(*v);
    }
    out.put_f32(change.y_rot);
    out.put_f32(change.x_rot);
    out.put_i32(relatives);
    out
}

/// `ClientboundGameEventPacket.Type` ids.
pub mod game_event {
    pub const NO_RESPAWN_BLOCK_AVAILABLE: u8 = 0;
    pub const START_RAINING: u8 = 1;
    pub const STOP_RAINING: u8 = 2;
    pub const CHANGE_GAME_MODE: u8 = 3;
    pub const WIN_GAME: u8 = 4;
    pub const DEMO_EVENT: u8 = 5;
    pub const PLAY_ARROW_HIT_SOUND: u8 = 6;
    pub const RAIN_LEVEL_CHANGE: u8 = 7;
    pub const THUNDER_LEVEL_CHANGE: u8 = 8;
    pub const PUFFER_FISH_STING: u8 = 9;
    pub const GUARDIAN_ELDER_EFFECT: u8 = 10;
    pub const IMMEDIATE_RESPAWN: u8 = 11;
    pub const LIMITED_CRAFTING: u8 = 12;
    pub const LEVEL_CHUNKS_LOAD_START: u8 = 13;
}

pub fn game_event(event: u8, param: f32) -> Vec<u8> {
    let mut out = start(cb::GAME_EVENT);
    out.put_u8(event);
    out.put_f32(param);
    out
}

pub fn set_chunk_cache_center(x: i32, z: i32) -> Vec<u8> {
    let mut out = start(cb::SET_CHUNK_CACHE_CENTER);
    put_var_int(&mut out, x);
    put_var_int(&mut out, z);
    out
}

pub fn set_chunk_cache_radius(radius: i32) -> Vec<u8> {
    let mut out = start(cb::SET_CHUNK_CACHE_RADIUS);
    put_var_int(&mut out, radius);
    out
}

pub fn set_simulation_distance(distance: i32) -> Vec<u8> {
    let mut out = start(cb::SET_SIMULATION_DISTANCE);
    put_var_int(&mut out, distance);
    out
}

/// `ClientboundForgetLevelChunkPacket`: `writeChunkPos`, the packed long.
pub fn forget_level_chunk(pos: crate::core::position::ChunkPos) -> Vec<u8> {
    let mut out = start(cb::FORGET_LEVEL_CHUNK);
    out.put_i64(pos.pack());
    out
}

/// `ClientboundBlockUpdatePacket`: the position's packed long, the state's id.
pub fn block_update(pos: BlockPos, state: u32) -> Vec<u8> {
    let mut out = start(cb::BLOCK_UPDATE);
    out.put_i64(pos.pack());
    put_var_int(&mut out, state as i32);
    out
}

/// `ClientboundSectionBlocksUpdatePacket`: the section's packed long, then per change
/// `(state id << 12) | section-relative position` as a VarLong.
pub fn section_blocks_update(
    section: crate::core::position::SectionPos,
    changes: &[(u16, u32)],
) -> Vec<u8> {
    let mut out = start(cb::SECTION_BLOCKS_UPDATE);
    out.put_i64(section.pack());
    put_var_int(&mut out, changes.len() as i32);
    for &(pos, state) in changes {
        put_var_long(&mut out, i64::from(state) << 12 | i64::from(pos));
    }
    out
}

/// `ClientboundLevelEventPacket`: event type, position, data, global.
pub fn level_event(event: i32, pos: BlockPos, data: i32, global: bool) -> Vec<u8> {
    let mut out = start(cb::LEVEL_EVENT);
    out.put_i32(event);
    out.put_i64(pos.pack());
    out.put_i32(data);
    out.put_u8(u8::from(global));
    out
}

/// `ClientboundCommandsPacket`.
pub fn commands(packet: &crate::command::network::CommandsPacket) -> Vec<u8> {
    use crate::azalea::buf::AzBuf;
    let mut out = start(cb::COMMANDS);
    packet
        .azalea_write(&mut out)
        .expect("writing to a Vec cannot fail");
    out
}

pub fn chunk_batch_start() -> Vec<u8> {
    start(cb::CHUNK_BATCH_START)
}

pub fn chunk_batch_finished(batch_size: i32) -> Vec<u8> {
    let mut out = start(cb::CHUNK_BATCH_FINISHED);
    put_var_int(&mut out, batch_size);
    out
}

/// `ClientboundSetDefaultSpawnPositionPacket` (`LevelData.RespawnData`: a global position, yaw, pitch).
pub fn set_default_spawn_position(dimension: &str, pos: BlockPos, yaw: f32, pitch: f32) -> Vec<u8> {
    let mut out = start(cb::SET_DEFAULT_SPAWN_POSITION);
    put_identifier(&mut out, dimension);
    out.put_i64(pos.pack());
    out.put_f32(yaw);
    out.put_f32(pitch);
    out
}

/// One entry of `ClientboundSetTimePacket`'s clock map: a `minecraft:world_clock` registry id
/// and its `ClockNetworkState`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockUpdate {
    pub clock: i32,
    pub total_ticks: i64,
    pub partial_tick: f32,
    pub rate: f32,
}

/// `ClientboundSetTimePacket` (26.3: the game time and per-clock states, in the map's order).
pub fn set_time(game_time: i64, clocks: &[ClockUpdate]) -> Vec<u8> {
    let mut out = start(cb::SET_TIME);
    out.put_i64(game_time);
    put_var_int(&mut out, clocks.len() as i32);
    for c in clocks {
        put_var_int(&mut out, c.clock);
        put_var_long(&mut out, c.total_ticks);
        out.put_f32(c.partial_tick);
        out.put_f32(c.rate);
    }
    out
}

pub fn keep_alive(id: i64) -> Vec<u8> {
    let mut out = start(cb::KEEP_ALIVE);
    out.put_i64(id);
    out
}

/// `ClientboundDisconnectPacket` (play): the reason as network NBT.
pub fn disconnect(reason: &FormattedText) -> Result<Vec<u8>, nbt::Error> {
    let mut out = start(cb::DISCONNECT);
    write_component(reason, &mut out)?;
    Ok(out)
}

pub fn system_chat(content: &FormattedText, overlay: bool) -> Result<Vec<u8>, nbt::Error> {
    let mut out = start(cb::SYSTEM_CHAT);
    write_component(content, &mut out)?;
    out.put_u8(overlay.into());
    Ok(out)
}

/// `ClientboundChangeDifficultyPacket`: `Difficulty` id (peaceful 0 .. hard 3).
pub fn change_difficulty(difficulty: u8, locked: bool) -> Vec<u8> {
    let mut out = start(cb::CHANGE_DIFFICULTY);
    put_var_int(&mut out, difficulty.into());
    out.put_u8(locked.into());
    out
}

/// The `Abilities` a player's packet carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Abilities {
    pub invulnerable: bool,
    pub flying: bool,
    pub may_fly: bool,
    pub instabuild: bool,
    pub flying_speed: f32,
    pub walking_speed: f32,
}

pub fn player_abilities(a: &Abilities) -> Vec<u8> {
    let mut out = start(cb::PLAYER_ABILITIES);
    let flags = u8::from(a.invulnerable)
        | u8::from(a.flying) << 1
        | u8::from(a.may_fly) << 2
        | u8::from(a.instabuild) << 3;
    out.put_u8(flags);
    out.put_f32(a.flying_speed);
    out.put_f32(a.walking_speed);
    out
}

pub fn set_held_slot(slot: i32) -> Vec<u8> {
    let mut out = start(cb::SET_HELD_SLOT);
    put_var_int(&mut out, slot);
    out
}

pub fn entity_event(entity_id: i32, event: i8) -> Vec<u8> {
    let mut out = start(cb::ENTITY_EVENT);
    out.put_i32(entity_id);
    out.put_i8(event);
    out
}

/// What `ClientboundInitializeBorderPacket` reads from a `WorldBorder`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub center_x: f64,
    pub center_z: f64,
    pub old_size: f64,
    pub new_size: f64,
    pub lerp_time: i64,
    pub absolute_max_size: i32,
    pub warning_blocks: i32,
    pub warning_time: i32,
}

impl Default for Border {
    /// `new WorldBorder()`.
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_z: 0.0,
            old_size: 5.999_996_8E7,
            new_size: 5.999_996_8E7,
            lerp_time: 0,
            absolute_max_size: 29_999_984,
            warning_blocks: 5,
            warning_time: 15,
        }
    }
}

pub fn initialize_border(b: &Border) -> Vec<u8> {
    let mut out = start(cb::INITIALIZE_BORDER);
    out.put_f64(b.center_x);
    out.put_f64(b.center_z);
    out.put_f64(b.old_size);
    out.put_f64(b.new_size);
    put_var_long(&mut out, b.lerp_time);
    put_var_int(&mut out, b.absolute_max_size);
    put_var_int(&mut out, b.warning_blocks);
    put_var_int(&mut out, b.warning_time);
    out
}

pub fn ticking_state(tick_rate: f32, frozen: bool) -> Vec<u8> {
    let mut out = start(cb::TICKING_STATE);
    out.put_f32(tick_rate);
    out.put_u8(frozen.into());
    out
}

pub fn ticking_step(steps: i32) -> Vec<u8> {
    let mut out = start(cb::TICKING_STEP);
    put_var_int(&mut out, steps);
    out
}

/// `ClientboundPlayerInfoUpdatePacket.Action` bits (`writeEnumSet`: one byte, bit = ordinal).
pub mod info_action {
    pub const ADD_PLAYER: u8 = 1;
    pub const INITIALIZE_CHAT: u8 = 2;
    pub const UPDATE_GAME_MODE: u8 = 4;
    pub const UPDATE_LISTED: u8 = 8;
    pub const UPDATE_LATENCY: u8 = 16;
    pub const UPDATE_DISPLAY_NAME: u8 = 32;
    pub const UPDATE_LIST_ORDER: u8 = 64;
    pub const UPDATE_HAT: u8 = 128;
    /// `createPlayerInitializing`: every action.
    pub const INITIALIZING: u8 = 0xff;
}

/// One `ClientboundPlayerInfoUpdatePacket.Entry`.
#[derive(Clone, Copy, Debug)]
pub struct PlayerInfoEntry<'a> {
    pub profile: &'a GameProfile,
    pub listed: bool,
    pub latency: i32,
    pub game_mode: GameType,
    pub display_name: Option<&'a FormattedText>,
    pub show_hat: bool,
    pub list_order: i32,
    /// `RemoteChatSession.Data` already encoded (session id, key data), when the player has one.
    pub chat_session: Option<&'a [u8]>,
}

/// `ClientboundPlayerInfoUpdatePacket`: each entry writes the fields of `actions`, in the
/// actions' order.
pub fn player_info_update(actions: u8, entries: &[PlayerInfoEntry]) -> Result<Vec<u8>, nbt::Error> {
    use info_action::*;
    let mut out = start(cb::PLAYER_INFO_UPDATE);
    out.put_u8(actions);
    put_var_int(&mut out, entries.len() as i32);
    for e in entries {
        out.put_slice(e.profile.id.as_bytes());
        if actions & ADD_PLAYER != 0 {
            put_utf(&mut out, &e.profile.name);
            put_var_int(&mut out, e.profile.properties.len() as i32);
            for p in &e.profile.properties {
                put_utf(&mut out, &p.name);
                put_utf(&mut out, &p.value);
                out.put_u8(p.signature.is_some().into());
                if let Some(signature) = &p.signature {
                    put_utf(&mut out, signature);
                }
            }
        }
        if actions & INITIALIZE_CHAT != 0 {
            out.put_u8(e.chat_session.is_some().into());
            if let Some(session) = e.chat_session {
                out.put_slice(session);
            }
        }
        if actions & UPDATE_GAME_MODE != 0 {
            put_var_int(&mut out, e.game_mode as i32);
        }
        if actions & UPDATE_LISTED != 0 {
            out.put_u8(e.listed.into());
        }
        if actions & UPDATE_LATENCY != 0 {
            put_var_int(&mut out, e.latency);
        }
        if actions & UPDATE_DISPLAY_NAME != 0 {
            out.put_u8(e.display_name.is_some().into());
            if let Some(name) = e.display_name {
                write_component(name, &mut out)?;
            }
        }
        if actions & UPDATE_LIST_ORDER != 0 {
            put_var_int(&mut out, e.list_order);
        }
        if actions & UPDATE_HAT != 0 {
            out.put_u8(e.show_hat.into());
        }
    }
    Ok(out)
}
