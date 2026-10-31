//! `RuleTest` (`structure.templatesystem`): the tests ores use to pick the blocks they replace.
//! Copied from Horizon's `rule_test` without its `horizon:*` types.

use serde_json::value::RawValue;

use crate::block::{BlockState, BlockStates};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, float, integer, object, required, string};
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
    AllOf(Vec<RuleTest>),
    AnyOf(Vec<RuleTest>),
    Not(Box<RuleTest>),
    /// `HeightMatchTest`: the position's y within the bounds.
    Height {
        min_inclusive: i32,
        max_inclusive: i32,
    },
}

/// `RuleTest.CODEC.listOf().fieldOf("rules")`.
fn rules(
    fields: &crate::worldgen::codec::Object,
    names: &DecodeContext<'_>,
) -> Result<Vec<RuleTest>, DecodeError> {
    let list: Vec<Box<RawValue>> = serde_json::from_str(required(fields, "rules")?.get())?;
    list.iter().map(|r| RuleTest::decode(r, names)).collect()
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
            "minecraft:all_of" => Self::AllOf(rules(&fields, names)?),
            "minecraft:any_of" => Self::AnyOf(rules(&fields, names)?),
            "minecraft:not" => {
                Self::Not(Box::new(Self::decode(required(&fields, "rule")?, names)?))
            }
            "minecraft:height_match" => Self::Height {
                min_inclusive: integer(required(&fields, "min_inclusive")?)?,
                max_inclusive: integer(required(&fields, "max_inclusive")?)?,
            },
            _ => return Err(DecodeError::Field("rule test type")),
        })
    }

    /// `test(state, pos, random)`: the random draw is made only when the state matches.
    pub fn test(
        &self,
        blocks: &BlockStates,
        state: BlockState,
        pos: BlockPos,
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
            Self::AllOf(rules) => rules.iter().all(|r| r.test(blocks, state, pos, random)),
            Self::AnyOf(rules) => rules.iter().any(|r| r.test(blocks, state, pos, random)),
            Self::Not(rule) => !rule.test(blocks, state, pos, random),
            Self::Height {
                min_inclusive,
                max_inclusive,
            } => *min_inclusive <= pos.y && pos.y <= *max_inclusive,
        }
    }
}
