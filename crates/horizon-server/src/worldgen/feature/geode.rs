//! `geode` (`GeodeFeature`).

use serde_json::value::RawValue;

use super::common::relative;
use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::FLAG_AIR;
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::{LegacyRandom, RandomSource};
use crate::worldgen::codec::{
    DecodeError, boolean, double_range, int_range, integer, object, optional, required,
};
use crate::worldgen::noise::NormalNoise;
use crate::worldgen::placement::{IntProvider, PlacementWorld};
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

#[derive(Clone, Debug)]
pub struct Geode {
    filling: BlockStateProvider,
    inner: BlockStateProvider,
    alternate_inner: BlockStateProvider,
    middle: BlockStateProvider,
    outer: BlockStateProvider,
    inner_placements: Vec<BlockState>,
    cannot_replace: BiomeSet,
    invalid_blocks: BiomeSet,
    /// `GeodeLayerSettings`: filling, inner, middle and outer layer.
    layers: [f64; 4],
    crack_chance: f64,
    base_crack_size: f64,
    crack_point_offset: i32,
    potential_chance: f64,
    alternate_chance: f64,
    require_alternate: bool,
    outer_wall_distance: IntProvider,
    distribution_points: IntProvider,
    point_offset: IntProvider,
    min_gen_offset: i32,
    max_gen_offset: i32,
    noise_multiplier: f64,
    invalid_threshold: i32,
    /// `NormalNoise.createParity(-4, 1.0)`.
    noise: NormalNoise,
    air: BlockState,
}

fn chance(
    fields: &crate::worldgen::codec::Object,
    name: &'static str,
    default: f64,
) -> Result<f64, DecodeError> {
    optional(fields, name).map_or(Ok(default), |raw| double_range(raw, name, 0.0, 1.0))
}

impl Geode {
    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let names = ctx.names;
        let settings = object(required(&fields, "blocks")?)?;
        let placements: Vec<Box<RawValue>> =
            serde_json::from_str(required(&settings, "inner_placements")?.get())?;
        if placements.is_empty() || placements.len() > 8192 {
            return Err(DecodeError::Field("geode inner placements"));
        }
        let inner_placements = placements
            .iter()
            .map(|p| names.block_state(p))
            .collect::<Result<_, _>>()?;
        let layers = object(required(&fields, "layers")?)?;
        let layer = |name: &'static str, default: f64| {
            optional(&layers, name).map_or(Ok(default), |raw| double_range(raw, name, 0.01, 50.0))
        };
        let crack = object(required(&fields, "crack")?)?;
        let provider = |name: &'static str, default: (i32, i32), low: i32, high: i32| match optional(
            &fields, name,
        ) {
            Some(raw) => IntProvider::decode_within(raw, low, high),
            None => Ok(IntProvider::Uniform(default.0, default.1)),
        };
        let int_or =
            |name: &'static str, default: i32| optional(&fields, name).map_or(Ok(default), integer);
        let state_provider = |name: &'static str| -> Result<BlockStateProvider, DecodeError> {
            BlockStateProvider::decode(required(&settings, name)?, ctx)
        };
        Ok(Self {
            filling: state_provider("filling_provider")?,
            inner: state_provider("inner_layer_provider")?,
            alternate_inner: state_provider("alternate_inner_layer_provider")?,
            middle: state_provider("middle_layer_provider")?,
            outer: state_provider("outer_layer_provider")?,
            inner_placements,
            cannot_replace: names.block_set(required(&settings, "cannot_replace")?)?,
            invalid_blocks: names.block_set(required(&settings, "invalid_blocks")?)?,
            layers: [
                layer("filling", 1.7)?,
                layer("inner_layer", 2.2)?,
                layer("middle_layer", 3.2)?,
                layer("outer_layer", 4.2)?,
            ],
            crack_chance: chance(&crack, "generate_crack_chance", 1.0)?,
            base_crack_size: optional(&crack, "base_crack_size")
                .map_or(Ok(2.0), |r| double_range(r, "base_crack_size", 0.0, 5.0))?,
            crack_point_offset: optional(&crack, "crack_point_offset")
                .map_or(Ok(2), |r| int_range(r, "crack_point_offset", 0, 10))?,
            potential_chance: chance(&fields, "use_potential_placements_chance", 0.35)?,
            alternate_chance: chance(&fields, "use_alternate_layer0_chance", 0.0)?,
            require_alternate: optional(&fields, "placements_require_layer0_alternate")
                .map_or(Ok(true), boolean)?,
            outer_wall_distance: provider("outer_wall_distance", (4, 5), 1, 20)?,
            distribution_points: provider("distribution_points", (3, 4), 1, 20)?,
            point_offset: provider("point_offset", (1, 2), 0, 10)?,
            min_gen_offset: int_or("min_gen_offset", -16)?,
            max_gen_offset: int_or("max_gen_offset", 16)?,
            noise_multiplier: chance(&fields, "noise_multiplier", 0.05)?,
            invalid_threshold: integer(required(&fields, "invalid_blocks_threshold")?)?,
            noise: NormalNoise::create_parity(-4, &[1.0])
                .map_err(|_| DecodeError::Field("geode noise"))?,
            air: super::common::default_state(names.blocks, "minecraft:air")?,
        })
    }

    /// `Feature.safeSetBlock`.
    fn safe_set<W: FeatureWorld>(
        &self,
        world: &mut W,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), ProviderError> {
        let existing = world.block_state(pos)?;
        if !self
            .cannot_replace
            .contains(world.blocks().block(existing).0)
        {
            world.set_block(pos, state, 2)?;
        }
        Ok(())
    }

    /// `GeodeFeature.place`.
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let num_points = self.distribution_points.sample(random)?;
        let noise = self
            .noise
            .create(&mut LegacyRandom::new(world.world_seed()?))
            .map_err(|_| ProviderError::Invalid("geode noise"))?;
        let outer_max = self.outer_wall_distance.bounds()[1];
        let crack_adjust = f64::from(num_points) / f64::from(outer_max);
        let [filling, inner, middle, outer] = self.layers;
        let inner_air = 1.0 / filling.sqrt();
        let innermost = 1.0 / (inner + crack_adjust).sqrt();
        let inner_crust = 1.0 / (middle + crack_adjust).sqrt();
        let outer_crust = 1.0 / (outer + crack_adjust).sqrt();
        let crack_size = 1.0
            / (self.base_crack_size
                + random.next_double() / 2.0
                + if num_points > 3 { crack_adjust } else { 0.0 })
            .sqrt();
        let crack = f64::from(random.next_float()) < self.crack_chance;
        let mut invalid = 0;
        let mut points = Vec::new();
        for _ in 0..num_points {
            let x = self.outer_wall_distance.sample(random)?;
            let y = self.outer_wall_distance.sample(random)?;
            let z = self.outer_wall_distance.sample(random)?;
            let pos = origin.offset(x, y, z);
            let state = world.block_state(pos)?;
            if world.state_data().has(state, FLAG_AIR)
                || self.invalid_blocks.contains(world.blocks().block(state).0)
            {
                invalid += 1;
                if invalid > self.invalid_threshold {
                    return Ok(false);
                }
            }
            points.push((pos, self.point_offset.sample(random)?));
        }
        let mut crack_points = Vec::new();
        if crack {
            let index = random.next_int_bound(4)?;
            let o = num_points * 2 + 1;
            let (cx, cz) = match index {
                0 => (o, 0),
                1 => (0, o),
                2 => (o, o),
                _ => (0, 0),
            };
            for y in [7, 5, 1] {
                crack_points.push(origin.offset(cx, y, cz));
            }
        }
        let dist_sqr = |a: BlockPos, b: BlockPos| -> f64 {
            let d = |a: i32, b: i32| f64::from(a) - f64::from(b);
            d(a.x, b.x) * d(a.x, b.x) + d(a.y, b.y) * d(a.y, b.y) + d(a.z, b.z) * d(a.z, b.z)
        };
        let mut potential = Vec::new();
        let (lo, hi) = (self.min_gen_offset, self.max_gen_offset);
        // `BlockPos.betweenClosed`: x fastest, then y, then z.
        for dz in lo..=hi {
            for dy in lo..=hi {
                for dx in lo..=hi {
                    let pos = origin.offset(dx, dy, dz);
                    let noise_offset =
                        f64::from(noise.get3(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)))
                            * self.noise_multiplier;
                    let mut shell = 0.0;
                    for &(point, offset) in &points {
                        shell +=
                            1.0 / (dist_sqr(pos, point) + f64::from(offset)).sqrt() + noise_offset;
                    }
                    if shell < outer_crust {
                        continue;
                    }
                    if shell >= inner_air {
                        let state = self.filling.state(world, random, pos)?;
                        self.safe_set(world, pos, state)?;
                        continue;
                    }
                    let mut crack_sum = 0.0;
                    for &point in &crack_points {
                        crack_sum += 1.0
                            / (dist_sqr(pos, point) + f64::from(self.crack_point_offset)).sqrt()
                            + noise_offset;
                    }
                    if crack && crack_sum >= crack_size {
                        self.safe_set(world, pos, self.air)?;
                        for direction in Direction::ALL {
                            let adjacent = relative(pos, direction);
                            let state = world.block_state(adjacent)?;
                            let fluid = world.state_data().fluid(state).fluid;
                            if fluid != 0 {
                                world.schedule_fluid_tick(adjacent, fluid, 0)?;
                            }
                        }
                    } else if shell >= innermost {
                        let alternate = f64::from(random.next_float()) < self.alternate_chance;
                        let provider = if alternate {
                            &self.alternate_inner
                        } else {
                            &self.inner
                        };
                        let state = provider.state(world, random, pos)?;
                        self.safe_set(world, pos, state)?;
                        if (!self.require_alternate || alternate)
                            && f64::from(random.next_float()) < self.potential_chance
                        {
                            potential.push(pos);
                        }
                    } else if shell >= inner_crust {
                        let state = self.middle.state(world, random, pos)?;
                        self.safe_set(world, pos, state)?;
                    } else {
                        let state = self.outer.state(world, random, pos)?;
                        self.safe_set(world, pos, state)?;
                    }
                }
            }
        }
        for crystal in potential {
            let index = random.next_int_bound(self.inner_placements.len() as i32)? as usize;
            let mut state = self.inner_placements[index];
            for direction in Direction::ALL {
                state = set_facing(world, state, direction);
                let target = relative(crystal, direction);
                let existing = world.block_state(target)?;
                state = set_waterlogged(world, state, is_fluid_source(world, existing));
                // `BuddingAmethystBlock.canClusterGrowAtState`.
                if world.state_data().has(existing, FLAG_AIR)
                    || (super::common::is_block(world, existing, "minecraft:water")
                        && world.state_data().fluid(existing).amount == 8)
                {
                    self.safe_set(world, target, state)?;
                    break;
                }
            }
        }
        Ok(true)
    }
}

/// `FluidState.isSource`.
pub(super) fn is_fluid_source(world: &impl PlacementWorld, state: BlockState) -> bool {
    let fluid = world.state_data().fluid(state);
    fluid.fluid != 0 && fluid.amount == 8 && !fluid.falling
}

/// `hasProperty(FACING)` then `setValue(FACING, direction)`.
fn set_facing(world: &impl PlacementWorld, state: BlockState, direction: Direction) -> BlockState {
    let blocks = world.blocks();
    let block = blocks.block(state);
    match blocks.property(block, "facing") {
        Some(property) if blocks.property_values(property).len() == 6 => blocks
            .with_property(state, "facing", super::common::face_name(direction))
            .unwrap_or(state),
        _ => state,
    }
}

/// `hasProperty(WATERLOGGED)` then `setValue(WATERLOGGED, value)`.
fn set_waterlogged(world: &impl PlacementWorld, state: BlockState, value: bool) -> BlockState {
    world
        .blocks()
        .with_property(state, "waterlogged", if value { "true" } else { "false" })
        .unwrap_or(state)
}
