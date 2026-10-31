//! Igloos (`IglooStructure`, `IglooPieces`).

use super::template_pieces::{PieceCx, PieceKind, TemplatePiece, igloo_offset, igloo_pivot_of};
use super::template_start::{GenError, Stub, TemplateGen, TemplateGenWorld};
use crate::block::transform::{Mirror, Rotation};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::state_provider::ProviderError;

const TOP: &str = "minecraft:igloo/top";
const MIDDLE: &str = "minecraft:igloo/middle";
const BOTTOM: &str = "minecraft:igloo/bottom";

/// `IglooStructure.findGenerationPoint` and `generatePieces`.
pub fn generate<W: TemplateGenWorld, R: RandomSource>(
    cx: &mut TemplateGen<'_, W>,
    random: &mut R,
) -> Result<Option<Stub>, GenError> {
    if !cx.could_valid_biome_exist_on_top_of_chunk_center() {
        return Ok(None);
    }
    let position = cx.top_of_chunk_center(Heightmap::WorldSurfaceWg);
    let (min_x, min_z) = cx.min_block();
    let start = BlockPos::new(min_x, 90, min_z);
    let rotation = Rotation::random(random);
    let registry = cx.registry;
    let piece = |location: &str, depth: i32| {
        let offset = igloo_offset(location).unwrap_or_default();
        TemplatePiece::new(
            registry,
            PieceKind::Igloo,
            location.to_owned(),
            start.offset(offset.x, offset.y - depth, offset.z),
            rotation,
            Mirror::None,
            igloo_pivot_of(location).unwrap_or_default(),
        )
    };
    let mut pieces = Vec::new();
    if random.next_double() < 0.5 {
        let depth = random.next_int_bound(8)? + 4;
        pieces.push(piece(BOTTOM, depth * 3));
        for i in 0..depth - 1 {
            pieces.push(piece(MIDDLE, i * 3));
        }
    }
    pieces.push(piece(TOP, 0));
    Ok(Some(Stub { position, pieces }))
}

/// `IglooPiece.postProcess`.
pub fn post_process<W: FeatureWorld, R: RandomSource>(
    piece: &mut TemplatePiece,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    let location = piece.location();
    let offset = igloo_offset(&location).unwrap_or_default();
    let relative = piece.relative_position(BlockPos::new(3 - offset.x, 0, -offset.z));
    let entrance = piece.position.offset(relative.x, relative.y, relative.z);
    let height = cx.height(Heightmap::WorldSurfaceWg, entrance.x, entrance.z)?;
    let old_position = piece.position;
    piece.position = piece.position.offset(0, height - 90 - 1, 0);
    piece.post_process_base(cx)?;
    if location == TOP {
        let relative = piece.relative_position(BlockPos::new(3, 0, 5));
        let trap_door = piece.position.offset(relative.x, relative.y, relative.z);
        let below = cx.world.block_state(trap_door.offset(0, -1, 0))?;
        let blocks = cx.blocks().clone();
        let is_air = cx.world.state_data().is_air(below);
        let is_ladder = cx.world.blocks().block(below) == blocks.ladder;
        if !is_air && !is_ladder {
            let snow = cx.world.blocks().default_state(blocks.snow_block);
            cx.world.set_block(trap_door, snow, 3)?;
        }
    }
    piece.position = old_position;
    Ok(())
}

/// `IglooPiece.handleDataMarker`.
pub fn data_marker<W: FeatureWorld, R: RandomSource>(
    marker: &str,
    pos: BlockPos,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    if marker == "chest" {
        let air = cx.world.blocks().default_state(cx.blocks().air);
        cx.world.set_block(pos, air, 3)?;
        cx.set_loot_table(pos.offset(0, -1, 0), "minecraft:chests/igloo_chest")?;
    }
    Ok(())
}
