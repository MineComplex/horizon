//! `BlockStateProvider` (`levelgen.feature.stateproviders`): what block a feature places at a
//! position. Decoded when the registries load; the named ones (`worldgen/block_state_provider`)
//! are inlined where a feature refers to them. Copied from Horizon's `state_provider` and
//! rebuilt on the server's block tables.
//!
//! Noise providers build their noise from `WorldgenRandom(LegacyRandomSource(seed))` as the
//! constructors do (Leaf's `FastRNG` for worldgen is off by default and not ported).

use serde_json::value::RawValue;

use crate::block::{BlockState, BlockStates};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::{InvalidBound, LegacyRandom, RandomSource, WorldgenRandom};
use crate::worldgen::codec::{
    DecodeError, Object, float, int_range, object, optional, required, string,
};
use crate::worldgen::noise::NoiseStack;
use crate::worldgen::placement::provider::{IntProvider, WeightedList, type_of};
use crate::worldgen::placement::{BlockPredicate, DecodeContext, PlacementWorld, WorldError};

/// Named providers nest at most this deep (a loop of references is a decode error).
const MAX_DEPTH: usize = 64;

/// Why a provider drew no state: the world, an invalid bound, or what vanilla throws.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error(transparent)]
    World(#[from] WorldError),
    #[error(transparent)]
    Random(#[from] InvalidBound),
    #[error("{0}")]
    Invalid(&'static str),
}

/// A `worldgen/block_state_provider` element's name and JSON, to inline where it is named.
pub struct NamedProviders<'a>(pub &'a [(String, Box<RawValue>)]);

/// What decoding a provider reads.
pub struct ProviderContext<'a> {
    pub names: &'a DecodeContext<'a>,
    pub named: NamedProviders<'a>,
}

#[derive(Clone, Debug)]
pub struct Noise {
    noise: NoiseStack,
    scale: f32,
}

#[derive(Clone, Debug)]
pub enum BlockStateProvider {
    Simple(BlockState),
    Weighted(WeightedList<BlockState>),
    /// The default states of a block set, in the set's order.
    RandomBlock(Vec<BlockState>),
    Rotated {
        state: Box<Self>,
        direction: Option<Direction>,
    },
    RuleBased {
        fallback: Option<Box<Self>>,
        rules: Vec<(BlockPredicate, Self)>,
    },
    CopyProperties(Box<Self>),
    RandomizedInt {
        source: Box<Self>,
        property: String,
        values: IntProvider,
    },
    Noise {
        noise: Noise,
        states: Vec<BlockState>,
    },
    NoiseThreshold {
        noise: Noise,
        threshold: f32,
        high_chance: f32,
        default: BlockState,
        low: Vec<BlockState>,
        high: Vec<BlockState>,
    },
    DualNoise {
        noise: Noise,
        slow: Noise,
        variety: [i32; 2],
        states: Vec<BlockState>,
    },
}

fn noise_from(
    object: &Object,
    field: &'static str,
    scale: &'static str,
    seed: i64,
) -> Result<Noise, DecodeError> {
    let parameters = crate::worldgen::codec::decode_noise(required(object, field)?)?;
    let scale = float(required(object, scale)?)?;
    // `ExtraCodecs.POSITIVE_FLOAT`.
    if scale.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return Err(DecodeError::Field("scale"));
    }
    let mut random = WorldgenRandom::new(LegacyRandom::new(seed));
    Ok(Noise {
        noise: parameters.create(&mut random)?,
        scale,
    })
}

fn state_list(
    object: &Object,
    field: &'static str,
    names: &DecodeContext<'_>,
) -> Result<Vec<BlockState>, DecodeError> {
    let items: Vec<Box<RawValue>> = serde_json::from_str(required(object, field)?.get())?;
    if items.is_empty() {
        return Err(DecodeError::Field(field));
    }
    items.iter().map(|raw| names.block_state(raw)).collect()
}

/// `RegistryCodecs.homogeneousList(BLOCK)` keeping the set's order: `"#tag"`, an id or a list.
fn ordered_blocks(
    raw: &RawValue,
    names: &DecodeContext<'_>,
) -> Result<Vec<BlockState>, DecodeError> {
    let blocks = |ids: Vec<u32>| {
        ids.into_iter()
            .map(|b| names.blocks.default_state(crate::block::BlockId(b)))
            .collect()
    };
    let by_name = |name: &str| {
        let name = crate::worldgen::codec::identifier(name)?;
        (names.blocks.block_by_name(&name))
            .map(|b| b.0)
            .ok_or_else(|| DecodeError::Registry(format!("Missing block: {name}")))
    };
    if raw.get().starts_with('[') {
        let list: Vec<String> = serde_json::from_str(raw.get())?;
        return Ok(blocks(
            list.iter().map(|n| by_name(n)).collect::<Result<_, _>>()?,
        ));
    }
    let name = string(raw)?;
    match name.strip_prefix('#') {
        Some(tag) => {
            let tag = crate::worldgen::codec::identifier(tag)?;
            let (_, ids) = (names.block_tags.iter())
                .find(|(t, _)| *t == tag)
                .ok_or_else(|| DecodeError::Registry(format!("Missing tag: #{tag} in block")))?;
            Ok(blocks(ids.iter().map(|&i| i as u32).collect()))
        }
        None => Ok(blocks(vec![by_name(&name)?])),
    }
}

impl BlockStateProvider {
    /// `BlockStateProvider.CODEC` (a holder): a name, a state or a typed provider.
    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        Self::decode_at(raw, ctx, 0)
    }

    fn decode_at(
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
        depth: usize,
    ) -> Result<Self, DecodeError> {
        if depth > MAX_DEPTH {
            return Err(DecodeError::Field("state provider depth"));
        }
        if raw.get().starts_with('"') {
            let name = crate::worldgen::codec::identifier(&string(raw)?)?;
            let (_, json) = (ctx.named.0.iter())
                .find(|(n, _)| *n == name)
                .ok_or_else(|| {
                    DecodeError::Registry(format!("Missing block state provider: {name}"))
                })?;
            return Self::decode_at(json, ctx, depth + 1);
        }
        let fields = object(raw)?;
        // `Codec.xor(BlockState.FULL_CODEC, TYPED_CODEC)`.
        if !fields.contains_key("type") {
            return Ok(Self::Simple(ctx.names.block_state(raw)?));
        }
        let names = ctx.names;
        let child = |name: &'static str| -> Result<Box<Self>, DecodeError> {
            Self::decode_at(required(&fields, name)?, ctx, depth + 1).map(Box::new)
        };
        let seed = |f: &Object| -> Result<i64, DecodeError> {
            let text = required(f, "seed")?.get();
            text.parse().map_err(|_| DecodeError::Field("seed"))
        };
        Ok(match type_of(&fields)?.as_str() {
            "minecraft:simple" => Self::Simple(names.block_state(required(&fields, "state")?)?),
            "minecraft:weighted" => Self::Weighted(WeightedList::decode(
                required(&fields, "entries")?,
                |raw| names.block_state(raw),
            )?),
            "minecraft:random_block" => {
                Self::RandomBlock(ordered_blocks(required(&fields, "blocks")?, names)?)
            }
            "minecraft:rotated" => Self::Rotated {
                state: child("state")?,
                direction: optional(&fields, "direction")
                    .map(|raw| match string(raw)?.as_str() {
                        "down" => Ok(Direction::Down),
                        "up" => Ok(Direction::Up),
                        "north" => Ok(Direction::North),
                        "south" => Ok(Direction::South),
                        "west" => Ok(Direction::West),
                        "east" => Ok(Direction::East),
                        _ => Err(DecodeError::Field("direction")),
                    })
                    .transpose()?,
            },
            "minecraft:rule_based" => {
                let fallback = optional(&fields, "fallback")
                    .map(|raw| Self::decode_at(raw, ctx, depth + 1).map(Box::new))
                    .transpose()?;
                let rules: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "rules")?.get())?;
                let rules = rules
                    .iter()
                    .map(|rule| {
                        let rule = object(rule)?;
                        Ok((
                            BlockPredicate::decode(required(&rule, "if_true")?, names)?,
                            Self::decode_at(required(&rule, "then")?, ctx, depth + 1)?,
                        ))
                    })
                    .collect::<Result<_, DecodeError>>()?;
                Self::RuleBased { fallback, rules }
            }
            "minecraft:copy_properties" => Self::CopyProperties(child("source")?),
            "minecraft:randomized_int" => Self::RandomizedInt {
                source: child("source")?,
                property: string(required(&fields, "property")?)?,
                values: IntProvider::decode(required(&fields, "values")?)?,
            },
            "minecraft:noise" => Self::Noise {
                noise: noise_from(&fields, "noise", "scale", seed(&fields)?)?,
                states: state_list(&fields, "states", names)?,
            },
            "minecraft:noise_threshold" => {
                let threshold = float(required(&fields, "threshold")?)?;
                let high_chance = float(required(&fields, "high_chance")?)?;
                if !(-1.0..=1.0).contains(&threshold) || !(0.0..=1.0).contains(&high_chance) {
                    return Err(DecodeError::Field("threshold or high_chance"));
                }
                Self::NoiseThreshold {
                    noise: noise_from(&fields, "noise", "scale", seed(&fields)?)?,
                    threshold,
                    high_chance,
                    default: names.block_state(required(&fields, "default_state")?)?,
                    low: state_list(&fields, "low_states", names)?,
                    high: state_list(&fields, "high_states", names)?,
                }
            }
            "minecraft:dual_noise" => {
                // `InclusiveRange.codec(INT, 1, 64)`: a `[min, max]` pair.
                let pair: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "variety")?.get())?;
                let [min, max] = &pair[..] else {
                    return Err(DecodeError::Field("variety"));
                };
                let min = int_range(min, "variety", 1, 64)?;
                let max = int_range(max, "variety", 1, 64)?;
                if min > max {
                    return Err(DecodeError::Field("variety"));
                }
                // The slow noise is built first, from the same seed.
                Self::DualNoise {
                    noise: noise_from(&fields, "noise", "scale", seed(&fields)?)?,
                    slow: noise_from(&fields, "slow_noise", "slow_scale", seed(&fields)?)?,
                    variety: [min, max],
                    states: state_list(&fields, "states", names)?,
                }
            }
            _ => return Err(DecodeError::Field("block state provider type")),
        })
    }

    /// `getOptionalState`: `None` where a rule-based provider has no rule for the position.
    pub fn optional_state<W: PlacementWorld>(
        &self,
        world: &W,
        random: &mut impl RandomSource,
        pos: BlockPos,
    ) -> Result<Option<BlockState>, ProviderError> {
        Ok(match self {
            Self::RandomBlock(states) => {
                if states.is_empty() {
                    None
                } else {
                    Some(states[random.next_int_bound(states.len() as i32)? as usize])
                }
            }
            Self::RuleBased { fallback, rules } => {
                for (predicate, then) in rules {
                    if predicate.test(world, pos)?
                        && let Some(state) = then.optional_state(world, random, pos)?
                    {
                        return Ok(Some(state));
                    }
                }
                match fallback {
                    Some(fallback) => fallback.optional_state(world, random, pos)?,
                    None => None,
                }
            }
            other => Some(other.state(world, random, pos)?),
        })
    }

    /// `getState`: a rule-based or random-block provider without a state gives the world's.
    pub fn state<W: PlacementWorld>(
        &self,
        world: &W,
        random: &mut impl RandomSource,
        pos: BlockPos,
    ) -> Result<BlockState, ProviderError> {
        let blocks = world.blocks();
        Ok(match self {
            Self::Simple(state) => *state,
            Self::Weighted(list) => *list.pick(random)?,
            Self::RandomBlock(_) | Self::RuleBased { .. } => {
                match self.optional_state(world, random, pos)? {
                    Some(state) => state,
                    None => world.block_state(pos)?,
                }
            }
            Self::Rotated { state, direction } => {
                let direction = match direction {
                    Some(direction) => *direction,
                    None => Direction::ALL[random.next_int_bound(6)? as usize],
                };
                rotate(blocks, state.state(world, random, pos)?, direction)
            }
            Self::CopyProperties(source) => {
                let target = source.state(world, random, pos)?;
                copy_properties(blocks, target, world.block_state(pos)?)
            }
            Self::RandomizedInt {
                source,
                property,
                values,
            } => {
                let state = source.state(world, random, pos)?;
                // `findProperty`: an integer property of that name, else the state unchanged.
                let found = blocks.property(blocks.block(state), property);
                let integer = found.filter(|&p| {
                    let values = blocks.property_values(p);
                    values.iter().all(|v| v.parse::<i32>().is_ok())
                });
                match integer {
                    None => state,
                    Some(property) => {
                        let value = values.sample(random)?.to_string();
                        let index = (blocks.property_values(property).iter())
                            .position(|v| **v == *value)
                            .ok_or(ProviderError::Invalid("property value out of range"))?;
                        blocks
                            .with_value_index(state, property, index)
                            .ok_or(ProviderError::Invalid("property value out of range"))?
                    }
                }
            }
            Self::Noise { noise, states } => {
                random_state(states, noise.value(pos, f64::from(noise.scale)))
            }
            Self::NoiseThreshold {
                noise,
                threshold,
                high_chance,
                default,
                low,
                high,
            } => {
                let value = f64::from(noise.value(pos, f64::from(noise.scale)));
                if value < f64::from(*threshold) {
                    low[random.next_int_bound(low.len() as i32)? as usize]
                } else if random.next_float() < *high_chance {
                    high[random.next_int_bound(high.len() as i32)? as usize]
                } else {
                    *default
                }
            }
            Self::DualNoise {
                noise,
                slow,
                variety,
                states,
            } => {
                let slow_at = |p: BlockPos| slow.value(p, f64::from(slow.scale));
                // `Mth.clampedMap(variety noise, -1, 1, min, max + 1)`, then the int cast.
                let t = (f64::from(slow_at(pos)) - -1.0) / (1.0 - -1.0);
                let (low, high) = (f64::from(variety[0]), f64::from(variety[1].wrapping_add(1)));
                let mapped = if t < 0.0 {
                    low
                } else if t > 1.0 {
                    high
                } else {
                    low + t * (high - low)
                };
                let count = mapped as i32;
                let possible: Vec<BlockState> = (0..count)
                    .map(|i| {
                        let at = pos.offset(i.wrapping_mul(54545), 0, i.wrapping_mul(34234));
                        random_state(states, slow_at(at))
                    })
                    .collect();
                random_state(&possible, noise.value(pos, f64::from(noise.scale)))
            }
        })
    }
}

impl Noise {
    /// `getNoiseValue`: `noise.get(x * scale, y * scale, z * scale)`.
    fn value(&self, pos: BlockPos, scale: f64) -> f32 {
        self.noise.get3(
            f64::from(pos.x) * scale,
            f64::from(pos.y) * scale,
            f64::from(pos.z) * scale,
        )
    }
}

/// `NoiseProvider.getRandomState(states, noiseValue)`.
fn random_state(states: &[BlockState], noise: f32) -> BlockState {
    let placement = ((1.0f32 + noise) / 2.0f32).clamp(0.0, 0.9999);
    states[(placement * states.len() as f32) as usize]
}

/// `trySetValue(AXIS)`, `trySetValue(FACING)`, and `HORIZONTAL_FACING` for horizontal directions.
/// `AXIS` and `FACING` are the properties with every axis / every direction; a block's two-axis or
/// five-way property of the same name is another property and stays.
fn rotate(blocks: &BlockStates, state: BlockState, direction: Direction) -> BlockState {
    const FACING: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
    let name = FACING[direction as usize];
    let block = blocks.block(state);
    // The property has exactly these values (the report lists `facing` as north..down).
    let has = |property, names: &[&str]| {
        let values = blocks.property_values(property);
        values.len() == names.len() && names.iter().all(|n| values.iter().any(|v| **v == **n))
    };
    let set = |state, property, value: &str| match blocks
        .property_values(property)
        .iter()
        .position(|v| **v == *value)
    {
        Some(index) => blocks.try_with_value_index(state, property, index),
        None => state,
    };
    let mut state = state;
    if let Some(axis) = blocks
        .property(block, "axis")
        .filter(|&p| has(p, &["x", "y", "z"]))
    {
        let value = match direction {
            Direction::Down | Direction::Up => "y",
            Direction::North | Direction::South => "z",
            Direction::West | Direction::East => "x",
        };
        state = set(state, axis, value);
    }
    if let Some(facing) = blocks.property(block, "facing")
        && (has(facing, &FACING) || (direction as usize >= 2 && has(facing, &FACING[2..])))
    {
        state = set(state, facing, name);
    }
    state
}

/// `withPropertiesOf`: every property of `target` that `source` also has takes its value.
fn copy_properties(blocks: &BlockStates, target: BlockState, source: BlockState) -> BlockState {
    let mut state = target;
    for property in blocks.properties(blocks.block(target)) {
        if let Some(index) = blocks.value_index(source, property) {
            state = blocks.try_with_value_index(state, property, index);
        }
    }
    state
}
