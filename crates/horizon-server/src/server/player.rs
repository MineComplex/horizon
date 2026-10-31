//! A player in the play state, as far as the join needs: `ServerGamePacketListenerImpl`'s teleport
//! acceptance, movement, client information, player-loaded and keep-alive handling, and Moonrise's
//! per-player chunk loader (`RegionizedPlayerChunkLoader.PlayerChunkLoaderData`) over a [`Level`].
//!
//! Movement is taken as the client sends it (finite values, clamped to the world's limits as
//! vanilla does): the movement checks (speed, collisions, flying) come with entity physics.
//!
//! The chunk loader is Moonrise's (`server::chunk_system::player_loader`): its tickets hold the
//! player's chunks in the level's chunk holders, and it sends them from the level's tick.

use super::chunk_system::{ChunkSink, PlayerChunkLoader};
use super::level::Level;
use super::player_data::SavedPlayer;
use crate::command::wrap_degrees;
use crate::core::position::ChunkPos;
use crate::network::Reason;
use crate::network::configuration::{ClientInformation, KeepAlive, KeepAliveTick};
use crate::network::listener::Connection;
use crate::network::login::GameProfile;
use crate::network::play::chunk_sending::{PlayerDistances, ViewDistances, player_distances};
use crate::network::play::clientbound::{self as cb, GameType, PositionMoveRotation};
use crate::network::play::serverbound::{self as sb, Serverbound};

/// Where a joining player is placed (`PrepareSpawnTask`'s result).
#[derive(Clone, Debug, PartialEq)]
pub struct Spawn {
    pub position: [f64; 3],
    pub y_rot: f32,
    pub x_rot: f32,
}

pub fn chunk_of(position: [f64; 3]) -> ChunkPos {
    ChunkPos::new(
        (position[0].floor() as i32) >> 4,
        (position[2].floor() as i32) >> 4,
    )
}

/// The loader's packets on the player's connection; chunks are queued and their packets built
/// once the loader is done with the level's holders.
struct Packets<'a> {
    conn: &'a Connection,
    chunks: Vec<ChunkPos>,
}

impl<'a> Packets<'a> {
    fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            chunks: Vec::new(),
        }
    }

    fn send_chunks(self, level: &Level) {
        for pos in self.chunks {
            if let Some(packet) = level.chunk_packet(pos) {
                self.conn.send(packet);
            }
        }
    }
}

impl ChunkSink for Packets<'_> {
    fn forget(&mut self, pos: ChunkPos) {
        self.conn.send(cb::forget_level_chunk(pos));
    }
    fn radius(&mut self, distance: i32) {
        self.conn.send(cb::set_chunk_cache_radius(distance));
    }
    fn simulation_distance(&mut self, distance: i32) {
        self.conn.send(cb::set_simulation_distance(distance));
    }
    fn center(&mut self, pos: ChunkPos) {
        self.conn.send(cb::set_chunk_cache_center(pos.x, pos.z));
    }
    fn send_chunk(&mut self, pos: ChunkPos) {
        self.chunks.push(pos);
    }
}

#[derive(Debug)]
pub struct Player {
    pub profile: GameProfile,
    pub entity_id: i32,
    pub position: [f64; 3],
    pub y_rot: f32,
    pub x_rot: f32,
    pub game_type: GameType,
    pub previous_game_type: Option<GameType>,
    pub client_information: ClientInformation,
    pub latency: i32,
    /// `hasClientLoaded`.
    pub client_loaded: bool,
    keep_alive: KeepAlive,
    /// The saved file's tag, written back with the ported fields replaced.
    saved: SavedPlayer,
    tick_count: i32,
    awaiting_teleport: i32,
    awaiting_position: Option<[f64; 3]>,
    awaiting_teleport_time: i32,
    level_distances: ViewDistances,
    /// The index of the player's level in `World::levels`.
    pub level: usize,
    loader: PlayerChunkLoader,
}

/// What a join brings to [`Player::new`].
pub struct NewPlayer {
    pub profile: GameProfile,
    pub entity_id: i32,
    pub spawn: Spawn,
    pub game_type: GameType,
    pub previous_game_type: Option<GameType>,
    pub client_information: ClientInformation,
    pub keep_alive: KeepAlive,
    pub latency: i32,
    pub saved: SavedPlayer,
    pub level_distances: ViewDistances,
    /// The level the player joins (its index in `World::levels`); `level` of [`Player::new`].
    pub level: usize,
}

impl Player {
    /// The new player, its chunk loader holding its chunks (`RegionizedPlayerChunkLoader.addPlayer`:
    /// the distance and center packets are in the join); the join teleport is id 1.
    pub fn new(p: NewPlayer, level: &mut Level, conn: &Connection, now_ns: i64) -> Self {
        let distances = Self::distances_for(p.level_distances, &p.client_information);
        let id = level.chunks.next_ticket_id();
        let loader = PlayerChunkLoader::new(
            id,
            chunk_of(p.spawn.position),
            distances,
            &mut level.chunks,
            &mut Packets::new(conn),
            now_ns,
        );
        Self {
            profile: p.profile,
            entity_id: p.entity_id,
            position: p.spawn.position,
            y_rot: p.spawn.y_rot,
            x_rot: p.spawn.x_rot,
            game_type: p.game_type,
            previous_game_type: p.previous_game_type,
            client_information: p.client_information,
            latency: p.latency,
            client_loaded: false,
            keep_alive: p.keep_alive,
            saved: p.saved,
            tick_count: 0,
            awaiting_teleport: 1,
            awaiting_position: Some(p.spawn.position),
            awaiting_teleport_time: 0,
            level_distances: p.level_distances,
            level: p.level,
            loader,
        }
    }

    fn distances_for(level: ViewDistances, info: &ClientInformation) -> PlayerDistances {
        let client = i32::from(info.view_distance).max(0);
        player_distances(ViewDistances::UNSET, level, client, true)
    }

    pub fn distances(&self) -> PlayerDistances {
        self.loader.distances()
    }

    pub fn sent_chunks(&self) -> usize {
        self.loader.sent_count()
    }

    /// `teleport`: a new id, the position awaited until the client accepts it.
    fn teleport(&mut self, conn: &Connection, position: [f64; 3]) {
        let id = self.begin_teleport(position);
        let change = PositionMoveRotation {
            position,
            delta: [0.0; 3],
            y_rot: self.y_rot,
            x_rot: self.x_rot,
        };
        conn.send(cb::player_position(id, &change, 0));
    }

    /// The state of `teleport` without the packet: the next id, which the client must accept.
    fn begin_teleport(&mut self, position: [f64; 3]) -> i32 {
        self.awaiting_teleport = self.awaiting_teleport.wrapping_add(1);
        if self.awaiting_teleport == i32::MAX {
            self.awaiting_teleport = 0;
        }
        self.awaiting_position = Some(position);
        self.awaiting_teleport_time = self.tick_count;
        self.awaiting_teleport
    }

    /// The player's side of `ServerPlayer.teleport` into another level (the new level's packets
    /// are [`crate::network::play::dimension::dimension_change_sequence`], which the caller sends
    /// with the id returned): the old level's chunk loader lets go
    /// (`removePlayerImmediately`: its tickets become delayed ones, nothing is sent), the player
    /// takes its place in the new level (`setServerLevel`, `internalTeleport`) and a new chunk
    /// loader holds the chunks around it there (`addDuringTeleport`). The client reloads the world
    /// (`restartClientLoadTimerAfterRespawn`), so `PlayerLoaded` is awaited again.
    #[allow(clippy::too_many_arguments)]
    pub fn change_level(
        &mut self,
        old: &mut Level,
        new: &mut Level,
        new_index: usize,
        position: [f64; 3],
        rotation: (f32, f32),
        conn: &Connection,
        now_ns: i64,
    ) -> i32 {
        self.loader.remove(&mut old.chunks);
        self.client_loaded = false;
        self.level = new_index;
        (self.y_rot, self.x_rot) = rotation;
        self.position = position;
        let id = self.begin_teleport(position);
        let distances = Self::distances_for(self.level_distances, &self.client_information);
        let ticket = new.chunks.next_ticket_id();
        self.loader = PlayerChunkLoader::new(
            ticket,
            chunk_of(position),
            distances,
            &mut new.chunks,
            &mut Packets::new(conn),
            now_ns,
        );
        id
    }

    /// `ServerPlayer.teleport` inside the level (`internalTeleport`): the position packet only.
    pub fn teleport_within_level(
        &mut self,
        position: [f64; 3],
        rotation: (f32, f32),
        conn: &Connection,
    ) {
        (self.y_rot, self.x_rot) = rotation;
        self.teleport(conn, position);
    }

    /// One play packet (keep-alives included). `Err` disconnects with the reason.
    pub fn handle(
        &mut self,
        body: &[u8],
        level: &mut Level,
        conn: &Connection,
        now_ns: i64,
    ) -> Result<(), Reason> {
        let packet = sb::decode(body).map_err(|fault| fault.reason())?;
        let invalid = || Reason::translatable("multiplayer.disconnect.invalid_player_movement");
        match packet {
            Serverbound::KeepAlive(challenge) => {
                self.latency = self.keep_alive.answer(challenge, now_ns).map_err(|why| {
                    tracing::info!(
                        "Disconnecting {} for sending keepalive response ({challenge}) {why}!",
                        self.profile.name
                    );
                    Reason::translatable("disconnect.timeout")
                })?;
            }
            // `handleAcceptTeleportPacket`.
            Serverbound::AcceptTeleportation { id, .. } if id == self.awaiting_teleport => {
                let position = self.awaiting_position.take().ok_or_else(invalid)?;
                self.move_to(position, level, conn);
            }
            Serverbound::AcceptTeleportation { .. } => {}
            // `handleMovePlayer` without the movement checks (module docs).
            Serverbound::MovePlayer {
                position, rotation, ..
            } => {
                let finite = position.is_none_or(|p| p.iter().all(|v| v.is_finite()))
                    && rotation.is_none_or(|(y, x)| y.is_finite() && x.is_finite());
                if !finite {
                    return Err(invalid());
                }
                if let Some(awaiting) = self.awaiting_position {
                    if self.tick_count - self.awaiting_teleport_time > 20 {
                        self.teleport(conn, awaiting);
                    }
                    return Ok(());
                }
                if let Some((y, x)) = rotation {
                    self.y_rot = wrap_degrees(y);
                    self.x_rot = wrap_degrees(x).clamp(-90.0, 90.0);
                }
                if let Some([x, y, z]) = position {
                    let clamp = |v: f64, limit: f64| v.clamp(-limit, limit);
                    let p = [clamp(x, 3.0E7), clamp(y, 2.0E7), clamp(z, 3.0E7)];
                    self.move_to(p, level, conn);
                }
            }
            // `updateOptions`: a new view distance moves the send distance (Moonrise).
            Serverbound::ClientInformation(info) => {
                let sb::ClientInformation {
                    language,
                    view_distance,
                    chat_visibility,
                    chat_colors,
                    model_customisation,
                    main_hand,
                    text_filtering_enabled,
                    allows_listing,
                    particle_status,
                } = info;
                self.client_information = ClientInformation {
                    language,
                    view_distance,
                    chat_visibility,
                    chat_colors,
                    model_customisation,
                    main_hand,
                    text_filtering_enabled,
                    allows_listing,
                    particle_status,
                };
                let distances = Self::distances_for(self.level_distances, &self.client_information);
                let center = self.loader.center();
                self.loader.update(
                    center,
                    distances,
                    &mut level.chunks,
                    &mut Packets::new(conn),
                );
            }
            Serverbound::PlayerLoaded => self.client_loaded = true,
            // Moonrise reads the desired rate and uses nothing of it.
            Serverbound::ChunkBatchReceived { .. } | Serverbound::ClientTickEnd => {}
            Serverbound::Other { id, .. } => tracing::debug!(
                "{}: play packet minecraft:{} is not handled yet",
                self.profile.name,
                crate::network::play::ids::SERVERBOUND[id as usize]
            ),
        }
        Ok(())
    }

    /// The new position; crossing into another chunk re-centres the chunk loader.
    fn move_to(&mut self, position: [f64; 3], level: &mut Level, conn: &Connection) {
        self.position = position;
        let chunk = chunk_of(position);
        if chunk != self.loader.center() {
            let distances = self.loader.distances();
            self.loader
                .update(chunk, distances, &mut level.chunks, &mut Packets::new(conn));
        }
    }

    /// The player's chunk loader in the level's tick (`RegionizedPlayerChunkLoader.tick`):
    /// loads, generation, ticking and sends.
    pub fn tick_chunks(&mut self, level: &mut Level, conn: &Connection, now_ns: i64) {
        let mut packets = Packets::new(conn);
        self.loader
            .update_queues(&mut level.chunks, &mut packets, now_ns);
        packets.send_chunks(level);
    }

    /// The player's part of the connection tick: keep-alive.
    pub fn tick(&mut self, conn: &Connection, now_ns: i64) -> Result<(), Reason> {
        self.tick_count = self.tick_count.wrapping_add(1);
        match self.keep_alive.tick(now_ns) {
            KeepAliveTick::Send(challenge) => conn.send(cb::keep_alive(challenge)),
            KeepAliveTick::Idle => {}
            KeepAliveTick::TimedOut => {
                tracing::info!("{} was kicked due to keepalive timeout!", self.profile.name);
                return Err(Reason::translatable("disconnect.timeout"));
            }
        }
        Ok(())
    }

    /// What `PlayerDataStorage.save` writes for this player.
    pub fn saved_data(&self, dimension: &str) -> SavedPlayer {
        SavedPlayer {
            dimension: Some(dimension.to_owned()),
            position: Some(self.position),
            rotation: Some((self.y_rot, self.x_rot)),
            game_type: Some(self.game_type),
            previous_game_type: self.previous_game_type,
            tag: self.saved.tag.clone(),
        }
    }

    /// Leaving: the chunk loader's tickets become delayed ones (`PlayerChunkLoaderData.remove`).
    pub fn remove(&mut self, level: &mut Level) {
        self.loader.remove(&mut level.chunks);
    }
}
