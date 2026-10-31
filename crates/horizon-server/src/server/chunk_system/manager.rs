//! Moonrise's `ChunkHolderManager`, `NewChunkHolder` and the scheduling half of
//! `ChunkTaskScheduler`, as plain data the tick thread owns: the tickets of each chunk, their
//! levels ([`Propagator`]), a holder per chunk at or below [`MAX_LEVEL`], each holder's chunk and
//! generation state, its full status, the unload queue and the autosave queue. Chunk data goes to
//! and comes from the region workers, and generation steps go to the level's generation workers,
//! through a [`ChunkStorage`]; their answers come back through [`ChunkHolderManager::chunk_loaded`]
//! and [`ChunkHolderManager::generation_done`].
//!
//! Derived from Moonrise's sources (Paper/Leaf 26.3; Moonrise is not in the vanilla jar, so it is
//! not run here), keeping its rules:
//! - a ticket's removal that lowers the chunk's level leaves an `UNKNOWN` ticket at that level for
//!   one tick; tickets with a timeout count down in [`ChunkHolderManager::tick`];
//! - generation (`ChunkTaskScheduler.schedule`, `NewChunkHolder.onChunkGenComplete`): a holder
//!   asked for a status (`FULL` when its level reaches `FULL`, `EMPTY` for a player loader's load)
//!   is brought there one status at a time; each step first needs the chunks around it at the
//!   statuses its pyramid step requires (`byRadius`), which are scheduled in turn and wake it when
//!   they get there. A chunk whose saved status is at or past the step goes through the loading
//!   pyramid (no neighbours; the steps with nothing to do complete at once), others through the
//!   generation pyramid. A ticket level that falls clamps the requested status (`generationStatus`
//!   of the new level) or drops it; running steps finish (Moonrise cancels queued ones);
//! - steps run where Moonrise runs them: the load on the region workers, STRUCTURE_STARTS to
//!   FEATURES on the generation workers, the light steps, SPAWN and FULL on the tick thread
//!   (the light steps only advance the status: Starlight is not ported). A step is handed out
//!   only once no running step writes the chunks it writes (its write radius: FEATURES 1, the
//!   others 0), Moonrise's area-dependent queue for every writer;
//! - the full status rises to `FULL`, `BLOCK_TICKING`, `ENTITY_TICKING` as the level allows and as
//!   the 1x1, 3x3, 5x5 chunks around are full; changes are applied on the tick thread in
//!   `processTicketUpdates`, upgrades and downgrades in Moonrise's order;
//! - a holder above `MAX_LEVEL`, inaccessible, not generating, not asked for a status and used by
//!   no neighbour's step, goes to the unload queue (by 64x64-chunk section, sections in creation
//!   order); each `processUnloads` unloads at least `chunks.min-chunk-unload-count` (50) or
//!   `chunks.min-chunk-unload-fraction` (5%) of it, saving each chunk that is unsaved (proto
//!   chunks included);
//! - autosave (`autoSave`, in `ChunkMap.tick` after the unloads): the full chunks whose last save
//!   is `chunks.auto-save-interval` ticks old, oldest first, at most
//!   `chunks.max-auto-save-chunks-per-tick` (24) saved per tick.
//!
//! - scheduled block and fluid ticks (`world::ticks`): a chunk's `LevelChunkTicks` go into the
//!   level's [`ScheduledTicks`] when its FULL step completes (`registerTickContainerInLevel`),
//!   unpack when the holder reaches `BLOCK_TICKING` (`startTickingChunk`), and leave after the
//!   unload's save (`ServerLevel.unload`); the ticks pending or queued in a registered chunk are
//!   packed at each save, and make it unsaved while the game time moves (Moonrise's
//!   `LevelChunk.isUnsaved`).
//!
//! Not ported: priorities (steps run in the order they become ready), entities and POI (their
//! chunks are never saved, counted 0 in the log).

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use indexmap::{IndexMap, IndexSet};

use super::generation::{GenerationJob, GenerationResult};
use super::propagator::Propagator;
use super::ticket::{
    FULL_LEVEL, FullChunkStatus, MAX_LEVEL, NO_TIMEOUT, Ticket, TicketSet, TicketType,
    level_for_radius,
};
use crate::block::BlockId;
use crate::core::position::ChunkPos;
use crate::world::chunk::{ChunkData, ChunkStatus};
use crate::world::level_chunk::LevelChunk;
use crate::world::ticks::{LevelChunkTicks, ScheduledTicks};
use crate::worldgen::generator::convert::ProtoExtras;
use crate::worldgen::generator::pyramid::{Pyramids, next_status, status_facts};
use crate::worldgen::proto_chunk::ProtoChunk;

/// Where chunks are read, written and generated: the region workers and the level's generation
/// workers (`server::level`), a recording fake in tests. Loads are answered later through
/// [`ChunkHolderManager::chunk_loaded`], generation steps through
/// [`ChunkHolderManager::generation_done`].
pub trait ChunkStorage {
    fn load(&mut self, pos: ChunkPos);
    fn save(&mut self, pos: ChunkPos, chunk: Box<ChunkData>);
    /// Every write sent so far to the disk (`MoonriseRegionFileIO.flush`).
    fn flush(&mut self);
    /// Runs a generation step on the workers.
    fn generate(&mut self, job: GenerationJob);
    /// An empty proto chunk (`ChunkLoadTask.getEmptyChunk`).
    fn empty_chunk(&self, pos: ChunkPos) -> ProtoChunk;
    /// A proto chunk's saved form.
    fn proto_data(&self, chunk: &ProtoChunk, extras: &ProtoExtras) -> ChunkData;
    /// The FULL step's level chunk.
    fn level_chunk(&self, chunk: &ProtoChunk, extras: &ProtoExtras) -> LevelChunk;
    /// A full chunk as a generation step's neighbour sees it (an `ImposterProtoChunk`).
    fn proto_view(&self, chunk: &LevelChunk) -> ProtoChunk;
}

/// A chunk read from disk.
#[derive(Debug)]
pub enum LoadedChunk {
    Level(Box<LevelChunk>),
    Proto(Box<ProtoChunk>, Box<ProtoExtras>),
}

/// Paper's `chunks` world settings that the chunk system reads (`paper-world-defaults.yml`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChunkSettings {
    /// `auto-save-interval` in ticks (`default`: bukkit.yml's `ticks-per.autosave`, 6000).
    pub auto_save_interval: i64,
    pub max_auto_save_chunks_per_tick: usize,
    pub min_chunk_unload_count: usize,
    pub min_chunk_unload_fraction: f64,
    /// `delay-chunk-unloads-by` in ticks: `PLAYER_TICKET_DELAYED`'s timeout (at least 1).
    pub delay_chunk_unloads_by: i64,
}

impl Default for ChunkSettings {
    fn default() -> Self {
        Self {
            auto_save_interval: 6000,
            max_auto_save_chunks_per_tick: 24,
            min_chunk_unload_count: 50,
            min_chunk_unload_fraction: 0.05,
            delay_chunk_unloads_by: 200,
        }
    }
}

/// Moonrise's unload queue sections are `ThreadedTicketLevelPropagator.SECTION_SHIFT` (64 chunks).
const UNLOAD_SECTION_SHIFT: i32 = 6;
/// `NewChunkHolder.NEIGHBOUR_RADIUS`.
const NEIGHBOUR_RADIUS: i32 = 2;

/// A holder's chunk (`currentChunk`).
enum Held {
    /// A chunk being generated (or saved before `full`): shared read-only with the steps that
    /// read it, replaced by the copy a step wrote.
    Proto {
        chunk: Arc<ProtoChunk>,
        extras: Box<ProtoExtras>,
    },
    /// A full chunk (read from disk, or after the FULL step); `view` is its proto chunk view for
    /// neighbouring steps, made when first needed and dropped when the chunk changes.
    Level {
        chunk: Box<LevelChunk>,
        view: Option<Arc<ProtoChunk>>,
    },
}

impl Held {
    /// `getPersistedStatus`.
    fn status(&self) -> ChunkStatus {
        match self {
            Self::Proto { chunk, .. } => chunk.status(),
            Self::Level { chunk, .. } => chunk.status,
        }
    }
}

/// Where a holder's step is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Created; waiting to be handed out (for the chunks it writes to be free).
    Ready,
    /// The region workers are reading the chunk.
    Loading,
    /// On a generation worker.
    Running,
    /// Failed: the holder's generation stops here (Moonrise keeps the failed task).
    Failed,
}

/// `generationTask` with `generationTaskStatus`.
struct Task {
    status: ChunkStatus,
    phase: Phase,
    /// The holders the step reads (they count it in `totalNeighboursUsingThisChunk`).
    neighbours: Vec<ChunkPos>,
}

/// `NewChunkHolder`.
struct Holder {
    /// `currentTicketLevel`, from the propagator.
    level: u8,
    /// `oldTicketLevel`: the level the holder's state follows.
    old_level: u8,
    chunk: Option<Held>,
    /// `currentGenStatus`.
    gen_status: Option<ChunkStatus>,
    /// `requestedGenStatus`.
    requested: Option<ChunkStatus>,
    task: Option<Task>,
    /// `neighboursBlockingGenTask`.
    blocking: IndexSet<ChunkPos>,
    /// `neighboursWaitingForUs`: who waits, for which status.
    waiting: IndexMap<ChunkPos, ChunkStatus>,
    /// `totalNeighboursUsingThisChunk`.
    using: u32,
    /// The level cannot generate this chunk (no generator: flat and debug levels), or a chunk it
    /// needs: it stays as it is, and is never asked for anything again.
    cannot_generate: bool,
    /// `fullNeighbourChunksLoadedBitset` over the 5x5 around.
    neighbours_full: u32,
    pending: FullChunkStatus,
    current: FullChunkStatus,
    last_auto_save: i64,
    in_autosave: bool,
    in_unload_queue: bool,
    /// The chunk's tick containers are the level's (`registerTickContainerInLevel` ran).
    ticks_registered: bool,
}

/// `getFullNeighbourIndex`.
const fn neighbour_bit(dx: i32, dz: i32) -> u32 {
    1 << (dx + dz * (NEIGHBOUR_RADIUS * 2 + 1) + NEIGHBOUR_RADIUS * (NEIGHBOUR_RADIUS * 2 + 2))
}

const fn loaded_mask(radius: i32) -> u32 {
    let mut mask = 0;
    let mut dx = -radius;
    while dx <= radius {
        let mut dz = -radius;
        while dz <= radius {
            mask |= neighbour_bit(dx, dz);
            dz += 1;
        }
        dx += 1;
    }
    mask
}

impl Holder {
    fn new() -> Self {
        Self {
            level: MAX_LEVEL + 1,
            old_level: MAX_LEVEL + 1,
            chunk: None,
            gen_status: None,
            requested: None,
            task: None,
            blocking: IndexSet::new(),
            waiting: IndexMap::new(),
            using: 0,
            cannot_generate: false,
            neighbours_full: 0,
            pending: FullChunkStatus::Inaccessible,
            current: FullChunkStatus::Inaccessible,
            last_auto_save: 0,
            in_autosave: false,
            in_unload_queue: false,
            ticks_registered: false,
        }
    }

    /// `currentGenStatus == FULL`: the chunk is the level's.
    fn promoted(&self) -> bool {
        self.gen_status == Some(ChunkStatus::Full)
    }

    fn failed(&self) -> bool {
        self.task.as_ref().is_some_and(|t| t.phase == Phase::Failed)
    }

    /// `updatePendingStatus`: true when the pending status changed.
    fn update_pending(&mut self) -> bool {
        let by_level = FullChunkStatus::from_level(self.old_level);
        let bits = self.neighbours_full;
        let mut pending = [
            (2, FullChunkStatus::EntityTicking),
            (1, FullChunkStatus::BlockTicking),
            (0, FullChunkStatus::Full),
        ]
        .into_iter()
        .find(|&(radius, _)| bits & loaded_mask(radius) == loaded_mask(radius))
        .map_or(FullChunkStatus::Inaccessible, |(_, status)| status);
        if pending == FullChunkStatus::Inaccessible
            && by_level >= FullChunkStatus::Full
            && self.promoted()
        {
            pending = FullChunkStatus::Full;
        }
        let pending = pending.min(by_level);
        let changed = self.pending != pending;
        self.pending = pending;
        changed
    }

    /// `isSafeToUnload() == null` (no entities or POI to wait for).
    fn is_safe_to_unload(&self) -> bool {
        self.old_level > MAX_LEVEL
            && self.using == 0
            && self.waiting.is_empty()
            && self.current == FullChunkStatus::Inaccessible
            && self.task.is_none()
            && self.requested.is_none()
    }
}

/// `registerTickContainerInLevel`: the full chunk's saved ticks become the level's containers,
/// pending until the chunk starts ticking.
fn register_ticks(h: &mut Holder, ticks: &mut LevelScheduledTicks, pos: ChunkPos) {
    if h.ticks_registered {
        return;
    }
    let Some(Held::Level { chunk, .. }) = &mut h.chunk else {
        return;
    };
    h.ticks_registered = true;
    let raw = &mut chunk.raw;
    let block = LevelChunkTicks::from_pending(std::mem::take(&mut raw.block_ticks));
    let fluid = LevelChunkTicks::from_pending(std::mem::take(&mut raw.fluid_ticks));
    ticks.block.add_container(pos, block);
    ticks.fluid.add_container(pos, fluid);
}

/// `ChunkPos.toLong`, the autosave queue's tie-break.
fn key(pos: ChunkPos) -> i64 {
    pos.pack()
}

fn chebyshev(a: ChunkPos, b: ChunkPos) -> i32 {
    (a.x - b.x).abs().max((a.z - b.z).abs())
}

/// The block and fluid ticks of the level, with the block ids and fluid ids they are of.
pub type LevelScheduledTicks = ScheduledTicks<BlockId, u8>;

/// `SerializableChunkData.copyOf` (with the level's game time) and `tryMarkSaved`, when unsaved.
/// A chunk whose tick containers are the level's (`registered`) is unsaved while they are dirty
/// and saves their ticks (`LevelChunk.isUnsaved`, `getTicksForSerialization`).
fn save_held<S: ChunkStorage>(
    storage: &mut S,
    ticks: &mut LevelScheduledTicks,
    registered: bool,
    pos: ChunkPos,
    held: &mut Held,
    game_time: i64,
) -> bool {
    match held {
        Held::Level { chunk, .. } => {
            let dirty = registered
                && (ticks
                    .block
                    .container(pos)
                    .is_some_and(|c| c.is_dirty(game_time))
                    || ticks
                        .fluid
                        .container(pos)
                        .is_some_and(|c| c.is_dirty(game_time)));
            if !chunk.is_unsaved() && !dirty {
                return false;
            }
            chunk.try_mark_saved();
            let mut data = chunk.to_data();
            if registered {
                if let Some(c) = ticks.block.container_mut(pos) {
                    c.clear_dirty();
                    data.raw.block_ticks = c.pack(game_time);
                }
                if let Some(c) = ticks.fluid.container_mut(pos) {
                    c.clear_dirty();
                    data.raw.fluid_ticks = c.pack(game_time);
                }
            }
            data.last_update = game_time;
            storage.save(pos, Box::new(data));
        }
        Held::Proto { chunk, extras } => {
            if !std::mem::replace(&mut extras.unsaved, false) {
                return false;
            }
            let mut data = storage.proto_data(chunk, extras);
            data.last_update = game_time;
            storage.save(pos, Box::new(data));
        }
    }
    true
}

/// One level's chunk holders (see the module docs).
pub struct ChunkHolderManager<S> {
    storage: S,
    settings: ChunkSettings,
    tickets: HashMap<ChunkPos, TicketSet>,
    /// Tickets with a timeout, per chunk (`sectionToChunkToExpireCount` without the sections).
    expiring: HashMap<ChunkPos, u32>,
    propagator: Propagator,
    holders: HashMap<ChunkPos, Holder>,
    /// `pendingFullLoadUpdate`.
    pending_full: VecDeque<ChunkPos>,
    /// `ChunkUnloadQueue`: sections in creation order, each its chunks in insertion order.
    unload_queue: IndexMap<ChunkPos, IndexSet<ChunkPos>>,
    /// `autoSaveQueue`: (last autosave tick, chunk key).
    autosave: BTreeSet<(i64, i64)>,
    /// Steps created and not handed out yet, in order.
    ready: VecDeque<ChunkPos>,
    /// The chunks running steps write.
    writing: HashSet<ChunkPos>,
    current_tick: i64,
    /// The level's game time, as the chunks it starts ticking unpack their ticks at
    /// ([`Self::set_game_time`]).
    game_time: i64,
    /// The level's scheduled block and fluid ticks.
    pub ticks: LevelScheduledTicks,
    next_id: u64,
    /// Generation steps that finished, and their time on the workers (for the logs).
    pub generated: GenerationStats,
}

/// How much generation ran (for logs and measurements).
#[derive(Clone, Copy, Debug, Default)]
pub struct GenerationStats {
    /// Chunks that reached `full` through generation.
    pub chunks: u64,
    /// Worker steps that finished, and their total time.
    pub steps: u64,
    pub nanos: u64,
}

impl<S: ChunkStorage> ChunkHolderManager<S> {
    pub fn new(storage: S, settings: ChunkSettings) -> Self {
        Self {
            storage,
            settings,
            tickets: HashMap::new(),
            expiring: HashMap::new(),
            propagator: Propagator::default(),
            holders: HashMap::new(),
            pending_full: VecDeque::new(),
            unload_queue: IndexMap::new(),
            autosave: BTreeSet::new(),
            ready: VecDeque::new(),
            writing: HashSet::new(),
            current_tick: 0,
            game_time: 0,
            ticks: ScheduledTicks::default(),
            next_id: 1,
            generated: GenerationStats::default(),
        }
    }

    /// The level's game time (`Level.getGameTime`), which the chunks starting to tick unpack
    /// their ticks at; the level sets it when the time advances.
    pub fn set_game_time(&mut self, game_time: i64) {
        self.game_time = game_time;
    }

    /// The collection of `ServerLevel.tick`'s `blockTicks.tick(..)`: what is due at `game_time`,
    /// for [`LevelTicks::pop_due`](crate::world::ticks::LevelTicks::pop_due) to hand out.
    pub fn collect_block_ticks(&mut self, game_time: i64, max_ticks: i32) {
        // `ServerLevel.isPositionTickingWithEntitiesLoaded`: at least `BLOCK_TICKING`.
        let holders = &self.holders;
        self.ticks.block.collect_ticks(game_time, max_ticks, |key| {
            holders
                .get(&ChunkPos::unpack(key))
                .is_some_and(|h| h.current >= FullChunkStatus::BlockTicking)
        });
    }

    /// The collection of `fluidTicks.tick(..)`.
    pub fn collect_fluid_ticks(&mut self, game_time: i64, max_ticks: i32) {
        let holders = &self.holders;
        self.ticks.fluid.collect_ticks(game_time, max_ticks, |key| {
            holders
                .get(&ChunkPos::unpack(key))
                .is_some_and(|h| h.current >= FullChunkStatus::BlockTicking)
        });
    }

    /// The storage back, once the holders are saved (`close`).
    pub fn into_storage(self) -> S {
        self.storage
    }

    pub fn storage(&self) -> &S {
        &self.storage
    }

    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// A fresh ticket identifier (Moonrise's id counters for player loaders and load requests).
    pub fn next_ticket_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id - 1
    }

    fn timeout(&self, kind: TicketType) -> i64 {
        let timeout = kind.timeout(self.settings.delay_chunk_unloads_by.max(1));
        if timeout <= 0 { NO_TIMEOUT } else { timeout }
    }

    fn count_expiring(&mut self, pos: ChunkPos, add: bool) {
        if add {
            *self.expiring.entry(pos).or_default() += 1;
        } else if let Some(count) = self.expiring.get_mut(&pos) {
            *count -= 1;
            if *count == 0 {
                self.expiring.remove(&pos);
            }
        }
    }

    /// `addTicketAtLevel`: true when the ticket is new (an equal one is replaced, its timeout
    /// restarted). Levels apply on the next [`Self::process_ticket_updates`].
    pub fn add_ticket(&mut self, kind: TicketType, pos: ChunkPos, level: u8, id: u64) -> bool {
        if level > MAX_LEVEL {
            return false;
        }
        let remove_delay = self.timeout(kind);
        let ticket = Ticket {
            kind,
            level,
            id,
            remove_delay,
        };
        let set = self.tickets.entry(pos).or_default();
        let before = set.level();
        let replaced = set.replace(ticket);
        let after = set.level();
        match replaced {
            Some(old) if old.remove_delay != remove_delay => {
                if old.remove_delay != NO_TIMEOUT && remove_delay == NO_TIMEOUT {
                    self.count_expiring(pos, false);
                } else if old.remove_delay == NO_TIMEOUT {
                    self.count_expiring(pos, true);
                }
            }
            Some(_) => {}
            None if remove_delay != NO_TIMEOUT => self.count_expiring(pos, true),
            None => {}
        }
        if before != after {
            self.propagator.set_source(pos, Some(after));
        }
        replaced.is_none()
    }

    /// `addTicketWithRadius`: at `33 - radius`, without an identifier.
    pub fn add_ticket_with_radius(&mut self, kind: TicketType, pos: ChunkPos, radius: u8) {
        self.add_ticket(kind, pos, level_for_radius(radius), 0);
    }

    /// `removeTicketAtLevel`: when the chunk's level would rise, an `UNKNOWN` ticket keeps it one
    /// more tick.
    pub fn remove_ticket(&mut self, kind: TicketType, pos: ChunkPos, level: u8, id: u64) -> bool {
        if level > MAX_LEVEL {
            return false;
        }
        let Some(set) = self.tickets.get_mut(&pos) else {
            return false;
        };
        let old_level = set.level();
        let probe = Ticket {
            kind,
            level,
            id,
            remove_delay: 0,
        };
        let Some(removed) = set.remove(&probe) else {
            return false;
        };
        if old_level != set.level() {
            let unknown = Ticket {
                kind: TicketType::Unknown,
                level,
                id: 0,
                remove_delay: TicketType::Unknown.timeout(0),
            };
            let added = set.add(unknown);
            debug_assert!(
                added,
                "an UNKNOWN ticket at the removed level was already there"
            );
            self.count_expiring(pos, true);
        }
        if removed.remove_delay != NO_TIMEOUT {
            self.count_expiring(pos, false);
        }
        true
    }

    /// `ChunkHolderManager.tick` (`TicketStorage.purgeStaleTickets`): the tick counter, expired
    /// tickets, then the level updates.
    pub fn tick(&mut self) {
        self.current_tick += 1;
        let positions: Vec<ChunkPos> = self.expiring.keys().copied().collect();
        for pos in positions {
            let Some(set) = self.tickets.get_mut(&pos) else {
                continue;
            };
            let before = set.level();
            let removed = set.expire() as u32;
            let after = set.level();
            if set.is_empty() {
                self.tickets.remove(&pos);
            }
            if before != after {
                self.propagator.set_source(pos, Some(after));
            }
            if let Some(count) = self.expiring.get_mut(&pos) {
                *count -= removed;
                if *count == 0 {
                    self.expiring.remove(&pos);
                }
            }
        }
        self.process_ticket_updates();
    }

    /// `processTicketUpdates`: new levels to the holders (created as needed), their state, the
    /// steps that became ready, then the pending full status changes.
    pub fn process_ticket_updates(&mut self) -> bool {
        let mut ret = false;
        if self.propagator.has_pending() {
            let changes = self.propagator.process();
            for &(pos, _, new) in &changes {
                if new > MAX_LEVEL && !self.holders.contains_key(&pos) {
                    continue;
                }
                self.holders.entry(pos).or_insert_with(Holder::new).level = new;
            }
            for (pos, _, _) in changes {
                self.process_level_update(pos);
            }
            ret = true;
        }
        self.run_ready();
        ret |= self.process_pending_full();
        ret
    }

    fn process_pending_full(&mut self) -> bool {
        let mut ret = false;
        while let Some(pos) = self.pending_full.pop_front() {
            ret |= self.handle_full_status_change(pos);
        }
        ret
    }

    /// `NewChunkHolder.processTicketLevelUpdate`.
    fn process_level_update(&mut self, pos: ChunkPos) {
        let pyramids = Pyramids::get();
        let Some(h) = self.holders.get_mut(&pos) else {
            return;
        };
        let (old, new) = (h.old_level, h.level);
        if old == new {
            return;
        }
        h.old_level = new;
        let (old_state, new_state) = (
            FullChunkStatus::from_level(old),
            FullChunkStatus::from_level(new),
        );
        // Cancellations from a falling level: the requested status is clamped or dropped.
        if let Some(requested) = h.requested
            && new_state < FullChunkStatus::Full
            && new > old
        {
            match pyramids.generation_status(new) {
                None => h.requested = None,
                Some(max) => {
                    if requested >= next_status(max) {
                        if h.gen_status.is_some_and(|s| s >= max) {
                            h.requested = None;
                        } else {
                            h.requested = Some(max);
                        }
                    }
                }
            }
        }
        if old_state != new_state {
            if old_state < FullChunkStatus::Full
                && new_state >= FullChunkStatus::Full
                && !h.promoted()
            {
                if h.requested.is_some() {
                    h.requested = Some(ChunkStatus::Full);
                } else {
                    self.schedule(pos, ChunkStatus::Full);
                }
            }
            let h = self.holders.get_mut(&pos).expect("still there");
            if h.update_pending() {
                self.pending_full.push_back(pos);
            }
        }
        self.check_unload(pos);
    }

    /// `upgradeGenTarget`: true when the holder is generating or asked for something already
    /// (further scheduling is its own).
    fn upgrade_gen_target(h: &mut Holder, to: ChunkStatus) -> bool {
        if h.requested.is_none() && h.task.is_none() {
            return false;
        }
        if h.requested.is_none_or(|r| r < to) {
            h.requested = Some(to);
        }
        true
    }

    /// `ChunkTaskScheduler.schedule`: the next step towards `target`, or the neighbours it waits
    /// for (see the module docs). New steps go to the ready queue.
    fn schedule(&mut self, pos: ChunkPos, target: ChunkStatus) {
        let Some(h) = self.holders.get_mut(&pos) else {
            tracing::error!("Missing chunkholder when required: [{}, {}]", pos.x, pos.z);
            return;
        };
        if h.cannot_generate {
            return;
        }
        if h.task.is_some() {
            Self::upgrade_gen_target(h, target);
            return;
        }
        let Some(current) = h.gen_status else {
            // Not loaded yet: the load first.
            h.requested = Some(target);
            h.task = Some(Task {
                status: ChunkStatus::Empty,
                phase: Phase::Ready,
                neighbours: vec![pos],
            });
            h.using += 1;
            self.ready.push_back(pos);
            return;
        };
        if current >= target {
            return;
        }
        h.requested = Some(target);
        let persisted = h.chunk.as_ref().map_or(ChunkStatus::Empty, Held::status);
        let to = next_status(current);
        let pyramids = Pyramids::get();
        let pyramid = if persisted >= to {
            &pyramids.loading
        } else {
            &pyramids.generation
        };
        let step = pyramid.step_to(to);
        let radius = step.read_radius().max(0);
        let mut blocked = false;
        if radius > 0 {
            for (dx, dz) in search_order(radius) {
                let ring = dx.abs().max(dz.abs());
                let required = step.required_status_at(ring);
                let n = ChunkPos::new(pos.x + dx, pos.z + dz);
                blocked |= self.check_neighbour(n, required, pos);
            }
        }
        if blocked {
            let dead_end = self.holders[&pos]
                .blocking
                .iter()
                .any(|n| self.holders.get(n).is_some_and(|h| h.cannot_generate));
            if dead_end {
                self.refuse(pos);
            }
            return;
        }
        let mut neighbours = Vec::with_capacity(((2 * radius + 1) * (2 * radius + 1)) as usize);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let n = ChunkPos::new(pos.x + dx, pos.z + dz);
                if let Some(nh) = self.holders.get_mut(&n) {
                    nh.using += 1;
                    neighbours.push(n);
                }
            }
        }
        let h = self.holders.get_mut(&pos).expect("scheduled holders exist");
        h.task = Some(Task {
            status: to,
            phase: Phase::Ready,
            neighbours,
        });
        self.ready.push_back(pos);
    }

    /// `checkNeighbour`: true when the neighbour is not at `required` yet (it is then scheduled,
    /// and wakes `center` when it gets there).
    fn check_neighbour(&mut self, pos: ChunkPos, required: ChunkStatus, center: ChunkPos) -> bool {
        let Some(h) = self.holders.get_mut(&pos) else {
            tracing::error!("Missing chunkholder when required: [{}, {}]", pos.x, pos.z);
            return true;
        };
        if h.gen_status.is_some_and(|s| s >= required) {
            return false;
        }
        if h.failed() {
            return true;
        }
        if h.cannot_generate {
            self.holders
                .get_mut(&center)
                .expect("the centre exists")
                .blocking
                .insert(pos);
            return true;
        }
        h.waiting.insert(center, required);
        let upgraded = Self::upgrade_gen_target(h, required);
        self.holders
            .get_mut(&center)
            .expect("the centre exists")
            .blocking
            .insert(pos);
        if !upgraded {
            self.schedule(pos, required);
        }
        true
    }

    /// Hands out the ready steps whose written chunks are free (see the module docs); the steps
    /// that complete at once may make more ready.
    fn run_ready(&mut self) {
        let mut deferred = VecDeque::new();
        while let Some(pos) = self.ready.pop_front() {
            if !self.start(pos) {
                deferred.push_back(pos);
            }
        }
        self.ready = deferred;
    }

    /// Starts a ready step: false when a chunk it writes is being written.
    fn start(&mut self, pos: ChunkPos) -> bool {
        let Some(h) = self.holders.get_mut(&pos) else {
            return true;
        };
        let Some(task) = &h.task else {
            return true;
        };
        if task.phase != Phase::Ready {
            return true;
        }
        let to = task.status;
        if to == ChunkStatus::Empty {
            // `ChunkLoadTask`.
            h.task.as_mut().expect("checked").phase = Phase::Loading;
            self.storage.load(pos);
            return true;
        }
        let persisted = h.chunk.as_ref().map_or(ChunkStatus::Empty, Held::status);
        let generation = persisted < to;
        let facts = status_facts(to);
        if !generation && facts.empty_load {
            self.complete(pos, to);
            return true;
        }
        let write_radius = if generation {
            facts.write_radius.min(1)
        } else {
            0
        };
        let writes: Vec<ChunkPos> = (-write_radius..=write_radius)
            .flat_map(|dz| (-write_radius..=write_radius).map(move |dx| (dx, dz)))
            .map(|(dx, dz)| ChunkPos::new(pos.x + dx, pos.z + dz))
            .collect();
        if writes.iter().any(|p| self.writing.contains(p)) {
            return false;
        }
        let on_worker = generation
            && matches!(
                to,
                ChunkStatus::StructureStarts
                    | ChunkStatus::StructureReferences
                    | ChunkStatus::Biomes
                    | ChunkStatus::Terrain
                    | ChunkStatus::Features
            );
        if !on_worker {
            // The loading pyramid's steps and the tick thread's generation steps.
            self.run_inline(pos, to, generation);
            return true;
        }
        let radius = Pyramids::get().generation.step_to(to).read_radius();
        let mut chunks = Vec::with_capacity(((2 * radius + 1) * (2 * radius + 1)) as usize);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let n = ChunkPos::new(pos.x + dx, pos.z + dz);
                let writable = chebyshev(n, pos) <= write_radius;
                let Some(view) = self.view(n) else {
                    tracing::error!(
                        "Chunk [{}, {}] has no chunk for the {} step of [{}, {}]",
                        n.x,
                        n.z,
                        to.name(),
                        pos.x,
                        pos.z
                    );
                    self.fail(pos);
                    return true;
                };
                let proto = matches!(
                    self.holders.get(&n).and_then(|h| h.chunk.as_ref()),
                    Some(Held::Proto { .. })
                );
                chunks.push((view, writable && proto));
            }
        }
        for p in writes {
            self.writing.insert(p);
        }
        let h = self.holders.get_mut(&pos).expect("still there");
        h.task.as_mut().expect("checked").phase = Phase::Running;
        self.storage.generate(GenerationJob {
            pos,
            status: to,
            radius,
            chunks,
        });
        true
    }

    /// A chunk as a step reads it: a proto chunk's shared snapshot, a full chunk's view.
    fn view(&mut self, pos: ChunkPos) -> Option<Arc<ProtoChunk>> {
        let h = self.holders.get_mut(&pos)?;
        match h.chunk.as_mut()? {
            Held::Proto { chunk, .. } => Some(chunk.clone()),
            Held::Level { chunk, view } => {
                if view.is_none() {
                    *view = Some(Arc::new(self.storage.proto_view(chunk)));
                }
                view.clone()
            }
        }
    }

    /// Steps run on the tick thread: the loading pyramid's (`loadStructureStarts`, the light
    /// steps: nothing to do), the light steps and SPAWN of generation (status only: Starlight
    /// and entities are not ported), and FULL (`ChunkStatusTasks.full`: the level chunk).
    fn run_inline(&mut self, pos: ChunkPos, to: ChunkStatus, generation: bool) {
        let h = self.holders.get_mut(&pos).expect("started holders exist");
        if to == ChunkStatus::Full {
            if let Some(Held::Proto { chunk, extras }) = &h.chunk {
                let level = self.storage.level_chunk(chunk, extras);
                h.chunk = Some(Held::Level {
                    chunk: Box::new(level),
                    view: None,
                });
                self.generated.chunks += 1;
            }
        } else if generation
            && let Some(Held::Proto { chunk, extras }) = &mut h.chunk
            && chunk.status() < to
        {
            Arc::make_mut(chunk).set_status(to);
            extras.unsaved = true;
        }
        self.complete(pos, to);
    }

    /// A step that failed: logged, the holder's generation stops (Moonrise's failed task).
    fn fail(&mut self, pos: ChunkPos) {
        if let Some(h) = self.holders.get_mut(&pos)
            && let Some(task) = &mut h.task
        {
            task.phase = Phase::Failed;
        }
    }

    /// A step the level cannot run (it has no generator): the chunk stays as it is, its step is
    /// dropped, and so are the chunks waiting for it (they need it), which can then unload.
    fn refuse(&mut self, pos: ChunkPos) {
        let mut stack = vec![pos];
        while let Some(at) = stack.pop() {
            let Some(h) = self.holders.get_mut(&at) else {
                continue;
            };
            h.cannot_generate = true;
            h.requested = None;
            let task = h.task.take();
            let waiting: Vec<ChunkPos> = h.waiting.drain(..).map(|(n, _)| n).collect();
            for n in task.map(|t| t.neighbours).unwrap_or_default() {
                if let Some(nh) = self.holders.get_mut(&n) {
                    nh.using -= 1;
                    if nh.using == 0 {
                        self.check_unload(n);
                    }
                }
            }
            for n in waiting {
                if let Some(nh) = self.holders.get_mut(&n) {
                    nh.blocking.shift_remove(&at);
                    if !nh.cannot_generate {
                        stack.push(n);
                    }
                }
            }
            self.check_unload(at);
        }
    }

    /// The end of a step: the neighbours it used are released, then `onChunkGenComplete`.
    fn complete(&mut self, pos: ChunkPos, status: ChunkStatus) {
        let h = self.holders.get_mut(&pos).expect("completed holders exist");
        let task = h.task.take().expect("a step was running");
        for n in task.neighbours {
            if let Some(nh) = self.holders.get_mut(&n) {
                nh.using -= 1;
                if nh.using == 0 {
                    self.check_unload(n);
                }
            }
        }
        self.on_gen_complete(pos, status);
    }

    /// `onChunkGenComplete` for a step that produced its chunk.
    fn on_gen_complete(&mut self, pos: ChunkPos, status: ChunkStatus) {
        let h = self.holders.get_mut(&pos).expect("completed holders exist");
        h.gen_status = Some(status);
        if status == ChunkStatus::Full {
            register_ticks(h, &mut self.ticks, pos);
        }
        let requested = h.requested;
        let waiting: Vec<(ChunkPos, ChunkStatus)> = h.waiting.drain(..).collect();
        let mut keep = IndexMap::new();
        let mut needs_scheduling = Vec::new();
        for (n, required) in waiting {
            if status < required {
                if requested.is_none_or(|r| r < required) {
                    // Cancelled for this neighbour: it stops waiting.
                    if let Some(nh) = self.holders.get_mut(&n) {
                        nh.blocking.shift_remove(&pos);
                        if nh.blocking.is_empty() {
                            self.check_unload(n);
                        }
                    }
                } else {
                    keep.insert(n, required);
                }
                continue;
            }
            if let Some(nh) = self.holders.get_mut(&n) {
                nh.blocking.shift_remove(&pos);
                if nh.blocking.is_empty() {
                    if nh.requested.is_some() {
                        needs_scheduling.push(n);
                    } else {
                        self.check_unload(n);
                    }
                }
            }
        }
        let h = self.holders.get_mut(&pos).expect("still there");
        h.waiting = keep;
        if status == ChunkStatus::Full && h.update_pending() {
            self.pending_full.push_back(pos);
        }
        let more = requested.filter(|&r| status < r);
        if more.is_none() {
            h.requested = None;
            self.check_unload(pos);
        }
        for n in needs_scheduling {
            if let Some(target) = self.holders.get(&n).and_then(|h| h.requested) {
                self.schedule(n, target);
            }
        }
        if let Some(target) = more {
            self.schedule(pos, target);
        }
    }

    /// `scheduleChunkLoad(..., EMPTY, addTicket = false)`: reads the chunk of a held chunk that
    /// nothing asked for yet (a player loader's `LOADED` stage).
    pub fn schedule_load(&mut self, pos: ChunkPos) {
        if self
            .holders
            .get(&pos)
            .is_some_and(|h| h.gen_status.is_none())
        {
            self.schedule(pos, ChunkStatus::Empty);
            self.run_ready();
        }
    }

    /// A load's answer: the chunk, or `None` when it is not on disk (or could not be read: its
    /// data is lost, as Moonrise logs): an empty proto chunk then.
    pub fn chunk_loaded(&mut self, pos: ChunkPos, chunk: Option<LoadedChunk>) {
        let Some(h) = self.holders.get_mut(&pos) else {
            return;
        };
        if !h
            .task
            .as_ref()
            .is_some_and(|t| t.status == ChunkStatus::Empty && t.phase == Phase::Loading)
        {
            return;
        }
        h.chunk = Some(match chunk {
            Some(LoadedChunk::Level(chunk)) => Held::Level { chunk, view: None },
            Some(LoadedChunk::Proto(chunk, extras)) => Held::Proto {
                chunk: Arc::new(*chunk),
                extras,
            },
            None => Held::Proto {
                chunk: Arc::new(self.storage.empty_chunk(pos)),
                extras: Box::default(),
            },
        });
        self.complete(pos, ChunkStatus::Empty);
        self.run_ready();
        self.process_pending_full();
    }

    /// A generation step's answer: the chunks it wrote replace the holders' (they are unsaved),
    /// the written chunks are free again, then the step completes. A failed step is logged and
    /// stops the chunk's generation.
    pub fn generation_done(&mut self, done: GenerationResult) {
        let GenerationResult {
            pos,
            status,
            result,
            nanos,
        } = done;
        let write_radius = status_facts(status).write_radius.min(1);
        for dz in -write_radius..=write_radius {
            for dx in -write_radius..=write_radius {
                self.writing.remove(&ChunkPos::new(pos.x + dx, pos.z + dz));
            }
        }
        let running = self.holders.get(&pos).is_some_and(|h| {
            h.task
                .as_ref()
                .is_some_and(|t| t.status == status && t.phase == Phase::Running)
        });
        if !running {
            return;
        }
        match result {
            Ok(written) => {
                self.generated.steps += 1;
                self.generated.nanos += nanos;
                for chunk in written {
                    let at = chunk.pos();
                    if let Some(Held::Proto {
                        chunk: slot,
                        extras,
                    }) = self.holders.get_mut(&at).and_then(|h| h.chunk.as_mut())
                    {
                        *slot = Arc::new(chunk);
                        extras.unsaved = true;
                    }
                }
                self.complete(pos, status);
            }
            Err(Some(e)) => {
                tracing::error!(
                    "Failed to complete status for chunk: status:{}, chunk: ({},{}): {e}",
                    status.name(),
                    pos.x,
                    pos.z
                );
                self.fail(pos);
            }
            Err(None) => self.refuse(pos),
        }
        self.run_ready();
        self.process_pending_full();
    }

    /// `handleFullStatusChange`: steps towards the pending status, in Moonrise's order.
    fn handle_full_status_change(&mut self, pos: ChunkPos) -> bool {
        use FullChunkStatus::*;
        let mut ret = false;
        loop {
            let Some(h) = self.holders.get_mut(&pos) else {
                return ret;
            };
            let (pending, current) = (h.pending, h.current);
            if pending == current {
                if pending == Inaccessible {
                    self.check_unload(pos);
                }
                return ret;
            }
            ret = true;
            if pending > current {
                if current < Full && pending >= Full {
                    h.current = Full;
                    if !h.in_autosave {
                        h.in_autosave = true;
                        h.last_auto_save = self.current_tick;
                        self.autosave.insert((self.current_tick, key(pos)));
                    }
                    self.on_full_load_change(pos, true);
                }
                let h = self.holders.get_mut(&pos).expect("still there");
                if current < BlockTicking && pending >= BlockTicking {
                    h.current = BlockTicking;
                    if h.ticks_registered {
                        // `startTickingChunk`.
                        self.ticks.block.unpack_container(pos, self.game_time);
                        self.ticks.fluid.unpack_container(pos, self.game_time);
                    }
                }
                if current < EntityTicking && pending >= EntityTicking {
                    h.current = EntityTicking;
                }
            } else {
                if current >= EntityTicking && pending < EntityTicking {
                    h.current = BlockTicking;
                }
                if current >= BlockTicking && pending < BlockTicking {
                    h.current = Full;
                }
                if current >= Full && pending < Full {
                    self.on_full_load_change(pos, false);
                    self.holders.get_mut(&pos).expect("still there").current = Inaccessible;
                }
            }
        }
    }

    /// `onFullChunkLoadChange`: the 5x5 around learns that this chunk is (no longer) full.
    fn on_full_load_change(&mut self, pos: ChunkPos, loaded: bool) {
        for dz in -NEIGHBOUR_RADIUS..=NEIGHBOUR_RADIUS {
            for dx in -NEIGHBOUR_RADIUS..=NEIGHBOUR_RADIUS {
                let n = ChunkPos::new(pos.x + dx, pos.z + dz);
                let Some(h) = self.holders.get_mut(&n) else {
                    continue;
                };
                let bit = neighbour_bit(-dx, -dz);
                if loaded {
                    h.neighbours_full |= bit;
                } else {
                    h.neighbours_full &= !bit;
                }
                if h.update_pending() {
                    self.pending_full.push_back(n);
                }
            }
        }
    }

    /// `checkUnload`: in the unload queue exactly while safe to unload.
    fn check_unload(&mut self, pos: ChunkPos) {
        let Some(h) = self.holders.get_mut(&pos) else {
            return;
        };
        let section = ChunkPos::new(pos.x >> UNLOAD_SECTION_SHIFT, pos.z >> UNLOAD_SECTION_SHIFT);
        if h.is_safe_to_unload() {
            if !h.in_unload_queue {
                h.in_unload_queue = true;
                self.unload_queue.entry(section).or_default().insert(pos);
            }
        } else if h.in_unload_queue {
            h.in_unload_queue = false;
            if let Some(chunks) = self.unload_queue.get_mut(&section) {
                chunks.shift_remove(&pos);
                if chunks.is_empty() {
                    self.unload_queue.shift_remove(&section);
                }
            }
        }
    }

    /// `processUnloads`: part of the unload queue, each chunk saved if unsaved, then dropped.
    pub fn process_unloads(&mut self, game_time: i64) {
        let tentative: usize = self.unload_queue.values().map(IndexSet::len).sum();
        if tentative == 0 {
            return;
        }
        self.process_ticket_updates();
        let fraction = self.settings.min_chunk_unload_fraction.clamp(0.0, 1.0);
        let to_unload = self
            .settings
            .min_chunk_unload_count
            .max((tentative as f64 * fraction).round() as usize);
        let mut processed = 0;
        while processed < to_unload {
            let Some((_, section)) = self.unload_queue.first_mut() else {
                break;
            };
            let stage: Vec<ChunkPos> = if section.len() + processed <= to_unload {
                let (_, all) = self.unload_queue.shift_remove_index(0).expect("first");
                all.into_iter().collect()
            } else {
                (0..to_unload - processed)
                    .filter_map(|_| section.shift_remove_index(0))
                    .collect()
            };
            processed += stage.len();
            for pos in stage {
                self.unload(pos, game_time);
            }
        }
    }

    /// `unloadStage1..3` and `removeChunkHolder` for a holder out of the unload queue.
    fn unload(&mut self, pos: ChunkPos, game_time: i64) {
        let Some(h) = self.holders.get_mut(&pos) else {
            return;
        };
        h.in_unload_queue = false;
        if !h.is_safe_to_unload() {
            tracing::error!(
                "Chunkholder [{}, {}] is not safe to unload but is inside the unload queue?",
                pos.x,
                pos.z
            );
            return;
        }
        let h = self.holders.remove(&pos).expect("just found");
        if h.in_autosave {
            self.autosave.remove(&(h.last_auto_save, key(pos)));
        }
        if let Some(mut held) = h.chunk {
            save_held(
                &mut self.storage,
                &mut self.ticks,
                h.ticks_registered,
                pos,
                &mut held,
                game_time,
            );
        }
        if h.ticks_registered {
            self.ticks.block.remove_container(pos);
            self.ticks.fluid.remove_container(pos);
        }
    }

    /// `autoSave`: see the module docs.
    pub fn auto_save(&mut self, game_time: i64) {
        let max_save_time = self.current_tick - self.settings.auto_save_interval.max(1);
        let mut reschedule = Vec::new();
        let mut saved = 0;
        while saved < self.settings.max_auto_save_chunks_per_tick {
            let Some(&(last, k)) = self.autosave.first() else {
                break;
            };
            if last > max_save_time {
                break;
            }
            self.autosave.pop_first();
            let pos = ChunkPos::unpack(k);
            let h = self.holders.get_mut(&pos).expect("queued holders exist");
            h.last_auto_save = self.current_tick;
            h.in_autosave = false;
            let promoted = h.promoted();
            if let Some(held @ Held::Level { .. }) = &mut h.chunk
                && promoted
                && save_held(
                    &mut self.storage,
                    &mut self.ticks,
                    h.ticks_registered,
                    pos,
                    held,
                    game_time,
                )
            {
                saved += 1;
            }
            if h.current >= FullChunkStatus::Full {
                reschedule.push(pos);
            }
        }
        for pos in reschedule {
            let h = self.holders.get_mut(&pos).expect("still there");
            h.in_autosave = true;
            self.autosave.insert((h.last_auto_save, key(pos)));
        }
    }

    /// `saveAllChunks`: every unsaved chunk (on shutdown also those not yet the level's, proto
    /// chunks included), then the regions synced when `flush`. Returns how many were saved.
    pub fn save_all(
        &mut self,
        flush: bool,
        shutdown: bool,
        log_as: Option<&str>,
        game_time: i64,
    ) -> usize {
        if let Some(world) = log_as {
            tracing::info!("Saving all chunkholders for world '{world}'");
        }
        let start = Instant::now();
        let mut last_log = start;
        let total = self.holders.len();
        let mut saved = 0;
        for (i, (&pos, h)) in self.holders.iter_mut().enumerate() {
            let promoted = h.promoted();
            if let Some(held) = &mut h.chunk
                && (shutdown || promoted)
                && save_held(
                    &mut self.storage,
                    &mut self.ticks,
                    h.ticks_registered,
                    pos,
                    held,
                    game_time,
                )
            {
                saved += 1;
            }
            if let Some(world) = log_as
                && last_log.elapsed().as_secs() >= 10
            {
                last_log = std::time::Instant::now();
                tracing::info!(
                    "Saved {saved} block chunks, 0 entity chunks, 0 poi chunks in world '{world}', progress: {:.2}",
                    (i + 1) as f64 / total as f64 * 100.0
                );
            }
        }
        if flush {
            self.storage.flush();
        }
        if let Some(world) = log_as {
            tracing::info!(
                "Saved {saved} block chunks, 0 entity chunks, 0 poi chunks in world '{world}' in {:.2}s",
                start.elapsed().as_secs_f64()
            );
        }
        saved
    }

    /// The chunk, when it is a full chunk (whatever its holder's status).
    pub fn chunk(&self, pos: ChunkPos) -> Option<&LevelChunk> {
        match self.holders.get(&pos)?.chunk.as_ref()? {
            Held::Level { chunk, .. } => Some(chunk),
            Held::Proto { .. } => None,
        }
    }

    /// The chunk, to change it (a change must [`LevelChunk::mark_unsaved`]).
    pub fn chunk_mut(&mut self, pos: ChunkPos) -> Option<&mut LevelChunk> {
        match self.holders.get_mut(&pos)?.chunk.as_mut()? {
            Held::Level { chunk, view } => {
                *view = None;
                Some(chunk)
            }
            Held::Proto { .. } => None,
        }
    }

    /// `ChunkHolder.getChunkToSend`: the full chunk, when it and the 8 chunks around it are at
    /// least `FULL` (`isRadiusLoaded(1)`).
    pub fn chunk_to_send(&self, pos: ChunkPos) -> Option<&LevelChunk> {
        let ready = |p: ChunkPos| {
            self.holders
                .get(&p)
                .is_some_and(|h| h.current >= FullChunkStatus::Full)
        };
        let around =
            (-1..=1).all(|dz| (-1..=1).all(|dx| ready(ChunkPos::new(pos.x + dx, pos.z + dz))));
        if around { self.chunk(pos) } else { None }
    }

    /// `getFullChunkIfLoaded`: the chunk when its holder is at least `FULL`.
    pub fn full_chunk(&self, pos: ChunkPos) -> Option<&LevelChunk> {
        let h = self.holders.get(&pos)?;
        if h.current < FullChunkStatus::Full {
            return None;
        }
        self.chunk(pos)
    }

    /// `getAnyChunkIfLoaded` is not null: the load is done. `Some(true)` when the chunk is full
    /// (on disk, or generated).
    pub fn loaded(&self, pos: ChunkPos) -> Option<bool> {
        let held = self.holders.get(&pos)?.chunk.as_ref()?;
        Some(held.status() == ChunkStatus::Full)
    }

    /// Whether a chunk asked for at `FULL` is still on its way (loading or generating; a chunk
    /// whose generation failed is not).
    pub fn is_loading(&self, pos: ChunkPos) -> bool {
        self.holders.get(&pos).is_some_and(|h| {
            h.old_level <= FULL_LEVEL
                && !h.promoted()
                && !h.failed()
                && !h.cannot_generate
                && !self.blocked_by_failure(pos)
        })
    }

    /// Whether the chunk's generation waits on a neighbour whose generation failed.
    fn blocked_by_failure(&self, pos: ChunkPos) -> bool {
        self.holders.get(&pos).is_some_and(|h| {
            h.blocking
                .iter()
                .any(|n| self.holders.get(n).is_some_and(Holder::failed))
        })
    }

    /// The holder's full status (`INACCESSIBLE` without a holder).
    pub fn full_status(&self, pos: ChunkPos) -> FullChunkStatus {
        self.holders
            .get(&pos)
            .map_or(FullChunkStatus::Inaccessible, |h| h.current)
    }

    /// The chunk's ticket level (`MAX_LEVEL + 1` without a holder).
    pub fn ticket_level(&self, pos: ChunkPos) -> u8 {
        self.holders.get(&pos).map_or(MAX_LEVEL + 1, |h| h.level)
    }

    /// The holder's generation status (`currentGenStatus`).
    pub fn gen_status(&self, pos: ChunkPos) -> Option<ChunkStatus> {
        self.holders.get(&pos)?.gen_status
    }

    /// Whether a step is running or waiting to run anywhere.
    pub fn is_generating(&self) -> bool {
        !self.ready.is_empty()
            || self
                .holders
                .values()
                .any(|h| h.task.as_ref().is_some_and(|t| t.phase != Phase::Failed))
    }

    /// A holder's state, for debugging.
    pub fn describe(&self, pos: ChunkPos) -> String {
        let Some(h) = self.holders.get(&pos) else {
            return "no holder".into();
        };
        format!(
            "level {} old {} gen {:?} req {:?} task {:?} blocking {:?} waiting {:?} using {} cannot {} current {:?} queued {}",
            h.level,
            h.old_level,
            h.gen_status,
            h.requested,
            h.task.as_ref().map(|t| (t.status, t.phase)),
            h.blocking,
            h.waiting,
            h.using,
            h.cannot_generate,
            h.current,
            h.in_unload_queue
        )
    }

    /// How many holders there are (for tests and logs).
    pub fn holder_count(&self) -> usize {
        self.holders.len()
    }

    #[cfg(test)]
    pub(super) fn tickets_at(&self, pos: ChunkPos) -> Vec<Ticket> {
        self.tickets
            .get(&pos)
            .map(|s| s.tickets().to_vec())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(super) fn unload_queue_len(&self) -> usize {
        self.unload_queue.values().map(IndexSet::len).sum()
    }
}

/// `ParallelSearchRadiusIteration.getSearchIteration(radius)`'s chunks: by ring from the centre
/// outwards (the order only decides which neighbour is scheduled first).
fn search_order(radius: i32) -> impl Iterator<Item = (i32, i32)> {
    (1..=radius).flat_map(|r| {
        (-r..=r).flat_map(move |dz| {
            (-r..=r)
                .filter(move |&dx| dx.abs().max(dz.abs()) == r)
                .map(move |dx| (dx, dz))
        })
    })
}
