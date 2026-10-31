//! The generators with a fixed biome and no noise: 26.3's `FlatLevelSource` (with its
//! `FlatLevelGeneratorSettings`) and `DebugLevelSource`, over `FixedBiomeSource`.
//!
//! Both run the same status steps as the noise generator ([`super::GenerationWorker::run`]):
//! STRUCTURE_STARTS and STRUCTURE_REFERENCES over the same placements (the flat generator's
//! structure overrides and its ring seed of 0 are `createForFlat`), BIOMES (one biome),
//! TERRAIN (the flat layers; nothing for the debug generator; the final heightmaps primed), and
//! FEATURES (`applyBiomeDecoration`: flat's [`FlatLevelGeneratorSettings.adjustGenerationSettings`]
//! feature lists, with its fill-layer features; debug's grid of block states).
//!
//! Structures: the jigsaw and the template structures start through the structure port's own
//! code over [`FixedColumns`] (the layers' heights, `getBaseHeight`/`getBaseColumn`); of the
//! piece-based ones only strongholds and mineshafts are made here (the flat preset's
//! `structure_overrides` has the strongholds; the rest of `legacy_structures` reads noise columns).
//!
//! Stand-ins: the flat layers are the ones `adjustGenerationSettings` leaves (non-opaque layers
//! become fill-layer features) from the first chunk on, where vanilla changes them lazily on the
//! first call; features of types not ported are skipped as the noise generator's are.
//!
//! [`FlatLevelGeneratorSettings.adjustGenerationSettings`]: super::flat_settings

use std::collections::HashSet;

use super::convert::ChunkShape;
use super::flat_settings::FlatSettings;
use super::{FINAL_HEIGHTMAPS, GeneratorError, GeneratorTables, StepError, structures};
use crate::block::BlockState;
use crate::core::position::{BlockPos, ChunkPos};
use crate::server::config::SpigotWorldSeeds;
use crate::util::random::{LegacyRandom, RandomSource, WorldgenRandom, XoroshiroRandom};
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{ChunkStatus, ContainerFactory};
use crate::world::level_chunk::HeightmapFlags;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::biome::zoom::{BiomeZoom, obfuscate_seed};
use crate::worldgen::feature::decorate::{DECORATION_STEPS, StructureDecorator};
use crate::worldgen::feature::sort::{StepFeatureData, build_features_per_step};
use crate::worldgen::feature::{FeatureRegistry, FeatureWorld, PlacedFeatureId};
use crate::worldgen::placement::RegionWorld;
use crate::worldgen::placement::survival::BlockSurvivalRules;
use crate::worldgen::placement::world::RegionRandom;
use crate::worldgen::proto_chunk::{GenerationRegion, ProtoChunk};
use crate::worldgen::structure::jigsaw::{
    GenerationContext, JigsawTables, JigsawWorld, generate as jigsaw_generate,
};
use crate::worldgen::structure::legacy_piece::LegacyPiece;
use crate::worldgen::structure::piece_place::{PieceTables, StartPlacer, writable_area};
use crate::worldgen::structure::placement::{LevelParams, LevelPlacements, ring_searches};
use crate::worldgen::structure::start::{StructurePiece, StructureStart};
use crate::worldgen::structure::template_start::{
    TemplateGen, TemplateGenWorld, generate as template_generate, is_template_structure,
};
use crate::worldgen::structure::{
    BiomeSet, Placement, StructureId, StructureSetId, mineshaft, stronghold,
};

/// `FlatLevelSource.getMinY` and `DebugLevelSource.getMinY`.
const GEN_MIN_Y: i32 = 0;
/// `getGenDepth` of both.
const GEN_DEPTH: i32 = 384;
/// `GenerationStep.Decoration.LAKES`, `UNDERGROUND_STRUCTURES`, `SURFACE_STRUCTURES` and
/// `TOP_LAYER_MODIFICATION`.
const LAKES: usize = 1;
const UNDERGROUND_STRUCTURES: usize = 3;
const SURFACE_STRUCTURES: usize = 4;
const TOP_LAYER_MODIFICATION: usize = 10;

/// `DebugLevelSource.HEIGHT` and `BARRIER_HEIGHT`.
const DEBUG_HEIGHT: i32 = 70;
const DEBUG_BARRIER_HEIGHT: i32 = 60;

/// Which fixed generator.
#[derive(Clone, Debug)]
pub enum FixedKind {
    Flat(FlatSettings),
    Debug,
}

/// What one level's fixed generator is made from.
pub struct FixedGeneratorSettings {
    pub kind: FixedKind,
    /// The generator's biome: the flat settings' (the same id), or the debug source's plains.
    pub biome: u32,
    /// Whether that biome is `minecraft:the_void` (flat features stay on in an air-only world).
    pub biome_is_void: bool,
    pub seed: i64,
    pub generate_structures: bool,
    pub spigot_seeds: SpigotWorldSeeds,
    pub min_section_y: i32,
    pub section_count: usize,
    pub factory: ContainerFactory,
}

/// One feature of a decoration step as the generator places it.
#[derive(Clone, Copy, Debug)]
enum StepItem {
    /// A placed feature and its index in the step's sorted list (its feature seed).
    Placed(PlacedFeatureId, i32),
    /// `new FillLayerFeature(layer, state)`, inline-placed in `TOP_LAYER_MODIFICATION`.
    FillLayer(i32, BlockState),
}

/// A flat or debug level's generator. Immutable.
pub struct FixedGenerator {
    tables: GeneratorTables,
    kind: FixedKind,
    biome: u32,
    seed: i64,
    generate_structures: bool,
    zoom: BiomeZoom,
    placements: LevelPlacements,
    min_section_y: i32,
    section_count: usize,
    factory: ContainerFactory,
    void_air: BlockState,
    barrier: BlockState,
    /// The flat layers `adjustGenerationSettings` leaves: `None` for the non-opaque ones, which
    /// the fill-layer features place instead.
    layers: Vec<Option<BlockState>>,
    /// The number of layers (`getLayers().size()`).
    layer_count: usize,
    /// Per decoration step, what is placed (flat only).
    steps: Vec<Vec<StepItem>>,
    unported: HashSet<PlacedFeatureId>,
    /// `DebugLevelSource`'s grid.
    grid: DebugGrid,
}

impl std::fmt::Debug for FixedGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FixedGenerator")
            .field("kind", &self.kind)
            .field("seed", &self.seed)
            .finish_non_exhaustive()
    }
}

impl FixedGenerator {
    /// `new FlatLevelSource(settings)` or `new DebugLevelSource(plains)` with their
    /// `createState` (`createForFlat` / `createForNormal`).
    pub fn new(
        tables: GeneratorTables,
        level: FixedGeneratorSettings,
    ) -> Result<Self, GeneratorError> {
        let blocks = &tables.blocks;
        let state_of = |name: &str| {
            blocks
                .block_by_name(name)
                .map(|b| blocks.default_state(b))
                .ok_or(GeneratorError::Blocks)
        };
        let void_air = state_of("minecraft:void_air")?;
        let barrier = state_of("minecraft:barrier")?;
        let biome = level.biome;
        let possible_biomes = BiomeSet::new(vec![biome]);
        let features = tables.data.features();
        let registry = tables.data.structures();

        // The flat layers and the feature lists of the generation settings.
        let (layers, layer_count, steps) = match &level.kind {
            FixedKind::Flat(settings) => {
                let all = settings.layer_states();
                let air = state_of("minecraft:air")?;
                let void_gen = all.iter().all(|&s| s == air);
                let layers: Vec<Option<BlockState>> = all
                    .iter()
                    .map(|&state| {
                        let opaque = tables.heightmaps.heightmap_mask(state)
                            >> Heightmap::MotionBlocking as u8
                            & 1
                            != 0;
                        opaque.then_some(state)
                    })
                    .collect();
                let steps = flat_steps(settings, void_gen, &layers, features, level.biome_is_void)?;
                (layers, all.len(), steps)
            }
            FixedKind::Debug => (Vec::new(), 0, Vec::new()),
        };
        let unported = {
            let placed: Vec<Vec<PlacedFeatureId>> = steps
                .iter()
                .map(|step| {
                    step.iter()
                        .filter_map(|i| match i {
                            StepItem::Placed(f, _) => Some(*f),
                            StepItem::FillLayer(..) => None,
                        })
                        .collect()
                })
                .collect();
            let data = build_features_per_step(&[placed]).map_err(|cycle| {
                GeneratorError::FeatureCycle {
                    cycle,
                    sources: vec![biome],
                }
            })?;
            crate::worldgen::feature::decorate::unported_features(features, &data)
        };

        // `createForFlat` (the overrides, a ring seed of 0) or `createForNormal`.
        let (sets, ring_seed): (Vec<StructureSetId>, i64) = match &level.kind {
            FixedKind::Flat(settings) => (
                settings.structure_overrides.clone().unwrap_or_else(|| {
                    (registry.sets.iter().enumerate())
                        .filter(|(_, set)| !set.inline)
                        .map(|(i, _)| StructureSetId(i as u32))
                        .collect()
                }),
                0,
            ),
            FixedKind::Debug => (
                (registry.sets.iter().enumerate())
                    .filter(|(_, set)| !set.inline)
                    .map(|(i, _)| StructureSetId(i as u32))
                    .collect(),
                level.seed,
            ),
        };
        let params = LevelParams {
            seed: level.seed,
            ring_seed,
            // `ChunkGenerator.getOrigin`.
            origin: ChunkPos::new(0, 0),
            sets: &sets,
            possible_biomes: &possible_biomes,
            seeds: &level.spigot_seeds,
        };
        let ring_positions: Vec<ChunkPos> = ring_searches(registry, &params)
            .iter()
            .map(|search| {
                let Placement::ConcentricRings(rings) = &registry.set(search.set).placement else {
                    return search.chunk;
                };
                // `FixedBiomeSource.findBiomeHorizontal`: a random block of the square.
                if !rings.preferred_biomes.contains(biome) {
                    return search.result(None);
                }
                let mut random = search.random.clone();
                let (x, z) = search.center();
                let dx = random.next_int_bound(112 * 2 + 1).unwrap_or(0);
                let dz = random.next_int_bound(112 * 2 + 1).unwrap_or(0);
                search.result(Some((x - 112 + dx, z - 112 + dz)))
            })
            .collect();
        let placements = LevelPlacements::new(registry, &params, &ring_positions);
        let grid = DebugGrid::new(tables.blocks.state_count());
        Ok(Self {
            kind: level.kind,
            biome,
            seed: level.seed,
            generate_structures: level.generate_structures,
            zoom: BiomeZoom::new(obfuscate_seed(level.seed)),
            placements,
            min_section_y: level.min_section_y,
            section_count: level.section_count,
            factory: level.factory,
            void_air,
            barrier,
            layers,
            layer_count,
            steps,
            unported,
            grid,
            tables,
        })
    }

    pub fn is_flat(&self) -> bool {
        matches!(self.kind, FixedKind::Flat(_))
    }
    pub fn kind(&self) -> &FixedKind {
        &self.kind
    }
    pub fn seed(&self) -> i64 {
        self.seed
    }
    pub fn generate_structures(&self) -> bool {
        self.generate_structures
    }
    pub fn placements(&self) -> &LevelPlacements {
        &self.placements
    }
    /// `getSeaLevel`: -63 for flat, 63 for debug.
    pub fn sea_level(&self) -> i32 {
        match self.kind {
            FixedKind::Flat(_) => -63,
            FixedKind::Debug => 63,
        }
    }
    /// `getOrigin`: the default, chunk 0 0.
    pub fn origin(&self) -> ChunkPos {
        ChunkPos::new(0, 0)
    }
    /// `getSpawnHeight`: flat's is the layers' top, debug keeps the default 64.
    pub fn spawn_height(&self) -> i32 {
        match self.kind {
            FixedKind::Flat(_) => {
                let height = self.section_count as i32 * 16;
                self.min_section_y * 16 + height.min(self.layer_count as i32)
            }
            FixedKind::Debug => 64,
        }
    }
    pub fn shape(&self) -> ChunkShape<'_> {
        ChunkShape {
            min_section_y: self.min_section_y,
            section_count: self.section_count,
            factory: &self.factory,
            void_air: self.void_air,
        }
    }
    pub fn empty_chunk(&self, pos: ChunkPos) -> ProtoChunk {
        self.shape().empty(pos)
    }
    /// `DebugLevelSource.getBlockStateFor`.
    pub fn debug_block_state(&self, x: i32, z: i32) -> BlockState {
        self.grid.state_for(x, z)
    }
    /// The flat layers after `adjustGenerationSettings` (`None`: a fill-layer feature's).
    pub fn layers(&self) -> &[Option<BlockState>] {
        &self.layers
    }

    /// The generator's feature order as the tests compare it: per step, the placed features'
    /// ids and the fill layers as `fill:<layer>:<state id>`.
    pub fn plan_names(&self) -> Vec<Vec<String>> {
        let features = self.tables.data.features();
        self.steps
            .iter()
            .map(|step| {
                step.iter()
                    .map(|item| match *item {
                        StepItem::Placed(feature, _) => features.placed(feature).id.clone(),
                        StepItem::FillLayer(layer, state) => format!("fill:{layer}:{}", state.0),
                    })
                    .collect()
            })
            .collect()
    }
    /// `DebugLevelSource`'s grid: the number of states, `GRID_WIDTH` and `GRID_HEIGHT`.
    pub fn debug_grid_size(&self) -> (usize, i32, i32) {
        (self.grid.states, self.grid.width, self.grid.height)
    }
    /// `getBaseHeight` of the level `(min_y, height)`, as the structures read it.
    pub fn base_height_at(&self, heightmap: Heightmap, level: (i32, i32)) -> i32 {
        self.base_height(heightmap, level, &*self.tables.heightmaps)
    }
    /// `getBaseColumn`'s states (`None`: an empty column).
    pub fn base_column_of(&self, level: (i32, i32)) -> Option<(i32, Vec<BlockState>)> {
        self.base_column(level)
    }

    pub fn worker(&self) -> FixedWorker<'_> {
        FixedWorker { generator: self }
    }

    /// `getBaseHeight` over a level of `(min_y, height)`.
    fn base_height(
        &self,
        heightmap: Heightmap,
        (min_y, height): (i32, i32),
        flags: &dyn HeightmapFlags,
    ) -> i32 {
        match self.kind {
            FixedKind::Debug => 0,
            FixedKind::Flat(_) => {
                let max_y = min_y.wrapping_add(height).wrapping_sub(1);
                let top = (self.layers.len() as i32 - 1).min(max_y);
                for index in (0..=top).rev() {
                    if let Some(state) = self.layers[index as usize]
                        && flags.heightmap_mask(state) >> heightmap as u8 & 1 != 0
                    {
                        return min_y + index + 1;
                    }
                }
                min_y
            }
        }
    }

    /// `getBaseColumn`: the lowest y and the states from it up.
    fn base_column(&self, (min_y, height): (i32, i32)) -> Option<(i32, Vec<BlockState>)> {
        let air = BlockState::AIR;
        let states: Vec<BlockState> = match self.kind {
            FixedKind::Debug => return None,
            FixedKind::Flat(_) => (self.layers.iter())
                .take(height.max(0) as usize)
                .map(|s| s.unwrap_or(air))
                .collect(),
        };
        (!states.is_empty()).then_some((min_y, states))
    }

    /// `Structure.generate` for `structure` starting in `chunk`.
    fn generate_start(
        &self,
        structure: StructureId,
        chunk: ChunkPos,
        (min_y, height): (i32, i32),
        references: i32,
    ) -> Option<StructureStart> {
        let registry = self.tables.data.structures();
        let info = registry.structure(structure);
        let mut columns = FixedColumns {
            generator: self,
            heightmaps: &*self.tables.heightmaps,
            min_y,
            height,
            biomes: &info.biomes,
        };
        let generation = WorldGenerationContext::new(
            min_y.max(GEN_MIN_Y),
            height.min(GEN_DEPTH),
            self.sea_level(),
        );
        let mut random = WorldgenRandom::new(LegacyRandom::new(0));
        random.set_large_feature_seed(self.seed, chunk.x, chunk.z);
        if is_template_structure(&info.kind) {
            let mut cx = TemplateGen {
                registry,
                blocks: &self.tables.blocks,
                data: &self.tables.flags,
                climates: self.tables.data.climates(),
                world: &mut columns,
                generation,
                min_y,
                height,
                chunk,
            };
            let stub = match template_generate(info, &mut cx, &mut random) {
                Ok(stub) => stub?,
                Err(e) => {
                    tracing::warn!("Structure {} in chunk {chunk:?}: {e}", info.id);
                    return None;
                }
            };
            if !info.biomes.contains(self.biome) {
                return None;
            }
            let pieces = (stub.pieces.into_iter())
                .map(StructurePiece::Template)
                .collect();
            return Some(StructureStart::new(
                registry, structure, chunk, references, pieces,
            ));
        }
        if let Some(config) = info.jigsaw.as_ref() {
            let mut cx = GenerationContext {
                tables: JigsawTables {
                    pools: &registry.pools,
                    templates: &registry.templates,
                    blocks: &self.tables.blocks,
                },
                min_y,
                height,
                world: &mut columns,
                generation,
                seed: self.seed,
                chunk,
            };
            let biome = self.biome;
            let start = jigsaw_generate(config, &mut cx, &mut random, |world, _| {
                world.biomes.contains(biome)
            });
            let start = match start {
                Ok(start) => start?,
                Err(e) => {
                    tracing::warn!("Structure {} in chunk {chunk:?}: {e}", info.id);
                    return None;
                }
            };
            let pieces = (start.pieces.into_iter())
                .map(StructurePiece::Jigsaw)
                .collect();
            return Some(StructureStart::new(
                registry, structure, chunk, references, pieces,
            ));
        }
        // The piece-based structures that have no noise to read.
        let (min_x, min_z) = (chunk.x * 16, chunk.z * 16);
        let sea_level = self.sea_level();
        let valid_biome = info.biomes.contains(self.biome);
        let pieces: Vec<StructurePiece> = match info.kind.as_str() {
            "minecraft:stronghold" => {
                if !valid_biome {
                    return None;
                }
                stronghold::generate(&mut random, self.seed, [chunk.x, chunk.z], sea_level, min_y)
                    .into_iter()
                    .map(|p| StructurePiece::Legacy(LegacyPiece::Stronghold(p)))
                    .collect()
            }
            "minecraft:mineshaft" => {
                let mesa = serde_json::from_str::<serde_json::Value>(info.json.get())
                    .ok()
                    .and_then(|v| v.get("mineshaft_type")?.as_str().map(|t| t == "mesa"))
                    .unwrap_or(false);
                let (_, pieces) = mineshaft::generate(
                    &mut random,
                    [chunk.x, chunk.z],
                    mesa,
                    sea_level,
                    min_y,
                    &mut |x, z| columns.first_free_height(x, z, Heightmap::WorldSurfaceWg),
                );
                if !valid_biome {
                    return None;
                }
                pieces
                    .into_iter()
                    .map(|p| StructurePiece::Legacy(LegacyPiece::Mineshaft(p)))
                    .collect()
            }
            _ => {
                let _ = (min_x, min_z);
                return None;
            }
        };
        Some(StructureStart::new(
            registry, structure, chunk, references, pieces,
        ))
    }

    /// `createStructures` for the chunk (see [`structures::create_structures`]).
    fn create_structures(&self, chunk: &mut ProtoChunk) {
        let pos = chunk.pos();
        let (min_y, height) = (chunk.min_y(), chunk.height());
        let registry = self.tables.data.structures();
        let starts = &mut chunk.structures;
        let chosen: std::cell::RefCell<Vec<(StructureId, StructureStart)>> = Default::default();
        self.placements.create_starts(
            registry,
            pos,
            |structure| {
                chosen
                    .borrow()
                    .iter()
                    .any(|(s, start)| *s == structure && start.is_valid())
                    || starts
                        .start(structure)
                        .is_some_and(StructureStart::is_valid)
            },
            |structure| {
                let references = starts.start(structure).map_or(0, |s| s.references);
                match self.generate_start(structure, pos, (min_y, height), references) {
                    Some(start) if start.is_valid() => {
                        chosen.borrow_mut().push((structure, start));
                        true
                    }
                    _ => false,
                }
            },
        );
        for (structure, start) in chosen.into_inner() {
            starts.set_start(structure, start);
        }
    }
}

/// `FlatLevelGeneratorSettings.adjustGenerationSettings` for the flat biome, then
/// `FeatureSorter.buildFeaturesPerStep` over that one source: the placed features of each step in
/// the sorted order with their indices, then the fill-layer features after them.
fn flat_steps(
    settings: &FlatSettings,
    void_gen: bool,
    layers: &[Option<BlockState>],
    features: &FeatureRegistry,
    biome_is_void: bool,
) -> Result<Vec<Vec<StepItem>>, GeneratorError> {
    let mut lists: Vec<Vec<PlacedFeatureId>> = Vec::new();
    let mut add = |step: usize, feature: PlacedFeatureId| {
        while lists.len() <= step {
            lists.push(Vec::new());
        }
        lists[step].push(feature);
    };
    if settings.lakes {
        for name in [
            "minecraft:lake_lava_underground",
            "minecraft:lake_lava_surface",
        ] {
            match features.placed_id(name) {
                Some(id) => add(LAKES, id),
                None => tracing::warn!("The placed feature {name} is not registered"),
            }
        }
    }
    if (!void_gen || biome_is_void) && settings.features {
        for (step, list) in features.biome(settings.biome).steps.iter().enumerate() {
            if step != UNDERGROUND_STRUCTURES
                && step != SURFACE_STRUCTURES
                && (!settings.lakes || step != LAKES)
            {
                for &feature in list {
                    add(step, feature);
                }
            }
        }
    }
    let sorted = build_features_per_step(std::slice::from_ref(&lists)).map_err(|cycle| {
        GeneratorError::FeatureCycle {
            cycle,
            sources: vec![settings.biome],
        }
    })?;
    let mut steps: Vec<Vec<StepItem>> = sorted
        .iter()
        .map(|data: &StepFeatureData| {
            (data.features().iter().enumerate())
                .map(|(i, &f)| StepItem::Placed(f, i as i32))
                .collect()
        })
        .collect();
    let fills = (layers.iter().zip(settings.layer_states()))
        .enumerate()
        .filter(|(_, (kept, _))| kept.is_none())
        .map(|(i, (_, state))| StepItem::FillLayer(i as i32, state));
    for fill in fills {
        while steps.len() <= TOP_LAYER_MODIFICATION {
            steps.push(Vec::new());
        }
        steps[TOP_LAYER_MODIFICATION].push(fill);
    }
    Ok(steps)
}

/// `DebugLevelSource`'s `ALL_BLOCKS` grid: every block state in id order.
pub(super) struct DebugGrid {
    pub(super) states: usize,
    pub(super) width: i32,
    pub(super) height: i32,
}

impl DebugGrid {
    pub(super) fn new(states: u32) -> Self {
        // `Mth.ceil(Mth.sqrt(size))` and `Mth.ceil((float) size / width)`, in `float`.
        let width = ((states as f32) as f64).sqrt() as f32;
        let width = mth_ceil(width);
        let height = mth_ceil(states as f32 / width as f32);
        Self {
            states: states as usize,
            width,
            height,
        }
    }

    pub(super) fn state_for(&self, mut x: i32, mut z: i32) -> BlockState {
        if x > 0 && z > 0 && x % 2 != 0 && z % 2 != 0 {
            x /= 2;
            z /= 2;
            if x <= self.width && z <= self.height {
                let index = x.wrapping_mul(self.width).wrapping_add(z).wrapping_abs();
                if (index as usize) < self.states {
                    return BlockState(index as u32);
                }
            }
        }
        BlockState::AIR
    }
}

/// `Mth.ceil(float)`.
fn mth_ceil(value: f32) -> i32 {
    let truncated = value as i32;
    if value > truncated as f32 {
        truncated + 1
    } else {
        truncated
    }
}

/// What a structure's generation reads of the level: the layers' heights, the one biome.
struct FixedColumns<'a> {
    generator: &'a FixedGenerator,
    heightmaps: &'a dyn HeightmapFlags,
    min_y: i32,
    height: i32,
    /// The structure's biomes (`validBiome`).
    biomes: &'a BiomeSet,
}

impl JigsawWorld for FixedColumns<'_> {
    fn first_free_height(&mut self, _: i32, _: i32, heightmap: Heightmap) -> i32 {
        self.generator
            .base_height(heightmap, (self.min_y, self.height), self.heightmaps)
    }

    fn could_exist_in_column(&mut self, _: i32, _: i32, _: i32, _: i32) -> bool {
        self.biomes.contains(self.generator.biome)
    }
}

impl TemplateGenWorld for FixedColumns<'_> {
    fn base_column(&mut self, _: i32, _: i32) -> Option<(i32, Vec<BlockState>)> {
        self.generator.base_column((self.min_y, self.height))
    }

    fn biome_at(&mut self, _: BlockPos) -> u32 {
        self.generator.biome
    }

    fn is_opaque(&self, state: BlockState, heightmap: Heightmap) -> bool {
        self.heightmaps.heightmap_mask(state) >> heightmap as u8 & 1 != 0
    }
}

/// A worker for a fixed generator: it keeps no state.
pub struct FixedWorker<'g> {
    generator: &'g FixedGenerator,
}

impl<'g> FixedWorker<'g> {
    pub fn generator(&self) -> &'g FixedGenerator {
        self.generator
    }

    /// The step to `status` for the region's centre, then the chunk's status set (see
    /// [`super::GenerationWorker::run`]).
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
                    generator.create_structures(region.chunk_mut(center)?);
                }
            }
            ChunkStatus::StructureReferences => structures::create_references_in(region)?,
            ChunkStatus::Biomes => {
                let biome = generator.biome;
                region.chunk_mut(center)?.fill_biomes_from_noise(|_| biome);
            }
            ChunkStatus::Terrain => self.terrain(region)?,
            ChunkStatus::Features => match generator.kind {
                FixedKind::Flat(_) => self.decorate_flat(region)?,
                FixedKind::Debug => self.decorate_debug(region)?,
            },
            ChunkStatus::Full => return Err(StepError::NotAWorkerStep(status)),
        }
        let chunk = region.chunk_mut(center)?;
        if chunk.status() < status {
            chunk.set_status(status);
        }
        Ok(())
    }

    /// `FlatLevelSource.buildTerrain` (the layers; the world-gen heightmaps it updates are not
    /// kept), then `ChunkStatusTasks.buildTerrain`'s final heightmaps.
    fn terrain(&mut self, region: &mut GenerationRegion<'_>) -> Result<(), StepError> {
        let generator = self.generator;
        let chunk = region.chunk_mut(region.center())?;
        let min_y = chunk.min_y();
        let flags = &*generator.tables.flags;
        for (index, state) in generator
            .layers
            .iter()
            .enumerate()
            .take(chunk.height().max(0) as usize)
        {
            let Some(state) = *state else { continue };
            let y = min_y + index as i32;
            for x in 0..16 {
                for z in 0..16 {
                    chunk.set_block_raw(x, y, z, state, flags);
                }
            }
        }
        let heightmaps = &*generator.tables.heightmaps;
        if generator.is_flat() {
            // `getOrCreateHeightmapUnprimed(OCEAN_FLOOR_WG / WORLD_SURFACE_WG)` updated by the fill:
            // the layers' values, which the features' writes leave as they are (structures and
            // placements read them).
            chunk.prime_heightmaps(
                &[Heightmap::OceanFloorWg, Heightmap::WorldSurfaceWg],
                heightmaps,
            );
        }
        chunk.prime_heightmaps(&FINAL_HEIGHTMAPS, heightmaps);
        Ok(())
    }

    /// `DebugLevelSource.applyBiomeDecoration`: a barrier at y 60 and the grid's state at y 70 in
    /// every column.
    fn decorate_debug(&mut self, region: &mut GenerationRegion<'_>) -> Result<(), StepError> {
        let generator = self.generator;
        let center = region.center();
        let (flags, heightmaps) = (&*generator.tables.flags, &*generator.tables.heightmaps);
        for x in 0..16 {
            for z in 0..16 {
                let (wx, wz) = (center.x * 16 + x, center.z * 16 + z);
                let barrier = BlockPos::new(wx, DEBUG_BARRIER_HEIGHT, wz);
                region.set_block_state(barrier, generator.barrier, flags, heightmaps)?;
                let state = generator.grid.state_for(wx, wz);
                let at = BlockPos::new(wx, DEBUG_HEIGHT, wz);
                region.set_block_state(at, state, flags, heightmaps)?;
            }
        }
        Ok(())
    }

    /// `ChunkGenerator.applyBiomeDecoration` with the flat generator's feature lists (see
    /// [`super::decorate`]).
    fn decorate_flat(&mut self, region: &mut GenerationRegion<'_>) -> Result<(), StepError> {
        let generator = self.generator;
        let tables = &generator.tables;
        let (min_y, height) = (
            generator.min_section_y * 16,
            generator.section_count as i32 * 16,
        );
        let sea_level = generator.sea_level();
        let level = WorldGenerationContext::new(min_y, height, sea_level);
        let generator_context =
            WorldGenerationContext::new(min_y.max(GEN_MIN_Y), height.min(GEN_DEPTH), sea_level);
        let center = region.center();
        let data = &tables.data;
        let registry = data.features();
        let pieces = PieceTables {
            structures: data.structures(),
            processors: data.processor_lists(),
            features: registry,
            seed: generator.seed,
            known_shape: false,
        };
        let center_chunk = region
            .chunk(center)
            .ok_or(crate::worldgen::proto_chunk::OutsideRegion(center))?;
        let mut structures = StartPlacer::new(
            pieces,
            generator.generate_structures,
            center_chunk.structures.all_references(),
            |pos, structure| {
                region
                    .chunk(pos)
                    .and_then(|c| c.structures.start(structure))
            },
            writable_area(center, min_y, height),
        );
        let mut world = RegionWorld {
            region,
            heightmaps: &tables.heightmaps,
            blocks: &tables.blocks,
            data: &tables.flags,
            level,
            generator: generator_context,
            zoom: generator.zoom,
            survival: BlockSurvivalRules {
                rules: &tables.survival,
                blocks: &tables.blocks,
                data: &tables.flags,
            },
            seed: generator.seed,
            // `ChunkMap`: a generator without noise gets `RandomState.create(..., false, ...)` (not legacy).
            random: RegionRandom::new(generator.seed, false, center),
        };
        // `GenerationRegion.getChunk` of the 3 x 3: the final heightmaps primed where missing.
        for dz in -1..=1 {
            for dx in -1..=1 {
                let chunk = world
                    .region
                    .chunk_mut(ChunkPos::new(center.x + dx, center.z + dz))?;
                let missing: Vec<Heightmap> = FINAL_HEIGHTMAPS
                    .into_iter()
                    .filter(|&kind| chunk.heightmap(kind).is_none())
                    .collect();
                if !missing.is_empty() {
                    chunk.prime_heightmaps(&missing, &*tables.heightmaps);
                }
            }
        }
        let origin = BlockPos::new(center.x << 4, min_y, center.z << 4);
        let result = generator.place_steps(&mut world, &mut structures, registry, origin);
        // The pieces the starts' placement changed go back to the chunks holding the starts.
        region.defer_piece_changes(structures.finish());
        if let Err(message) = result {
            tracing::warn!(
                "Chunk [{}, {}]: {message}; its decoration stops there",
                center.x,
                center.z
            );
        }
        Ok(())
    }

    // (the loop itself is `FixedGenerator::place_steps`)
}

impl FixedGenerator {
    /// The loop of `addVanillaDecorations`: each step's structures, then its features (each with
    /// its own feature seed), flat's fill layers last in `TOP_LAYER_MODIFICATION`.
    fn place_steps<W: FeatureWorld>(
        &self,
        world: &mut W,
        structures: &mut impl StructureDecorator<W>,
        registry: &FeatureRegistry,
        origin: BlockPos,
    ) -> Result<(), String> {
        let mut random = WorldgenRandom::new(XoroshiroRandom::new(0));
        let decoration_seed = random.set_decoration_seed(self.seed, origin.x, origin.z);
        for step in 0..DECORATION_STEPS.max(self.steps.len()) {
            for structure in 0..structures.structures_in_step(step) {
                random.set_feature_seed(decoration_seed, structure as i32, step as i32);
                structures
                    .place(step, structure, world, &mut random)
                    .map_err(|e| format!("structure {structure} of step {step}: {e}"))?;
            }
            for item in self.steps.get(step).map_or(&[][..], Vec::as_slice) {
                match *item {
                    StepItem::Placed(feature, index) => {
                        random.set_feature_seed(decoration_seed, index, step as i32);
                        if self.unported.contains(&feature) {
                            continue;
                        }
                        registry
                            .place_placed_listed(feature, world, &mut random, origin)
                            .map_err(|e| format!("{}: {e}", registry.placed(feature).id))?;
                    }
                    StepItem::FillLayer(layer, state) => {
                        let y = world.level_generation().min_y + layer;
                        for dx in 0..16 {
                            for dz in 0..16 {
                                let pos = BlockPos::new(origin.x + dx, y, origin.z + dz);
                                let current = world.block_state(pos).map_err(|e| e.to_string())?;
                                if world.blocks().is_air(current) {
                                    world.set_block(pos, state, 2).map_err(|e| e.to_string())?;
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
