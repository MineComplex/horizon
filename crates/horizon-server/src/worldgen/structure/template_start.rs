//! `Structure.generate` for the structures built from templates: `findGenerationPoint` of
//! `shipwreck`, `ocean_ruin`, `igloo`, `ruined_portal`, `nether_fossil`, `end_city` and
//! `woodland_mansion`, and the pieces each puts in its `StructurePiecesBuilder`.
//!
//! Vanilla hands the stub's position to `isValidBiome` before the pieces are built; the pieces draw
//! from the same random but nothing between draws, so they are built first here and dropped by the
//! caller when the biome does not allow the structure. What a structure asks of the level
//! (`ChunkGenerator.getFirstOccupiedHeight`, `getBaseColumn`, the biome at a position) is
//! [`TemplateGenWorld`].

use super::jigsaw::JigsawWorld;
use super::template_pieces::TemplatePiece;
use super::{Structure, StructureRegistry};
use crate::block::flags::SupportType;
use crate::block::transform::Rotation;
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::geometry::Direction;
use crate::core::position::{BlockPos, ChunkPos};
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::surface::temperature::BiomeClimates;

/// What a structure's generation reads of the level, beyond the jigsaw assembly's needs.
pub trait TemplateGenWorld: JigsawWorld {
    /// `ChunkGenerator.getBaseColumn(x, z, ...)`: the lowest y and the blocks from it up, `None`
    /// for an empty column.
    fn base_column(&mut self, x: i32, z: i32) -> Option<(i32, Vec<BlockState>)>;
    /// `biomeResolver.getNoiseBiome` at a block position: a `worldgen/biome` id.
    fn biome_at(&mut self, pos: BlockPos) -> u32;
    /// `heightmap.isOpaque().test(state)`.
    fn is_opaque(&self, state: BlockState, heightmap: Heightmap) -> bool;
}

/// `NoiseColumn`.
pub struct Column {
    min_y: i32,
    states: Vec<BlockState>,
    air: BlockState,
}

impl Column {
    /// `NoiseColumn.getBlock(y)`: air outside the column.
    pub fn block(&self, y: i32) -> BlockState {
        let index = i64::from(y) - i64::from(self.min_y);
        usize::try_from(index)
            .ok()
            .and_then(|i| self.states.get(i))
            .copied()
            .unwrap_or(self.air)
    }
}

/// `Structure.GenerationContext` as the template structures use it.
pub struct TemplateGen<'a, W> {
    pub registry: &'a StructureRegistry,
    pub blocks: &'a BlockStates,
    pub data: &'a BlockStateData,
    pub climates: &'a BiomeClimates,
    pub world: &'a mut W,
    /// `new WorldGenerationContext(generator, heightAccessor)`; its `sea_level` is the generator's.
    pub generation: WorldGenerationContext,
    /// The chunk's height accessor.
    pub min_y: i32,
    pub height: i32,
    pub chunk: ChunkPos,
}

/// `Structure.GenerationStub` with the pieces already built.
#[derive(Clone, Debug, PartialEq)]
pub struct Stub {
    pub position: BlockPos,
    pub pieces: Vec<TemplatePiece>,
}

/// A structure's generation failing where vanilla throws (the start is not made).
#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error(transparent)]
    Bound(#[from] crate::util::random::InvalidBound),
    #[error("{0}")]
    Config(String),
}

impl From<crate::worldgen::codec::DecodeError> for GenError {
    fn from(e: crate::worldgen::codec::DecodeError) -> Self {
        Self::Config(e.to_string())
    }
}

impl<W: TemplateGenWorld> TemplateGen<'_, W> {
    pub fn min_block(&self) -> (i32, i32) {
        (self.chunk.x << 4, self.chunk.z << 4)
    }

    /// `couldValidBiomeExistInTerrainColumn(x, z)`.
    pub fn could_valid_biome_exist_in_terrain_column(&mut self, x: i32, z: i32) -> bool {
        let (min_y, max_y) = (self.min_y - 1, self.min_y + self.height - 1);
        self.world.could_exist_in_column(x, z, min_y, max_y)
    }

    /// `couldValidBiomeExistOnTopOfChunkCenter()`.
    pub fn could_valid_biome_exist_on_top_of_chunk_center(&mut self) -> bool {
        let (x, z) = (self.min_block().0 + 8, self.min_block().1 + 8);
        self.could_valid_biome_exist_in_terrain_column(x, z)
    }

    /// `onTopOfChunkCenterWithoutBiomeCheck`'s position.
    pub fn top_of_chunk_center(&mut self, heightmap: Heightmap) -> BlockPos {
        let (x, z) = (self.min_block().0 + 8, self.min_block().1 + 8);
        let y = self.first_occupied_height(x, z, heightmap);
        BlockPos::new(x, y, z)
    }

    /// `ChunkGenerator.getFirstOccupiedHeight`: `getBaseHeight - 1`.
    pub fn first_occupied_height(&mut self, x: i32, z: i32, heightmap: Heightmap) -> i32 {
        self.world
            .first_free_height(x, z, heightmap)
            .wrapping_sub(1)
    }

    /// `ChunkGenerator.getBaseHeight` (`getFirstFreeHeight`).
    pub fn base_height(&mut self, x: i32, z: i32, heightmap: Heightmap) -> i32 {
        self.world.first_free_height(x, z, heightmap)
    }

    /// `ChunkGenerator.getBaseColumn`.
    pub fn base_column(&mut self, x: i32, z: i32) -> Column {
        let air = self
            .blocks
            .block_by_name("minecraft:air")
            .map_or(BlockState(0), |b| self.blocks.default_state(b));
        match self.world.base_column(x, z) {
            Some((min_y, states)) => Column { min_y, states, air },
            None => Column {
                min_y: self.min_y,
                states: Vec::new(),
                air,
            },
        }
    }

    /// `Structure.getCornerHeights(context, minX, sizeX, minZ, sizeZ)`.
    fn corner_heights(&mut self, min_x: i32, size_x: i32, min_z: i32, size_z: i32) -> [i32; 4] {
        let at = |cx: &mut Self, x: i32, z: i32| {
            cx.first_occupied_height(x, z, Heightmap::WorldSurfaceWg)
        };
        [
            at(self, min_x, min_z),
            at(self, min_x, min_z.wrapping_add(size_z)),
            at(self, min_x.wrapping_add(size_x), min_z),
            at(self, min_x.wrapping_add(size_x), min_z.wrapping_add(size_z)),
        ]
    }

    /// `Structure.getMeanFirstOccupiedHeight(context, minX, sizeX, minZ, sizeZ)`.
    pub fn mean_first_occupied_height(
        &mut self,
        min_x: i32,
        size_x: i32,
        min_z: i32,
        size_z: i32,
    ) -> i32 {
        let h = self.corner_heights(min_x, size_x, min_z, size_z);
        h.iter().fold(0i32, |a, &b| a.wrapping_add(b)) / 4
    }

    /// `Structure.getLowestY(context, minX, minZ, sizeX, sizeZ)`. (The parameter order is vanilla's:
    /// a shipwreck's call passes the spans in the wrong slots, and so does this.)
    pub fn lowest_y(&mut self, min_x: i32, min_z: i32, size_x: i32, size_z: i32) -> i32 {
        let h = self.corner_heights(min_x, size_x, min_z, size_z);
        h[0].min(h[1]).min(h[2].min(h[3]))
    }

    /// `getLowestYIn5by5Box(context, blockX, blockZ, rotation)`.
    pub fn lowest_y_in_5_by_5_box(&mut self, x: i32, z: i32, rotation: Rotation) -> BlockPos {
        let (mut offset_x, mut offset_z) = (5, 5);
        match rotation {
            Rotation::Clockwise90 => offset_x = -5,
            Rotation::Clockwise180 => {
                offset_x = -5;
                offset_z = -5;
            }
            Rotation::Counterclockwise90 => offset_z = -5,
            Rotation::None => {}
        }
        BlockPos::new(x, self.lowest_y(x, z, offset_x, offset_z), z)
    }

    /// `state.isFaceSturdy(EmptyBlockGetter.INSTANCE, pos, direction)`.
    pub fn is_face_sturdy(&self, state: BlockState, face: Direction) -> bool {
        self.data.is_face_sturdy(state, face, SupportType::Full)
    }
}

/// `Util.getRandom(array, random)`.
pub fn pick<'a, T, R: RandomSource>(
    items: &'a [T],
    random: &mut R,
) -> Result<&'a T, crate::util::random::InvalidBound> {
    Ok(&items[random.next_int_bound(items.len() as i32)? as usize])
}

/// `Structure.generate`'s generation point and pieces for a template structure: `Ok(None)` for no
/// point, and for a structure type that is not one of them (`None` from [`is_template_structure`]).
pub fn generate<W: TemplateGenWorld, R: RandomSource>(
    structure: &Structure,
    cx: &mut TemplateGen<'_, W>,
    random: &mut R,
) -> Result<Option<Stub>, GenError> {
    match structure.kind.as_str() {
        "minecraft:shipwreck" => super::shipwreck::generate(structure, cx, random),
        "minecraft:ocean_ruin" => super::ocean_ruin::generate(structure, cx, random),
        "minecraft:igloo" => super::igloo::generate(cx, random),
        "minecraft:ruined_portal" => super::ruined_portal::generate(structure, cx, random),
        "minecraft:nether_fossil" => super::nether_fossil::generate(structure, cx, random),
        "minecraft:end_city" => super::end_city::generate(cx, random),
        "minecraft:woodland_mansion" => super::mansion::generate(cx, random),
        _ => Ok(None),
    }
}

/// Whether a structure type is built from templates by this module.
pub fn is_template_structure(kind: &str) -> bool {
    matches!(
        kind,
        "minecraft:shipwreck"
            | "minecraft:ocean_ruin"
            | "minecraft:igloo"
            | "minecraft:ruined_portal"
            | "minecraft:nether_fossil"
            | "minecraft:end_city"
            | "minecraft:woodland_mansion"
    )
}
