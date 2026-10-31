//! `BlockPredicate` (`levelgen.blockpredicates`): the tests the block predicate filter, the
//! environment scan and the features' own conditions apply to a position. Block, biome and fluid
//! sets are resolved to ids when the predicate is decoded.

use serde_json::value::RawValue;

use super::DecodeContext;
use super::provider::type_of;
use super::world::{PlacementWorld, WorldError};
use crate::block::flags::FLAG_LEGACY_SOLID;
use crate::block::{BlockState, StateFluid, SupportType};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::worldgen::anchor::VerticalAnchor;
use crate::worldgen::codec::{DecodeError, Object, object, optional, required, string};
use crate::worldgen::structure::BiomeSet;

#[derive(Clone, Debug)]
pub enum BlockPredicate {
    True,
    Unobstructed,
    WouldSurvive {
        state: BlockState,
        offset: [i32; 3],
    },
    HasSturdyFace {
        direction: Direction,
        offset: [i32; 3],
    },
    Solid([i32; 3]),
    Replaceable([i32; 3]),
    MatchingBiomes(BiomeSet),
    /// Fluid registry ids (`StateFluid::fluid`).
    MatchingFluids {
        fluids: BiomeSet,
        offset: [i32; 3],
    },
    /// Block ids: `matching_blocks` and `matching_block_tag` once their sets are resolved.
    MatchingBlocks {
        blocks: BiomeSet,
        offset: [i32; 3],
    },
    HeightRange {
        min: VerticalAnchor,
        max: VerticalAnchor,
    },
    Volume {
        min: [i32; 3],
        max: [i32; 3],
        matching: Box<Self>,
    },
    Not(Box<Self>),
    AllOf(Vec<Self>),
    AnyOf(Vec<Self>),
    InsideWorldBounds([i32; 3]),
}

/// `Vec3i.offsetCodec(16)`; the check is `Math.abs`, which keeps `MIN_VALUE` negative.
fn offset_vector(raw: &RawValue) -> Result<[i32; 3], DecodeError> {
    let vector = super::vector(raw)?;
    if vector.iter().any(|c| c.wrapping_abs() > 16) {
        return Err(DecodeError::Field("offset"));
    }
    Ok(vector)
}

fn offset_field(fields: &Object) -> Result<[i32; 3], DecodeError> {
    optional(fields, "offset").map_or(Ok([0; 3]), offset_vector)
}

impl BlockPredicate {
    pub fn decode(raw: &RawValue, names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let offset = || offset_field(&fields);
        Ok(match type_of(&fields)?.as_str() {
            "minecraft:true" => Self::True,
            "minecraft:unobstructed" => {
                // The offset is part of the codec but the test ignores it.
                offset()?;
                Self::Unobstructed
            }
            "minecraft:would_survive" => Self::WouldSurvive {
                state: names.block_state(required(&fields, "state")?)?,
                offset: offset()?,
            },
            "minecraft:has_sturdy_face" => Self::HasSturdyFace {
                direction: match string(required(&fields, "direction")?)?.as_str() {
                    "down" => Direction::Down,
                    "up" => Direction::Up,
                    "north" => Direction::North,
                    "south" => Direction::South,
                    "west" => Direction::West,
                    "east" => Direction::East,
                    _ => return Err(DecodeError::Field("direction")),
                },
                offset: offset()?,
            },
            "minecraft:solid" => Self::Solid(offset()?),
            "minecraft:replaceable" => Self::Replaceable(offset()?),
            "minecraft:matching_biomes" => {
                Self::MatchingBiomes(names.biome_set(required(&fields, "biomes")?)?)
            }
            "minecraft:matching_fluids" => Self::MatchingFluids {
                fluids: names.fluid_set(required(&fields, "fluids")?)?,
                offset: offset()?,
            },
            "minecraft:matching_blocks" => Self::MatchingBlocks {
                blocks: names.block_set(required(&fields, "blocks")?)?,
                offset: offset()?,
            },
            "minecraft:matching_block_tag" => Self::MatchingBlocks {
                blocks: names.block_tag(&string(required(&fields, "tag")?)?)?,
                offset: offset()?,
            },
            "minecraft:height_range" => Self::HeightRange {
                min: VerticalAnchor::decode(required(&fields, "min_inclusive")?)?,
                max: VerticalAnchor::decode(required(&fields, "max_inclusive")?)?,
            },
            "minecraft:volume_match" => {
                let min = offset_vector(required(&fields, "min")?)?;
                let max = offset_vector(required(&fields, "max")?)?;
                if (0..3).any(|axis| min[axis] > max[axis]) {
                    return Err(DecodeError::Field(
                        "min bound cannot be larger than max bound",
                    ));
                }
                Self::Volume {
                    min,
                    max,
                    matching: Box::new(Self::decode(required(&fields, "match")?, names)?),
                }
            }
            "minecraft:not" => Self::Not(Box::new(Self::decode(
                required(&fields, "predicate")?,
                names,
            )?)),
            kind @ ("minecraft:all_of" | "minecraft:any_of") => {
                let items: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "predicates")?.get())?;
                let list = items
                    .iter()
                    .map(|item| Self::decode(item, names))
                    .collect::<Result<_, _>>()?;
                if kind == "minecraft:all_of" {
                    Self::AllOf(list)
                } else {
                    Self::AnyOf(list)
                }
            }
            "minecraft:inside_world_bounds" => Self::InsideWorldBounds(offset()?),
            _ => return Err(DecodeError::Field("block predicate type")),
        })
    }

    /// `BlockPredicate.test(level, pos)`.
    pub fn test(&self, world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, WorldError> {
        let at = |offset: &[i32; 3]| pos.offset(offset[0], offset[1], offset[2]);
        let state = |offset: &[i32; 3]| world.block_state(at(offset));
        Ok(match self {
            Self::True => true,
            // `WorldGenRegion` holds no entities, so `EntityGetter.isUnobstructed` finds none.
            Self::Unobstructed => true,
            Self::WouldSurvive { state, offset } => world.block_survives(*state, at(offset))?,
            Self::HasSturdyFace { direction, offset } => {
                let target = at(offset);
                let state = world.block_state(target)?;
                world
                    .state_data()
                    .is_face_sturdy(state, *direction, SupportType::Full)
            }
            Self::Solid(offset) => world.state_data().has(state(offset)?, FLAG_LEGACY_SOLID),
            Self::Replaceable(offset) => world.state_data().is_replaceable(state(offset)?),
            Self::MatchingBiomes(biomes) => biomes.contains(world.biome(pos)?),
            Self::MatchingFluids { fluids, offset } => {
                let StateFluid { fluid, .. } = world.state_data().fluid(state(offset)?);
                fluids.contains(u32::from(fluid))
            }
            Self::MatchingBlocks { blocks, offset } => {
                blocks.contains(world.blocks().block(state(offset)?).0)
            }
            Self::HeightRange { min, max } => {
                let level = world.level_generation();
                pos.y >= min.resolve(level) && pos.y <= max.resolve(level)
            }
            // X, then Z, then Y, as vanilla loops.
            Self::Volume { min, max, matching } => {
                for x in min[0]..=max[0] {
                    for z in min[2]..=max[2] {
                        for y in min[1]..=max[1] {
                            if !matching.test(world, pos.offset(x, y, z))? {
                                return Ok(false);
                            }
                        }
                    }
                }
                true
            }
            Self::Not(inner) => !inner.test(world, pos)?,
            Self::AllOf(list) => {
                for predicate in list {
                    if !predicate.test(world, pos)? {
                        return Ok(false);
                    }
                }
                true
            }
            Self::AnyOf(list) => {
                for predicate in list {
                    if predicate.test(world, pos)? {
                        return Ok(true);
                    }
                }
                false
            }
            Self::InsideWorldBounds(offset) => !world.outside_build_height(at(offset).y),
        })
    }
}
