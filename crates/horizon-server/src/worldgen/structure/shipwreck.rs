//! Shipwrecks (`ShipwreckStructure`, `ShipwreckPieces`).

use super::Structure;
use super::template_pieces::{PieceCx, PieceKind, SHIPWRECK_PIVOT, TemplatePiece};
use super::template_start::{GenError, Stub, TemplateGen, TemplateGenWorld, pick};
use crate::block::transform::{Mirror, Rotation};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{boolean, object, required};
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::state_provider::ProviderError;

const BEACHED: [&str; 11] = [
    "with_mast",
    "sideways_full",
    "sideways_fronthalf",
    "sideways_backhalf",
    "rightsideup_full",
    "rightsideup_fronthalf",
    "rightsideup_backhalf",
    "with_mast_degraded",
    "rightsideup_full_degraded",
    "rightsideup_fronthalf_degraded",
    "rightsideup_backhalf_degraded",
];

const OCEAN: [&str; 20] = [
    "with_mast",
    "upsidedown_full",
    "upsidedown_fronthalf",
    "upsidedown_backhalf",
    "sideways_full",
    "sideways_fronthalf",
    "sideways_backhalf",
    "rightsideup_full",
    "rightsideup_fronthalf",
    "rightsideup_backhalf",
    "with_mast_degraded",
    "upsidedown_full_degraded",
    "upsidedown_fronthalf_degraded",
    "upsidedown_backhalf_degraded",
    "sideways_full_degraded",
    "sideways_fronthalf_degraded",
    "sideways_backhalf_degraded",
    "rightsideup_full_degraded",
    "rightsideup_fronthalf_degraded",
    "rightsideup_backhalf_degraded",
];

/// `NUMBER_OF_BLOCKS_ALLOWED_IN_WORLD_GEN_REGION`.
const ALLOWED: i32 = 32;

fn too_big(size: [i32; 3]) -> bool {
    size[0] > ALLOWED || size[1] > ALLOWED
}

/// `ShipwreckStructure.findGenerationPoint` and `generatePieces`.
pub fn generate<W: TemplateGenWorld, R: RandomSource>(
    structure: &Structure,
    cx: &mut TemplateGen<'_, W>,
    random: &mut R,
) -> Result<Option<Stub>, GenError> {
    let fields = object(&structure.json)?;
    let beached = boolean(required(&fields, "is_beached")?)?;
    let heightmap = if beached {
        Heightmap::WorldSurfaceWg
    } else {
        Heightmap::OceanFloorWg
    };
    if !cx.could_valid_biome_exist_on_top_of_chunk_center() {
        return Ok(None);
    }
    let position = cx.top_of_chunk_center(heightmap);
    let rotation = Rotation::random(random);
    let (min_x, min_z) = cx.min_block();
    let offset = BlockPos::new(min_x, 90, min_z);
    let names: &[&str] = if beached { &BEACHED } else { &OCEAN };
    let name = format!("minecraft:shipwreck/{}", pick(names, random)?);
    let mut piece = TemplatePiece::new(
        cx.registry,
        PieceKind::Shipwreck {
            beached,
            height_adjusted: false,
        },
        name,
        offset,
        rotation,
        Mirror::None,
        SHIPWRECK_PIVOT,
    );
    let size = piece.template(cx.registry).size();
    if too_big(size) {
        let b = piece.bounding_box;
        let (lo, hi) = (b.minimum(), b.maximum());
        let (span_x, span_z) = (hi[0] - lo[0] + 1, hi[2] - lo[2] + 1);
        let height = if beached {
            // Vanilla passes the spans in the slots of `minZ` and `sizeX`.
            let min_y = cx.lowest_y(lo[0], span_x, lo[2], span_z);
            min_y - size[1] / 2 - random.next_int_bound(3)?
        } else {
            cx.mean_first_occupied_height(lo[0], span_x, lo[2], span_z)
        };
        adjust_position_height(&mut piece, height);
    }
    Ok(Some(Stub {
        position,
        pieces: vec![piece],
    }))
}

/// `adjustPositionHeight`: the position moves, the box stays until `postProcess`.
fn adjust_position_height(piece: &mut TemplatePiece, height: i32) {
    if let PieceKind::Shipwreck {
        height_adjusted, ..
    } = &mut piece.kind
    {
        *height_adjusted = true;
    }
    piece.position = BlockPos::new(piece.position.x, height, piece.position.z);
}

/// `ShipwreckPiece.postProcess`.
pub fn post_process<W: FeatureWorld, R: RandomSource>(
    piece: &mut TemplatePiece,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    let PieceKind::Shipwreck {
        beached,
        height_adjusted,
    } = piece.kind
    else {
        return Ok(());
    };
    let size = piece.template(cx.tables.structures).size();
    if !height_adjusted && !too_big(size) {
        let level = cx.world.level_generation();
        let mut min_y = level.min_y.wrapping_add(level.height);
        let mut mean: i32 = 0;
        let heightmap = if beached {
            Heightmap::WorldSurfaceWg
        } else {
            Heightmap::OceanFloorWg
        };
        let base_size = size[0].wrapping_mul(size[2]);
        let origin = piece.position;
        if base_size == 0 {
            mean = cx.height(heightmap, origin.x, origin.z)?;
        } else {
            for z in origin.z..origin.z + size[2] {
                for x in origin.x..origin.x + size[0] {
                    let height = cx.height(heightmap, x, z)?;
                    mean = mean.wrapping_add(height);
                    min_y = min_y.min(height);
                }
            }
            mean /= base_size;
        }
        let height = if beached {
            min_y - size[1] / 2 - cx.random.next_int_bound(3)?
        } else {
            mean
        };
        adjust_position_height(piece, height);
    }
    piece.post_process_base(cx)
}

/// `handleDataMarker`: the chests' loot tables.
pub fn data_marker<W: FeatureWorld, R: RandomSource>(
    marker: &str,
    pos: BlockPos,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    let table = match marker {
        "map_chest" => "minecraft:chests/shipwreck_map",
        "treasure_chest" => "minecraft:chests/shipwreck_treasure",
        "supply_chest" => "minecraft:chests/shipwreck_supply",
        _ => return Ok(()),
    };
    cx.set_loot_table(pos.offset(0, -1, 0), table)
}
