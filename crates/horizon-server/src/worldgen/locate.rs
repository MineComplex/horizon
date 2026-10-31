//! Locating: `ChunkGenerator.findNearestMapStructure` (`/locate structure`, explorer maps) and
//! `BiomeSource.findClosestBiome3d` (`/locate biome`) over a level's generator.
//!
//! The structure search follows Paper's version: a placement scan per structure set in the order
//! the wanted structures meet them; concentric rings are all measured (nearest first by the
//! block (8, 32, 8) of their chunk, only the closer ones are checked), random spreads walk square
//! rings of potential chunks outwards until a ring finds something. Each candidate chunk goes
//! through [`StructureCheck`] first; a chunk that may hold a start is loaded to `STRUCTURE_STARTS`
//! (vanilla's `syncLoadNonFull`) and its start of the structure read.
//!
//! Stand-ins: there is no chunk cache to read from yet, so "loading" a chunk generates its
//! structure starts (`createStructures`) in the locator's own cache; a level with saved chunks
//! would also hand its [`ChunkScan`] to [`Locator::with_scan`] (nothing implements that for the
//! region files yet). `canCreateStructure` runs the whole structure generation (the pieces too:
//! the start is invalid, and so would the chunk's, exactly when there are none), which costs more
//! than vanilla's generation point alone.

use std::collections::HashMap;

use crate::core::position::{BlockPos, ChunkPos};
use crate::world::chunk::ChunkStatus;
use crate::worldgen::generator::structures::generate_with_worker;
use crate::worldgen::generator::{GenerationWorker, GeneratorError, LevelGenerator};
use crate::worldgen::proto_chunk::{GenerationRegion, RegionChunk};
use crate::worldgen::structure::check::{
    ChunkScan, NoSavedChunks, StructureCheck, StructureCheckResult,
};
use crate::worldgen::structure::placement::locate_pos;
use crate::worldgen::structure::start::ChunkStructures;
use crate::worldgen::structure::{Placement, StructureId, StructureSetId};

/// `Vec3i.distSqr`.
fn dist_sqr(a: BlockPos, b: BlockPos) -> f64 {
    let d = |p: i32, q: i32| f64::from(p) - f64::from(q);
    let (dx, dy, dz) = (d(a.x, b.x), d(a.y, b.y), d(a.z, b.z));
    dx * dx + dy * dy + dz * dz
}

/// One level's locating state: its structure check, the chunks it loaded for it and a
/// generation worker.
pub struct Locator<'g> {
    generator: &'g LevelGenerator,
    worker: GenerationWorker<'g>,
    check: StructureCheck,
    scan: &'g dyn ChunkScan,
    /// The structure starts of the chunks loaded so far.
    chunks: HashMap<ChunkPos, ChunkStructures>,
    /// The level's world border test for ring positions
    /// (`!locateStructuresOutsideWorldBorder` then skips chunks outside it).
    in_border: Box<dyn Fn(ChunkPos) -> bool + 'g>,
}

impl<'g> Locator<'g> {
    pub fn new(generator: &'g LevelGenerator) -> Result<Self, GeneratorError> {
        Ok(Self {
            generator,
            worker: generator.worker()?,
            check: StructureCheck::new(),
            scan: &NoSavedChunks,
            chunks: HashMap::new(),
            in_border: Box::new(|_| true),
        })
    }

    /// The saved chunks the structure check may read.
    pub fn with_scan(mut self, scan: &'g dyn ChunkScan) -> Self {
        self.scan = scan;
        self
    }

    /// The world border's `isChunkInBounds`, for Paper's `locate-structures-outside-world-border`
    /// off (the default).
    pub fn with_border(mut self, in_border: impl Fn(ChunkPos) -> bool + 'g) -> Self {
        self.in_border = Box::new(in_border);
        self
    }

    /// `findNearestMapStructure(level, structures, pos, maxSearchRadius, createReference)`: the
    /// locate position of the nearest start and its structure.
    pub fn find_nearest_map_structure(
        &mut self,
        wanted: &[StructureId],
        pos: BlockPos,
        max_search_radius: i32,
        create_reference: bool,
    ) -> Option<(BlockPos, StructureId)> {
        let generator = self.generator;
        if !generator.generate_structures() {
            return None;
        }
        let registry = generator.data().structures();
        let placements = generator.placements();
        // `placementScans`: placements in the order they are met, each with its structures.
        let mut scans: Vec<(StructureSetId, Vec<StructureId>)> = Vec::new();
        for &structure in wanted {
            for set in
                placements.sets_for_structure(registry, structure, generator.possible_biomes())
            {
                match scans.iter_mut().find(|(s, _)| *s == set) {
                    Some((_, structures)) => {
                        if !structures.contains(&structure) {
                            structures.push(structure);
                        }
                    }
                    None => scans.push((set, vec![structure])),
                }
            }
        }
        if scans.is_empty() {
            return None;
        }
        let mut nearest = None;
        let mut distance = f64::MAX;
        let mut random_spreads = Vec::new();
        for (set, structures) in &scans {
            match &registry.set(*set).placement {
                Placement::ConcentricRings(_) => {
                    if let Some(found) =
                        self.nearest_ring_structure(structures, *set, pos, create_reference)
                    {
                        let found_distance = dist_sqr(pos, found.0);
                        if found_distance < distance {
                            distance = found_distance;
                            nearest = Some(found);
                        }
                    }
                }
                Placement::RandomSpread(_) => random_spreads.push((*set, structures)),
                Placement::DimensionOrigin => {}
            }
        }
        if random_spreads.is_empty() {
            return nearest;
        }
        let (origin_x, origin_z) = (pos.x >> 4, pos.z >> 4);
        for radius in 0..=max_search_radius {
            let mut found_something = false;
            for &(set, structures) in &random_spreads {
                let found = self.nearest_spread_structure(
                    structures,
                    set,
                    (origin_x, origin_z),
                    radius,
                    create_reference,
                );
                if let Some(found) = found {
                    found_something = true;
                    let found_distance = dist_sqr(pos, found.0);
                    if found_distance < distance {
                        distance = found_distance;
                        nearest = Some(found);
                    }
                }
            }
            if found_something {
                return nearest;
            }
        }
        nearest
    }

    /// `getNearestGeneratedStructure` for concentric rings.
    fn nearest_ring_structure(
        &mut self,
        structures: &[StructureId],
        set: StructureSetId,
        pos: BlockPos,
        create_reference: bool,
    ) -> Option<(BlockPos, StructureId)> {
        let generator = self.generator;
        let positions = generator
            .placements()
            .ring_positions(set)
            .expect("a ring set of the level has its positions");
        let mut closest_pos = None;
        let mut closest = f64::MAX;
        for &chunk in positions {
            if !(self.in_border)(chunk) {
                continue;
            }
            let structure_pos = BlockPos::new((chunk.x << 4) + 8, 32, (chunk.z << 4) + 8);
            let distance = dist_sqr(structure_pos, pos);
            if closest_pos.is_none() || distance < closest {
                if let Some(found) = self.structure_generating_at(structures, set, chunk, create_reference)
                {
                    closest_pos = Some(found);
                    closest = distance;
                }
            }
        }
        closest_pos
    }

    /// `getNearestGeneratedStructure` for a random spread: the ring of sectors `radius` away
    /// (Paper walks the border only).
    fn nearest_spread_structure(
        &mut self,
        structures: &[StructureId],
        set: StructureSetId,
        (origin_x, origin_z): (i32, i32),
        radius: i32,
        create_reference: bool,
    ) -> Option<(BlockPos, StructureId)> {
        let generator = self.generator;
        let registry = generator.data().structures();
        let Placement::RandomSpread(spread) = &registry.set(set).placement else {
            return None;
        };
        for x in -radius..=radius {
            let x_edge = x == -radius || x == radius;
            let step = if x_edge { 1 } else { radius * 2 };
            let mut z = -radius;
            while z <= radius {
                let sector_x = origin_x.wrapping_add(spread.spacing.wrapping_mul(x));
                let sector_z = origin_z.wrapping_add(spread.spacing.wrapping_mul(z));
                let target = (generator.placements())
                    .potential_structure_chunk(registry, set, sector_x, sector_z)
                    .expect("a random spread set of the level");
                if let Some(found) =
                    self.structure_generating_at(structures, set, target, create_reference)
                {
                    return Some(found);
                }
                z += step;
            }
        }
        None
    }

    /// `getStructureGeneratingAt`.
    fn structure_generating_at(
        &mut self,
        structures: &[StructureId],
        set: StructureSetId,
        target: ChunkPos,
        create_reference: bool,
    ) -> Option<(BlockPos, StructureId)> {
        let generator = self.generator;
        let registry = generator.data().structures();
        let placements = generator.placements();
        let placement = &registry.set(set).placement;
        for &structure in structures {
            let (min_y, height) = {
                let shape = generator.shape();
                (shape.min_section_y * 16, shape.section_count as i32 * 16)
            };
            let Self {
                worker,
                check,
                scan,
                ..
            } = self;
            let fast = check.check_start(
                registry,
                target,
                structure,
                create_reference,
                *scan,
                || placements.additional_restrictions(registry, set, target.x, target.z),
                || generate_with_worker(worker, structure, target, (min_y, height), 0).is_some(),
            );
            if fast == StructureCheckResult::StartNotPresent {
                continue;
            }
            if !create_reference && fast == StructureCheckResult::StartPresent {
                return Some((locate_pos(placement, target), structure));
            }
            self.load_chunk(target);
            let start_chunk = (self.chunks[&target].start(structure))
                .filter(|s| s.is_valid())
                .map(|s| s.chunk);
            if let Some(start_chunk) = start_chunk
                && (!create_reference || self.try_add_reference(target, structure))
            {
                return Some((locate_pos(placement, start_chunk), structure));
            }
        }
        None
    }

    /// `tryAddReference`: `StructureManager.addReference` when the start can still be taken.
    fn try_add_reference(&mut self, chunk: ChunkPos, structure: StructureId) -> bool {
        let Some(mut start) = self.chunks[&chunk].start(structure).cloned() else {
            return false;
        };
        if !start.can_be_referenced() {
            return false;
        }
        start.references += 1;
        let start_chunk = start.chunk;
        self.chunks
            .get_mut(&chunk)
            .expect("a loaded chunk")
            .set_start(structure, start);
        self.check.increment_reference(start_chunk, structure);
        true
    }

    /// Loads the chunk to `STRUCTURE_STARTS` (once) and tells the structure check
    /// (`onStructureLoad`).
    fn load_chunk(&mut self, pos: ChunkPos) {
        if self.chunks.contains_key(&pos) {
            return;
        }
        let mut chunk = self.generator.empty_chunk(pos);
        {
            let mut region = GenerationRegion::new(pos, 0, vec![RegionChunk::Writable(&mut chunk)]);
            self.worker
                .run(ChunkStatus::StructureStarts, &mut region)
                .expect("the structure starts step");
        }
        let structures = std::mem::take(&mut chunk.structures);
        self.check.on_structure_load(
            pos,
            (structures.starts())
                .filter(|(_, start)| start.is_valid())
                .map(|(structure, start)| (structure, start.references)),
        );
        self.chunks.insert(pos, structures);
    }
}

/// `ServerLevel.findClosestBiome3d` on the level's generator: the nearest sampled block whose
/// biome `allowed` takes, and that biome.
pub fn find_closest_biome_3d(
    generator: &LevelGenerator,
    origin: BlockPos,
    max_search_radius: i32,
    sample_resolution: (i32, i32),
    allowed: impl Fn(u32) -> bool,
) -> Option<(BlockPos, u32)> {
    let shape = generator.shape();
    let min_y = shape.min_section_y * 16;
    let max_y = min_y + shape.section_count as i32 * 16 - 1;
    let found = generator.biome_source().find_closest_biome_3d(
        generator.density(),
        [origin.x, origin.y, origin.z],
        max_search_radius,
        sample_resolution,
        (min_y, max_y),
        allowed,
    )?;
    Some((BlockPos::new(found.0[0], found.0[1], found.0[2]), found.1))
}

#[cfg(test)]
mod oracle_tests;
