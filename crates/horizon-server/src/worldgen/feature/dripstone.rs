//! `speleothem`, `speleothem_cluster` and `large_dripstone` (`SpeleothemFeature`,
//! `SpeleothemClusterFeature`, `LargeDripstoneFeature` with `SpeleothemUtils`).

use serde_json::value::RawValue;

use super::common::{HORIZONTAL, default_state, empty_or_water, is_block, relative, scan_column};
use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::core::geometry::Direction;
use crate::core::geometry::trig;
use crate::core::position::BlockPos;
use crate::util::jmath;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::carver::float_provider::FloatProvider;
use crate::worldgen::codec::{DecodeError, float, int_range, object, optional, required};
use crate::worldgen::placement::{DecodeContext, IntProvider, PlacementWorld};
use crate::worldgen::state_provider::{ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

/// `FloatProviders.codec(low, high)`.
fn float_provider(raw: &RawValue, low: f32, high: f32) -> Result<FloatProvider, DecodeError> {
    FloatProvider::decode_within(raw, low, high)
}

/// `Codec.floatRange(low, high)`.
fn float_range(raw: &RawValue, low: f32, high: f32) -> Result<f32, DecodeError> {
    let value = float(raw)?;
    if !(low..=high).contains(&value) {
        return Err(DecodeError::Field("float out of range"));
    }
    Ok(value)
}

fn clamped_lerp(factor: f64, min: f64, max: f64) -> f64 {
    if factor < 0.0 {
        min
    } else if factor > 1.0 {
        max
    } else {
        min + factor * (max - min)
    }
}

fn clamped_lerp_f32(factor: f32, min: f32, max: f32) -> f32 {
    if factor < 0.0 {
        min
    } else if factor > 1.0 {
        max
    } else {
        min + factor * (max - min)
    }
}

/// The base and pointed blocks every speleothem feature carries, with `SpeleothemUtils`.
#[derive(Clone, Debug)]
struct Blocks {
    base: BlockState,
    pointed: BlockState,
    replaceable: BiomeSet,
}

impl Blocks {
    fn decode(
        fields: &crate::worldgen::codec::Object,
        names: &DecodeContext<'_>,
    ) -> Result<Self, DecodeError> {
        // `baseBlock.getBlock()`: only the block counts; the states are not placed as written.
        Ok(Self {
            base: names.block_state(required(fields, "base_block")?)?,
            pointed: names.block_state(required(fields, "pointed_block")?)?,
            replaceable: names.block_set(required(fields, "replaceable_blocks")?)?,
        })
    }

    /// `SpeleothemUtils.isBase`.
    fn is_base(&self, world: &impl PlacementWorld, state: BlockState) -> bool {
        let block = world.blocks().block(state);
        block == world.blocks().block(self.base) || self.replaceable.contains(block.0)
    }

    /// `SpeleothemUtils.placeBaseBlockIfPossible`.
    fn place_base_if_possible<W: FeatureWorld>(
        &self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        if self.replaceable.contains(world.blocks().block(state).0) {
            let base = world.blocks().block(self.base);
            let state = world.blocks().default_state(base);
            world.set_block(pos, state, 2)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// `createPointedBlock`.
    fn pointed_state(
        &self,
        world: &impl PlacementWorld,
        direction: Direction,
        thickness: &str,
    ) -> Result<BlockState, ProviderError> {
        let blocks = world.blocks();
        let state = blocks.default_state(blocks.block(self.pointed));
        let direction = if direction == Direction::Up {
            "up"
        } else {
            "down"
        };
        let state = blocks
            .with_property(state, "vertical_direction", direction)
            .ok_or(ProviderError::Invalid("pointed dripstone direction"))?;
        blocks
            .with_property(state, "thickness", thickness)
            .ok_or(ProviderError::Invalid("pointed dripstone thickness"))
    }

    /// `SpeleothemUtils.growSpeleothem`.
    fn grow<W: FeatureWorld>(
        &self,
        world: &mut W,
        start: BlockPos,
        tip: Direction,
        height: i32,
        merged_tip: bool,
    ) -> Result<(), ProviderError> {
        let behind = world.block_state(relative(start, tip.opposite()))?;
        if !self.is_base(world, behind) {
            return Ok(());
        }
        // `buildBaseToTipColumn`.
        let mut sequence = Vec::new();
        if height >= 3 {
            sequence.push("base");
            sequence.extend(std::iter::repeat_n("middle", (height - 3) as usize));
        }
        if height >= 2 {
            sequence.push("frustum");
        }
        if height >= 1 {
            sequence.push(if merged_tip { "tip_merge" } else { "tip" });
        }
        let mut pos = start;
        for thickness in sequence {
            let mut state = self.pointed_state(world, tip, thickness)?;
            // `level.isWaterAt(pos)`.
            let water = fluid_is_water(world, world.block_state(pos)?);
            state = world
                .blocks()
                .with_property(state, "waterlogged", if water { "true" } else { "false" })
                .ok_or(ProviderError::Invalid("pointed dripstone waterlogged"))?;
            world.set_block(pos, state, 2)?;
            pos = relative(pos, tip);
        }
        Ok(())
    }
}

/// `FluidState.is(FluidTags.WATER)`.
fn fluid_is_water(world: &impl PlacementWorld, state: BlockState) -> bool {
    let fluid = world.state_data().fluid(state).fluid;
    matches!(
        world.state_data().fluid_name(fluid),
        "minecraft:water" | "minecraft:flowing_water"
    )
}

fn empty_water_or_lava(world: &impl PlacementWorld, state: BlockState) -> bool {
    empty_or_water(world, state) || is_block(world, state, "minecraft:lava")
}

// ---------------------------------------------------------------------------------------------

/// `SpeleothemFeature`.
#[derive(Clone, Debug)]
pub struct Speleothem {
    blocks: Blocks,
    chance_of_taller_generation: f32,
    chance_of_directional_spread: f32,
    chance_of_spread_radius2: f32,
    chance_of_spread_radius3: f32,
}

impl Speleothem {
    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let chance = |name: &str, default: f32| {
            optional(&fields, name).map_or(Ok(default), |r| float_range(r, 0.0, 1.0))
        };
        Ok(Self {
            blocks: Blocks::decode(&fields, ctx.names)?,
            chance_of_taller_generation: chance("chance_of_taller_generation", 0.2)?,
            chance_of_directional_spread: chance("chance_of_directional_spread", 0.7)?,
            chance_of_spread_radius2: chance("chance_of_spread_radius2", 0.5)?,
            chance_of_spread_radius3: chance("chance_of_spread_radius3", 0.5)?,
        })
    }

    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let tip = {
            let above = world.block_state(origin.offset(0, 1, 0))?;
            let below = world.block_state(origin.offset(0, -1, 0))?;
            let can_above = self.blocks.is_base(world, above);
            let can_below = self.blocks.is_base(world, below);
            if can_above && can_below {
                if random.next_boolean() {
                    Direction::Down
                } else {
                    Direction::Up
                }
            } else if can_above {
                Direction::Down
            } else if can_below {
                Direction::Up
            } else {
                return Ok(false);
            }
        };
        let root = relative(origin, tip.opposite());
        self.create_patch_of_base_blocks(world, random, root)?;
        let ahead = world.block_state(relative(origin, tip))?;
        let height = if random.next_float() < self.chance_of_taller_generation
            && empty_or_water(world, ahead)
        {
            2
        } else {
            1
        };
        self.blocks.grow(world, origin, tip, height, false)?;
        Ok(true)
    }

    fn create_patch_of_base_blocks<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
    ) -> Result<(), ProviderError> {
        self.blocks.place_base_if_possible(world, pos)?;
        for direction in HORIZONTAL {
            if random.next_float() > self.chance_of_directional_spread {
                continue;
            }
            let pos1 = relative(pos, direction);
            self.blocks.place_base_if_possible(world, pos1)?;
            if random.next_float() > self.chance_of_spread_radius2 {
                continue;
            }
            let pos2 = relative(pos1, random_direction(random)?);
            self.blocks.place_base_if_possible(world, pos2)?;
            if random.next_float() > self.chance_of_spread_radius3 {
                continue;
            }
            let pos3 = relative(pos2, random_direction(random)?);
            self.blocks.place_base_if_possible(world, pos3)?;
        }
        Ok(())
    }
}

/// `Direction.getRandom(random)`: `Util.getRandom(VALUES, random)`.
fn random_direction(random: &mut impl RandomSource) -> Result<Direction, ProviderError> {
    Ok(Direction::ALL[random.next_int_bound(6)? as usize])
}

// ---------------------------------------------------------------------------------------------

/// `SpeleothemClusterFeature`.
#[derive(Clone, Debug)]
pub struct SpeleothemCluster {
    blocks: Blocks,
    search_range: i32,
    height: IntProvider,
    radius: IntProvider,
    max_height_diff: i32,
    height_deviation: i32,
    layer_thickness: IntProvider,
    density: FloatProvider,
    wetness: FloatProvider,
    chance_at_max_distance: f32,
    max_distance_from_edge: i32,
    max_distance_from_center: i32,
    base_stone: BiomeSet,
}

impl SpeleothemCluster {
    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let int = |name: &'static str, low: i32, high: i32| {
            int_range(required(&fields, name)?, name, low, high)
        };
        let provider = |name: &'static str, low: i32, high: i32| {
            IntProvider::decode_within(required(&fields, name)?, low, high)
        };
        Ok(Self {
            blocks: Blocks::decode(&fields, ctx.names)?,
            search_range: int("floor_to_ceiling_search_range", 1, 512)?,
            height: provider("height", 1, 128)?,
            radius: provider("radius", 1, 128)?,
            max_height_diff: int("max_stalagmite_stalactite_height_diff", 0, 64)?,
            height_deviation: int("height_deviation", 1, 64)?,
            layer_thickness: provider("speleothem_block_layer_thickness", 0, 128)?,
            density: float_provider(required(&fields, "density")?, 0.0, 2.0)?,
            wetness: float_provider(required(&fields, "wetness")?, 0.0, 2.0)?,
            chance_at_max_distance: float_range(
                required(&fields, "chance_of_speleothem_at_max_distance_from_center")?,
                0.0,
                1.0,
            )?,
            max_distance_from_edge: int(
                "max_distance_from_edge_affecting_chance_of_speleothem",
                1,
                64,
            )?,
            max_distance_from_center: int("max_distance_from_center_affecting_height_bias", 1, 64)?,
            base_stone: ctx.names.block_tag("minecraft:base_stone_overworld")?,
        })
    }

    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        if !empty_or_water(world, world.block_state(origin)?) {
            return Ok(false);
        }
        let height = self.height.sample(random)?;
        let wetness = self.wetness.sample(random);
        let density = self.density.sample(random);
        let x_radius = self.radius.sample(random)?;
        let z_radius = self.radius.sample(random)?;
        for dx in -x_radius..=x_radius {
            for dz in -z_radius..=z_radius {
                let distance = (x_radius - dx.abs()).min(z_radius - dz.abs());
                // `Mth.clampedMap(float...)`.
                let chance = f64::from(clamped_lerp_f32(
                    (distance as f32 - 0.0) / (self.max_distance_from_edge as f32 - 0.0),
                    self.chance_at_max_distance,
                    1.0,
                ));
                let pos = origin.offset(dx, 0, dz);
                self.column(
                    world,
                    random,
                    pos,
                    (dx, dz),
                    wetness,
                    chance,
                    height,
                    density,
                )?;
            }
        }
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn column<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
        (dx, dz): (i32, i32),
        chance_of_water: f32,
        chance: f64,
        cluster_height: i32,
        density: f32,
    ) -> Result<(), ProviderError> {
        let Some(base_column) = scan_column(
            world,
            pos,
            self.search_range,
            |w, s| empty_or_water(w, s),
            |w, s| !empty_or_water(w, s),
        )?
        else {
            return Ok(());
        };
        let ceiling = base_column.ceiling;
        let base_floor = base_column.floor;
        if ceiling.is_none() && base_floor.is_none() {
            return Ok(());
        }
        let want_pool = random.next_float() < chance_of_water;
        let at = |y: i32| BlockPos::new(pos.x, y, pos.z);
        let mut column = base_column;
        if let Some(floor_y) = base_floor
            && want_pool
            && self.can_place_pool(world, at(floor_y))?
        {
            column.floor = Some(floor_y - 1);
            let water = default_state(world.blocks(), "minecraft:water")
                .map_err(|_| ProviderError::Invalid("no water"))?;
            world.set_block(at(floor_y), water, 2)?;
        }
        let floor = column.floor;
        let want_stalactite = random.next_double() < chance;
        let mut stalactite_height = 0;
        if let Some(c) = ceiling
            && want_stalactite
            && !is_lava(world, at(c))?
        {
            let thickness = self.layer_thickness.sample(random)?;
            self.replace_with_base(world, at(c), thickness, Direction::Up)?;
            let max = match floor {
                Some(f) => cluster_height.min(c - f),
                None => cluster_height,
            };
            stalactite_height = self.speleothem_height(random, dx, dz, density, max)?;
        }
        let want_stalagmite = random.next_double() < chance;
        let mut stalagmite_height = 0;
        if let Some(f) = floor
            && want_stalagmite
            && !is_lava(world, at(f))?
        {
            let thickness = self.layer_thickness.sample(random)?;
            self.replace_with_base(world, at(f), thickness, Direction::Down)?;
            stalagmite_height = if ceiling.is_some() {
                let diff = self.max_height_diff;
                // `Mth.randomBetweenInclusive(random, -diff, diff)`.
                0.max(stalactite_height + random.next_int_bound(diff * 2 + 1)? - diff)
            } else {
                self.speleothem_height(random, dx, dz, density, cluster_height)?
            };
        }
        let (actual_stalactite, actual_stalagmite) = match (ceiling, floor) {
            (Some(c), Some(f)) if c - stalactite_height <= f + stalagmite_height => {
                let lowest_bottom = (c - stalactite_height).max(f + 1);
                let highest_top = (f + stalagmite_height).min(c - 1);
                let bottom =
                    random.next_int_bound(highest_top + 1 - lowest_bottom + 1)? + lowest_bottom;
                (c - bottom, bottom - 1 - f)
            }
            _ => (stalactite_height, stalagmite_height),
        };
        let merge = random.next_boolean()
            && actual_stalactite > 0
            && actual_stalagmite > 0
            && column
                .height()
                .is_some_and(|h| actual_stalactite + actual_stalagmite == h);
        if let Some(c) = ceiling {
            self.blocks
                .grow(world, at(c - 1), Direction::Down, actual_stalactite, merge)?;
        }
        if let Some(f) = floor {
            self.blocks
                .grow(world, at(f + 1), Direction::Up, actual_stalagmite, merge)?;
        }
        Ok(())
    }

    fn speleothem_height(
        &self,
        random: &mut impl RandomSource,
        dx: i32,
        dz: i32,
        density: f32,
        max_height: i32,
    ) -> Result<i32, ProviderError> {
        if random.next_float() > density {
            return Ok(0);
        }
        let distance = dx.abs() + dz.abs();
        let mean = clamped_lerp(
            (f64::from(distance) - 0.0) / (f64::from(self.max_distance_from_center) - 0.0),
            f64::from(max_height) / 2.0,
            0.0,
        ) as f32;
        // `ClampedNormalFloat.sample(random, mean, deviation, 0, maxHeight)`.
        let value = mean + random.next_gaussian() as f32 * self.height_deviation as f32;
        let clamped = if value < 0.0 {
            0.0
        } else {
            value.min(max_height as f32)
        };
        Ok(clamped as i32)
    }

    fn can_place_pool(
        &self,
        world: &impl PlacementWorld,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        if is_block(world, state, "minecraft:water")
            || world.blocks().block(state) == world.blocks().block(self.blocks.base)
            || world.blocks().block(state) == world.blocks().block(self.blocks.pointed)
        {
            return Ok(false);
        }
        if fluid_is_water(world, world.block_state(pos.offset(0, 1, 0))?) {
            return Ok(false);
        }
        for direction in HORIZONTAL {
            if !self.adjacent_ok(world, relative(pos, direction))? {
                return Ok(false);
            }
        }
        self.adjacent_ok(world, pos.offset(0, -1, 0))
    }

    fn adjacent_ok(
        &self,
        world: &impl PlacementWorld,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        Ok(self.base_stone.contains(world.blocks().block(state).0) || fluid_is_water(world, state))
    }

    fn replace_with_base<W: FeatureWorld>(
        &self,
        world: &mut W,
        first: BlockPos,
        count: i32,
        direction: Direction,
    ) -> Result<(), ProviderError> {
        let mut pos = first;
        for _ in 0..count {
            if !self.blocks.place_base_if_possible(world, pos)? {
                return Ok(());
            }
            pos = relative(pos, direction);
        }
        Ok(())
    }
}

fn is_lava(world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, ProviderError> {
    Ok(is_block(world, world.block_state(pos)?, "minecraft:lava"))
}

// ---------------------------------------------------------------------------------------------

/// `LargeDripstoneFeature`.
#[derive(Clone, Debug)]
pub struct LargeDripstone {
    replaceable: BiomeSet,
    search_range: i32,
    column_radius: IntProvider,
    height_scale: FloatProvider,
    max_radius_to_height_ratio: f32,
    stalactite_bluntness: FloatProvider,
    stalagmite_bluntness: FloatProvider,
    wind_speed: FloatProvider,
    min_radius_for_wind: i32,
    min_bluntness_for_wind: f32,
    base_stone: BiomeSet,
    dripstone_block: BlockState,
}

/// One of the two cones: `LargeDripstoneFeature.LargeDripstone`.
struct Cone {
    root: BlockPos,
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

/// `LargeDripstoneFeature.WindOffsetter`.
struct Wind {
    origin_y: i32,
    /// `None` for `noWind`.
    speed: Option<(f64, f64)>,
    max_offset: i32,
}

impl Wind {
    fn offset(&self, pos: BlockPos) -> BlockPos {
        let Some((sx, sz)) = self.speed else {
            return pos;
        };
        let dy = f64::from(self.origin_y - pos.y);
        let dx = ((sx * dy).floor() as i32).clamp(-self.max_offset, self.max_offset);
        let dz = ((sz * dy).floor() as i32).clamp(-self.max_offset, self.max_offset);
        pos.offset(dx, 0, dz)
    }
}

/// `SpeleothemUtils.getSpeleothemHeight`.
fn speleothem_height(distance: f64, radius: f64, scale: f64, bluntness: f64) -> f64 {
    let distance = if distance < bluntness {
        bluntness
    } else {
        distance
    };
    let r = distance / radius * 0.384;
    let part1 = 0.75 * jmath::pow(r, 1.3333333333333333);
    let part2 = jmath::pow(r, 0.6666666666666666);
    let part3 = 0.3333333333333333 * jmath::log(r);
    let height = (scale * (part1 - part2 - part3)).max(0.0);
    height / 0.384 * radius
}

impl Cone {
    fn height_at(&self, radius: f32) -> i32 {
        speleothem_height(
            f64::from(radius),
            f64::from(self.radius),
            self.scale,
            self.bluntness,
        ) as i32
    }
}

/// `Mth.PI`.
const PI: f32 = std::f32::consts::PI;

impl LargeDripstone {
    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let names = ctx.names;
        Ok(Self {
            replaceable: names.block_set(required(&fields, "replaceable_blocks")?)?,
            search_range: optional(&fields, "floor_to_ceiling_search_range")
                .map_or(Ok(30), |r| {
                    int_range(r, "floor_to_ceiling_search_range", 1, 512)
                })?,
            column_radius: IntProvider::decode_within(required(&fields, "column_radius")?, 1, 16)?,
            height_scale: float_provider(required(&fields, "height_scale")?, 0.0, 20.0)?,
            max_radius_to_height_ratio: float_range(
                required(&fields, "max_column_radius_to_cave_height_ratio")?,
                0.1,
                1.0,
            )?,
            stalactite_bluntness: float_provider(
                required(&fields, "stalactite_bluntness")?,
                0.1,
                10.0,
            )?,
            stalagmite_bluntness: float_provider(
                required(&fields, "stalagmite_bluntness")?,
                0.1,
                10.0,
            )?,
            wind_speed: float_provider(required(&fields, "wind_speed")?, 0.0, 2.0)?,
            min_radius_for_wind: int_range(
                required(&fields, "min_radius_for_wind")?,
                "min_radius_for_wind",
                0,
                100,
            )?,
            min_bluntness_for_wind: float_range(
                required(&fields, "min_bluntness_for_wind")?,
                0.0,
                5.0,
            )?,
            base_stone: names.block_tag("minecraft:base_stone_overworld")?,
            dripstone_block: default_state(names.blocks, "minecraft:dripstone_block")?,
        })
    }

    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        if !empty_or_water(world, world.block_state(origin)?) {
            return Ok(false);
        }
        let dripstone = world.blocks().block(self.dripstone_block);
        let Some(column) = scan_column(
            world,
            origin,
            self.search_range,
            |w, s| empty_or_water(w, s),
            |w, s| {
                let block = w.blocks().block(s);
                block == dripstone
                    || self.replaceable.contains(block.0)
                    || is_block(w, s, "minecraft:lava")
            },
        )?
        else {
            return Ok(false);
        };
        // Only a range with both edges.
        let (Some(floor), Some(ceiling)) = (column.floor, column.ceiling) else {
            return Ok(false);
        };
        let height = ceiling - floor - 1;
        if height < 4 {
            return Ok(false);
        }
        let [min_radius, max_radius] = self.column_radius.bounds();
        let based_on_height = (height as f32 * self.max_radius_to_height_ratio) as i32;
        let max_column_radius = based_on_height.clamp(min_radius, max_radius);
        // `Mth.randomBetweenInclusive`.
        let radius = random.next_int_bound(max_column_radius - min_radius + 1)? + min_radius;
        let mut stalactite = Cone {
            root: origin.offset(0, ceiling - 1 - origin.y, 0),
            pointing_up: false,
            radius,
            bluntness: f64::from(self.stalactite_bluntness.sample(random)),
            scale: f64::from(self.height_scale.sample(random)),
        };
        let mut stalagmite = Cone {
            root: origin.offset(0, floor + 1 - origin.y, 0),
            pointing_up: true,
            radius,
            bluntness: f64::from(self.stalagmite_bluntness.sample(random)),
            scale: f64::from(self.height_scale.sample(random)),
        };
        let suitable = |cone: &Cone| {
            cone.radius >= self.min_radius_for_wind
                && cone.bluntness >= f64::from(self.min_bluntness_for_wind)
        };
        let wind = if suitable(&stalactite) && suitable(&stalagmite) {
            // `new WindOffsetter(origin.getY(), random, windSpeed, 16 - radius)`.
            let speed = self.wind_speed.sample(random);
            let direction = random.next_float() * (PI - 0.0) + 0.0;
            Wind {
                origin_y: origin.y,
                speed: Some((
                    f64::from(trig::cos(f64::from(direction)) * speed),
                    f64::from(trig::sin(f64::from(direction)) * speed),
                )),
                max_offset: 16 - radius,
            }
        } else {
            Wind {
                origin_y: 0,
                speed: None,
                max_offset: 0,
            }
        };
        let stalactite_embedded = self.move_back(world, &mut stalactite, &wind)?;
        let stalagmite_embedded = self.move_back(world, &mut stalagmite, &wind)?;
        if stalactite_embedded {
            self.place_blocks(world, random, &stalactite, &wind)?;
        }
        if stalagmite_embedded {
            self.place_blocks(world, random, &stalagmite, &wind)?;
        }
        Ok(true)
    }

    /// `moveBackUntilBaseIsInsideStoneAndShrinkRadiusIfNecessary`.
    fn move_back(
        &self,
        world: &impl PlacementWorld,
        cone: &mut Cone,
        wind: &Wind,
    ) -> Result<bool, ProviderError> {
        while cone.radius > 1 {
            let mut new_root = cone.root;
            let max_tries = 10.min(cone.height_at(0.0));
            for _ in 0..max_tries {
                if is_lava(world, new_root)? {
                    return Ok(false);
                }
                if self.circle_embedded(world, wind.offset(new_root), cone.radius)? {
                    cone.root = new_root;
                    return Ok(true);
                }
                new_root = new_root.offset(0, if cone.pointing_up { -1 } else { 1 }, 0);
            }
            cone.radius /= 2;
        }
        Ok(false)
    }

    /// `SpeleothemUtils.isCircleMostlyEmbeddedInStone`.
    fn circle_embedded(
        &self,
        world: &impl PlacementWorld,
        center: BlockPos,
        radius: i32,
    ) -> Result<bool, ProviderError> {
        if empty_water_or_lava(world, world.block_state(center)?) {
            return Ok(false);
        }
        let increment = 6.0f32 / radius as f32;
        let mut angle = 0.0f32;
        while angle < PI * 2.0 {
            let dx = (trig::cos(f64::from(angle)) * radius as f32) as i32;
            let dz = (trig::sin(f64::from(angle)) * radius as f32) as i32;
            if empty_water_or_lava(world, world.block_state(center.offset(dx, 0, dz))?) {
                return Ok(false);
            }
            angle += increment;
        }
        Ok(true)
    }

    fn place_blocks<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        cone: &Cone,
        wind: &Wind,
    ) -> Result<(), ProviderError> {
        for dx in -cone.radius..=cone.radius {
            for dz in -cone.radius..=cone.radius {
                let current = ((dx * dx + dz * dz) as f32).sqrt();
                if current > cone.radius as f32 {
                    continue;
                }
                let mut height = cone.height_at(current);
                if height <= 0 {
                    continue;
                }
                if f64::from(random.next_float()) < 0.2 {
                    // `Mth.randomBetween(random, 0.8F, 1.0F)`.
                    let factor = random.next_float() * (1.0 - 0.8) + 0.8;
                    height = (height as f32 * factor) as i32;
                }
                let mut pos = cone.root.offset(dx, 0, dz);
                let mut has_been_out_of_stone = false;
                let max_y = if cone.pointing_up {
                    world.height(Heightmap::WorldSurfaceWg, pos.x, pos.z)?
                } else {
                    i32::MAX
                };
                let mut i = 0;
                while i < height && pos.y < max_y {
                    let adjusted = wind.offset(pos);
                    let state = world.block_state(adjusted)?;
                    if empty_water_or_lava(world, state) {
                        has_been_out_of_stone = true;
                        world.set_block(adjusted, self.dripstone_block, 2)?;
                    } else if has_been_out_of_stone
                        && self.base_stone.contains(world.blocks().block(state).0)
                    {
                        break;
                    }
                    pos = pos.offset(0, if cone.pointing_up { 1 } else { -1 }, 0);
                    i += 1;
                }
            }
        }
        Ok(())
    }
}
