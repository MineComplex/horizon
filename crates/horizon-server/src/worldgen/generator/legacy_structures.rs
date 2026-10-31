//! `Structure.generate` for the piece-based structure types: where each one starts
//! (`findGenerationPoint`, the biome check at that point) and the pieces its builder makes.
//! The jigsaw structures are `structures::generate_start`'s; the pieces themselves are
//! `worldgen::structure::{scattered, mineshaft, ...}`.

use super::LevelGenerator;
use super::structures::LevelColumns;
use crate::core::position::{BlockPos, ChunkPos};
use crate::util::random::{LegacyRandom, WorldgenRandom};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::structure::StructureId;
use crate::worldgen::structure::legacy_piece::LegacyPiece;
use crate::worldgen::structure::scattered::{
    BuriedTreasurePiece, DesertPyramidPiece, JungleTemplePiece, SwampHutPiece,
};
use crate::worldgen::structure::start::StructurePiece;

/// `Structure.GenerationContext` for the code-placed structures.
struct Context<'a, 'b, 'g> {
    columns: &'a mut LevelColumns<'b, 'g>,
    chunk: ChunkPos,
    sea_level: i32,
}

impl Context<'_, '_, '_> {
    /// `couldValidBiomeExistOnTopOfChunkCenter`.
    fn could_exist_on_top_of_chunk_center(&mut self) -> bool {
        use crate::worldgen::structure::jigsaw::JigsawWorld;
        let (x, z) = (self.chunk.x * 16 + 8, self.chunk.z * 16 + 8);
        let (min, max) = (
            self.columns.min_y - 1,
            self.columns.min_y + self.columns.height - 1,
        );
        self.columns.could_exist_in_column(x, z, min, max)
    }

    /// `chunkGenerator.getFirstOccupiedHeight(x, z, heightmap, ...)`.
    fn first_occupied(&mut self, x: i32, z: i32, heightmap: Heightmap) -> i32 {
        use crate::worldgen::structure::jigsaw::JigsawWorld;
        self.columns.first_free_height(x, z, heightmap) - 1
    }

    /// `onTopOfChunkCenterWithoutBiomeCheck`'s position.
    fn on_top_of_chunk_center(&mut self, heightmap: Heightmap) -> BlockPos {
        let (x, z) = (self.chunk.x * 16 + 8, self.chunk.z * 16 + 8);
        BlockPos::new(x, self.first_occupied(x, z, heightmap), z)
    }

    /// `Structure.getLowestY`.
    fn lowest_y(&mut self, size_x: i32, size_z: i32) -> i32 {
        let (x, z) = (self.chunk.x * 16, self.chunk.z * 16);
        let w = Heightmap::WorldSurfaceWg;
        [
            self.first_occupied(x, z, w),
            self.first_occupied(x, z + size_z, w),
            self.first_occupied(x + size_x, z, w),
            self.first_occupied(x + size_x, z + size_z, w),
        ]
        .into_iter()
        .min()
        .unwrap_or(0)
    }

    /// `isValidBiome(stub)`.
    fn valid_biome(&mut self, pos: BlockPos) -> bool {
        let biome = self.columns.biome_at(pos);
        self.columns.biomes.contains(biome)
    }
}

/// The pieces of a piece-based structure starting in `chunk`; `None` for an invalid start (also
/// for a structure type that has no generator here yet).
pub fn generate(
    generator: &LevelGenerator,
    columns: &mut LevelColumns<'_, '_>,
    structure: StructureId,
    chunk: ChunkPos,
) -> Option<Vec<StructurePiece>> {
    let kind = generator
        .data()
        .structures()
        .structure(structure)
        .kind
        .clone();
    let mut random = WorldgenRandom::new(LegacyRandom::new(0));
    random.set_large_feature_seed(generator.seed, chunk.x, chunk.z);
    let mut cx = Context {
        columns,
        chunk,
        sea_level: generator.settings().sea_level,
    };
    let (min_x, min_z) = (chunk.x * 16, chunk.z * 16);
    let piece = match kind.as_str() {
        "minecraft:buried_treasure" => {
            if !cx.could_exist_on_top_of_chunk_center() {
                return None;
            }
            let stub = cx.on_top_of_chunk_center(Heightmap::OceanFloorWg);
            if !cx.valid_biome(stub) {
                return None;
            }
            LegacyPiece::BuriedTreasure(BuriedTreasurePiece::new([min_x + 9, 90, min_z + 9]))
        }
        // `SwampHutStructure`: on top of the chunk centre, with no height check.
        "minecraft:swamp_hut" => {
            if !cx.could_exist_on_top_of_chunk_center() {
                return None;
            }
            let stub = cx.on_top_of_chunk_center(Heightmap::WorldSurfaceWg);
            if !cx.valid_biome(stub) {
                return None;
            }
            LegacyPiece::SwampHut(SwampHutPiece::new(&mut random, min_x, min_z))
        }
        // `SinglePieceStructure`.
        "minecraft:desert_pyramid" | "minecraft:jungle_temple" => {
            let (width, depth) = match kind.as_str() {
                "minecraft:desert_pyramid" => (21, 21),
                _ => (12, 15),
            };
            if !cx.could_exist_on_top_of_chunk_center() || cx.lowest_y(width, depth) < cx.sea_level
            {
                return None;
            }
            let stub = cx.on_top_of_chunk_center(Heightmap::WorldSurfaceWg);
            if !cx.valid_biome(stub) {
                return None;
            }
            match kind.as_str() {
                "minecraft:desert_pyramid" => {
                    LegacyPiece::DesertPyramid(DesertPyramidPiece::new(&mut random, min_x, min_z))
                }
                _ => LegacyPiece::JungleTemple(JungleTemplePiece::new(&mut random, min_x, min_z)),
            }
        }
        _ => return None,
    };
    Some(vec![StructurePiece::Legacy(piece)])
}
