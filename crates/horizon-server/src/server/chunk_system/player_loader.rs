//! Moonrise's `RegionizedPlayerChunkLoader.PlayerChunkLoaderData`: a player's chunks go through
//! ticket stages — loading (`LOADED_TICKET_LEVEL`, 44), generated (`FULL`, 33), ticking
//! (`ENTITY_TICKING`, 31) — and are sent once full with their 8 neighbours generated, nearest
//! first. The distances and rate limiters are `network::play::chunk_sending`'s; this keeps the
//! queues and tickets (derived from Moonrise's sources).
//!
//! - `update` (a new chunk or distances) forgets the chunks that left the send area, moves the
//!   tickets that left the load and tick areas to `PLAYER_TICKET_DELAYED` (they keep their level
//!   for `chunks.delay-chunk-unloads-by`), and rebuilds the queues from the square of the load
//!   distance + 1, nearest first. Removals follow `SingleUserAreaMap`'s iteration order.
//! - `update_queues` (each level tick) moves chunks along: up to Paper's 100 loads per second
//!   (at most a fifth of the area at once), generation without limit (`-1`), up to 75 sends per
//!   second. Queues are heaps by squared distance; ties (unordered in Moonrise's heap) go by
//!   chunk coordinates.
//! - A loaded chunk is generated when it is full on disk, or when the player may generate
//!   chunks: no generator exists yet (WP6), so a chunk that is not on disk (or not finished)
//!   stays loaded and is never sent; the chunks next to it are not sent either (their neighbours
//!   are not generated), and the send queue waits on the nearest such chunk, as Moonrise's does.
//! - Removing the loader moves every ticket to `PLAYER_TICKET_DELAYED`. The forget packets
//!   Moonrise sends then would reach a closed or closing connection (leaving, stopping): none are
//!   sent.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use super::manager::{ChunkHolderManager, ChunkStorage};
use super::ticket::{ENTITY_TICKING_LEVEL, FULL_LEVEL, LOADED_LEVEL, TicketType};
use crate::core::position::ChunkPos;
use crate::network::play::chunk_sending::{
    ChunkRateLimiter, MAX_RATE, PlayerDistances, effective_rate, is_within_distance,
    want_chunk_sent,
};

/// Paper's `chunk-loading-basic` defaults (`paper-global.yml` is not read for them yet).
const MAX_CHUNK_SEND_RATE: f64 = 75.0;
const MAX_CHUNK_LOAD_RATE: f64 = 100.0;
const MAX_CHUNK_GENERATE_RATE: f64 = -1.0;
/// `chunk-loading-advanced.player-max-concurrent-chunk-{loads,generates}`: 0 is automatic.
const MAX_CONCURRENT: i64 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Loading,
    Loaded,
    Generating,
    Generated,
    Tick,
}

impl Stage {
    /// `TICKET_STAGE_TO_LEVEL`.
    const fn level(self) -> u8 {
        match self {
            Self::Loading | Self::Loaded => LOADED_LEVEL,
            Self::Generating | Self::Generated => FULL_LEVEL,
            Self::Tick => ENTITY_TICKING_LEVEL,
        }
    }
}

/// `ChunkHolderManager.TicketOperation`, all with the loader's id.
#[derive(Clone, Copy, Debug)]
enum TicketOp {
    Add(ChunkPos, TicketType, u8),
    AddAndRemove(ChunkPos, (TicketType, u8), (TicketType, u8)),
}

/// A queue by distance to the center (`LongHeapPriorityQueue` with the distance comparator).
#[derive(Debug, Default)]
struct Queue(BinaryHeap<Reverse<(i64, i32, i32)>>);

impl Queue {
    fn push(&mut self, center: ChunkPos, pos: ChunkPos) {
        let (dx, dz) = (i64::from(pos.x - center.x), i64::from(pos.z - center.z));
        self.0.push(Reverse((dx * dx + dz * dz, pos.x, pos.z)));
    }
    fn first(&self) -> Option<ChunkPos> {
        self.0
            .peek()
            .map(|Reverse((_, x, z))| ChunkPos::new(*x, *z))
    }
    fn pop(&mut self) -> Option<ChunkPos> {
        self.0.pop().map(|Reverse((_, x, z))| ChunkPos::new(x, z))
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn clear(&mut self) {
        self.0.clear();
    }
}

/// What the loader asks of the connection: Moonrise's packets, in order.
pub trait ChunkSink {
    /// `ClientboundForgetLevelChunkPacket`.
    fn forget(&mut self, pos: ChunkPos);
    /// `ClientboundSetChunkCacheRadiusPacket`.
    fn radius(&mut self, distance: i32);
    /// `ClientboundSetSimulationDistancePacket`.
    fn simulation_distance(&mut self, distance: i32);
    /// `ClientboundSetChunkCacheCenterPacket`.
    fn center(&mut self, pos: ChunkPos);
    /// `PlayerChunkSender.sendChunk` for a full chunk.
    fn send_chunk(&mut self, pos: ChunkPos);
}

/// One player's chunk loader (see the module docs).
#[derive(Debug)]
pub struct PlayerChunkLoader {
    id: u64,
    center: ChunkPos,
    distances: PlayerDistances,
    can_generate: bool,
    last_sent_center: ChunkPos,
    last_sent_radius: i32,
    last_sent_simulation: i32,
    stages: HashMap<ChunkPos, Stage>,
    sent: HashSet<ChunkPos>,
    send_limiter: ChunkRateLimiter,
    load_limiter: ChunkRateLimiter,
    gen_limiter: ChunkRateLimiter,
    send_queue: Queue,
    ticking_queue: Queue,
    generating_queue: Queue,
    gen_queue: Queue,
    loading_queue: Queue,
    load_queue: Queue,
    delayed: Vec<TicketOp>,
}

/// The square of `radius` around `center` minus the square of `new_radius` around `to`, in
/// `SingleUserAreaMap.update`'s order of remove callbacks.
fn area_removed(from: ChunkPos, old: i32, to: ChunkPos, new: i32) -> Vec<ChunkPos> {
    let mut out = Vec::new();
    let outside_new = |x: i32, z: i32| (x - to.x).abs().max((z - to.z).abs()) > new;
    let (dx, dz) = (to.x - from.x, to.z - from.z);
    if dx.abs().max(dz.abs()) > 2 * new.max(old) || old != new {
        for x in from.x - old..=from.x + old {
            for z in from.z - old..=from.z + old {
                if outside_new(x, z) {
                    out.push(ChunkPos::new(x, z));
                }
            }
        }
        return out;
    }
    let sign = |v: i32| if v >= 0 { 1 } else { -1 };
    let (up, right) = (sign(dz), sign(dx));
    let mut rect = |min_x: i32, max_x: i32, min_z: i32, max_z: i32| {
        let mut x = min_x;
        while x != max_x {
            let mut z = min_z;
            while z != max_z {
                out.push(ChunkPos::new(x, z));
                z += up;
            }
            x += right;
        }
    };
    if dx != 0 {
        rect(
            from.x - old * right,
            to.x - old * right,
            to.z - old * up,
            from.z + old * up + up,
        );
    }
    if dz != 0 {
        rect(
            from.x - old * right,
            from.x + old * right + right,
            from.z - old * up,
            to.z - old * up,
        );
    }
    out
}

impl PlayerChunkLoader {
    /// `add`: the radius, simulation distance and center are in the join packets already.
    pub fn new<S: ChunkStorage>(
        id: u64,
        center: ChunkPos,
        distances: PlayerDistances,
        chunks: &mut ChunkHolderManager<S>,
        sink: &mut impl ChunkSink,
        now_ns: i64,
    ) -> Self {
        let mut loader = Self {
            id,
            center,
            distances,
            can_generate: false,
            last_sent_center: center,
            last_sent_radius: distances.send,
            last_sent_simulation: distances.tick,
            stages: HashMap::new(),
            sent: HashSet::new(),
            send_limiter: ChunkRateLimiter::new(now_ns),
            load_limiter: ChunkRateLimiter::new(now_ns),
            gen_limiter: ChunkRateLimiter::new(now_ns),
            send_queue: Queue::default(),
            ticking_queue: Queue::default(),
            generating_queue: Queue::default(),
            gen_queue: Queue::default(),
            loading_queue: Queue::default(),
            load_queue: Queue::default(),
            delayed: Vec::new(),
        };
        loader.rebuild(chunks, sink);
        loader.flush(chunks);
        loader
    }

    pub fn distances(&self) -> PlayerDistances {
        self.distances
    }

    pub fn center(&self) -> ChunkPos {
        self.center
    }

    pub fn sent_count(&self) -> usize {
        self.sent.len()
    }

    fn flush<S: ChunkStorage>(&mut self, chunks: &mut ChunkHolderManager<S>) {
        for op in self.delayed.drain(..) {
            match op {
                TicketOp::Add(pos, kind, level) => {
                    chunks.add_ticket(kind, pos, level, self.id);
                }
                TicketOp::AddAndRemove(pos, (add, add_level), (remove, remove_level)) => {
                    chunks.add_ticket(add, pos, add_level, self.id);
                    chunks.remove_ticket(remove, pos, remove_level, self.id);
                }
            }
        }
    }

    /// The `loadTicketCleanup` and `tickMap` remove callbacks.
    fn release_load(&mut self, pos: ChunkPos) {
        if let Some(stage) = self.stages.remove(&pos) {
            let level = stage.level();
            self.delayed.push(TicketOp::AddAndRemove(
                pos,
                (TicketType::PlayerDelayed, level),
                (TicketType::Player, level),
            ));
        }
    }

    fn release_tick(&mut self, pos: ChunkPos) {
        if let Some(stage) = self.stages.get_mut(&pos)
            && *stage == Stage::Tick
        {
            *stage = Stage::Generated;
            self.delayed.push(TicketOp::AddAndRemove(
                pos,
                (TicketType::PlayerDelayed, ENTITY_TICKING_LEVEL),
                (TicketType::Player, ENTITY_TICKING_LEVEL),
            ));
            self.delayed
                .push(TicketOp::Add(pos, TicketType::Player, FULL_LEVEL));
        }
    }

    /// `update`: see the module docs.
    pub fn update<S: ChunkStorage>(
        &mut self,
        center: ChunkPos,
        distances: PlayerDistances,
        chunks: &mut ChunkHolderManager<S>,
        sink: &mut impl ChunkSink,
    ) {
        if center == self.center && distances == self.distances {
            return;
        }
        let (from, old) = (self.center, self.distances);
        for pos in area_removed(from, old.send + 1, center, distances.send + 1) {
            if self.sent.remove(&pos) {
                sink.forget(pos);
            }
        }
        for pos in area_removed(from, old.load + 1, center, distances.load + 1) {
            self.release_load(pos);
        }
        for pos in area_removed(from, old.tick, center, distances.tick) {
            self.release_tick(pos);
        }
        if self.last_sent_radius != distances.send {
            self.last_sent_radius = distances.send;
            sink.radius(distances.send);
        }
        if self.last_sent_simulation != distances.tick {
            self.last_sent_simulation = distances.tick;
            sink.simulation_distance(distances.tick);
        }
        self.center = center;
        self.distances = distances;
        self.rebuild(chunks, sink);
        if self.last_sent_center != center {
            self.last_sent_center = center;
            sink.center(center);
        }
        self.flush(chunks);
    }

    /// The queues from the square of the load distance + 1, nearest first (Moonrise's Euclidean
    /// iteration; its order among equal distances is its BFS's, by coordinates here): a sent chunk
    /// now out of the send area is forgotten on the way.
    fn rebuild<S: ChunkStorage>(
        &mut self,
        chunks: &ChunkHolderManager<S>,
        sink: &mut impl ChunkSink,
    ) {
        for queue in [
            &mut self.send_queue,
            &mut self.ticking_queue,
            &mut self.generating_queue,
            &mut self.gen_queue,
            &mut self.loading_queue,
            &mut self.load_queue,
        ] {
            queue.clear();
        }
        let (c, d) = (self.center, self.distances);
        let r = d.load + 1;
        let mut area: Vec<(i32, i32, i32)> = (-r..=r)
            .flat_map(|dx| (-r..=r).map(move |dz| (dx * dx + dz * dz, dx, dz)))
            .collect();
        area.sort_unstable();
        for (_, dx, dz) in area {
            let (x, z) = (c.x + dx, c.z + dz);
            let pos = ChunkPos::new(x, z);
            let square = dx.abs().max(dz.abs());
            let send = square <= d.send + 1 && is_within_distance((c.x, c.z), d.send, (x, z));
            let sent = if send {
                self.sent.contains(&pos)
            } else {
                self.sent.remove(&pos)
            };
            if !send && sent {
                sink.forget(pos);
            }
            {
                match self.stages.get(&pos) {
                    None => self.load_queue.push(c, pos),
                    Some(Stage::Loading) => self.loading_queue.push(c, pos),
                    Some(Stage::Loaded) => {
                        if self.can_generate || chunks.loaded(pos) == Some(true) {
                            self.gen_queue.push(c, pos);
                        }
                    }
                    Some(Stage::Generating) => self.generating_queue.push(c, pos),
                    Some(Stage::Generated) => {
                        if send && !sent {
                            self.send_queue.push(c, pos);
                        }
                        if square <= d.tick {
                            self.ticking_queue.push(c, pos);
                        }
                    }
                    Some(Stage::Tick) => {
                        if send && !sent {
                            self.send_queue.push(c, pos);
                        }
                    }
                }
            }
        }
    }

    fn neighbours_generated(&self, pos: ChunkPos, radius: i32) -> bool {
        (-radius..=radius).all(|dz| {
            (-radius..=radius).all(|dx| {
                (dx | dz) == 0
                    || matches!(
                        self.stages.get(&ChunkPos::new(pos.x + dx, pos.z + dz)),
                        Some(Stage::Generated | Stage::Tick)
                    )
            })
        })
    }

    fn max_concurrent(&self, active: usize) -> i64 {
        let area = (2 * i64::from(self.distances.load) + 1).pow(2);
        let limit = match MAX_CONCURRENT {
            0 => (area / 5).max(5),
            n if n < 0 => i64::from(i32::MAX),
            n => n,
        };
        limit - active as i64
    }

    /// `updateQueues`: see the module docs.
    pub fn update_queues<S: ChunkStorage>(
        &mut self,
        chunks: &mut ChunkHolderManager<S>,
        sink: &mut impl ChunkSink,
        now_ns: i64,
    ) {
        self.load_limiter
            .tick(now_ns, effective_rate(MAX_CHUNK_LOAD_RATE));
        self.gen_limiter
            .tick(now_ns, effective_rate(MAX_CHUNK_GENERATE_RATE));
        self.send_limiter
            .tick(now_ns, effective_rate(MAX_CHUNK_SEND_RATE));
        let c = self.center;
        let cap = |n: i64| n.clamp(0, MAX_RATE as i64);

        // Loads that finished.
        while let Some(pos) = self.loading_queue.first() {
            let Some(full) = chunks.loaded(pos) else {
                break;
            };
            self.loading_queue.pop();
            self.stages.insert(pos, Stage::Loaded);
            if self.can_generate || full {
                self.gen_queue.push(c, pos);
            }
        }
        // New loads.
        let max_loads =
            cap((self.load_queue.len() as i64).min(self.max_concurrent(self.loading_queue.len())));
        let loads = self.load_limiter.take(max_loads);
        if loads > 0 {
            let mut started = Vec::with_capacity(loads as usize);
            for _ in 0..loads {
                let pos = self
                    .load_queue
                    .pop()
                    .expect("limited by the queue's length");
                self.stages.insert(pos, Stage::Loading);
                self.delayed
                    .push(TicketOp::Add(pos, TicketType::Player, LOADED_LEVEL));
                started.push(pos);
                self.loading_queue.push(c, pos);
            }
            self.flush(chunks);
            chunks.process_ticket_updates();
            for pos in started {
                chunks.schedule_load(pos);
            }
        }
        // Generations that finished.
        while let Some(pos) = self.generating_queue.first() {
            if chunks.full_chunk(pos).is_none() {
                break;
            }
            self.generating_queue.pop();
            self.stages.insert(pos, Stage::Generated);
            if want_chunk_sent((c.x, c.z), self.distances.send, (pos.x, pos.z)) {
                self.send_queue.push(c, pos);
            }
            if (pos.x - c.x).abs().max((pos.z - c.z).abs()) <= self.distances.tick {
                self.ticking_queue.push(c, pos);
            }
        }
        // New generations (a chunk full on disk is not counted against the rate).
        let max_gens = cap(
            (self.gen_queue.len() as i64).min(self.max_concurrent(self.generating_queue.len()))
        );
        let gens = self.gen_limiter.take(max_gens);
        let mut rated = 0;
        while let Some(pos) = self.gen_queue.first() {
            if chunks.loaded(pos) != Some(true) {
                if rated + 1 > gens {
                    break;
                }
                rated += 1;
            }
            self.gen_queue.pop();
            self.stages.insert(pos, Stage::Generating);
            self.delayed.push(TicketOp::AddAndRemove(
                pos,
                (TicketType::Player, FULL_LEVEL),
                (TicketType::Player, LOADED_LEVEL),
            ));
            self.generating_queue.push(c, pos);
        }
        self.gen_limiter.return_unused(gens - rated);
        // Ticking.
        while let Some(pos) = self.ticking_queue.first() {
            if !self.neighbours_generated(pos, i32::from(FULL_LEVEL - ENTITY_TICKING_LEVEL)) {
                break;
            }
            self.ticking_queue.pop();
            self.delayed.push(TicketOp::AddAndRemove(
                pos,
                (TicketType::Player, ENTITY_TICKING_LEVEL),
                (TicketType::Player, FULL_LEVEL),
            ));
            self.stages.insert(pos, Stage::Tick);
        }
        // Sends.
        let sends = self.send_limiter.take(cap(self.send_queue.len() as i64));
        for _ in 0..sends {
            let Some(pos) = self.send_queue.first() else {
                break;
            };
            if !self.neighbours_generated(pos, 1) {
                break;
            }
            self.send_queue.pop();
            if self.sent.insert(pos) {
                sink.send_chunk(pos);
            }
        }
        self.flush(chunks);
    }

    /// `remove`: every ticket moves to `PLAYER_TICKET_DELAYED` (module docs).
    pub fn remove<S: ChunkStorage>(&mut self, chunks: &mut ChunkHolderManager<S>) {
        self.sent.clear();
        let held: Vec<ChunkPos> = self.stages.keys().copied().collect();
        for pos in held {
            self.release_load(pos);
        }
        self.flush(chunks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The removal order is `SingleUserAreaMap`'s; the set is always the old square minus the new.
    #[test]
    fn removed_areas_are_the_old_square_minus_the_new() {
        let square = |c: ChunkPos, r: i32| -> HashSet<ChunkPos> {
            (c.x - r..=c.x + r)
                .flat_map(|x| (c.z - r..=c.z + r).map(move |z| ChunkPos::new(x, z)))
                .collect()
        };
        let from = ChunkPos::new(3, -2);
        for (dx, dz) in [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (2, -3),
            (-4, 4),
            (9, 1),
            (0, 0),
        ] {
            for (old, new) in [(3, 3), (3, 5), (5, 2)] {
                let to = ChunkPos::new(from.x + dx, from.z + dz);
                let removed = area_removed(from, old, to, new);
                let set: HashSet<ChunkPos> = removed.iter().copied().collect();
                assert_eq!(set.len(), removed.len(), "no duplicates");
                let expected: HashSet<ChunkPos> = square(from, old)
                    .difference(&square(to, new))
                    .copied()
                    .collect();
                assert_eq!(set, expected, "{dx} {dz} {old} {new}");
            }
        }
        // One chunk east at the same distance: the west column, from the north.
        let removed = area_removed(ChunkPos::new(0, 0), 1, ChunkPos::new(1, 0), 1);
        assert_eq!(
            removed,
            [(-1, -1), (-1, 0), (-1, 1)].map(|(x, z)| ChunkPos::new(x, z))
        );
    }
}
