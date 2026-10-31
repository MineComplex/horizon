//! Terrain features: `lake`, `freeze_top_layer`, `blue_ice`, `iceberg` ([`ice`](super::ice)),
//! `block_blob` and the huge mushrooms ([`mushroom`](super::mushroom)).

use serde_json::value::RawValue;

use super::FeatureRegistry;
use super::ice::{self, IceBlocks};
use super::lake::Lake;
use super::mushroom::HugeMushroom;
use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::FLAG_AIR;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, object, required};
use crate::worldgen::placement::BlockPredicate;
use crate::worldgen::placement::provider::type_of;
use crate::worldgen::state_provider::{ProviderContext, ProviderError};

/// A feature of this module.
#[derive(Clone, Debug)]
pub enum Terrain {
    FreezeTopLayer(IceBlocks),
    BlueIce(IceBlocks),
    Iceberg {
        blocks: IceBlocks,
        state: BlockState,
    },
    Lake(Box<Lake>),
    BlockBlob {
        state: BlockState,
        can_place_on: BlockPredicate,
    },
    HugeMushroom(Box<HugeMushroom>),
}

impl Terrain {
    pub fn decode(
        kind: &str,
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        let fields = object(raw)?;
        debug_assert_eq!(type_of(&fields)?, kind);
        let names = ctx.names;
        Ok(Some(match kind {
            "minecraft:freeze_top_layer" => Self::FreezeTopLayer(IceBlocks::new(names)?),
            "minecraft:blue_ice" => Self::BlueIce(IceBlocks::new(names)?),
            "minecraft:iceberg" => Self::Iceberg {
                blocks: IceBlocks::new(names)?,
                state: names.block_state(required(&fields, "state")?)?,
            },
            "minecraft:lake" => Self::Lake(Box::new(Lake::decode(raw, ctx)?)),
            "minecraft:block_blob" => Self::BlockBlob {
                state: names.block_state(required(&fields, "state")?)?,
                can_place_on: BlockPredicate::decode(required(&fields, "can_place_on")?, names)?,
            },
            "minecraft:huge_brown_mushroom" => {
                Self::HugeMushroom(Box::new(HugeMushroom::decode(false, raw, ctx)?))
            }
            "minecraft:huge_red_mushroom" => {
                Self::HugeMushroom(Box::new(HugeMushroom::decode(true, raw, ctx)?))
            }
            _ => return Ok(None),
        }))
    }

    pub fn place<W: FeatureWorld>(
        &self,
        registry: &FeatureRegistry,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match self {
            Self::FreezeTopLayer(blocks) => ice::freeze_top_layer(registry, blocks, world, origin),
            Self::BlueIce(blocks) => ice::blue_ice(blocks, world, random, origin),
            Self::Iceberg { blocks, state } => ice::iceberg(blocks, *state, world, random, origin),
            Self::Lake(lake) => lake.place(registry, world, random, origin),
            Self::BlockBlob {
                state,
                can_place_on,
            } => block_blob(world, random, origin, *state, can_place_on),
            Self::HugeMushroom(mushroom) => mushroom.place(world, random, origin),
        }
    }
}

/// `Feature.markAboveForPostProcessing`.
pub(super) fn mark_above<W: FeatureWorld>(
    world: &mut W,
    place: BlockPos,
) -> Result<(), ProviderError> {
    let mut pos = place;
    for _ in 0..2 {
        pos = pos.offset(0, 1, 0);
        let state = world.block_state(pos)?;
        if world.state_data().has(state, FLAG_AIR) {
            return Ok(());
        }
        world.mark_postprocessing(pos)?;
    }
    Ok(())
}

/// `BlockBlobFeature.place`.
fn block_blob<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    state: BlockState,
    can_place_on: &BlockPredicate,
) -> Result<bool, ProviderError> {
    let min_y = world.min_y();
    let mut origin = origin;
    while origin.y > min_y + 3 && !can_place_on.test(world, origin.offset(0, -1, 0))? {
        origin = origin.offset(0, -1, 0);
    }
    if origin.y <= min_y + 3 {
        return Ok(false);
    }
    for _ in 0..3 {
        let xr = random.next_int_bound(2)?;
        let yr = random.next_int_bound(2)?;
        let zr = random.next_int_bound(2)?;
        let tr = (xr + yr + zr) as f32 * 0.333f32 + 0.5f32;
        // `BlockPos.betweenClosed`: x fastest, then y, then z.
        for dz in -zr..=zr {
            for dy in -yr..=yr {
                for dx in -xr..=xr {
                    let dist = f64::from(dx * dx + dy * dy + dz * dz);
                    if dist <= f64::from(tr * tr) {
                        world.set_block(origin.offset(dx, dy, dz), state, 3)?;
                    }
                }
            }
        }
        origin = origin.offset(
            -1 + random.next_int_bound(2)?,
            -random.next_int_bound(2)?,
            -1 + random.next_int_bound(2)?,
        );
    }
    Ok(true)
}
