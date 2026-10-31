//! A level's chunk generator (26.3's `NoiseBasedChunkGenerator` with its `RandomState` and
//! `ChunkGeneratorStructureState`, Paper's flat bedrock and Purpur's End void rings) and the
//! generation steps of the status pipeline (`ChunkStatusTasks`), run by a worker over a
//! [`GenerationRegion`].
//!
//! - [`LevelGenerator`]: built once per level from its stem; immutable, shared read-only (one
//!   `Arc`) with the generation workers.
//! - [`GenerationWorker`]: a worker thread's state for one level (its surface system, density
//!   contexts and climate search: what vanilla keeps per thread or per `NoiseChunk`).
//! - [`pyramid`]: the statuses' neighbour requirements and write radii, with Moonrise's tables.
//! - [`structures`]: STRUCTURE_STARTS and STRUCTURE_REFERENCES.
//! - [`decorate`]: the FEATURES step's entry point.
//! - [`convert`]: proto chunks to and from their saved form, and the FULL step's level chunk.
//!
//! The scheduler that runs these steps is `server::chunk_system`. Steps not ported: SPAWN's
//! `spawnOriginalMobs` (no entities yet), the light steps (Starlight is not ported: chunks reach
//! `light` with `isLightCorrect` false, as Moonrise leaves a chunk it did not light, and clients
//! get no light data), flat and debug generators, old-world blending and below-zero retrogen.

pub mod any;
pub mod convert;
pub mod decorate;
pub mod fixed;
pub mod flat_settings;
pub mod legacy_structures;
pub mod pyramid;
pub mod structures;

#[cfg(test)]
mod fixed_tests;
#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod structure_chunks_oracle_tests;

use std::sync::Arc;

pub use any::{ChunkGenerator, ChunkWorker, SharedGenerator};
pub use decorate::decorate;

use crate::block::flags::BlockStateData;
use crate::block::{BlockState, BlockStates};
use crate::core::position::{BlockPos, ChunkPos};
use crate::server::config::SpigotWorldSeeds;
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{ChunkStatus, ContainerFactory, PalettedContainer};
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::biome::BiomeSource;
use crate::worldgen::biome::codec::NoiseGeneratorStem;
use crate::worldgen::biome::spawn::find_spawn_position;
use crate::worldgen::biome::zoom::{BiomeZoom, obfuscate_seed};
use crate::worldgen::carver::{CarveError, CarverStep};
use crate::worldgen::climate::ClimateSearch;
use crate::worldgen::data::{LevelDensity, WorldgenData};
use crate::worldgen::density::{DensityContext, WorldFields};
use crate::worldgen::proto_chunk::{GenerationRegion, OutsideRegion, ProtoChunk};
use crate::worldgen::settings::NoiseGeneratorSettings;
use crate::worldgen::structure::placement::{LevelParams, LevelPlacements, ring_searches};
use crate::worldgen::structure::{BiomeSet, Placement, StructureSetId};
use crate::worldgen::surface::{SurfaceError, SurfaceSystem};
use crate::worldgen::terrain::{NoiseTerrain, TerrainError};
use convert::ChunkShape;

/// Why a level's generator could not be made (a datapack naming what is not there).
#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("the noise settings {0} are not registered")]
    UnknownSettings(String),
    #[error("the level's density functions do not compile: {0}")]
    Density(String),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    #[error("the aquifer's fluids are missing: {0}")]
    Fluids(String),
    #[error("the block table lacks grass, mycelium, dirt or void air")]
    Blocks,
    /// `FeatureSorter`'s cycle; `sources` are the biome ids its indices point into, which the
    /// caller names (the message lists biome ids, the user's decision).
    #[error("feature order cycle among biome sources {:?}", cycle.involved)]
    FeatureCycle {
        cycle: crate::worldgen::feature::sort::FeatureCycle,
        sources: Vec<u32>,
    },
}

/// Why a generation step failed for a chunk (Moonrise logs it and the chunk's generation stops).
#[derive(Debug, thiserror::Error)]
pub enum StepError {
    #[error(transparent)]
    Region(#[from] OutsideRegion),
    #[error(transparent)]
    Terrain(#[from] TerrainError),
    #[error(transparent)]
    Carve(#[from] CarveError),
    #[error(transparent)]
    Decorate(#[from] decorate::DecorateError),
    #[error("the {0:?} step does not run on a generation worker")]
    NotAWorkerStep(ChunkStatus),
}

/// The tables every level's generator reads (startup's, shared).
#[derive(Clone)]
pub struct GeneratorTables {
    pub data: Arc<WorldgenData>,
    pub blocks: Arc<BlockStates>,
    pub flags: Arc<BlockStateData>,
    pub heightmaps: Arc<HeightmapTable>,
    /// The blocks' `canSurvive` rules over the data-pack tags (features' placement checks).
    pub survival: Arc<crate::block::survival::BlockSurvival>,
}

/// What one level's generator is made from.
pub struct LevelGeneratorSettings {
    /// The stem's `minecraft:noise` generator.
    pub stem: NoiseGeneratorStem,
    /// `WorldOptions.seed` of the level.
    pub seed: i64,
    /// `WorldOptions.generateStructures` of the level.
    pub generate_structures: bool,
    /// Paper's `environment.generate-flat-bedrock`.
    pub flat_bedrock: bool,
    /// Purpur's `settings.generate-end-void-rings`.
    pub end_void_rings: bool,
    /// The level's `spigot.yml` structure seeds.
    pub spigot_seeds: SpigotWorldSeeds,
    pub min_section_y: i32,
    pub section_count: usize,
    pub factory: ContainerFactory,
}

/// A level's generator (see the module docs). Immutable.
pub struct LevelGenerator {
    tables: GeneratorTables,
    settings_id: String,
    density: LevelDensity,
    source: BiomeSource,
    possible_biomes: BiomeSet,
    placements: LevelPlacements,
    seed: i64,
    generate_structures: bool,
    flat_bedrock: bool,
    zoom: BiomeZoom,
    origin: ChunkPos,
    min_section_y: i32,
    section_count: usize,
    factory: ContainerFactory,
    void_air: BlockState,
    /// `FeatureSorter.buildFeaturesPerStep` over the biome source's `possibleBiomes()`.
    feature_steps: Vec<crate::worldgen::feature::sort::StepFeatureData>,
    /// The placed features whose type is not ported yet: skipped (a stand-in).
    unported_features: std::collections::HashSet<crate::worldgen::feature::PlacedFeatureId>,
}

impl std::fmt::Debug for LevelGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LevelGenerator")
            .field("settings", &self.settings_id)
            .field("seed", &self.seed)
            .finish_non_exhaustive()
    }
}

impl LevelGenerator {
    /// `new NoiseBasedChunkGenerator(source, settings)` with the level's `RandomState.create`
    /// and `createState` (the concentric ring searches run here, spread over threads; vanilla
    /// runs them as background tasks on first use, in the same order).
    pub fn new(
        tables: GeneratorTables,
        level: LevelGeneratorSettings,
    ) -> Result<Self, GeneratorError> {
        let settings = tables
            .data
            .noise_settings(&level.stem.settings)
            .ok_or_else(|| GeneratorError::UnknownSettings(level.stem.settings.clone()))?;
        let density = tables
            .data
            .compile_level(settings, level.seed, level.end_void_rings)
            .map_err(|e| GeneratorError::Density(e.to_string()))?;
        let source = level.stem.biome_source;
        let possible_biomes = BiomeSet::new(source.possible_biomes());
        let features = tables.data.features();
        let feature_steps = features
            .features_per_step(&source.possible_biomes())
            .map_err(|cycle| GeneratorError::FeatureCycle {
                cycle,
                sources: source.possible_biomes(),
            })?;
        let unported_features =
            crate::worldgen::feature::decorate::unported_features(features, &feature_steps);
        let origin = find_spawn_position(&density)
            .map_or(ChunkPos::new(0, 0), |[x, z]| ChunkPos::new(x >> 4, z >> 4));
        let registry = tables.data.structures();
        let sets: Vec<StructureSetId> = (registry.sets.iter().enumerate())
            .filter(|(_, set)| !set.inline)
            .map(|(i, _)| StructureSetId(i as u32))
            .collect();
        let params = LevelParams {
            seed: level.seed,
            ring_seed: level.seed,
            origin,
            sets: &sets,
            possible_biomes: &possible_biomes,
            seeds: &level.spigot_seeds,
        };
        let searches = ring_searches(registry, &params);
        let ring_positions = run_ring_searches(&searches, registry, &source, &density);
        let placements = LevelPlacements::new(registry, &params, &ring_positions);
        let void_air = tables
            .blocks
            .block_by_name("minecraft:void_air")
            .map(|b| tables.blocks.default_state(b))
            .ok_or(GeneratorError::Blocks)?;
        let generator = Self {
            settings_id: level.stem.settings,
            density,
            source,
            possible_biomes,
            placements,
            seed: level.seed,
            generate_structures: level.generate_structures,
            flat_bedrock: level.flat_bedrock,
            zoom: BiomeZoom::new(obfuscate_seed(level.seed)),
            origin,
            min_section_y: level.min_section_y,
            section_count: level.section_count,
            factory: level.factory,
            void_air,
            feature_steps,
            unported_features,
            tables,
        };
        generator.worker()?;
        Ok(generator)
    }

    pub fn data(&self) -> &WorldgenData {
        &self.tables.data
    }
    pub fn blocks(&self) -> &BlockStates {
        &self.tables.blocks
    }
    pub fn flags(&self) -> &BlockStateData {
        &self.tables.flags
    }
    pub fn heightmaps(&self) -> &HeightmapTable {
        &self.tables.heightmaps
    }
    /// The level's noise settings.
    pub fn settings(&self) -> &NoiseGeneratorSettings {
        self.tables
            .data
            .noise_settings(&self.settings_id)
            .expect("checked when the generator was made")
    }
    pub fn density(&self) -> &LevelDensity {
        &self.density
    }
    pub fn biome_source(&self) -> &BiomeSource {
        &self.source
    }
    /// `biomeSource.possibleBiomes()`.
    pub fn possible_biomes(&self) -> &BiomeSet {
        &self.possible_biomes
    }
    pub fn placements(&self) -> &LevelPlacements {
        &self.placements
    }
    pub fn seed(&self) -> i64 {
        self.seed
    }
    pub fn generate_structures(&self) -> bool {
        self.generate_structures
    }
    /// The level's `BiomeManager` zoom.
    pub fn zoom(&self) -> BiomeZoom {
        self.zoom
    }
    /// `getSeaLevel`: the noise settings'.
    pub fn sea_level(&self) -> i32 {
        self.settings().sea_level
    }
    /// `getOrigin(randomState)`: the noise spawn finder's chunk, or chunk 0 0 without spawn
    /// targets.
    pub fn origin(&self) -> ChunkPos {
        self.origin
    }
    /// `ChunkGenerator.getSpawnHeight` (26.3's noise generator keeps the default).
    pub fn spawn_height(&self) -> i32 {
        64
    }
    /// The level's chunk shape.
    pub fn shape(&self) -> ChunkShape<'_> {
        ChunkShape {
            min_section_y: self.min_section_y,
            section_count: self.section_count,
            factory: &self.factory,
            void_air: self.void_air,
        }
    }
    /// An empty chunk (what a chunk not on disk starts as).
    pub fn empty_chunk(&self, pos: ChunkPos) -> ProtoChunk {
        self.shape().empty(pos)
    }

    /// A worker's state for this level.
    pub fn worker(&self) -> Result<GenerationWorker<'_>, GeneratorError> {
        let settings = self.settings();
        let blocks = &*self.tables.blocks;
        Ok(GenerationWorker {
            generator: self,
            terrain: NoiseTerrain::new(&self.density, settings, blocks)
                .map_err(|e| GeneratorError::Fluids(e.to_string()))?,
            surface: SurfaceSystem::new(
                &self.tables.data,
                &self.density,
                settings,
                blocks,
                self.seed,
                self.flat_bedrock,
            )?,
            carvers: CarverStep::new(
                self.tables.data.carvers(),
                &self.density,
                settings,
                blocks,
                self.seed,
            )
            .ok_or(GeneratorError::Blocks)?,
            search: ClimateSearch::default(),
            context: DensityContext::cached(),
            uncached: DensityContext::uncached(),
        })
    }
}

/// The concentric ring searches (`generateRingPositions`' `findBiomeHorizontal` per ring
/// position) over the level's biome source, spread over threads, results in order.
fn run_ring_searches(
    searches: &[crate::worldgen::structure::placement::RingSearch],
    registry: &crate::worldgen::structure::StructureRegistry,
    source: &BiomeSource,
    density: &LevelDensity,
) -> Vec<ChunkPos> {
    if searches.is_empty() {
        return Vec::new();
    }
    let one = |search: &crate::worldgen::structure::placement::RingSearch| {
        let Placement::ConcentricRings(rings) = &registry.set(search.set).placement else {
            return search.chunk;
        };
        let preferred = &rings.preferred_biomes;
        let (x, z) = search.center();
        let mut random = search.random.clone();
        let found = source.find_biome_horizontal(
            density,
            [x, 0, z],
            112,
            |biome| preferred.contains(biome),
            &mut random,
        );
        search.result(found.map(|([x, _, z], _)| (x, z)))
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let per = searches.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let handles: Vec<_> = searches
            .chunks(per)
            .map(|part| scope.spawn(move || part.iter().map(one).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a ring search thread"))
            .collect()
    })
}

/// The final heightmaps the TERRAIN step primes (`ChunkStatusTasks.buildTerrain`).
const FINAL_HEIGHTMAPS: [Heightmap; 4] = [
    Heightmap::MotionBlocking,
    Heightmap::MotionBlockingNoLeaves,
    Heightmap::OceanFloor,
    Heightmap::WorldSurface,
];

/// A worker thread's generation state for one level (see the module docs).
pub struct GenerationWorker<'g> {
    generator: &'g LevelGenerator,
    terrain: NoiseTerrain<'g>,
    surface: SurfaceSystem<'g>,
    carvers: CarverStep<'g>,
    /// The climate tree's last result (vanilla's `RTree.lastResult` thread-local).
    search: ClimateSearch,
    /// The `NoiseChunk`'s caching samplers, reset per chunk.
    context: DensityContext,
    /// `createUncachedResolver`'s context (the carvers' biome reads).
    uncached: DensityContext,
}

impl<'g> GenerationWorker<'g> {
    pub fn generator(&self) -> &'g LevelGenerator {
        self.generator
    }

    /// Forgets the climate tree's last result (tests replay vanilla's per-thread order).
    pub fn reset_search(&mut self) {
        self.search = ClimateSearch::default();
    }

    /// The generation step to `status` for the region's centre (`ChunkStep.apply` with the
    /// generation pyramid's task), then the chunk's status set (`completeChunkGeneration`). The
    /// region is the step's read radius around the centre, writable within its write radius.
    pub fn run(
        &mut self,
        status: ChunkStatus,
        region: &mut GenerationRegion<'_>,
    ) -> Result<(), StepError> {
        let generator = self.generator;
        let center = region.center();
        match status {
            ChunkStatus::Empty
            | ChunkStatus::InitializeLight
            | ChunkStatus::Light
            | ChunkStatus::Spawn => {}
            ChunkStatus::StructureStarts => {
                if generator.generate_structures {
                    structures::create_structures(self, region.chunk_mut(center)?);
                }
            }
            ChunkStatus::StructureReferences => structures::create_references_in(region)?,
            ChunkStatus::Biomes => {
                generator.source.create_biomes(
                    region.chunk_mut(center)?,
                    &generator.density,
                    &mut self.search,
                );
            }
            ChunkStatus::Terrain => self.terrain(region)?,
            ChunkStatus::Features => decorate(generator, region)?,
            ChunkStatus::Full => return Err(StepError::NotAWorkerStep(status)),
        }
        let chunk = region.chunk_mut(center)?;
        if chunk.status() < status {
            chunk.set_status(status);
        }
        Ok(())
    }

    /// `buildTerrain`: the noise fill, the surface (with the region's stored biomes through the
    /// level's zoom, and the biomes of the 3 x 3 chunks' palettes as the possible ones), the
    /// carvers, then the final heightmaps primed.
    fn terrain(&mut self, region: &mut GenerationRegion<'_>) -> Result<(), StepError> {
        let generator = self.generator;
        let center = region.center();
        let window = BiomeWindow::new(region)?;
        let possible = window.possible_biomes();
        let flags = &*generator.tables.flags;
        let heightmaps = &*generator.tables.heightmaps;
        let zoom = generator.zoom;
        let beardifier = structures::beardifier_for(region, generator.data().structures());
        let chunk = region.chunk_mut(center)?;
        self.context.reset();
        self.context.set_fields(WorldFields { beardifier });
        let aquifer =
            self.terrain
                .fill_with_aquifer(chunk, &mut self.context, flags, heightmaps)?;
        if let Some(mut aquifer) = aquifer {
            self.surface.build_surface(
                chunk,
                &mut self.context,
                flags,
                heightmaps,
                |pos| window.biome(zoom.quart_at_block(pos)),
                Some(&possible),
            );
            let (source, density) = (&generator.source, &generator.density);
            let (uncached, search) = (&mut self.uncached, &mut self.search);
            self.carvers.carve(
                chunk,
                &mut aquifer,
                &self.surface,
                &mut self.context,
                flags,
                heightmaps,
                zoom,
                |quart| source.noise_biome(density, uncached, search, quart),
            )?;
        }
        chunk.prime_heightmaps(&FINAL_HEIGHTMAPS, heightmaps);
        Ok(())
    }
}

/// The stored biomes of the 3 x 3 chunks around a TERRAIN step's centre (what the region's
/// `BiomeManager` reads), copied so the centre can be written meanwhile.
struct BiomeWindow {
    center: ChunkPos,
    min_section_y: i32,
    /// Per chunk (z, then x), its sections' biome containers.
    chunks: Vec<Vec<PalettedContainer<u32>>>,
}

impl BiomeWindow {
    fn new(region: &GenerationRegion<'_>) -> Result<Self, OutsideRegion> {
        let center = region.center();
        let mut chunks = Vec::with_capacity(9);
        let mut min_section_y = 0;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let pos = ChunkPos::new(center.x + dx, center.z + dz);
                let chunk = region.chunk(pos).ok_or(OutsideRegion(pos))?;
                min_section_y = chunk.min_y() >> 4;
                chunks.push(
                    chunk
                        .sections()
                        .iter()
                        .map(|s| s.biomes().clone())
                        .collect(),
                );
            }
        }
        Ok(Self {
            center,
            min_section_y,
            chunks,
        })
    }

    /// `collectPossibleBiomes(region, 1)`: every biome in the 3 x 3 chunks' palettes.
    fn possible_biomes(&self) -> BiomeSet {
        let mut biomes = Vec::new();
        for chunk in &self.chunks {
            for section in chunk {
                section.for_each_in_palette(|b| biomes.push(b));
            }
        }
        BiomeSet::new(biomes)
    }

    /// `getNoiseBiome` of the chunk holding the quart (height clamped).
    fn biome(&self, quart: [i32; 3]) -> u32 {
        let dx = (quart[0] >> 2) - self.center.x + 1;
        let dz = (quart[2] >> 2) - self.center.z + 1;
        let chunk = &self.chunks[(dz.clamp(0, 2) * 3 + dx.clamp(0, 2)) as usize];
        let min_quart = self.min_section_y << 2;
        let max_quart = min_quart + chunk.len() as i32 * 4 - 1;
        let y = quart[1].clamp(min_quart, max_quart);
        let section = &chunk[((y >> 2) - self.min_section_y) as usize];
        section.get_xyz((quart[0] & 3) as u32, (y & 3) as u32, (quart[2] & 3) as u32)
    }
}

/// `PlayerSpawnFinder.getLevelRespawnPos` reduced to a proto/level chunk's data: the column's
/// spawn block, as `setInitialSpawn` asks for each column of a chunk. The heights are the
/// column's top (`MOTION_BLOCKING`, or the spawn height under a ceiling), `WORLD_SURFACE` and
/// `OCEAN_FLOOR` heights and the level's minimum y.
pub fn respawn_pos_in_column(
    [top, surface, ocean_floor, min_y]: [i32; 4],
    state_at: impl Fn(i32) -> BlockState,
    is_fluid: impl Fn(BlockState) -> bool,
    is_full_top_face: impl Fn(BlockState) -> bool,
    (x, z): (i32, i32),
) -> Option<BlockPos> {
    if top < min_y {
        return None;
    }
    if surface <= top && surface > ocean_floor {
        return None;
    }
    let mut y = top + 1;
    while y >= min_y {
        let state = state_at(y);
        if is_fluid(state) {
            break;
        }
        if is_full_top_face(state) {
            return Some(BlockPos::new(x, y + 1, z));
        }
        y -= 1;
    }
    None
}
