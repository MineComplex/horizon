//! The first feature implementations: `simple_block`, `scattered_ore`, `disk` and `block_column`
//! (`levelgen.feature`), decoded with the registries and placed into a [`FeatureWorld`].
//! Copied from Horizon's `simple_block`, `ore_feature`, `disk` and `block_column`.
//!
//! Paper's `WorldGenRegion.setBlock` (what a [`RegionWorld`] write is) refuses a write outside the
//! step's write radius (logging `Detected setBlock in a far chunk`) and otherwise stores the state;
//! it also removes or adds the block entity (a `DUMMY` tag before the chunk is a level chunk),
//! updates the point-of-interest data and queues `getPostProcessPos`. None of those exist on the
//! proto chunk yet: WP6 adds them where the driver builds the region.

use serde_json::value::RawValue;

use super::rule_test::RuleTest;
use crate::block::BlockState;
use crate::block::flags::{FLAG_AIR, FLAG_DOUBLE_PLANT, FLAG_MOSSY_CARPET};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{
    DecodeError, boolean, float, int_range, object, optional, required, string,
};
use crate::worldgen::placement::provider::type_of;
use crate::worldgen::placement::world::SurvivalRules;
use crate::worldgen::placement::{
    BlockPredicate, IntProvider, PlacementWorld, RegionWorld, WorldError,
};
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext, ProviderError};

/// What a feature asks of the level besides reading it.
pub trait FeatureWorld: PlacementWorld {
    /// `WorldGenLevel.setBlock(pos, state, flags)`: false when the write was refused.
    fn set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: i32,
    ) -> Result<bool, WorldError>;
    /// `scheduleTick(pos, block at pos, delay)`.
    fn schedule_tick(&mut self, pos: BlockPos, delay: i32) -> Result<(), WorldError>;
    /// `getChunk(pos).markPosForPostProcessing(pos)`.
    fn mark_postprocessing(&mut self, pos: BlockPos) -> Result<(), WorldError>;
}

impl<S: SurvivalRules> FeatureWorld for RegionWorld<'_, '_, S> {
    fn set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        _flags: i32,
    ) -> Result<bool, WorldError> {
        match self
            .region
            .set_block_state(pos, state, self.data, self.heightmaps)
        {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    fn schedule_tick(&mut self, _: BlockPos, _: i32) -> Result<(), WorldError> {
        // The proto chunk keeps no scheduled ticks yet.
        Err(WorldError::Unsupported("scheduled ticks"))
    }
    fn mark_postprocessing(&mut self, pos: BlockPos) -> Result<(), WorldError> {
        let chunk = self.region.chunk_mut(pos.chunk())?;
        chunk.mark_postprocessing(pos.x, pos.y, pos.z);
        Ok(())
    }
}

/// A decoded feature of a type this module implements.
#[derive(Clone, Debug)]
pub enum Feature {
    SimpleBlock {
        provider: BlockStateProvider,
        schedule_tick: bool,
    },
    ScatteredOre {
        targets: Vec<(RuleTest, BlockState)>,
        size: i32,
        discard_chance_on_air_exposure: f32,
    },
    Disk {
        provider: BlockStateProvider,
        target: BlockPredicate,
        radius: IntProvider,
        half_height: i32,
    },
    BlockColumn {
        layers: Vec<(IntProvider, BlockStateProvider)>,
        direction: Direction,
        allowed_placement: BlockPredicate,
        prioritize_tip: bool,
    },
}

fn direction(raw: &RawValue) -> Result<Direction, DecodeError> {
    Ok(match string(raw)?.as_str() {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => return Err(DecodeError::Field("direction")),
    })
}

impl Feature {
    /// The feature of a `worldgen/feature` element, or `None` for a type not ported yet.
    pub fn decode(
        kind: &str,
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        let fields = object(raw)?;
        debug_assert_eq!(type_of(&fields)?, kind);
        let names = ctx.names;
        Ok(Some(match kind {
            "minecraft:simple_block" => Self::SimpleBlock {
                provider: BlockStateProvider::decode(required(&fields, "to_place")?, ctx)?,
                schedule_tick: optional(&fields, "schedule_tick").map_or(Ok(false), boolean)?,
            },
            // `AbstractOreFeature.makeCodec`: targets, size 0..=64, the air exposure chance.
            "minecraft:scattered_ore" => {
                let targets: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "targets")?.get())?;
                let targets = targets
                    .iter()
                    .map(|target| {
                        let target = object(target)?;
                        Ok((
                            RuleTest::decode(required(&target, "target")?, names)?,
                            names.block_state(required(&target, "state")?)?,
                        ))
                    })
                    .collect::<Result<_, DecodeError>>()?;
                let chance = float(required(&fields, "discard_chance_on_air_exposure")?)?;
                if !(0.0..=1.0).contains(&chance) {
                    return Err(DecodeError::Field("discard_chance_on_air_exposure"));
                }
                Self::ScatteredOre {
                    targets,
                    size: int_range(required(&fields, "size")?, "size", 0, 64)?,
                    discard_chance_on_air_exposure: chance,
                }
            }
            "minecraft:disk" => Self::Disk {
                provider: BlockStateProvider::decode(required(&fields, "state_provider")?, ctx)?,
                target: BlockPredicate::decode(required(&fields, "target")?, names)?,
                radius: IntProvider::decode_within(required(&fields, "radius")?, 0, 8)?,
                half_height: int_range(required(&fields, "half_height")?, "half_height", 0, 4)?,
            },
            "minecraft:block_column" => {
                let layers: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "layers")?.get())?;
                let layers = layers
                    .iter()
                    .map(|layer| {
                        let layer = object(layer)?;
                        Ok((
                            IntProvider::decode_within(required(&layer, "height")?, 0, i32::MAX)?,
                            BlockStateProvider::decode(required(&layer, "provider")?, ctx)?,
                        ))
                    })
                    .collect::<Result<_, DecodeError>>()?;
                Self::BlockColumn {
                    layers,
                    direction: direction(required(&fields, "direction")?)?,
                    allowed_placement: BlockPredicate::decode(
                        required(&fields, "allowed_placement")?,
                        names,
                    )?,
                    prioritize_tip: boolean(required(&fields, "prioritize_tip")?)?,
                }
            }
            _ => return Ok(None),
        }))
    }

    /// `Feature.place(level, generator, random, origin)`.
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match self {
            Self::SimpleBlock {
                provider,
                schedule_tick,
            } => simple_block(world, random, origin, provider, *schedule_tick),
            Self::ScatteredOre {
                targets,
                size,
                discard_chance_on_air_exposure,
            } => scattered_ore(
                world,
                random,
                origin,
                targets,
                *size,
                *discard_chance_on_air_exposure,
            ),
            Self::Disk {
                provider,
                target,
                radius,
                half_height,
            } => disk(
                world,
                random,
                origin,
                provider,
                target,
                radius,
                *half_height,
            ),
            Self::BlockColumn {
                layers,
                direction,
                allowed_placement,
                prioritize_tip,
            } => block_column(
                world,
                random,
                origin,
                layers,
                *direction,
                allowed_placement,
                *prioritize_tip,
            ),
        }
    }
}

fn is_water(world: &impl PlacementWorld, state: BlockState) -> bool {
    let data = world.state_data();
    matches!(
        data.fluid_name(data.fluid(state).fluid),
        "minecraft:water" | "minecraft:flowing_water"
    )
}

fn simple_block<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    provider: &BlockStateProvider,
    schedule_tick: bool,
) -> Result<bool, ProviderError> {
    let Some(state) = provider.optional_state(world, random, origin)? else {
        return Ok(false);
    };
    if !world.block_survives(state, origin)? {
        return Ok(false);
    }
    let data = world.state_data();
    if data.has(state, FLAG_DOUBLE_PLANT) {
        let above = world.block_state(origin.offset(0, 1, 0))?;
        let data = world.state_data();
        if !data.has(above, FLAG_AIR)
            && (data.fluid(state) != data.fluid(above) || !data.is_replaceable(above))
        {
            return Ok(false);
        }
        // `DoublePlantBlock.placeAt`: lower half, then upper, each waterlogged by the water there.
        for (half, at) in [("lower", origin), ("upper", origin.offset(0, 1, 0))] {
            let blocks = world.blocks();
            let mut half_state = blocks
                .with_property(state, "half", half)
                .ok_or(ProviderError::Invalid("a double plant without a half"))?;
            if blocks
                .property(blocks.block(state), "waterlogged")
                .is_some()
            {
                let water = is_water(world, world.block_state(at)?);
                let value = if water { "true" } else { "false" };
                half_state = world
                    .blocks()
                    .with_property(half_state, "waterlogged", value)
                    .unwrap_or(half_state);
            }
            world.set_block(at, half_state, 2)?;
        }
    } else if data.has(state, FLAG_MOSSY_CARPET) {
        // `MossyCarpetBlock.placeAt` draws from the level's own random: not ported.
        return Err(ProviderError::Invalid(
            "mossy carpet placement is not ported",
        ));
    } else {
        world.set_block(origin, state, 2)?;
    }
    if schedule_tick {
        world.schedule_tick(origin, 1)?;
    }
    Ok(true)
}

/// `AbstractOreFeature.canPlaceOre` and the scattered placement.
fn scattered_ore<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    targets: &[(RuleTest, BlockState)],
    size: i32,
    discard: f32,
) -> Result<bool, ProviderError> {
    let tries = random.next_int_bound(size + 1)?;
    for i in 0..tries {
        let reach = i.min(7);
        let mut offsets = [0; 3];
        for offset in &mut offsets {
            // `Math.round((nextFloat() - nextFloat()) * reach)`: floor(x + 0.5).
            let value = (random.next_float() - random.next_float()) * reach as f32;
            *offset = (value + 0.5).floor() as i32;
        }
        let pos = origin.offset(offsets[0], offsets[1], offsets[2]);
        let state = world.block_state(pos)?;
        for (test, replacement) in targets {
            if !test.test(world.blocks(), state, random) {
                continue;
            }
            let skip_air_check =
                discard <= 0.0 || (discard < 1.0 && random.next_float() >= discard);
            if skip_air_check || !adjacent_to_air(world, pos)? {
                world.set_block(pos, *replacement, 2)?;
                break;
            }
        }
    }
    Ok(true)
}

fn adjacent_to_air(world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, WorldError> {
    for d in Direction::ALL {
        let n = d.normal();
        let state = world.block_state(pos.offset(n[0], n[1], n[2]))?;
        if world.state_data().has(state, FLAG_AIR) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn disk<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    provider: &BlockStateProvider,
    target: &BlockPredicate,
    radius: &IntProvider,
    half_height: i32,
) -> Result<bool, ProviderError> {
    let top = origin.y.wrapping_add(half_height);
    let bottom = origin.y.wrapping_sub(half_height).wrapping_sub(1);
    let r = radius.sample(random)?;
    let mut placed_any = false;
    // `BlockPos.betweenClosed`: x fastest, then z (the y extent is one).
    for dz in -r..=r {
        for dx in -r..=r {
            if dx * dx + dz * dz > r * r {
                continue;
            }
            let mut placed_above = false;
            let mut y = top;
            while y > bottom {
                let pos = BlockPos::new(origin.x.wrapping_add(dx), y, origin.z.wrapping_add(dz));
                if target.test(world, pos)? {
                    if let Some(state) = provider.optional_state(world, random, pos)? {
                        world.set_block(pos, state, 2)?;
                        if !placed_above {
                            // `markAboveForPostProcessing`.
                            let mut above = pos;
                            for _ in 0..2 {
                                above = above.offset(0, 1, 0);
                                let state = world.block_state(above)?;
                                if world.state_data().has(state, FLAG_AIR) {
                                    break;
                                }
                                world.mark_postprocessing(above)?;
                            }
                        }
                        placed_any = true;
                        placed_above = true;
                    }
                } else {
                    placed_above = false;
                }
                y -= 1;
            }
        }
    }
    Ok(placed_any)
}

fn block_column<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    layers: &[(IntProvider, BlockStateProvider)],
    direction: Direction,
    allowed: &BlockPredicate,
    prioritize_tip: bool,
) -> Result<bool, ProviderError> {
    let mut heights = Vec::with_capacity(layers.len());
    let mut total = 0_i32;
    for (height, _) in layers {
        let h = height.sample(random)?;
        heights.push(h);
        total = total.wrapping_add(h);
    }
    if total == 0 {
        return Ok(false);
    }
    let step = direction.normal();
    let at = |from: BlockPos| from.offset(step[0], step[1], step[2]);
    let mut next = at(origin);
    for y in 0..total {
        if !allowed.test(world, next)? {
            truncate(&mut heights, total, y, prioritize_tip);
            break;
        }
        next = at(next);
    }
    let mut place = origin;
    for (count, (_, provider)) in heights.iter().zip(layers) {
        for _ in 0..*count {
            let state = provider.state(world, random, place)?;
            world.set_block(place, state, 2)?;
            place = at(place);
        }
    }
    Ok(true)
}

/// `BlockColumnFeature.truncate`.
fn truncate(heights: &mut [i32], total: i32, new_height: i32, prioritize_tip: bool) {
    let mut remove = total - new_height;
    let order: Vec<usize> = if prioritize_tip {
        (0..heights.len()).collect()
    } else {
        (0..heights.len()).rev().collect()
    };
    for i in order {
        if remove <= 0 {
            break;
        }
        let take = heights[i].min(remove);
        remove -= take;
        heights[i] -= take;
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncation_removes_from_the_base_or_the_tip() {
        // Layers of 3, 2 and 4 blocks cut to 5: from the end by default, from the first when the
        // tip is prioritised (`BlockColumnFeature.truncate`).
        let mut base = [3, 2, 4];
        truncate(&mut base, 9, 5, false);
        assert_eq!(base, [3, 2, 0]);
        let mut tip = [3, 2, 4];
        truncate(&mut tip, 9, 5, true);
        assert_eq!(tip, [0, 1, 4]);
        let mut all = [1, 1];
        truncate(&mut all, 2, 0, false);
        assert_eq!(all, [0, 0]);
    }
}
