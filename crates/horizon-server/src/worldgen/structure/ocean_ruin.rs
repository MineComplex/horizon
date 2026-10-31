//! Ocean ruins (`OceanRuinStructure`, `OceanRuinPieces`).

use super::Structure;
use super::template::transform;
use super::template_pieces::{PieceCx, PieceEvent, PieceKind, TemplatePiece};
use super::template_start::{GenError, Stub, TemplateGen, TemplateGenWorld, pick};
use crate::block::transform::{Mirror, Rotation};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{float, object, required, string};
use crate::worldgen::density::StructureBounds;
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::state_provider::ProviderError;

const WARM: [&str; 8] = [
    "warm_1", "warm_2", "warm_3", "warm_4", "warm_5", "warm_6", "warm_7", "warm_8",
];
const BIG_WARM: [&str; 4] = ["big_warm_4", "big_warm_5", "big_warm_6", "big_warm_7"];
const BRICK: [&str; 8] = [
    "brick_1", "brick_2", "brick_3", "brick_4", "brick_5", "brick_6", "brick_7", "brick_8",
];
const CRACKED: [&str; 8] = [
    "cracked_1",
    "cracked_2",
    "cracked_3",
    "cracked_4",
    "cracked_5",
    "cracked_6",
    "cracked_7",
    "cracked_8",
];
const MOSSY: [&str; 8] = [
    "mossy_1", "mossy_2", "mossy_3", "mossy_4", "mossy_5", "mossy_6", "mossy_7", "mossy_8",
];
const BIG_BRICK: [&str; 4] = ["big_brick_1", "big_brick_2", "big_brick_3", "big_brick_8"];
const BIG_CRACKED: [&str; 4] = [
    "big_cracked_1",
    "big_cracked_2",
    "big_cracked_3",
    "big_cracked_8",
];
const BIG_MOSSY: [&str; 4] = ["big_mossy_1", "big_mossy_2", "big_mossy_3", "big_mossy_8"];

/// `OceanRuinStructure`'s own fields.
struct Config {
    cold: bool,
    large_probability: f32,
    cluster_probability: f32,
}

/// `Mth.nextInt(random, min, max)`.
fn next_int<R: RandomSource>(
    random: &mut R,
    min: i32,
    max: i32,
) -> Result<i32, crate::util::random::InvalidBound> {
    if min >= max {
        Ok(min)
    } else {
        Ok(random.next_int_bound(max - min + 1)? + min)
    }
}

/// `OceanRuinStructure.findGenerationPoint` and `generatePieces`.
pub fn generate<W: TemplateGenWorld, R: RandomSource>(
    structure: &Structure,
    cx: &mut TemplateGen<'_, W>,
    random: &mut R,
) -> Result<Option<Stub>, GenError> {
    let fields = object(&structure.json)?;
    let probability = |name: &'static str| -> Result<f32, GenError> {
        let raw = required(&fields, name)?;
        let value = float(raw)?;
        double_range_check(value, name)?;
        Ok(value)
    };
    let config = Config {
        cold: match string(required(&fields, "biome_temp")?)?.as_str() {
            "warm" => false,
            "cold" => true,
            _ => return Err(GenError::Config("biome_temp".into())),
        },
        large_probability: probability("large_probability")?,
        cluster_probability: probability("cluster_probability")?,
    };
    if !cx.could_valid_biome_exist_on_top_of_chunk_center() {
        return Ok(None);
    }
    let position = cx.top_of_chunk_center(Heightmap::OceanFloorWg);
    let (min_x, min_z) = cx.min_block();
    let offset = BlockPos::new(min_x, 90, min_z);
    let rotation = Rotation::random(random);
    let mut pieces = Vec::new();
    add_pieces(cx, offset, rotation, &mut pieces, random, &config)?;
    Ok(Some(Stub { position, pieces }))
}

fn double_range_check(value: f32, name: &str) -> Result<(), GenError> {
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(GenError::Config(name.to_owned()))
    }
}

/// `OceanRuinPieces.addPieces`.
fn add_pieces<W: TemplateGenWorld, R: RandomSource>(
    cx: &TemplateGen<'_, W>,
    position: BlockPos,
    rotation: Rotation,
    pieces: &mut Vec<TemplatePiece>,
    random: &mut R,
    config: &Config,
) -> Result<(), GenError> {
    let large = random.next_float() <= config.large_probability;
    let integrity = if large { 0.9 } else { 0.8 };
    add_piece(
        cx, position, rotation, pieces, random, config, large, integrity,
    )?;
    if large && random.next_float() <= config.cluster_probability {
        add_cluster_ruins(cx, random, rotation, position, config, pieces)?;
    }
    Ok(())
}

/// `OceanRuinPieces.addClusterRuins`.
fn add_cluster_ruins<W: TemplateGenWorld, R: RandomSource>(
    cx: &TemplateGen<'_, W>,
    random: &mut R,
    rotation: Rotation,
    p: BlockPos,
    config: &Config,
    pieces: &mut Vec<TemplatePiece>,
) -> Result<(), GenError> {
    let parent_pos = BlockPos::new(p.x, 90, p.z);
    let corner = transform(
        BlockPos::new(15, 0, 15),
        Mirror::None,
        rotation,
        BlockPos::default(),
    );
    let parent_corner = parent_pos.offset(corner.x, corner.y, corner.z);
    let parent_box = StructureBounds::from_corners(
        [parent_pos.x, parent_pos.y, parent_pos.z],
        [parent_corner.x, parent_corner.y, parent_corner.z],
    );
    let bottom_left = BlockPos::new(
        parent_pos.x.min(parent_corner.x),
        parent_pos.y,
        parent_pos.z.min(parent_corner.z),
    );
    let mut all = all_positions(random, bottom_left)?;
    let ruins = next_int(random, 4, 8)?;
    for _ in 0..ruins {
        if all.is_empty() {
            continue;
        }
        let index = random.next_int_bound(all.len() as i32)? as usize;
        let pos = all.remove(index);
        let next_rotation = Rotation::random(random);
        let c = transform(
            BlockPos::new(5, 0, 6),
            Mirror::None,
            next_rotation,
            BlockPos::default(),
        );
        let next_box = StructureBounds::from_corners(
            [pos.x, pos.y, pos.z],
            [pos.x + c.x, pos.y + c.y, pos.z + c.z],
        );
        if !super::piece_place::intersects(next_box, parent_box) {
            add_piece(cx, pos, next_rotation, pieces, random, config, false, 0.8)?;
        }
    }
    Ok(())
}

/// `OceanRuinPieces.allPositions`.
fn all_positions<R: RandomSource>(
    random: &mut R,
    origin: BlockPos,
) -> Result<Vec<BlockPos>, GenError> {
    let mut positions = Vec::with_capacity(8);
    let mut push = |dx: i32, dz: i32| positions.push(origin.offset(dx, 0, dz));
    push(-16 + next_int(random, 1, 8)?, 16 + next_int(random, 1, 7)?);
    push(-16 + next_int(random, 1, 8)?, next_int(random, 1, 7)?);
    push(-16 + next_int(random, 1, 8)?, -16 + next_int(random, 4, 8)?);
    push(next_int(random, 1, 7)?, 16 + next_int(random, 1, 7)?);
    push(next_int(random, 1, 7)?, -16 + next_int(random, 4, 6)?);
    push(16 + next_int(random, 1, 7)?, 16 + next_int(random, 3, 8)?);
    push(16 + next_int(random, 1, 7)?, next_int(random, 1, 7)?);
    push(16 + next_int(random, 1, 7)?, -16 + next_int(random, 4, 8)?);
    Ok(positions)
}

#[allow(clippy::too_many_arguments)]
fn add_piece<W: TemplateGenWorld, R: RandomSource>(
    cx: &TemplateGen<'_, W>,
    position: BlockPos,
    rotation: Rotation,
    pieces: &mut Vec<TemplatePiece>,
    random: &mut R,
    config: &Config,
    large: bool,
    integrity: f32,
) -> Result<(), GenError> {
    let make = |name: &str, integrity: f32| {
        TemplatePiece::new(
            cx.registry,
            PieceKind::OceanRuin {
                cold: config.cold,
                integrity_bits: integrity.to_bits(),
                large,
            },
            format!("minecraft:underwater_ruin/{name}"),
            position,
            rotation,
            Mirror::None,
            BlockPos::default(),
        )
    };
    if !config.cold {
        let names: &[&str] = if large { &BIG_WARM } else { &WARM };
        let name = pick(names, random)?;
        pieces.push(make(name, integrity));
    } else {
        let (bricks, cracked, mossy): (&[&str], &[&str], &[&str]) = if large {
            (&BIG_BRICK, &BIG_CRACKED, &BIG_MOSSY)
        } else {
            (&BRICK, &CRACKED, &MOSSY)
        };
        let index = random.next_int_bound(bricks.len() as i32)? as usize;
        pieces.push(make(bricks[index], integrity));
        pieces.push(make(cracked[index], 0.7));
        pieces.push(make(mossy[index], 0.5));
    }
    Ok(())
}

/// `OceanRuinPiece.postProcess`.
pub fn post_process<W: FeatureWorld, R: RandomSource>(
    piece: &mut TemplatePiece,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    let height = cx.height(Heightmap::OceanFloorWg, piece.position.x, piece.position.z)?;
    piece.position = BlockPos::new(piece.position.x, height, piece.position.z);
    let size = piece.template(cx.tables.structures).size();
    let relative = transform(
        BlockPos::new(size[0] - 1, 0, size[2] - 1),
        Mirror::None,
        piece.rotation,
        BlockPos::default(),
    );
    let corner = piece.position.offset(relative.x, relative.y, relative.z);
    let new_y = floor_height(cx, piece.position, corner)?;
    piece.position = BlockPos::new(piece.position.x, new_y, piece.position.z);
    piece.post_process_base(cx)
}

/// `OceanRuinPiece.getHeight`.
fn floor_height<W: FeatureWorld, R: RandomSource>(
    cx: &PieceCx<'_, '_, W, R>,
    pos: BlockPos,
    corner: BlockPos,
) -> Result<i32, ProviderError> {
    let mut new_y = pos.y;
    let mut min_y: i32 = 512;
    let top_y = new_y - 1;
    let mut area = 0;
    let ice = &cx.tables.processors.pieces().ice;
    let world_min_y = cx.world.min_y();
    let open = |state| {
        let data = cx.world.state_data();
        data.is_air(state)
            || matches!(
                data.fluid_name(data.fluid(state).fluid),
                "minecraft:water" | "minecraft:flowing_water"
            )
            || ice.contains(cx.world.blocks().block(state).0)
    };
    // `betweenClosed(pos, corner)`: the corner has the position's y.
    for z in pos.z.min(corner.z)..=pos.z.max(corner.z) {
        for x in pos.x.min(corner.x)..=pos.x.max(corner.x) {
            let mut floor_y = pos.y - 1;
            let mut state = cx.world.block_state(BlockPos::new(x, floor_y, z))?;
            while open(state) && floor_y > world_min_y + 1 {
                floor_y -= 1;
                state = cx.world.block_state(BlockPos::new(x, floor_y, z))?;
            }
            min_y = min_y.min(floor_y);
            if floor_y < top_y - 2 {
                area += 1;
            }
        }
    }
    let width = (pos.x - corner.x).abs();
    if top_y - min_y > 2 && area > width - 2 {
        new_y = min_y + 1;
    }
    Ok(new_y)
}

/// `OceanRuinPiece.handleDataMarker`.
pub fn data_marker<W: FeatureWorld, R: RandomSource>(
    large: bool,
    marker: &str,
    pos: BlockPos,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    if marker == "chest" {
        let there = cx.world.block_state(pos)?;
        let data = cx.world.state_data();
        let water = matches!(
            data.fluid_name(data.fluid(there).fluid),
            "minecraft:water" | "minecraft:flowing_water"
        );
        let chest = cx.blocks().chest;
        let default = cx.world.blocks().default_state(chest);
        let state = cx
            .world
            .blocks()
            .with_property(default, "waterlogged", if water { "true" } else { "false" })
            .unwrap_or(default);
        cx.world.set_block(pos, state, 2)?;
        let table = if large {
            "minecraft:chests/underwater_ruin_big"
        } else {
            "minecraft:chests/underwater_ruin_small"
        };
        cx.set_loot_table(pos, table)?;
    } else if marker == "drowned" {
        cx.out.events.push(PieceEvent::Spawn {
            entity: "minecraft:drowned",
            pos,
            group: false,
        });
        let sea_level = cx.world.generation().sea_level;
        let block = if pos.y > sea_level {
            cx.blocks().air
        } else {
            cx.blocks().water
        };
        let state = cx.world.blocks().default_state(block);
        cx.world.set_block(pos, state, 2)?;
    }
    Ok(())
}
