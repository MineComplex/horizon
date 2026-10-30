//! A player in the play state, as far as the join needs: `ServerGamePacketListenerImpl`'s teleport
//! acceptance, movement, client information, player-loaded and keep-alive handling, and Moonrise's
//! per-player chunk loader (`RegionizedPlayerChunkLoader.PlayerChunkLoaderData`) over a [`Level`].
//!
//! Movement is taken as the client sends it (finite values, clamped to the world's limits as
//! vanilla does): the movement checks (speed, collisions, flying) come with entity physics.
//!
//! The chunk loader keeps Moonrise's distances, its "unload, then radius/simulation, then center"
//! update order, its nearest-first sending and its rate limiter (Paper's default 75 chunks per
//! second). Stand-ins: the chunks it holds are the square of the load distance around the player
//! (Moonrise walks its load, generate, tick and send queues by ticket stage), and the queue is
//! rebuilt from that square each tick instead of being kept incrementally.

use std::collections::HashSet;

use super::level::Level;
use super::player_data::SavedPlayer;
use crate::core::position::ChunkPos;
use crate::network::Reason;
use crate::network::configuration::{ClientInformation, KeepAlive, KeepAliveTick};
use crate::network::listener::Connection;
use crate::network::login::GameProfile;
use crate::network::play::chunk_sending::{
    ChunkRateLimiter, PlayerDistances, ViewDistances, effective_rate, player_distances,
    want_chunk_sent,
};
use crate::network::play::clientbound::{self as cb, GameType, PositionMoveRotation};
use crate::network::play::serverbound::{self as sb, Serverbound};

/// Paper's `chunk-loading-basic.player-max-chunk-send-rate` default.
const CHUNK_SEND_RATE: f64 = 75.0;

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

/// Moonrise's `PlayerChunkLoaderData` (see the module docs).
#[derive(Debug)]
struct ChunkLoader {
    center: ChunkPos,
    distances: PlayerDistances,
    held: HashSet<ChunkPos>,
    sent: HashSet<ChunkPos>,
    limiter: ChunkRateLimiter,
}

fn square(center: ChunkPos, radius: i32) -> impl Iterator<Item = ChunkPos> {
    (-radius..=radius).flat_map(move |dx| {
        (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
    })
}

impl ChunkLoader {
    /// `add`: the radius, simulation and center packets are part of the join.
    fn new(center: ChunkPos, distances: PlayerDistances, level: &mut Level, now_ns: i64) -> Self {
        let held: HashSet<ChunkPos> = square(center, distances.load).collect();
        for &pos in &held {
            level.hold(pos);
        }
        Self {
            center,
            distances,
            held,
            sent: HashSet::new(),
            limiter: ChunkRateLimiter::new(now_ns),
        }
    }

    /// `update`: forget what is no longer wanted, resend changed distances, hold the new square,
    /// then the new center last.
    fn update(
        &mut self,
        center: ChunkPos,
        distances: PlayerDistances,
        level: &mut Level,
        conn: &Connection,
    ) {
        if center == self.center && distances == self.distances {
            return;
        }
        let c = (center.x, center.z);
        self.sent.retain(|&p| {
            let keep = want_chunk_sent(c, distances.send, (p.x, p.z));
            if !keep {
                conn.send(cb::forget_level_chunk(p));
            }
            keep
        });
        if distances.send != self.distances.send {
            conn.send(cb::set_chunk_cache_radius(distances.send));
        }
        if distances.tick != self.distances.tick {
            conn.send(cb::set_simulation_distance(distances.tick));
        }
        let held: HashSet<ChunkPos> = square(center, distances.load).collect();
        for &pos in held.difference(&self.held) {
            level.hold(pos);
        }
        for &pos in self.held.difference(&held) {
            level.release(pos);
        }
        self.held = held;
        if center != self.center {
            conn.send(cb::set_chunk_cache_center(center.x, center.z));
        }
        self.center = center;
        self.distances = distances;
    }

    /// The send queue: wanted, ready chunks nearest first, as many as the limiter allows.
    fn send_chunks(&mut self, level: &Level, conn: &Connection, now_ns: i64) {
        self.limiter.tick(now_ns, effective_rate(CHUNK_SEND_RATE));
        let c = (self.center.x, self.center.z);
        let mut queue: Vec<(i64, i32, i32)> = self
            .held
            .iter()
            .filter(|p| !self.sent.contains(p))
            .filter(|p| want_chunk_sent(c, self.distances.send, (p.x, p.z)))
            .filter(|&&p| level.ready_to_send(p))
            .map(|p| {
                let (dx, dz) = (i64::from(p.x - c.0), i64::from(p.z - c.1));
                (dx * dx + dz * dz, p.x, p.z)
            })
            .collect();
        if queue.is_empty() {
            return;
        }
        queue.sort_unstable();
        let take = self.limiter.take(queue.len() as i64) as usize;
        for &(_, x, z) in &queue[..take] {
            let pos = ChunkPos::new(x, z);
            if let Some(packet) = level.chunk_packet(pos) {
                conn.send(packet);
                self.sent.insert(pos);
            }
        }
    }

    fn release(&mut self, level: &mut Level) {
        for pos in self.held.drain() {
            level.release(pos);
        }
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
    loader: ChunkLoader,
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
}

impl Player {
    /// The new player, its chunk loader holding its chunks; the join teleport is id 1.
    pub fn new(p: NewPlayer, level: &mut Level, now_ns: i64) -> Self {
        let distances = Self::distances_for(p.level_distances, &p.client_information);
        let loader = ChunkLoader::new(chunk_of(p.spawn.position), distances, level, now_ns);
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
            loader,
        }
    }

    fn distances_for(level: ViewDistances, info: &ClientInformation) -> PlayerDistances {
        let client = i32::from(info.view_distance).max(0);
        player_distances(ViewDistances::UNSET, level, client, true)
    }

    pub fn distances(&self) -> PlayerDistances {
        self.loader.distances
    }

    pub fn sent_chunks(&self) -> usize {
        self.loader.sent.len()
    }

    /// `teleport`: a new id, the position awaited until the client accepts it.
    fn teleport(&mut self, conn: &Connection, position: [f64; 3]) {
        self.awaiting_teleport = self.awaiting_teleport.wrapping_add(1);
        if self.awaiting_teleport == i32::MAX {
            self.awaiting_teleport = 0;
        }
        self.awaiting_position = Some(position);
        self.awaiting_teleport_time = self.tick_count;
        let change = PositionMoveRotation {
            position,
            delta: [0.0; 3],
            y_rot: self.y_rot,
            x_rot: self.x_rot,
        };
        conn.send(cb::player_position(self.awaiting_teleport, &change, 0));
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
                self.loader
                    .update(self.loader.center, distances, level, conn);
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
        if chunk != self.loader.center {
            let distances = self.loader.distances;
            self.loader.update(chunk, distances, level, conn);
        }
    }

    /// The player's part of the tick: keep-alive, then the chunks to send.
    pub fn tick(
        &mut self,
        level: &mut Level,
        conn: &Connection,
        now_ns: i64,
    ) -> Result<(), Reason> {
        self.tick_count = self.tick_count.wrapping_add(1);
        match self.keep_alive.tick(now_ns) {
            KeepAliveTick::Send(challenge) => conn.send(cb::keep_alive(challenge)),
            KeepAliveTick::Idle => {}
            KeepAliveTick::TimedOut => {
                tracing::info!("{} was kicked due to keepalive timeout!", self.profile.name);
                return Err(Reason::translatable("disconnect.timeout"));
            }
        }
        self.loader.send_chunks(level, conn, now_ns);
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

    /// Leaving: the chunk loader's holds are given back (`PlayerChunkLoaderData.remove`).
    pub fn remove(&mut self, level: &mut Level) {
        self.loader.release(level);
    }
}

/// `Mth.wrapDegrees(float)`.
fn wrap_degrees(v: f32) -> f32 {
    let mut w = v % 360.0;
    if w >= 180.0 {
        w -= 360.0;
    }
    if w < -180.0 {
        w += 360.0;
    }
    w
}
