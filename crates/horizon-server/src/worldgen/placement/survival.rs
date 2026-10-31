//! The real `canSurvive` for placement: [`BlockSurvival`]'s rules read through a region. Paper's
//! `WorldGenRegion.getRawBrightness` answers 0 while the chunk's light is not finished, which holds
//! during features.

use super::world::{SurvivalRules, WorldError};
use crate::block::survival::{BlockSurvival, SurvivalError, SurvivalWorld};
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::position::BlockPos;
use crate::worldgen::proto_chunk::GenerationRegion;

/// [`SurvivalRules`] over the loaded rules and block tables.
pub struct BlockSurvivalRules<'a> {
    pub rules: &'a BlockSurvival,
    pub blocks: &'a BlockStates,
    pub data: &'a BlockStateData,
}

struct RegionReader<'a, 'r>(&'a GenerationRegion<'r>);

impl SurvivalWorld for RegionReader<'_, '_> {
    type Error = WorldError;
    fn block_state(&self, x: i32, y: i32, z: i32) -> Result<BlockState, WorldError> {
        Ok(self.0.block_state(BlockPos::new(x, y, z))?)
    }
    fn raw_brightness(&self, _: i32, _: i32, _: i32) -> Result<i32, WorldError> {
        Ok(0)
    }
}

impl SurvivalRules for BlockSurvivalRules<'_> {
    fn can_survive(
        &self,
        state: BlockState,
        pos: BlockPos,
        region: &GenerationRegion<'_>,
    ) -> Result<bool, WorldError> {
        let world = RegionReader(region);
        (self.rules)
            .can_survive(self.blocks, self.data, &world, state, [pos.x, pos.y, pos.z])
            .map_err(|e| match e {
                SurvivalError::World(e) => e,
                SurvivalError::Unsupported(what) => WorldError::Unsupported(what),
            })
    }
}
