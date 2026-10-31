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
use crate::worldgen::structure::legacy_piece::random_horizontal;
use crate::worldgen::structure::monument::MonumentBuilding;
use crate::worldgen::structure::scattered::{
    BuriedTreasurePiece, DesertPyramidPiece, JungleTemplePiece, SwampHutPiece,
};
use crate::worldgen::structure::start::StructurePiece;
use crate::worldgen::structure::{fortress, mineshaft, stronghold};

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

/// `MineshaftStructure`'s `mineshaft_type`: whether it is `mesa`.
fn mesa_type(structure: &crate::worldgen::structure::Structure) -> bool {
    serde_json::from_str::<serde_json::Value>(structure.json.get())
        .ok()
        .and_then(|v| {
            v.get("mineshaft_type")
                .and_then(|t| t.as_str().map(str::to_owned))
        })
        .is_some_and(|t| t == "mesa")
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
        // `NetherFortressStructure`: the pieces first, then the biome check at the stub (x, 64, z).
        "minecraft:fortress" => {
            let pieces = fortress::generate(&mut random, [chunk.x, chunk.z]);
            let stub = BlockPos::new(min_x, 64, min_z);
            if !cx.valid_biome(stub) {
                return None;
            }
            return Some(
                pieces
                    .into_iter()
                    .map(|p| StructurePiece::Legacy(LegacyPiece::Fortress(p)))
                    .collect(),
            );
        }
        // `StrongholdStructure`: the stub is the chunk's corner at y 0; the pieces are retried until
        // one has a portal room.
        "minecraft:stronghold" => {
            if !cx.valid_biome(BlockPos::new(min_x, 0, min_z)) {
                return None;
            }
            let pieces = stronghold::generate(
                &mut random,
                generator.seed,
                [chunk.x, chunk.z],
                cx.sea_level,
                cx.columns.min_y,
            );
            return Some(
                pieces
                    .into_iter()
                    .map(|p| StructurePiece::Legacy(LegacyPiece::Stronghold(p)))
                    .collect(),
            );
        }
        // `OceanMonumentStructure`: every biome within 29 blocks of the middle must be an ocean one.
        "minecraft:ocean_monument" => {
            let (x, z) = (min_x + 9, min_z + 9);
            let (y, r) = (cx.sea_level, 29);
            let surrounding = &generator.data().structures().monument_surrounding;
            for qz in (z - r) >> 2..=(z + r) >> 2 {
                for qx in (x - r) >> 2..=(x + r) >> 2 {
                    for qy in (y - r) >> 2..=(y + r) >> 2 {
                        let biome = cx
                            .columns
                            .biome_at(BlockPos::new(qx << 2, qy << 2, qz << 2));
                        if !surrounding.contains(biome) {
                            return None;
                        }
                    }
                }
            }
            let stub = cx.on_top_of_chunk_center(Heightmap::OceanFloorWg);
            if !cx.valid_biome(stub) {
                return None;
            }
            let direction = random_horizontal(&mut random);
            LegacyPiece::Monument(MonumentBuilding::new(
                &mut random,
                min_x - 29,
                min_z - 29,
                direction,
            ))
        }
        "minecraft:mineshaft" => {
            let mesa = mesa_type(generator.data().structures().structure(structure));
            // `MineshaftStructure.findGenerationPoint`: the stub's biome is checked at its position
            // after the pieces were made (`Either.right(builder)`).
            let sea_level = cx.sea_level;
            let min_y = cx.columns.min_y;
            let (position, pieces) = mineshaft::generate(
                &mut random,
                [chunk.x, chunk.z],
                mesa,
                sea_level,
                min_y,
                &mut |x, z| {
                    use crate::worldgen::structure::jigsaw::JigsawWorld;
                    cx.columns
                        .first_free_height(x, z, Heightmap::WorldSurfaceWg)
                },
            );
            let position = BlockPos::new(position[0], position[1], position[2]);
            if !cx.valid_biome(position) {
                return None;
            }
            return Some(
                pieces
                    .into_iter()
                    .map(|p| StructurePiece::Legacy(LegacyPiece::Mineshaft(p)))
                    .collect(),
            );
        }
        _ => return None,
    };
    Some(vec![StructurePiece::Legacy(piece)])
}
