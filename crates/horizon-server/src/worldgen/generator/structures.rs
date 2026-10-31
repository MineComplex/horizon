//! The STRUCTURE_STARTS and STRUCTURE_REFERENCES steps (`ChunkGenerator.createStructures`,
//! `createReferences`) over `worldgen::structure`'s placements and start/reference storage.
//!
//! Making a start (`Structure.generate`: the generation point, the biome check at it, the pieces)
//! is [`generate_start`]. Jigsaw structures (`worldgen::structure::jigsaw`) and the structures built
//! from templates (`worldgen::structure::template_start`) are ported; the other types make no start yet.

use super::{GenerationWorker, LevelGenerator};
use crate::block::BlockState;
use crate::core::position::{BlockPos, ChunkPos};
use crate::util::random::{LegacyRandom, WorldgenRandom};
use crate::world::chunk::serial::Heightmap;
use crate::world::level_chunk::HeightmapFlags;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::biome::BiomeSource;
use crate::worldgen::climate::{ClimateSampler, ClimateSearch};
use crate::worldgen::data::LevelDensity;
use crate::worldgen::density::DensityContext;
use crate::worldgen::density::{
    Beardifier, JigsawJunction, RigidPiece, StructureBounds, TerrainAdjustment,
};
use crate::worldgen::proto_chunk::{GenerationRegion, OutsideRegion, ProtoChunk};
use crate::worldgen::structure::jigsaw::{GenerationContext, JigsawTables, JigsawWorld, generate};
use crate::worldgen::structure::pool::Projection;
use crate::worldgen::structure::start::{StructurePiece, StructureStart, create_references};
use crate::worldgen::structure::template_start::{
    TemplateGen, TemplateGenWorld, generate as template_generate, is_template_structure,
};
use crate::worldgen::structure::{BiomeSet, StructureId, StructureRegistry};
use crate::worldgen::terrain::NoiseTerrain;

/// What a structure's generation reads of the level: the noise columns' heights and the biome
/// source (`Structure.GenerationContext` over the chunk generator and its random state).
pub struct LevelColumns<'a, 'g> {
    pub terrain: &'a NoiseTerrain<'g>,
    pub density: &'a LevelDensity,
    pub source: &'a BiomeSource,
    pub heightmaps: &'a dyn HeightmapFlags,
    pub context: &'a mut DensityContext,
    pub search: &'a mut ClimateSearch,
    /// The level's height (the chunk's `heightAccessor`).
    pub min_y: i32,
    pub height: i32,
    /// The structure's biomes (`validBiome`).
    pub biomes: &'a BiomeSet,
}

impl LevelColumns<'_, '_> {
    /// `biomeResolver.getNoiseBiome` at a block.
    pub fn biome_at(&mut self, pos: BlockPos) -> u32 {
        let mut context = DensityContext::uncached();
        let quart = [pos.x >> 2, pos.y >> 2, pos.z >> 2];
        self.source
            .noise_biome(self.density, &mut context, self.search, quart)
    }
}

impl JigsawWorld for LevelColumns<'_, '_> {
    fn first_free_height(&mut self, x: i32, z: i32, heightmap: Heightmap) -> i32 {
        let flags = self.heightmaps;
        let opaque = |state| flags.heightmap_mask(state) >> heightmap as u8 & 1 != 0;
        let world = (self.min_y, self.height);
        (self.terrain)
            .base_height(x, z, world, self.context, opaque)
            .unwrap_or(self.min_y)
    }

    fn could_exist_in_column(&mut self, x: i32, z: i32, min_y: i32, max_y: i32) -> bool {
        let (qx, qz) = (x >> 2, z >> 2);
        let (min_q, max_q) = (min_y >> 2, max_y >> 2);
        let mut context = DensityContext::uncached();
        if let BiomeSource::MultiNoise(index) = self.source {
            // `createResolverForChunk`: the climate over the column first.
            let Ok(volume) = ClimateSampler::new(self.density).sample_volume(
                &mut context,
                [qx, min_q, qz],
                [1, max_q - min_q + 1, 1],
            ) else {
                return false;
            };
            return (min_q..=max_q).any(|qy| {
                volume
                    .target([qx, qy, qz])
                    .is_some_and(|t| self.biomes.contains(*index.find(self.search, t)))
            });
        }
        (min_q..=max_q).any(|qy| {
            let biome =
                (self.source).noise_biome(self.density, &mut context, self.search, [qx, qy, qz]);
            self.biomes.contains(biome)
        })
    }
}

impl TemplateGenWorld for LevelColumns<'_, '_> {
    fn base_column(&mut self, x: i32, z: i32) -> Option<(i32, Vec<BlockState>)> {
        let world = (self.min_y, self.height);
        self.terrain
            .base_column(x, z, world, self.context)
            .ok()
            .flatten()
    }

    fn biome_at(&mut self, pos: BlockPos) -> u32 {
        LevelColumns::biome_at(self, pos)
    }

    fn is_opaque(&self, state: BlockState, heightmap: Heightmap) -> bool {
        self.heightmaps.heightmap_mask(state) >> heightmap as u8 & 1 != 0
    }
}

/// `Structure.generate` for the structures built from templates: the generation point, the biome
/// check at it and the pieces.
fn generate_template_start(
    generator: &LevelGenerator,
    columns: &mut LevelColumns<'_, '_>,
    structure: StructureId,
    chunk: ChunkPos,
    references: i32,
) -> Option<StructureStart> {
    let registry = generator.data().structures();
    let settings = generator.settings();
    let generation = WorldGenerationContext::new(
        columns.min_y.max(settings.noise.min_y),
        columns.height.min(settings.noise.height),
        settings.sea_level,
    );
    let mut random = WorldgenRandom::new(LegacyRandom::new(0));
    random.set_large_feature_seed(generator.seed, chunk.x, chunk.z);
    let (min_y, height) = (columns.min_y, columns.height);
    let mut cx = TemplateGen {
        registry,
        blocks: generator.blocks(),
        data: generator.flags(),
        climates: generator.data().climates(),
        world: &mut *columns,
        generation,
        min_y,
        height,
        chunk,
    };
    let stub = match template_generate(registry.structure(structure), &mut cx, &mut random) {
        Ok(stub) => stub?,
        Err(e) => {
            tracing::warn!(
                "Structure {} in chunk {chunk:?}: {e}",
                registry.structure(structure).id
            );
            return None;
        }
    };
    // `isValidBiome`.
    let biome = columns.biome_at(stub.position);
    if !columns.biomes.contains(biome) {
        return None;
    }
    let pieces = (stub.pieces.into_iter())
        .map(StructurePiece::Template)
        .collect();
    Some(StructureStart::new(
        registry, structure, chunk, references, pieces,
    ))
}

/// `tryGenerateStructure`'s `structure.generate(...)` for `structure` starting in `chunk`, with
/// the references a previous start of it in the chunk had (`fetchReferences`). `None` is an
/// invalid start (no generation point, a biome the structure does not allow, or no pieces).
pub fn generate_start(
    generator: &LevelGenerator,
    columns: &mut LevelColumns<'_, '_>,
    structure: StructureId,
    chunk: ChunkPos,
    references: i32,
) -> Option<StructureStart> {
    let registry = generator.data().structures();
    if is_template_structure(&registry.structure(structure).kind) {
        return generate_template_start(generator, columns, structure, chunk, references);
    }
    let Some(config) = registry.structure(structure).jigsaw.as_ref() else {
        // The piece-based structure types (`legacy_structures`).
        let pieces = super::legacy_structures::generate(generator, columns, structure, chunk)?;
        return Some(StructureStart::new(
            registry, structure, chunk, references, pieces,
        ));
    };
    let settings = generator.settings();
    let generation = WorldGenerationContext::new(
        columns.min_y.max(settings.noise.min_y),
        columns.height.min(settings.noise.height),
        settings.sea_level,
    );
    let mut random = WorldgenRandom::new(LegacyRandom::new(0));
    random.set_large_feature_seed(generator.seed, chunk.x, chunk.z);
    let mut cx = GenerationContext {
        tables: JigsawTables {
            pools: &registry.pools,
            templates: &registry.templates,
            blocks: generator.blocks(),
        },
        min_y: columns.min_y,
        height: columns.height,
        world: columns,
        generation,
        seed: generator.seed,
        chunk,
    };
    let start = generate(config, &mut cx, &mut random, |world, pos| {
        let biome = world.biome_at(pos);
        world.biomes.contains(biome)
    });
    let start = match start {
        Ok(start) => start?,
        Err(e) => {
            tracing::warn!(
                "Structure {} in chunk {chunk:?}: {e}",
                registry.structure(structure).id
            );
            return None;
        }
    };
    let pieces = start
        .pieces
        .into_iter()
        .map(StructurePiece::Jigsaw)
        .collect();
    Some(StructureStart::new(
        registry, structure, chunk, references, pieces,
    ))
}

/// [`generate_start`] with a worker's noise columns and climate search, for a chunk of the level's
/// height `(min_y, height)`.
pub fn generate_with_worker(
    worker: &mut GenerationWorker<'_>,
    structure: StructureId,
    chunk: ChunkPos,
    (min_y, height): (i32, i32),
    references: i32,
) -> Option<StructureStart> {
    let generator = worker.generator;
    let mut columns = LevelColumns {
        terrain: &worker.terrain,
        density: &generator.density,
        source: &generator.source,
        heightmaps: &*generator.tables.heightmaps,
        context: &mut worker.context,
        search: &mut worker.search,
        min_y,
        height,
        biomes: &generator.data().structures().structure(structure).biomes,
    };
    generate_start(generator, &mut columns, structure, chunk, references)
}

/// `createStructures` for the chunk (when the level generates structures): for each possible
/// set whose placement allows the chunk and that has no valid start here yet, its structures are
/// tried (`create_starts`' order) until one makes a valid start, which the chunk keeps.
pub fn create_structures(worker: &mut GenerationWorker<'_>, chunk: &mut ProtoChunk) {
    let generator = worker.generator;
    let pos = chunk.pos();
    let (min_y, height) = (chunk.min_y(), chunk.height());
    let registry = generator.data().structures();
    let structures = &mut chunk.structures;
    // Both callbacks see the starts made so far (vanilla stores each in the chunk at once).
    let chosen: std::cell::RefCell<Vec<(StructureId, StructureStart)>> = Default::default();
    generator.placements().create_starts(
        registry,
        pos,
        |structure| {
            chosen
                .borrow()
                .iter()
                .any(|(s, start)| *s == structure && start.is_valid())
                || structures
                    .start(structure)
                    .is_some_and(StructureStart::is_valid)
        },
        |structure| {
            let references = structures.start(structure).map_or(0, |s| s.references);
            match generate_with_worker(worker, structure, pos, (min_y, height), references) {
                Some(start) if start.is_valid() => {
                    chosen.borrow_mut().push((structure, start));
                    true
                }
                _ => false,
            }
        },
    );
    for (structure, start) in chosen.into_inner() {
        structures.set_start(structure, start);
    }
}

/// `createReferences`: every valid start within 8 chunks whose box meets the centre's columns
/// is referenced by the centre chunk. The region reaches 8 chunks (STRUCTURE_STARTS done there).
pub fn create_references_in(region: &mut GenerationRegion<'_>) -> Result<(), OutsideRegion> {
    let center = region.center();
    let references = create_references(center, |pos| region.chunk(pos).map(|c| &c.structures));
    let chunk = region.chunk_mut(center)?;
    for (structure, key) in references {
        chunk.structures.add_reference(structure, key);
    }
    Ok(())
}

/// `Beardifier.forStructuresInChunk(structureManager, chunk)`: the pieces within 12 blocks of the
/// chunk of the starts it references whose structure adapts the terrain (rigid jigsaw pieces with
/// their ground delta, every other piece kind whole) and the jigsaw junctions near it. `None` (the
/// empty beardifier) when nothing is near. The order is the references' (structure, then key),
/// then each start's pieces.
pub fn beardifier_for(
    region: &GenerationRegion<'_>,
    registry: &StructureRegistry,
) -> Option<Beardifier> {
    let pos = region.center();
    let chunk = region.chunk(pos)?;
    let (cx, cz) = (pos.x << 4, pos.z << 4);
    let mut rigids = Vec::new();
    let mut junctions = Vec::new();
    let mut any: Option<StructureBounds> = None;
    let mut include = |bounds: StructureBounds| {
        any = Some(any.map_or(bounds, |a| a.encapsulating(bounds)));
    };
    for (structure, keys) in chunk.structures.all_references() {
        let adjustment = registry.structure(structure).terrain_adaptation;
        if adjustment == TerrainAdjustment::None {
            continue;
        }
        for &key in keys {
            let Some(start) = region
                .chunk(ChunkPos::unpack(key))
                .and_then(|c| c.structures.start(structure))
                .filter(|s| s.is_valid())
            else {
                continue;
            };
            for piece in &start.children {
                let Some(bounds) = piece.bounding_box() else {
                    continue;
                };
                // `isCloseToChunk(chunk, 12)`.
                if !bounds.intersects_columns(cx - 12, cz - 12, cx + 15 + 12, cz + 15 + 12) {
                    continue;
                }
                let StructurePiece::Jigsaw(piece) = piece else {
                    rigids.push(RigidPiece {
                        bounds,
                        adjustment,
                        ground_level_delta: 0,
                    });
                    include(bounds);
                    continue;
                };
                if registry.pools.element(piece.element).projection == Projection::Rigid {
                    rigids.push(RigidPiece {
                        bounds,
                        adjustment,
                        ground_level_delta: piece.ground_level_delta,
                    });
                    include(bounds);
                }
                for junction in &piece.junctions {
                    let (x, z) = (junction.source_x, junction.source_z);
                    if x > cx - 12 && z > cz - 12 && x < cx + 15 + 12 && z < cz + 15 + 12 {
                        junctions.push(JigsawJunction {
                            source_x: x,
                            source_ground_y: junction.source_ground_y,
                            source_z: z,
                        });
                        let at = [x, junction.source_ground_y, z];
                        include(StructureBounds::from_corners(at, at));
                    }
                }
            }
        }
    }
    let affected = any?.inflated_by(24);
    Some(Beardifier::new(rigids, junctions, Some(affected)))
}
