//! What placement asks of the world: heightmaps, blocks, biomes and the bounds. The traits are
//! narrow so features, which also write, can share one world with the modifiers that read it;
//! [`RegionWorld`] answers them from a [`GenerationRegion`], as a `WorldGenRegion` does.
//!
//! Paper's `PlacementContext` patch passes the level on to `WorldGenerationContext` only so that
//! flat bedrock can read its configuration there; no placement result depends on it, so it is not
//! carried here (the bedrock rules read it through `worldgen::anchor`).

use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::position::{BlockPos, ChunkPos};
use crate::util::random::{
    InvalidBound, LegacyRandom, PositionalRandom, RandomSource, XoroshiroRandom,
};
use crate::world::chunk::serial::Heightmap;
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::biome::zoom::BiomeZoom;
use crate::worldgen::proto_chunk::{GenerationRegion, OutsideRegion};

/// A query the world cannot answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WorldError {
    #[error(transparent)]
    Outside(#[from] OutsideRegion),
    #[error("the world has no {0}")]
    Unsupported(&'static str),
}

/// The world as placement and block predicates read it.
pub trait PlacementWorld {
    fn blocks(&self) -> &BlockStates;
    fn state_data(&self) -> &BlockStateData;
    /// The `PlacementContext`'s bounds: the level's and the generator's combined.
    fn generation(&self) -> WorldGenerationContext;
    /// `WorldGenerationContext.of(level)`: the level's own bounds, as block predicates use them.
    fn level_generation(&self) -> WorldGenerationContext;
    /// `WorldGenLevel.getHeight(type, x, z)`: the first free y above the heightmap's column.
    fn height(&self, kind: Heightmap, x: i32, z: i32) -> Result<i32, WorldError>;
    fn block_state(&self, pos: BlockPos) -> Result<BlockState, WorldError>;
    /// The biome (a `worldgen/biome` registry id) at a block position.
    fn biome(&self, pos: BlockPos) -> Result<u32, WorldError>;
    /// `BlockState.canSurvive(level, pos)` for a state that is not placed (`would_survive`).
    fn block_survives(&self, state: BlockState, pos: BlockPos) -> Result<bool, WorldError>;

    /// `LevelHeightAccessor.getMinY()`.
    fn min_y(&self) -> i32 {
        self.level_generation().min_y
    }
    /// `isOutsideBuildHeight(y)`.
    fn outside_build_height(&self, y: i32) -> bool {
        let level = self.level_generation();
        y < level.min_y || y > level.min_y.wrapping_add(level.height).wrapping_sub(1)
    }
}

/// `canSurvive`, which belongs to the block behaviour port: the world a region is wrapped in
/// supplies it.
pub trait SurvivalRules {
    fn can_survive(
        &self,
        state: BlockState,
        pos: BlockPos,
        region: &GenerationRegion<'_>,
    ) -> Result<bool, WorldError>;
}

/// No block behaviour yet: `would_survive` is an error rather than a guess.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSurvivalRules;

impl SurvivalRules for NoSurvivalRules {
    fn can_survive(
        &self,
        _: BlockState,
        _: BlockPos,
        _: &GenerationRegion<'_>,
    ) -> Result<bool, WorldError> {
        Err(WorldError::Unsupported("block survival rules"))
    }
}

/// A [`GenerationRegion`] with the facts placement reads from outside it.
pub struct RegionWorld<'a, 'r, S> {
    /// Writable: features set blocks through [`FeatureWorld`](crate::worldgen::feature::FeatureWorld).
    pub region: &'a mut GenerationRegion<'r>,
    pub heightmaps: &'a HeightmapTable,
    pub blocks: &'a BlockStates,
    pub data: &'a BlockStateData,
    pub level: WorldGenerationContext,
    pub generator: WorldGenerationContext,
    /// `BiomeManager`'s fuzzy zoom, which `Level.getBiome` reads through.
    pub zoom: BiomeZoom,
    pub survival: S,
    /// `WorldGenLevel.getSeed()`.
    pub seed: i64,
    /// `WorldGenRegion.getRandom()`.
    pub random: RegionRandom,
}

/// `WorldGenRegion.random`: `randomState.getOrCreateRandomFactory("minecraft:worldgen_region_random")
/// .at(center.getWorldPosition())`, one per region, drawn from by everything placed in it that asks
/// the level for its random (the desert pyramid's cellar, mossy carpets, a mansion's allays). The
/// random state's root is the noise settings' kind (`legacy_random_source`).
#[derive(Clone, Debug)]
pub enum RegionRandom {
    Legacy(LegacyRandom),
    Xoroshiro(XoroshiroRandom),
}

impl RegionRandom {
    pub fn new(seed: i64, legacy_random_source: bool, center: ChunkPos) -> Self {
        const NAME: &str = "minecraft:worldgen_region_random";
        let corner = BlockPos::new(center.x << 4, 0, center.z << 4);
        if legacy_random_source {
            let root = LegacyRandom::new(seed).fork_positional();
            Self::Legacy(root.from_hash_of(NAME).fork_positional().at(corner))
        } else {
            let root = XoroshiroRandom::new(seed).fork_positional();
            Self::Xoroshiro(root.from_hash_of(NAME).fork_positional().at(corner))
        }
    }
    pub fn next_boolean(&mut self) -> bool {
        match self {
            Self::Legacy(r) => r.next_boolean(),
            Self::Xoroshiro(r) => r.next_boolean(),
        }
    }
    pub fn next_float(&mut self) -> f32 {
        match self {
            Self::Legacy(r) => r.next_float(),
            Self::Xoroshiro(r) => r.next_float(),
        }
    }
    pub fn next_int_bound(&mut self, bound: i32) -> Result<i32, InvalidBound> {
        match self {
            Self::Legacy(r) => r.next_int_bound(bound),
            Self::Xoroshiro(r) => r.next_int_bound(bound),
        }
    }
    pub fn next_long(&mut self) -> i64 {
        match self {
            Self::Legacy(r) => r.next_long(),
            Self::Xoroshiro(r) => r.next_long(),
        }
    }
}

impl<S: SurvivalRules> PlacementWorld for RegionWorld<'_, '_, S> {
    fn blocks(&self) -> &BlockStates {
        self.blocks
    }
    fn state_data(&self) -> &BlockStateData {
        self.data
    }
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::with_generator(
            self.level.min_y,
            self.level.height,
            self.generator.min_y,
            self.generator.height,
            self.generator.sea_level,
        )
    }
    fn level_generation(&self) -> WorldGenerationContext {
        self.level
    }
    fn height(&self, kind: Heightmap, x: i32, z: i32) -> Result<i32, WorldError> {
        let chunk = self
            .region
            .chunk(BlockPos::new(x, 0, z).chunk())
            .ok_or(OutsideRegion(BlockPos::new(x, 0, z).chunk()))?;
        // `WorldGenRegion.getHeight` is the chunk's `getHeight` plus one.
        Ok(chunk.height_or_prime(kind, x & 15, z & 15, self.heightmaps) + 1)
    }
    fn block_state(&self, pos: BlockPos) -> Result<BlockState, WorldError> {
        Ok(self.region.block_state(pos)?)
    }
    fn biome(&self, pos: BlockPos) -> Result<u32, WorldError> {
        Ok(self.region.noise_biome(self.zoom.quart_at_block(pos))?)
    }
    fn block_survives(&self, state: BlockState, pos: BlockPos) -> Result<bool, WorldError> {
        self.survival.can_survive(state, pos, &*self.region)
    }
}
