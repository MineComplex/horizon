//! `CountOnEveryLayerPlacement`: positions on every walkable layer of a column, layer by layer.

use super::world::{PlacementWorld, WorldError};
use super::{IntProvider, PlacementError};
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::DecodeError;

/// The blocks the layer scan treats specially, resolved when the modifier is decoded.
#[derive(Clone, Copy, Debug)]
pub struct SpecialBlocks {
    water: BlockId,
    lava: BlockId,
    bedrock: BlockId,
}

impl SpecialBlocks {
    pub fn resolve(blocks: &BlockStates) -> Result<Self, DecodeError> {
        let find = |name: &str| {
            blocks
                .block_by_name(name)
                .ok_or_else(|| DecodeError::Registry(format!("Missing block: {name}")))
        };
        Ok(Self {
            water: find("minecraft:water")?,
            lava: find("minecraft:lava")?,
            bedrock: find("minecraft:bedrock")?,
        })
    }
}

pub(super) fn modify<W: PlacementWorld>(
    count: &IntProvider,
    special: &SpecialBlocks,
    world: &W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    out: &mut Vec<BlockPos>,
) -> Result<(), PlacementError> {
    let mut layer = 0_i32;
    loop {
        let mut found_any = false;
        let mut i = 0_i32;
        // The bound is sampled on every check of the loop condition, including the last.
        while i < count.sample(random)? {
            let x = random.next_int_bound(16)?.wrapping_add(origin.x);
            let z = random.next_int_bound(16)?.wrapping_add(origin.z);
            let start = world.height(Heightmap::MotionBlocking, x, z)?;
            if let Some(y) = on_ground(world, special, BlockPos::new(x, start, z), layer)? {
                out.push(BlockPos::new(x, y, z));
                found_any = true;
            }
            i = i.wrapping_add(1);
        }
        layer = layer.wrapping_add(1);
        if !found_any {
            return Ok(());
        }
    }
}

/// `findOnGroundYPosition`: the y of the `layer`th empty block above a solid one, scanning down.
fn on_ground<W: PlacementWorld>(
    world: &W,
    special: &SpecialBlocks,
    start: BlockPos,
    layer: i32,
) -> Result<Option<i32>, WorldError> {
    let blocks: &BlockStates = world.blocks();
    let empty = |state: BlockState| {
        world.state_data().is_air(state) || {
            let block = blocks.block(state);
            block == special.water || block == special.lava
        }
    };
    let mut current = world.block_state(start)?;
    let mut current_layer = 0_i32;
    let mut y = start.y;
    while y >= world.min_y().wrapping_add(1) {
        let below = world.block_state(BlockPos::new(start.x, y.wrapping_sub(1), start.z))?;
        if !empty(below) && empty(current) && blocks.block(below) != special.bedrock {
            if current_layer == layer {
                return Ok((y != i32::MAX).then_some(y));
            }
            current_layer = current_layer.wrapping_add(1);
        }
        current = below;
        y = y.wrapping_sub(1);
    }
    Ok(None)
}
