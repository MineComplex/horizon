//! `RuleTest` (`structure.templatesystem`): the tests ores use to pick the blocks they replace.
//! Copied from Horizon's `rule_test` without its `horizon:*` types.

use serde_json::value::RawValue;

use crate::block::{BlockState, BlockStates};
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, float, object, required, string};
use crate::worldgen::placement::DecodeContext;
use crate::worldgen::structure::BiomeSet;

#[derive(Clone, Debug)]
pub enum RuleTest {
    AlwaysTrue,
    /// A block id.
    Block(u32),
    BlockState(BlockState),
    Tag(BiomeSet),
    RandomBlock(u32, f32),
    RandomBlockState(BlockState, f32),
}

impl RuleTest {
    pub fn decode(raw: &RawValue, names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let block = || -> Result<u32, DecodeError> {
            let name = crate::worldgen::codec::identifier(&string(required(&fields, "block")?)?)?;
            (names.blocks.block_by_name(&name))
                .map(|b| b.0)
                .ok_or_else(|| DecodeError::Registry(format!("Missing block: {name}")))
        };
        // `RuleTest.CODEC` dispatches on `predicate_type`.
        let kind =
            crate::worldgen::codec::identifier(&string(required(&fields, "predicate_type")?)?)?;
        Ok(match kind.as_str() {
            "minecraft:always_true" => Self::AlwaysTrue,
            "minecraft:block_match" => Self::Block(block()?),
            "minecraft:blockstate_match" => {
                Self::BlockState(names.block_state(required(&fields, "block_state")?)?)
            }
            "minecraft:tag_match" => {
                Self::Tag(names.block_tag(&string(required(&fields, "tag")?)?)?)
            }
            "minecraft:random_block_match" => {
                Self::RandomBlock(block()?, float(required(&fields, "probability")?)?)
            }
            "minecraft:random_blockstate_match" => Self::RandomBlockState(
                names.block_state(required(&fields, "block_state")?)?,
                float(required(&fields, "probability")?)?,
            ),
            _ => return Err(DecodeError::Field("rule test type")),
        })
    }

    /// `test(state, pos, random)`: the random draw is made only when the state matches.
    pub fn test(
        &self,
        blocks: &BlockStates,
        state: BlockState,
        random: &mut impl RandomSource,
    ) -> bool {
        match self {
            Self::AlwaysTrue => true,
            Self::Block(block) => blocks.block(state).0 == *block,
            Self::BlockState(wanted) => state == *wanted,
            Self::Tag(set) => set.contains(blocks.block(state).0),
            Self::RandomBlock(block, p) => {
                blocks.block(state).0 == *block && random.next_float() < *p
            }
            Self::RandomBlockState(wanted, p) => state == *wanted && random.next_float() < *p,
        }
    }
}
