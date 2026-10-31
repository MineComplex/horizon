//! The surface of the TERRAIN step (`NoiseBasedChunkGenerator.buildSurface`, 26.3's
//! `MaterialSystem`/`MaterialRuleContext` with Paper's optionally flat bedrock): the noise settings'
//! material rule decides each block of a filled chunk from the biome, the depth below the surface
//! and the surface noises, column by column, with the eroded badlands' pillars and the frozen
//! ocean's icebergs. Carvers are not here.
//!
//! - [`rules`]: the decoded `worldgen/material_rule` and `material_condition` registries.
//! - [`temperature`]: a biome's temperature, for the temperature condition and the icebergs.
//!
//! Copied from Horizon's `horizon-world::worldgen::{material_stage, material_surface,
//! material_extensions, material_condition}` and rewritten over the server's own pieces: rules are
//! arenas of enums, the seeded noises and random factories belong to the level's
//! [`SurfaceSystem`], and biomes come from a closure (the BIOMES step's result and
//! `BiomeManager`'s zoom, which the caller owns). Vanilla's per-column/per-block condition caches
//! only skip repeated work and are kept for the expensive inputs (biome, noises, secondary
//! depth, preliminary surface); the conditions themselves are recomputed.

mod context;
pub mod rules;
pub mod temperature;

#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod tests;

use context::RuleContext;
use rules::{MaterialRules, RuleId};
use temperature::BiomeClimates;

use super::anchor::WorldGenerationContext;
use super::data::{LevelDensity, WorldgenData};
use super::density::{DensityContext, NodeId};
use super::noise::{DensityVolume, NoiseBuildError, NoiseStack};
use super::proto_chunk::ProtoChunk;
use super::settings::{NoiseGeneratorSettings, RouterField};
use super::structure::BiomeSet;
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::position::BlockPos;
use crate::util::random::{
    LegacyPositionalRandom, LegacyRandom, PositionalRandom, RandomSource,
    XoroshiroPositionalRandom, XoroshiroRandom,
};
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{SectionFlags, StateFlags};
use crate::world::level_chunk::HeightmapFlags;

/// Why a level's surface could not be set up (a datapack naming what is not there).
#[derive(Debug, thiserror::Error)]
pub enum SurfaceError {
    #[error("the noise settings name the unknown material rule {0}")]
    UnknownRule(String),
    #[error("a material condition names the unknown noise {0}")]
    UnknownNoise(String),
    #[error("the block {0} is not registered")]
    UnknownBlock(&'static str),
    #[error(transparent)]
    Noise(#[from] NoiseBuildError),
}

/// A level's root positional random (`RandomState.random`) of either algorithm.
#[derive(Clone, Copy, Debug)]
enum SurfaceRandom {
    Legacy(LegacyPositionalRandom),
    Xoroshiro(XoroshiroPositionalRandom),
}

impl SurfaceRandom {
    fn root(seed: i64, legacy: bool) -> Self {
        if legacy {
            Self::Legacy(LegacyRandom::new(seed).fork_positional())
        } else {
            Self::Xoroshiro(XoroshiroRandom::new(seed).fork_positional())
        }
    }
    /// `getOrCreateRandomFactory(name)`: `fromHashOf(name).forkPositional()`.
    fn factory(&self, name: &str) -> Self {
        match self {
            Self::Legacy(root) => Self::Legacy(root.from_hash_of(name).fork_positional()),
            Self::Xoroshiro(root) => Self::Xoroshiro(root.from_hash_of(name).fork_positional()),
        }
    }
    /// `at(x, 0, z).nextDouble()`.
    fn double_at(&self, x: i32, z: i32) -> f64 {
        let pos = BlockPos::new(x, 0, z);
        match self {
            Self::Legacy(root) => root.at(pos).next_double(),
            Self::Xoroshiro(root) => root.at(pos).next_double(),
        }
    }
}

/// The blocks the extensions name.
#[derive(Clone, Copy, Debug)]
struct SurfaceBlocks {
    water: BlockId,
    snow_block: BlockState,
    packed_ice: BlockState,
}

/// The fixed surface noises (`Noises.SURFACE`, the clay bands' and the badlands' and icebergs').
#[derive(Debug)]
struct ExtensionNoises {
    clay_offset: NoiseStack,
    surface: NoiseStack,
    secondary: NoiseStack,
    badlands_pillar: NoiseStack,
    badlands_roof: NoiseStack,
    badlands_surface: NoiseStack,
    iceberg_pillar: NoiseStack,
    iceberg_roof: NoiseStack,
    iceberg_surface: NoiseStack,
}

/// A level's surface: the rule of its noise settings with the seeded noises, random factories and
/// clay bands (`MaterialSystem`). Immutable; every worker builds chunks with its own context.
#[derive(Debug)]
pub struct SurfaceSystem<'a> {
    rules: &'a MaterialRules,
    climates: &'a BiomeClimates,
    states: &'a BlockStates,
    settings: &'a NoiseGeneratorSettings,
    level: &'a LevelDensity,
    root: RuleId,
    random: SurfaceRandom,
    /// `rules.noise_names()`, seeded.
    rule_noises: Vec<NoiseStack>,
    /// `rules.random_names()`, as factories.
    rule_randoms: Vec<SurfaceRandom>,
    ore_random: SurfaceRandom,
    preliminary_surface: NodeId,
    bands: [BlockState; 192],
    noises: ExtensionNoises,
    blocks: SurfaceBlocks,
    flat_bedrock: bool,
}

impl<'a> SurfaceSystem<'a> {
    /// `RandomState`'s surface system for `seed` and the settings' rule. `flat_bedrock` is Paper's
    /// `environment.generate-flat-bedrock` of the level's world.
    pub fn new(
        data: &'a WorldgenData,
        level: &'a LevelDensity,
        settings: &'a NoiseGeneratorSettings,
        states: &'a BlockStates,
        seed: i64,
        flat_bedrock: bool,
    ) -> Result<Self, SurfaceError> {
        let rules = data.surface_rules();
        let root = rules
            .element(&settings.material_rule)
            .ok_or_else(|| SurfaceError::UnknownRule(settings.material_rule.clone()))?;
        let random = SurfaceRandom::root(seed, settings.legacy_random_source);
        // `getOrCreateNoise`: the named noise seeded from `fromHashOf(name)`.
        let noise = |name: &str| -> Result<NoiseStack, SurfaceError> {
            let definition = data
                .graph()
                .noise_definition(name)
                .ok_or_else(|| SurfaceError::UnknownNoise(name.to_owned()))?;
            Ok(match random {
                SurfaceRandom::Legacy(root) => definition.create(&mut root.from_hash_of(name))?,
                SurfaceRandom::Xoroshiro(root) => {
                    definition.create(&mut root.from_hash_of(name))?
                }
            })
        };
        let state = |name: &'static str| {
            states
                .block_by_name(name)
                .map(|block| states.default_state(block))
                .ok_or(SurfaceError::UnknownBlock(name))
        };
        let colors = ClayColors {
            terracotta: state("minecraft:terracotta")?,
            orange: state("minecraft:orange_terracotta")?,
            yellow: state("minecraft:yellow_terracotta")?,
            brown: state("minecraft:brown_terracotta")?,
            red: state("minecraft:red_terracotta")?,
            white: state("minecraft:white_terracotta")?,
            light_gray: state("minecraft:light_gray_terracotta")?,
        };
        let bands = match random {
            SurfaceRandom::Legacy(root) => {
                clay_bands(&mut root.from_hash_of("minecraft:clay_bands"), colors)
            }
            SurfaceRandom::Xoroshiro(root) => {
                clay_bands(&mut root.from_hash_of("minecraft:clay_bands"), colors)
            }
        };
        Ok(Self {
            rules,
            climates: data.climates(),
            states,
            settings,
            level,
            root,
            random,
            rule_noises: rules
                .noise_names()
                .iter()
                .map(|name| noise(name))
                .collect::<Result<_, _>>()?,
            rule_randoms: rules
                .random_names()
                .iter()
                .map(|name| random.factory(name))
                .collect(),
            ore_random: random.factory("minecraft:ore"),
            preliminary_surface: level.router(RouterField::ChunkSurfaceLevel),
            bands,
            noises: ExtensionNoises {
                clay_offset: noise("minecraft:clay_bands_offset")?,
                surface: noise("minecraft:surface")?,
                secondary: noise("minecraft:surface_secondary")?,
                badlands_pillar: noise("minecraft:badlands_pillar")?,
                badlands_roof: noise("minecraft:badlands_pillar_roof")?,
                badlands_surface: noise("minecraft:badlands_surface")?,
                iceberg_pillar: noise("minecraft:iceberg_pillar")?,
                iceberg_roof: noise("minecraft:iceberg_pillar_roof")?,
                iceberg_surface: noise("minecraft:iceberg_surface")?,
            },
            blocks: SurfaceBlocks {
                water: states
                    .block_by_name("minecraft:water")
                    .ok_or(SurfaceError::UnknownBlock("minecraft:water"))?,
                snow_block: state("minecraft:snow_block")?,
                packed_ice: state("minecraft:packed_ice")?,
            },
            flat_bedrock,
        })
    }

    /// `MaterialSystem.getSurfaceDepth`.
    fn surface_depth(&self, x: i32, z: i32) -> i32 {
        let noise = f64::from(self.noises.surface.get3(f64::from(x), 0.0, f64::from(z)));
        (noise * 2.75 + 3.0 + self.random.double_at(x, z) * 0.25) as i32
    }

    /// `MaterialSystem.getBand`: `Math.round` of a float, then the remainder (Java's would throw
    /// for a negative index, which heights above `-192` never make).
    fn band(&self, x: i32, z: i32, y: i32) -> BlockState {
        let noise = self
            .noises
            .clay_offset
            .get3(f64::from(x), 0.0, f64::from(z));
        let offset = (f64::from(noise * 4.0) + 0.5).floor() as i32;
        self.bands[y.wrapping_add(offset).wrapping_add(192).rem_euclid(192) as usize]
    }

    /// `buildSurface`: the rule applied to every non-fluid, non-air block of the filled `chunk`,
    /// top down, column by column, with the eroded badlands' and frozen oceans' extensions around
    /// it. `density` is the context the chunk's fill used (its caches stay valid), `biome_at` is
    /// `BiomeManager.getBiome` for a block, `possible_biomes` the biomes the chunk may hold
    /// (biome conditions that can never or always match are decided up front; `None` tests each).
    pub fn build_surface(
        &self,
        chunk: &mut ProtoChunk,
        density: &mut DensityContext,
        flags: &impl SectionFlags,
        heightmap_flags: &impl HeightmapFlags,
        biome_at: impl FnMut(BlockPos) -> u32,
        possible_biomes: Option<&BiomeSet>,
    ) {
        let noise = self
            .settings
            .noise
            .clamp_to_height(chunk.min_y(), chunk.height());
        if noise.height <= 0 {
            return;
        }
        let pos = chunk.pos();
        let (chunk_x, chunk_z) = (pos.x.wrapping_shl(4), pos.z.wrapping_shl(4));
        let (min_y, max_y) = (chunk.min_y(), chunk.min_y() + chunk.height() - 1);
        let top_section = chunk
            .sections()
            .iter()
            .rposition(|section| !section.has_only_air());
        let max_block_y = top_section.map_or(min_y - 1, |i| min_y + i as i32 * 16 + 15);
        let expected = DensityVolume::new(
            [16, (max_block_y - noise.min_y + 1).max(1), 16],
            [chunk_x, noise.min_y, chunk_z],
            [1; 3],
        )
        .expect("a chunk's volume is valid");
        let generation = WorldGenerationContext::with_generator(
            chunk.min_y(),
            chunk.height(),
            self.settings.noise.min_y,
            self.settings.noise.height,
            self.settings.sea_level,
        );
        let mut context = RuleContext::new(
            self,
            density,
            expected,
            generation,
            possible_biomes,
            biome_at,
        );
        let height = |chunk: &mut ProtoChunk, x: i32, z: i32| {
            if chunk.heightmap(Heightmap::WorldSurfaceWg).is_none() {
                chunk.prime_heightmaps(&[Heightmap::WorldSurfaceWg], heightmap_flags);
            }
            chunk
                .height_at(Heightmap::WorldSurfaceWg, x, z)
                .expect("primed")
        };
        for lx in 0..16 {
            for lz in 0..16 {
                let (x, z) = (chunk_x.wrapping_add(lx), chunk_z.wrapping_add(lz));
                let starting = height(chunk, lx, lz) + 1;
                let surface_biome = context.biome_at(BlockPos::new(x, starting, z));
                let mut column = Column {
                    chunk: &mut *chunk,
                    flags,
                    heightmap_flags,
                    x,
                    z,
                    min_y,
                    max_y,
                };
                if Some(surface_biome) == self.rules.eroded_badlands {
                    self.eroded_badlands(&mut column, starting);
                }
                let top = height(column.chunk, lx, lz) + 1;
                let gradient_x = height(column.chunk, (lx + 1).min(15), lz).wrapping_sub(height(
                    column.chunk,
                    (lx - 1).max(0),
                    lz,
                ));
                let gradient_z = height(column.chunk, lx, (lz + 1).min(15)).wrapping_sub(height(
                    column.chunk,
                    lx,
                    (lz - 1).max(0),
                ));
                context.update_xz(x, z, [gradient_x, gradient_z]);
                let (mut stone_above, mut water_height, mut ceiling) = (0i32, i32::MIN, i32::MAX);
                for y in (min_y..=top).rev() {
                    let old = column.block(y);
                    if column.is_air(old) {
                        stone_above = 0;
                        water_height = i32::MIN;
                    } else if column.has_fluid(old) {
                        if water_height == i32::MIN {
                            water_height = y + 1;
                        }
                    } else {
                        if ceiling >= y {
                            // `DimensionType.WAY_BELOW_MIN_Y`.
                            ceiling = -32512;
                            for below in (min_y - 1..y).rev() {
                                let state = column.block(below);
                                if column.is_air(state) || column.has_fluid(state) {
                                    ceiling = below + 1;
                                    break;
                                }
                            }
                        }
                        stone_above += 1;
                        context.update_y(
                            stone_above,
                            y.wrapping_sub(ceiling).wrapping_add(1),
                            water_height,
                            y,
                        );
                        if y >= min_y
                            && y <= max_y
                            && let Some(state) = context.apply(self.root)
                        {
                            column.set_block(y, state);
                        }
                    }
                }
                if self.rules.frozen_oceans.contains(&Some(surface_biome)) {
                    let minimum = context.min_surface_level();
                    self.frozen_ocean(&mut column, surface_biome, minimum, starting);
                }
            }
        }
    }

    /// `erodedBadlandsExtension`: pillars of the default block up from the ground, unless the
    /// column is under water.
    fn eroded_badlands<F: SectionFlags, H: HeightmapFlags>(
        &self,
        column: &mut Column<'_, F, H>,
        height: i32,
    ) {
        let (x, z) = (f64::from(column.x), f64::from(column.z));
        let noises = &self.noises;
        let buffer = (f64::from(noises.badlands_surface.get3(x, 0.0, z)) * 8.25)
            .abs()
            .min(f64::from(
                noises.badlands_pillar.get3(x * 0.2, 0.0, z * 0.2) * 15.0f32,
            ));
        if buffer <= 0.0 {
            return;
        }
        let floor = (f64::from(noises.badlands_roof.get3(x * 0.75, 0.0, z * 0.75)) * 1.5).abs();
        let top = 64.0 + (buffer * buffer * 2.5).min((floor * 50.0).ceil() + 24.0);
        let start = top.floor() as i32;
        if height > start {
            return;
        }
        let default = self.settings.default_block;
        for y in (column.min_y..=start).rev() {
            let block = self.states.block(column.block(y));
            if block == self.states.block(default) {
                break;
            }
            if block == self.blocks.water {
                return;
            }
        }
        for y in (column.min_y..=start).rev() {
            if !column.is_air(column.block(y)) {
                break;
            }
            column.set_block(y, default);
        }
    }

    /// `frozenOceanExtension`: icebergs of packed ice (snow on top) over the sea.
    fn frozen_ocean<F: SectionFlags, H: HeightmapFlags>(
        &self,
        column: &mut Column<'_, F, H>,
        biome: u32,
        minimum_surface: i32,
        height: i32,
    ) {
        let (x, z) = (f64::from(column.x), f64::from(column.z));
        let sea = self.settings.sea_level;
        let noises = &self.noises;
        let iceberg = (f64::from(noises.iceberg_surface.get3(x, 0.0, z)) * 8.25)
            .abs()
            .min(f64::from(
                noises.iceberg_pillar.get3(x * 1.28, 0.0, z * 1.28) * 15.0f32,
            ));
        if iceberg <= 1.8 {
            return;
        }
        let roof = (f64::from(noises.iceberg_roof.get3(x * 1.17, 0.0, z * 1.17)) * 1.5).abs();
        let mut top = (iceberg * iceberg * 1.2).min((roof * 40.0).ceil() + 14.0);
        if self
            .climates
            .melts_iceberg_slightly(biome, BlockPos::new(column.x, sea, column.z), sea)
        {
            top -= 2.0;
        }
        if top <= 2.0 {
            return;
        }
        let bottom = (f64::from(sea) - top - 7.0) as i32;
        let top = (top + f64::from(sea)) as i32;
        let at = BlockPos::new(column.x, 0, column.z);
        let iceberg = Iceberg {
            top,
            bottom,
            first: height.max(top.wrapping_add(1)),
            minimum_surface,
        };
        match self.random {
            SurfaceRandom::Legacy(root) => self.pack_iceberg(column, root.at(at), iceberg),
            SurfaceRandom::Xoroshiro(root) => self.pack_iceberg(column, root.at(at), iceberg),
        }
    }

    /// The iceberg's blocks, top down, from the column's own random.
    fn pack_iceberg<F: SectionFlags, H: HeightmapFlags>(
        &self,
        column: &mut Column<'_, F, H>,
        mut random: impl RandomSource,
        iceberg: Iceberg,
    ) {
        let sea = self.settings.sea_level;
        let max_snow_depth = 2 + random.next_int_bound(4).expect("a positive bound");
        let min_snow_height = sea + 18 + random.next_int_bound(10).expect("a positive bound");
        let mut snow_depth = 0;
        for y in (iceberg.minimum_surface..=iceberg.first).rev() {
            let old = column.block(y);
            let air = column.is_air(old) && y < iceberg.top && random.next_double() > 0.01;
            let water = !air
                && self.states.block(old) == self.blocks.water
                && y > iceberg.bottom
                && y < sea
                && random.next_double() > 0.15;
            if air || water {
                if snow_depth <= max_snow_depth && y > min_snow_height {
                    column.set_block(y, self.blocks.snow_block);
                    snow_depth += 1;
                } else {
                    column.set_block(y, self.blocks.packed_ice);
                }
            }
        }
    }
}

/// An iceberg's heights: its `(int)` top and bottom, where its blocks start and the lowest one.
#[derive(Clone, Copy)]
struct Iceberg {
    top: i32,
    bottom: i32,
    first: i32,
    minimum_surface: i32,
}

/// The block column of a chunk being built (`BlockColumn`): writes inside the world's height
/// only, fluids marked for post-processing.
struct Column<'c, F, H> {
    chunk: &'c mut ProtoChunk,
    flags: &'c F,
    heightmap_flags: &'c H,
    x: i32,
    z: i32,
    min_y: i32,
    max_y: i32,
}

impl<F: SectionFlags, H: HeightmapFlags> Column<'_, F, H> {
    fn block(&self, y: i32) -> BlockState {
        self.chunk.block_state(self.x, y, self.z)
    }
    fn is_air(&self, state: BlockState) -> bool {
        ProtoChunk::is_air(state, self.flags)
    }
    fn has_fluid(&self, state: BlockState) -> bool {
        self.flags.section_flags(state).has(StateFlags::HAS_FLUID)
    }
    fn set_block(&mut self, y: i32, state: BlockState) {
        if y >= self.min_y && y <= self.max_y {
            self.chunk
                .set_block_state(self.x, y, self.z, state, self.flags, self.heightmap_flags);
            if self.has_fluid(state) {
                self.chunk.mark_postprocessing(self.x, y, self.z);
            }
        }
    }
}

/// The clay bands' seven blocks.
#[derive(Clone, Copy)]
struct ClayColors {
    terracotta: BlockState,
    orange: BlockState,
    yellow: BlockState,
    brown: BlockState,
    red: BlockState,
    white: BlockState,
    light_gray: BlockState,
}

/// `generateBands`: 192 layers of terracotta, banded with the colours in vanilla's draw order.
fn clay_bands(random: &mut impl RandomSource, colors: ClayColors) -> [BlockState; 192] {
    let mut bands = [colors.terracotta; 192];
    let mut i = 0;
    while i < bands.len() {
        i += random.next_int_bound(5).expect("a positive bound") as usize + 1;
        if i < bands.len() {
            bands[i] = colors.orange;
        }
        i += 1;
    }
    make_bands(random, &mut bands, 1, colors.yellow);
    make_bands(random, &mut bands, 2, colors.brown);
    make_bands(random, &mut bands, 1, colors.red);
    let white_count = random.next_int_between_inclusive(9, 15).expect("a range");
    let (mut count, mut start) = (0, 0usize);
    while count < white_count && start < bands.len() {
        bands[start] = colors.white;
        if start > 1 && random.next_boolean() {
            bands[start - 1] = colors.light_gray;
        }
        if start + 1 < bands.len() && random.next_boolean() {
            bands[start + 1] = colors.light_gray;
        }
        count += 1;
        start += random.next_int_bound(16).expect("a positive bound") as usize + 4;
    }
    bands
}

fn make_bands(
    random: &mut impl RandomSource,
    bands: &mut [BlockState; 192],
    base_width: usize,
    state: BlockState,
) {
    let count = random.next_int_between_inclusive(6, 15).expect("a range");
    for _ in 0..count {
        let width = base_width + random.next_int_bound(3).expect("a positive bound") as usize;
        let start = random.next_int_bound(192).expect("a positive bound") as usize;
        let end = (start + width).min(bands.len());
        bands[start..end].fill(state);
    }
}
