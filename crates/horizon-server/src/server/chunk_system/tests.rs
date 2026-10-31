use std::collections::{HashMap, HashSet};

use super::generation::{GenerationJob, GenerationResult};
use super::manager::{ChunkHolderManager, ChunkSettings, ChunkStorage, LoadedChunk};
use super::player_loader::{ChunkSink, PlayerChunkLoader};
use super::propagator::Propagator;
use super::ticket::*;
use crate::block::BlockState;
use crate::core::position::ChunkPos;
use crate::network::play::chunk_sending::PlayerDistances;
use crate::world::chunk::{ChunkData, ChunkStatus, ContainerFactory};
use crate::world::level_chunk::{HeightmapTable, LevelChunk};
use crate::worldgen::generator::convert::{
    ChunkShape, ProtoExtras, proto_from_data, proto_into_level_chunk, proto_to_data,
};
use crate::worldgen::generator::pyramid::{Pyramids, status_facts};
use crate::worldgen::proto_chunk::ProtoChunk;
use crate::worldgen::structure::StructureRegistry;

/// Records the requests; loads are answered by [`answer`], generation steps by [`generate`].
#[derive(Default)]
struct Fake {
    loads: Vec<ChunkPos>,
    saves: Vec<(ChunkPos, i64)>,
    flushes: usize,
    jobs: Vec<GenerationJob>,
    /// The chunks the jobs handed out and not answered yet write.
    writing: HashSet<ChunkPos>,
    structures: StructureRegistry,
}

thread_local! {
    static FACTORY: ContainerFactory = ContainerFactory::new(4, 1, 0);
}

fn shape_with<T>(f: impl FnOnce(ChunkShape<'_>) -> T) -> T {
    FACTORY.with(|factory| {
        f(ChunkShape {
            min_section_y: 0,
            section_count: 1,
            factory,
            void_air: BlockState(3),
        })
    })
}

fn heightmaps() -> HeightmapTable {
    HeightmapTable::new(4, |s| if s.0 == 0 { 0 } else { 0x3f })
}

impl ChunkStorage for Fake {
    fn load(&mut self, pos: ChunkPos) {
        self.loads.push(pos);
    }
    fn save(&mut self, pos: ChunkPos, chunk: Box<ChunkData>) {
        assert_eq!(chunk.pos, pos);
        self.saves.push((pos, chunk.last_update));
    }
    fn flush(&mut self) {
        self.flushes += 1;
    }
    fn generate(&mut self, job: GenerationJob) {
        // No two running steps write the same chunk.
        let radius = status_facts(job.status).write_radius.min(1);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let at = ChunkPos::new(job.pos.x + dx, job.pos.z + dz);
                assert!(self.writing.insert(at), "{at:?} written twice");
            }
        }
        let side = (2 * job.radius + 1) as usize;
        assert_eq!(job.chunks.len(), side * side);
        self.jobs.push(job);
    }
    fn empty_chunk(&self, pos: ChunkPos) -> ProtoChunk {
        shape_with(|shape| shape.empty(pos))
    }
    fn proto_data(&self, chunk: &ProtoChunk, extras: &ProtoExtras) -> ChunkData {
        proto_to_data(chunk, extras, &self.structures)
    }
    fn level_chunk(&self, chunk: &ProtoChunk, extras: &ProtoExtras) -> LevelChunk {
        shape_with(|shape| {
            proto_into_level_chunk(chunk, extras, shape, &self.structures, &heightmaps())
        })
    }
    fn proto_view(&self, chunk: &LevelChunk) -> ProtoChunk {
        shape_with(|shape| proto_from_data(chunk.to_data(), shape, &self.structures))
            .unwrap()
            .0
    }
}

/// Answers every pending generation step as a worker would (each chunk the step's region
/// requires is at its required status), the centre advanced to the step's status. Returns how
/// many steps ran.
fn generate(m: &mut ChunkHolderManager<Fake>) -> usize {
    let jobs = std::mem::take(&mut m.storage_mut().jobs);
    let count = jobs.len();
    for job in jobs {
        let step = Pyramids::get().generation.step_to(job.status);
        let side = 2 * job.radius + 1;
        let mut written = Vec::new();
        for (i, (chunk, writable)) in job.chunks.into_iter().enumerate() {
            let (dx, dz) = (i as i32 % side - job.radius, i as i32 / side - job.radius);
            let ring = dx.abs().max(dz.abs());
            assert!(
                chunk.status() >= step.required_status_at(ring),
                "{:?} for {:?} at {dx} {dz}",
                chunk.status(),
                job.status
            );
            if writable {
                let mut chunk = std::sync::Arc::unwrap_or_clone(chunk);
                if (dx, dz) == (0, 0) {
                    chunk.set_status(job.status);
                }
                written.push(chunk);
            }
        }
        let radius = status_facts(job.status).write_radius.min(1);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                m.storage_mut()
                    .writing
                    .remove(&ChunkPos::new(job.pos.x + dx, job.pos.z + dz));
            }
        }
        m.generation_done(GenerationResult {
            pos: job.pos,
            status: job.status,
            result: Ok(written),
            nanos: 1,
        });
    }
    count
}

fn manager(settings: ChunkSettings) -> ChunkHolderManager<Fake> {
    ChunkHolderManager::new(Fake::default(), settings)
}

fn level_chunk(pos: ChunkPos, status: ChunkStatus) -> LevelChunk {
    let mut chunk = LevelChunk::empty(pos, 0, 1, &ContainerFactory::new(4, 1, 0));
    chunk.status = status;
    chunk
}

/// Answers every pending load: a full chunk, or `None` for the chunks in `missing`.
fn answer(m: &mut ChunkHolderManager<Fake>, missing: &[ChunkPos]) {
    for pos in std::mem::take(&mut m.storage_mut().loads) {
        let chunk = (!missing.contains(&pos))
            .then(|| LoadedChunk::Level(level_chunk(pos, ChunkStatus::Full)));
        m.chunk_loaded(pos, chunk);
    }
}

fn p(x: i32, z: i32) -> ChunkPos {
    ChunkPos::new(x, z)
}

/// xorshift64*, for the propagation test.
struct Rng(u64);
impl Rng {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) % bound
    }
}

#[test]
fn propagated_levels_are_the_nearest_source_plus_its_chebyshev_distance() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut propagator = Propagator::default();
    let mut sources: HashMap<ChunkPos, u8> = HashMap::new();
    let mut levels: HashMap<ChunkPos, u8> = HashMap::new();
    let mut checked = 0;
    for round in 0..300 {
        // A few changes per batch: new sources, moved levels, removals.
        for _ in 0..1 + rng.next(4) {
            let pos = p(rng.next(40) as i32 - 20, rng.next(40) as i32 - 20);
            if rng.next(3) == 0 {
                sources.remove(&pos);
                propagator.set_source(pos, None);
            } else {
                let level = 20 + rng.next(26) as u8; // 45 is above MAX_LEVEL: no source
                propagator.set_source(pos, Some(level));
                if level <= MAX_LEVEL {
                    sources.insert(pos, level);
                } else {
                    sources.remove(&pos);
                }
            }
        }
        for (pos, old, new) in propagator.process() {
            assert_eq!(levels.get(&pos).copied().unwrap_or(MAX_LEVEL + 1), old);
            assert_ne!(old, new);
            if new > MAX_LEVEL {
                levels.remove(&pos);
            } else {
                levels.insert(pos, new);
            }
        }
        if round % 10 == 0 {
            for x in -46..46 {
                for z in -46..46 {
                    let expected = sources
                        .iter()
                        .map(|(s, &l)| {
                            let d = (s.x - x).abs().max((s.z - z).abs());
                            (i32::from(l) + d).min(i32::from(MAX_LEVEL) + 1) as u8
                        })
                        .min()
                        .unwrap_or(MAX_LEVEL + 1);
                    assert_eq!(propagator.level(p(x, z)), expected, "({x}, {z})");
                    assert_eq!(
                        levels.get(&p(x, z)).copied().unwrap_or(MAX_LEVEL + 1),
                        expected
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 30 * 92 * 92);
}

#[test]
fn levels_and_full_statuses() {
    for (level, status) in [
        (0, FullChunkStatus::EntityTicking),
        (31, FullChunkStatus::EntityTicking),
        (32, FullChunkStatus::BlockTicking),
        (33, FullChunkStatus::Full),
        (34, FullChunkStatus::Inaccessible),
        (45, FullChunkStatus::Inaccessible),
    ] {
        assert_eq!(FullChunkStatus::from_level(level), status);
    }
    assert_eq!(level_for_radius(3), 30);
}

#[test]
fn a_removed_ticket_keeps_its_level_one_more_tick() {
    let mut m = manager(ChunkSettings::default());
    m.add_ticket(TicketType::Player, p(0, 0), FULL_LEVEL, 7);
    m.process_ticket_updates();
    assert_eq!(m.ticket_level(p(0, 0)), 33);
    assert_eq!(m.ticket_level(p(11, 0)), 44);
    assert_eq!(m.ticket_level(p(12, 0)), MAX_LEVEL + 1);
    assert_eq!(m.holder_count(), 23 * 23);
    // Removing it: an UNKNOWN ticket (1 tick) at the same level.
    assert!(m.remove_ticket(TicketType::Player, p(0, 0), FULL_LEVEL, 7));
    assert!(!m.remove_ticket(TicketType::Player, p(0, 0), FULL_LEVEL, 7));
    let tickets = m.tickets_at(p(0, 0));
    assert_eq!(tickets.len(), 1);
    assert_eq!(
        (tickets[0].kind, tickets[0].level),
        (TicketType::Unknown, 33)
    );
    m.process_ticket_updates();
    assert_eq!(m.ticket_level(p(0, 0)), 33);
    m.tick();
    assert_eq!(m.ticket_level(p(0, 0)), MAX_LEVEL + 1);
    assert!(m.tickets_at(p(0, 0)).is_empty());
}

#[test]
fn timeouts_count_down_and_a_new_ticket_restarts_them() {
    let settings = ChunkSettings {
        delay_chunk_unloads_by: 3,
        ..ChunkSettings::default()
    };
    let mut m = manager(settings);
    m.add_ticket_with_radius(TicketType::PlayerSpawn, p(0, 0), 3);
    m.add_ticket(TicketType::PlayerDelayed, p(5, 5), ENTITY_TICKING_LEVEL, 1);
    m.process_ticket_updates();
    assert_eq!(m.ticket_level(p(0, 0)), 30);
    for _ in 0..2 {
        m.tick();
    }
    assert_eq!(m.ticket_level(p(5, 5)), 31);
    m.tick(); // the third tick: `delay-chunk-unloads-by` 3
    assert_eq!(m.ticket_level(p(5, 5)), 30 + 5);
    for _ in 0..10 {
        m.tick();
    }
    // Replaced 13 ticks in: 20 more from here.
    assert!(!m.add_ticket(TicketType::PlayerSpawn, p(0, 0), 30, 0));
    for _ in 0..19 {
        m.tick();
    }
    assert_eq!(m.ticket_level(p(0, 0)), 30);
    m.tick();
    assert_eq!(m.ticket_level(p(0, 0)), MAX_LEVEL + 1);
}

#[test]
fn full_statuses_follow_levels_and_full_neighbours() {
    let mut m = manager(ChunkSettings::default());
    m.add_ticket(TicketType::Player, p(0, 0), ENTITY_TICKING_LEVEL, 1);
    m.process_ticket_updates();
    // Levels 31 at the center, 33 two chunks away: those 5x5 are loaded.
    let mut loads = m.storage_mut().loads.clone();
    loads.sort_by_key(|c| (c.x, c.z));
    assert_eq!(loads.len(), 25);
    assert!(m.is_loading(p(2, 2)) && !m.is_loading(p(3, 0)));
    assert_eq!(m.full_status(p(0, 0)), FullChunkStatus::Inaccessible);
    answer(&mut m, &[]);
    m.process_ticket_updates();
    assert_eq!(m.full_status(p(0, 0)), FullChunkStatus::EntityTicking);
    assert_eq!(m.full_status(p(1, 1)), FullChunkStatus::BlockTicking);
    assert_eq!(m.full_status(p(-2, 2)), FullChunkStatus::Full);
    assert_eq!(m.full_status(p(3, 0)), FullChunkStatus::Inaccessible);
    assert!(m.full_chunk(p(2, 0)).is_some() && m.full_chunk(p(3, 0)).is_none());

    // A missing neighbour: the center can't be entity ticking, the chunks next to it can't
    // be block ticking; the missing chunk is being generated (its first step is out).
    let mut m = manager(ChunkSettings::default());
    m.add_ticket(TicketType::Player, p(0, 0), ENTITY_TICKING_LEVEL, 1);
    m.process_ticket_updates();
    answer(&mut m, &[p(2, 2)]);
    m.process_ticket_updates();
    assert_eq!(m.full_status(p(0, 0)), FullChunkStatus::BlockTicking);
    assert_eq!(m.full_status(p(1, 1)), FullChunkStatus::Full);
    assert_eq!(m.full_status(p(1, 0)), FullChunkStatus::BlockTicking);
    let jobs: Vec<_> = m
        .storage_mut()
        .jobs
        .iter()
        .map(|j| (j.pos, j.status))
        .collect();
    assert_eq!(jobs, [(p(2, 2), ChunkStatus::StructureStarts)]);
    assert!(m.is_loading(p(2, 2)));

    // Downgrades: the ticket goes, the UNKNOWN one after a tick, then everything is inaccessible.
    // The running step finishes; the chunk is no longer asked for and can unload.
    m.remove_ticket(TicketType::Player, p(0, 0), ENTITY_TICKING_LEVEL, 1);
    m.tick();
    assert_eq!(m.full_status(p(0, 0)), FullChunkStatus::Inaccessible);
    assert_eq!(m.unload_queue_len(), m.holder_count() - 1);
    assert_eq!(generate(&mut m), 1);
    assert_eq!(m.gen_status(p(2, 2)), Some(ChunkStatus::StructureStarts));
    assert_eq!(m.storage_mut().jobs.len(), 0);
    assert_eq!(m.unload_queue_len(), m.holder_count());
}

#[test]
fn unloading_saves_unsaved_chunks_only() {
    let settings = ChunkSettings {
        min_chunk_unload_count: 3,
        min_chunk_unload_fraction: 0.0,
        ..ChunkSettings::default()
    };
    let mut m = manager(settings);
    for x in 0..4 {
        m.add_ticket(TicketType::Player, p(x * 100, 0), FULL_LEVEL, 1);
    }
    m.process_ticket_updates();
    answer(&mut m, &[]);
    m.process_ticket_updates();
    m.chunk_mut(p(100, 0)).unwrap().mark_unsaved();
    m.chunk_mut(p(300, 0)).unwrap().mark_unsaved();
    for x in 0..4 {
        m.remove_ticket(TicketType::Player, p(x * 100, 0), FULL_LEVEL, 1);
    }
    m.tick();
    // 4 x 23 x 23 holders queued in 4 sections; 3 unloaded per pass, sections in order.
    assert_eq!(m.unload_queue_len(), 4 * 23 * 23);
    m.process_unloads(77);
    assert_eq!(m.holder_count(), 4 * 23 * 23 - 3);
    let mut passes = 1;
    while m.holder_count() > 0 {
        m.process_unloads(77 + passes);
        passes += 1;
    }
    assert_eq!(passes, (4 * 23 * 23 + 2) / 3);
    // The two changed chunks, with the game time of their unload as `LastUpdate`.
    let saves = &m.storage_mut().saves;
    assert_eq!(saves.len(), 2, "{saves:?}");
    assert!(saves.iter().any(|&(pos, _)| pos == p(100, 0)));
    assert!(saves.iter().any(|&(pos, _)| pos == p(300, 0)));
    assert!(saves.iter().all(|&(_, time)| time > 77));
}

#[test]
fn autosave_saves_old_unsaved_full_chunks_a_few_per_tick() {
    let settings = ChunkSettings {
        auto_save_interval: 10,
        max_auto_save_chunks_per_tick: 2,
        ..ChunkSettings::default()
    };
    let mut m = manager(settings);
    m.add_ticket(TicketType::Player, p(0, 0), 32, 1);
    m.process_ticket_updates();
    answer(&mut m, &[]);
    m.process_ticket_updates();
    // 3x3 full chunks, 5 of them changed.
    let changed = [p(-1, -1), p(0, 0), p(1, 0), p(0, 1), p(1, 1)];
    for pos in changed {
        m.chunk_mut(pos).unwrap().mark_unsaved();
    }
    for _ in 0..9 {
        m.tick();
        m.auto_save(5);
    }
    assert!(m.storage_mut().saves.is_empty(), "not 10 ticks old yet");
    m.tick();
    m.auto_save(5);
    assert_eq!(m.storage_mut().saves.len(), 2);
    m.tick();
    m.auto_save(6);
    m.tick();
    m.auto_save(7);
    let saves: Vec<ChunkPos> = m.storage_mut().saves.iter().map(|s| s.0).collect();
    assert_eq!(saves.len(), 5);
    let mut sorted = saves.clone();
    sorted.sort_by_key(|c| c.pack());
    let mut expected = changed.to_vec();
    expected.sort_by_key(|c| c.pack());
    assert_eq!(sorted, expected);
    // Saved chunks are saved again only when changed again.
    for _ in 0..30 {
        m.tick();
        m.auto_save(8);
    }
    assert_eq!(m.storage_mut().saves.len(), 5);
    m.chunk_mut(p(0, 0)).unwrap().mark_unsaved();
    for _ in 0..10 {
        m.tick();
        m.auto_save(9);
    }
    assert_eq!(m.storage_mut().saves.len(), 6);
    assert_eq!(m.save_all(true, true, None, 10), 0);
    assert_eq!(m.storage_mut().flushes, 1);
}

#[derive(Default)]
struct Sink {
    forgotten: Vec<ChunkPos>,
    sent: Vec<ChunkPos>,
    centers: Vec<ChunkPos>,
}

impl ChunkSink for Sink {
    fn forget(&mut self, pos: ChunkPos) {
        self.forgotten.push(pos);
    }
    fn radius(&mut self, _: i32) {}
    fn simulation_distance(&mut self, _: i32) {}
    fn center(&mut self, pos: ChunkPos) {
        self.centers.push(pos);
    }
    fn send_chunk(&mut self, pos: ChunkPos) {
        self.sent.push(pos);
    }
}

/// The level's tick for one player: tickets, the loader, unloads; loads answered between ticks.
fn level_tick(
    m: &mut ChunkHolderManager<Fake>,
    loader: &mut PlayerChunkLoader,
    sink: &mut Sink,
    tick: i64,
    missing: &[ChunkPos],
) {
    answer(m, missing);
    // This level cannot generate (as a flat level): the missing chunk's step is refused.
    for job in std::mem::take(&mut m.storage_mut().jobs) {
        m.storage_mut().writing.remove(&job.pos);
        m.generation_done(GenerationResult {
            pos: job.pos,
            status: job.status,
            result: Err(None),
            nanos: 0,
        });
    }
    m.tick();
    m.process_ticket_updates();
    loader.update_queues(m, sink, tick * 50_000_000);
    m.process_unloads(tick);
    m.auto_save(tick);
}

#[test]
fn a_player_loader_loads_sends_and_leaves_chunks_behind() {
    let settings = ChunkSettings {
        delay_chunk_unloads_by: 20,
        ..ChunkSettings::default()
    };
    let mut m = manager(settings);
    let distances = PlayerDistances {
        tick: 2,
        load: 4,
        send: 3,
    };
    let mut sink = Sink::default();
    let missing = [p(-4, 3)];
    let mut loader = PlayerChunkLoader::new(1, p(0, 0), distances, &mut m, &mut sink, 0);
    for tick in 1..100 {
        level_tick(&mut m, &mut loader, &mut sink, tick, &missing);
    }
    // The send area (square 4, circle of 3 with the 2-chunk margin), nearest first, up to the
    // first chunk next to the missing one: the queue waits there.
    let mut wanted: Vec<(i32, ChunkPos)> = (-4..=4)
        .flat_map(|x| (-4..=4).map(move |z| p(x, z)))
        .filter(|c| crate::network::play::chunk_sending::want_chunk_sent((0, 0), 3, (c.x, c.z)))
        .map(|c| (c.x * c.x + c.z * c.z, c))
        .collect();
    wanted.sort_by_key(|&(d, c)| (d, c.x, c.z));
    let blocked = wanted
        .iter()
        .position(|(_, c)| (c.x + 4).abs() <= 1 && (c.z - 3).abs() <= 1)
        .unwrap();
    assert_eq!(wanted[blocked].1, p(-3, 2));
    let wanted: Vec<ChunkPos> = wanted[..blocked].iter().map(|&(_, c)| c).collect();
    assert_eq!(sink.sent, wanted);
    let sent = sink.sent.clone();
    // Load + 1 = 5: the 11x11 square is held at FULL (generated), the tick square at 31.
    assert_eq!(m.full_status(p(5, 5)), FullChunkStatus::Full);
    assert_eq!(m.ticket_level(p(1, 1)), ENTITY_TICKING_LEVEL);
    assert_eq!(m.full_status(p(0, 0)), FullChunkStatus::EntityTicking);
    let held = m.holder_count();

    // Far away: everything is forgotten; the old chunks stay 20 ticks, then unload.
    let far = p(100, 0);
    loader.update(far, distances, &mut m, &mut sink);
    assert_eq!(sink.forgotten.len(), sent.len());
    assert_eq!(sink.centers, [far]);
    for tick in 100..119 {
        level_tick(&mut m, &mut loader, &mut sink, tick, &missing);
    }
    assert_eq!(m.full_status(p(0, 0)), FullChunkStatus::EntityTicking);
    for tick in 119..200 {
        level_tick(&mut m, &mut loader, &mut sink, tick, &missing);
    }
    assert_eq!(m.full_status(p(0, 0)), FullChunkStatus::Inaccessible);
    assert_eq!(m.ticket_level(p(0, 0)), MAX_LEVEL + 1);
    assert!(m.chunk(p(0, 0)).is_none());
    // Only the new area's holders remain (the same shape, without a missing chunk).
    assert_eq!(m.holder_count(), held);
    assert!(m.chunk(p(100, 0)).is_some());

    // Leaving: the tickets are delayed, then everything unloads.
    loader.remove(&mut m);
    for tick in 200..260 {
        answer(&mut m, &[]);
        m.tick();
        m.process_unloads(tick);
    }
    assert_eq!(m.holder_count(), 0);
}

/// Derived from Moonrise's sources: a chunk not on disk is generated one status at a time, each
/// step after the chunks around it reach what the generation pyramid requires; the chunks around
/// end at the statuses `generationStatus` gives their distance, and nothing past 11 chunks loads.
#[test]
fn a_missing_chunk_is_generated_through_the_pyramid() {
    let mut m = manager(ChunkSettings::default());
    m.add_ticket(TicketType::Player, p(0, 0), FULL_LEVEL, 1);
    m.process_ticket_updates();
    assert_eq!(m.storage_mut().loads, [p(0, 0)]);
    let mut steps = 0;
    loop {
        let loads = m.storage_mut().loads.clone();
        answer(&mut m, &loads);
        let ran = generate(&mut m);
        steps += ran;
        if loads.is_empty() && ran == 0 {
            break;
        }
    }
    m.process_ticket_updates();
    assert_eq!(m.gen_status(p(0, 0)), Some(ChunkStatus::Full));
    assert!(m.full_chunk(p(0, 0)).is_some());
    assert!(!m.is_loading(p(0, 0)));
    let at = |x| m.gen_status(p(x, 0));
    assert_eq!(at(1), Some(ChunkStatus::InitializeLight));
    assert_eq!(at(2), Some(ChunkStatus::Terrain));
    assert_eq!(at(3), Some(ChunkStatus::Biomes));
    assert_eq!(at(10), Some(ChunkStatus::StructureStarts));
    assert_eq!(at(11), Some(ChunkStatus::StructureStarts));
    assert_eq!(at(12), None);
    // Every worker step of the region ran once: STRUCTURE_STARTS out to 11 chunks, references and
    // biomes to 3, terrain to 2, features to 1.
    let squares = |r: usize| (2 * r + 1) * (2 * r + 1);
    assert_eq!(
        steps,
        squares(11) + squares(3) + squares(3) + squares(2) + squares(1)
    );
    // Stopping saves every chunk, the proto chunks with their status.
    let saved = m.save_all(true, true, None, 9);
    assert_eq!(saved, squares(11));
    assert_eq!(m.storage_mut().saves.len(), squares(11));
}
