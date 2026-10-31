//! Nether fossils (`NetherFossilStructure`, `NetherFossilPieces`).

use super::Structure;
use super::template_pieces::{PieceCx, PieceKind, TemplatePiece};
use super::template_start::{GenError, Stub, TemplateGen, TemplateGenWorld, pick};
use crate::block::flags::FLAG_AIR;
use crate::block::transform::{Mirror, Rotation};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::{LegacyRandom, PositionalRandom, RandomSource};
use crate::worldgen::codec::{object, required};
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::placement::HeightProvider;
use crate::worldgen::state_provider::ProviderError;

const FOSSILS: [&str; 14] = [
    "fossil_1",
    "fossil_2",
    "fossil_3",
    "fossil_4",
    "fossil_5",
    "fossil_6",
    "fossil_7",
    "fossil_8",
    "fossil_9",
    "fossil_10",
    "fossil_11",
    "fossil_12",
    "fossil_13",
    "fossil_14",
];

/// `NetherFossilStructure.findGenerationPoint` and `NetherFossilPieces.addPieces`.
pub fn generate<W: TemplateGenWorld, R: RandomSource>(
    structure: &Structure,
    cx: &mut TemplateGen<'_, W>,
    random: &mut R,
) -> Result<Option<Stub>, GenError> {
    let fields = object(&structure.json)?;
    let height = HeightProvider::decode(required(&fields, "height")?)?;
    let (min_x, min_z) = cx.min_block();
    let x = min_x + random.next_int_bound(16)?;
    let z = min_z + random.next_int_bound(16)?;
    let sea_level = cx.generation.sea_level;
    let mut y = height.sample(random, cx.generation)?;
    let column = cx.base_column(x, z);
    let soul_sand = cx.blocks.block_by_name("minecraft:soul_sand");
    while y > sea_level {
        let current = column.block(y);
        y = y.wrapping_sub(1);
        let below = column.block(y);
        if cx.data.has(current, FLAG_AIR)
            && (soul_sand == Some(cx.blocks.block(below))
                || cx.is_face_sturdy(below, Direction::Up))
        {
            break;
        }
    }
    if y <= sea_level {
        return Ok(None);
    }
    let position = BlockPos::new(x, y, z);
    let rotation = Rotation::random(random);
    let name = format!("minecraft:nether_fossils/{}", pick(&FOSSILS, random)?);
    let piece = TemplatePiece::new(
        cx.registry,
        PieceKind::NetherFossil,
        name,
        position,
        rotation,
        Mirror::None,
        BlockPos::default(),
    );
    Ok(Some(Stub {
        position,
        pieces: vec![piece],
    }))
}

/// `NetherFossilPiece.postProcess` and `placeDriedGhast`.
pub fn post_process<W: FeatureWorld, R: RandomSource>(
    piece: &mut TemplatePiece,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    let fossil_box = piece.template_box(cx.tables.structures);
    *cx.chunk_box = cx.chunk_box.encapsulating(fossil_box);
    piece.post_process_base(cx)?;
    let (lo, hi) = (fossil_box.minimum(), fossil_box.maximum());
    let center = BlockPos::new(
        lo[0] + (hi[0] - lo[0] + 1) / 2,
        lo[1] + (hi[1] - lo[1] + 1) / 2,
        lo[2] + (hi[2] - lo[2] + 1) / 2,
    );
    let mut random = LegacyRandom::new(cx.tables.seed)
        .fork_positional()
        .at(center);
    if random.next_float() < 0.5 {
        let x = lo[0] + random.next_int_bound(hi[0] - lo[0] + 1)?;
        let y = lo[1];
        let z = lo[2] + random.next_int_bound(hi[2] - lo[2] + 1)?;
        let pos = BlockPos::new(x, y, z);
        let state = cx.world.block_state(pos)?;
        if cx.world.state_data().is_air(state) && cx.chunk_box.contains([x, y, z]) {
            let ghast = cx.blocks().dried_ghast;
            let default = cx.world.blocks().default_state(ghast);
            let rotation = Rotation::random(&mut random);
            let rotated = cx.world.state_data().rotate(default, rotation);
            cx.world.set_block(pos, rotated, 2)?;
        }
    }
    Ok(())
}
