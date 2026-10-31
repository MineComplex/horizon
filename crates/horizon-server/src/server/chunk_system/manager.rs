//! Moonrise's `ChunkHolderManager` and `NewChunkHolder`, as plain data the tick thread owns: the
//! tickets of each chunk, their levels ([`Propagator`]), a holder per chunk at or below
//! [`MAX_LEVEL`], each holder's full status, the unload queue and the autosave queue. Chunk data
//! goes to and comes from the region workers through a [`ChunkStorage`] (the I/O workers'
//! requests are messages; their answers come back through [`ChunkHolderManager::chunk_loaded`]).
//!
//! Derived from Moonrise's sources (Paper/Leaf 26.3; Moonrise is not in the vanilla jar, so it is
//! not run here), keeping its rules:
//! - a ticket's removal that lowers the chunk's level leaves an `UNKNOWN` ticket at that level for
//!   one tick; tickets with a timeout count down in [`ChunkHolderManager::tick`];
//! - a holder entering `FULL` (level 33 or less) asks for its chunk: loaded from disk, or (for a
//!   chunk not on disk, or not finished) left to the generator — which is not ported: such a chunk
//!   waits in [`ChunkHolderManager::awaiting_generation`] (WP6's hook). The `ChunkFullTask` that
//!   turns a saved full chunk into the level's chunk runs at once here (Moonrise queues it on the
//!   tick thread's executor);
//! - the full status rises to `FULL`, `BLOCK_TICKING`, `ENTITY_TICKING` as the level allows and as
//!   the 1x1, 3x3, 5x5 chunks around are full; changes are applied on the tick thread in
//!   `processTicketUpdates`, upgrades and downgrades in Moonrise's order;
//! - a holder above `MAX_LEVEL`, inaccessible, not loading and not waiting for generation, goes to
//!   the unload queue (by 64x64-chunk section, sections in creation order); each
//!   `processUnloads` unloads at least `chunks.min-chunk-unload-count` (50) or
//!   `chunks.min-chunk-unload-fraction` (5%) of it, saving each chunk that is unsaved;
//! - autosave (`autoSave`, in `ChunkMap.tick` after the unloads): the full chunks whose last save
//!   is `chunks.auto-save-interval` ticks old, oldest first, at most
//!   `chunks.max-auto-save-chunks-per-tick` (24) saved per tick.
//!
//! Entities and POI are not ported: their chunks are never saved (counted 0 in the log), and no
//! neighbour holds a chunk for generation. Pending block and fluid ticks make a chunk unsaved in
//! Moonrise while the game time moves; they are raw NBT here (saved as read) until ticks are ported.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::time::Instant;

use indexmap::{IndexMap, IndexSet};

use super::propagator::Propagator;
use super::ticket::{
    FULL_LEVEL, FullChunkStatus, MAX_LEVEL, NO_TIMEOUT, Ticket, TicketSet, TicketType,
    level_for_radius,
};
use crate::core::position::ChunkPos;
use crate::world::chunk::{ChunkData, ChunkStatus};
use crate::world::level_chunk::LevelChunk;

/// Where chunks are read and written: the region workers ([`crate::world::chunk_io::ChunkIo`]),
/// a recording fake in tests. Loads are answered later through
/// [`ChunkHolderManager::chunk_loaded`].
pub trait ChunkStorage {
    fn load(&mut self, pos: ChunkPos);
    fn save(&mut self, pos: ChunkPos, chunk: Box<ChunkData>);
    /// Every write sent so far to the disk (`MoonriseRegionFileIO.flush`).
    fn flush(&mut self);
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

enum Slot {
    /// Nothing asked for it.
    Unloaded,
    Loading,
    Loaded(Box<LevelChunk>),
    /// Not on disk (or unreadable): where Moonrise would have an empty proto chunk to generate.
    Missing,
}

/// `NewChunkHolder`.
struct Holder {
    /// `currentTicketLevel`, from the propagator.
    level: u8,
    /// `oldTicketLevel`: the level the holder's state follows.
    old_level: u8,
    slot: Slot,
    /// `currentGenStatus == FULL`: the chunk is the level's.
    promoted: bool,
    /// `requestedGenStatus == FULL`: waiting for the load, or for the generator.
    full_requested: bool,
    /// `fullNeighbourChunksLoadedBitset` over the 5x5 around.
    neighbours_full: u32,
    pending: FullChunkStatus,
    current: FullChunkStatus,
    last_auto_save: i64,
    in_autosave: bool,
    in_unload_queue: bool,
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
            slot: Slot::Unloaded,
            promoted: false,
            full_requested: false,
            neighbours_full: 0,
            pending: FullChunkStatus::Inaccessible,
            current: FullChunkStatus::Inaccessible,
            last_auto_save: 0,
            in_autosave: false,
            in_unload_queue: false,
        }
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
            && self.promoted
        {
            pending = FullChunkStatus::Full;
        }
        let pending = pending.min(by_level);
        let changed = self.pending != pending;
        self.pending = pending;
        changed
    }

    /// `isSafeToUnload() == null` (no entities, POI or generation neighbours to wait for).
    fn is_safe_to_unload(&self) -> bool {
        self.old_level > MAX_LEVEL
            && self.current == FullChunkStatus::Inaccessible
            && !self.full_requested
            && !matches!(self.slot, Slot::Loading)
    }
}

/// `ChunkPos.toLong`, the autosave queue's tie-break.
fn key(pos: ChunkPos) -> i64 {
    pos.pack()
}

/// `SerializableChunkData.copyOf` (with the level's game time) and `tryMarkSaved`, when unsaved.
fn save_chunk(
    storage: &mut impl ChunkStorage,
    pos: ChunkPos,
    chunk: &mut LevelChunk,
    game_time: i64,
) -> bool {
    if !chunk.try_mark_saved() {
        return false;
    }
    let mut data = chunk.to_data();
    data.last_update = game_time;
    storage.save(pos, Box::new(data));
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
    current_tick: i64,
    next_id: u64,
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
            current_tick: 0,
            next_id: 1,
        }
    }

    /// The storage back, once the holders are saved (`close`).
    pub fn into_storage(self) -> S {
        self.storage
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

    /// `processTicketUpdates`: new levels to the holders (created as needed), their state, then
    /// the pending full status changes.
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
        while let Some(pos) = self.pending_full.pop_front() {
            ret |= self.handle_full_status_change(pos);
        }
        ret
    }

    /// `NewChunkHolder.processTicketLevelUpdate`.
    fn process_level_update(&mut self, pos: ChunkPos) {
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
        if new_state < FullChunkStatus::Full && new > old {
            h.full_requested = false; // the request is cancelled
        }
        if old_state != new_state {
            if old_state < FullChunkStatus::Full
                && new_state >= FullChunkStatus::Full
                && !h.promoted
            {
                self.schedule_full(pos);
            }
            let h = self.holders.get_mut(&pos).expect("still there");
            if h.update_pending() {
                self.pending_full.push_back(pos);
            }
        }
        if (old > MAX_LEVEL) != (new > MAX_LEVEL) {
            self.check_unload(pos);
        }
    }

    /// `ChunkTaskScheduler.schedule(..., FULL)` for a holder now at `FULL`.
    fn schedule_full(&mut self, pos: ChunkPos) {
        let h = self.holders.get_mut(&pos).expect("scheduled holders exist");
        match &h.slot {
            Slot::Unloaded => {
                h.slot = Slot::Loading;
                h.full_requested = true;
                self.storage.load(pos);
            }
            Slot::Loaded(chunk) if chunk.status == ChunkStatus::Full => self.promote(pos),
            // Loading, or waiting for the generator (a chunk not on disk, or not finished).
            _ => h.full_requested = true,
        }
    }

    /// `ChunkFullTask`: the saved full chunk becomes the level's.
    fn promote(&mut self, pos: ChunkPos) {
        let h = self.holders.get_mut(&pos).expect("promoted holders exist");
        h.promoted = true;
        h.full_requested = false;
        if h.update_pending() {
            self.pending_full.push_back(pos);
        }
    }

    /// `scheduleChunkLoad(..., EMPTY, addTicket = false)`: reads the chunk of a held chunk that
    /// nothing asked for yet (a player loader's `LOADED` stage).
    pub fn schedule_load(&mut self, pos: ChunkPos) {
        if let Some(h) = self.holders.get_mut(&pos)
            && matches!(h.slot, Slot::Unloaded)
        {
            h.slot = Slot::Loading;
            self.storage.load(pos);
        }
    }

    /// A load's answer: the chunk, or `None` when it is not on disk (or failed).
    pub fn chunk_loaded(&mut self, pos: ChunkPos, chunk: Option<LevelChunk>) {
        let Some(h) = self.holders.get_mut(&pos) else {
            return;
        };
        if !matches!(h.slot, Slot::Loading) {
            return;
        }
        let full = chunk
            .as_ref()
            .is_some_and(|c| c.status == ChunkStatus::Full);
        h.slot = chunk.map_or(Slot::Missing, |c| Slot::Loaded(Box::new(c)));
        if h.full_requested && full && h.old_level <= FULL_LEVEL {
            self.promote(pos);
        }
        self.check_unload(pos);
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
        if let Slot::Loaded(mut chunk) = h.slot {
            save_chunk(&mut self.storage, pos, &mut chunk, game_time);
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
            if let Slot::Loaded(chunk) = &mut h.slot
                && h.promoted
                && save_chunk(&mut self.storage, pos, chunk, game_time)
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

    /// `saveAllChunks`: every unsaved chunk (on shutdown also those not yet the level's), then
    /// the regions synced when `flush`. Returns how many were saved.
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
            if let Slot::Loaded(chunk) = &mut h.slot
                && (shutdown || h.promoted)
                && save_chunk(&mut self.storage, pos, chunk, game_time)
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

    /// The chunk, when loaded (whatever its status).
    pub fn chunk(&self, pos: ChunkPos) -> Option<&LevelChunk> {
        match &self.holders.get(&pos)?.slot {
            Slot::Loaded(chunk) => Some(chunk),
            _ => None,
        }
    }

    /// The chunk, to change it (a change must [`LevelChunk::mark_unsaved`]).
    pub fn chunk_mut(&mut self, pos: ChunkPos) -> Option<&mut LevelChunk> {
        match &mut self.holders.get_mut(&pos)?.slot {
            Slot::Loaded(chunk) => Some(chunk),
            _ => None,
        }
    }

    /// `getFullChunkIfLoaded`: the chunk when its holder is at least `FULL`.
    pub fn full_chunk(&self, pos: ChunkPos) -> Option<&LevelChunk> {
        let h = self.holders.get(&pos)?;
        if h.current < FullChunkStatus::Full {
            return None;
        }
        match &h.slot {
            Slot::Loaded(chunk) => Some(chunk),
            _ => None,
        }
    }

    /// `getAnyChunkIfLoaded` is not null: the load is done (a missing chunk counts, as the empty
    /// proto chunk Moonrise would have). `Some(true)` when the chunk on disk is full.
    pub fn loaded(&self, pos: ChunkPos) -> Option<bool> {
        match &self.holders.get(&pos)?.slot {
            Slot::Loaded(chunk) => Some(chunk.status == ChunkStatus::Full),
            Slot::Missing => Some(false),
            Slot::Unloaded | Slot::Loading => None,
        }
    }

    /// Whether a load asked for at `FULL` is still running (or not yet started).
    pub fn is_loading(&self, pos: ChunkPos) -> bool {
        self.holders.get(&pos).is_some_and(|h| {
            matches!(h.slot, Slot::Loading)
                || (matches!(h.slot, Slot::Unloaded) && h.old_level <= FULL_LEVEL)
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

    /// The hook for world generation (WP6): chunks at `FULL` that are not on disk, or not
    /// finished. Without a generator they stay as they are and are never sent.
    pub fn awaiting_generation(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.holders
            .iter()
            .filter(|(_, h)| h.full_requested && !matches!(h.slot, Slot::Loading))
            .map(|(&pos, _)| pos)
    }

    /// How many holders there are (for tests and logs).
    pub fn holder_count(&self) -> usize {
        self.holders.len()
    }

    #[cfg(test)]
    pub(super) fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
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
