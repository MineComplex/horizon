//! The cold features: `freeze_top_layer` (`SnowAndFreezeFeature`), `blue_ice` and `iceberg`
//! (`levelgen.feature`), with `Biome.shouldFreeze` / `shouldSnow` over the registry's biome climates.
//! Light is 0 in a generating region (Paper's `WorldGenRegion.getBrightness`), so the `< 10` tests hold.

use std::f64::consts::PI;

use super::FeatureRegistry;
use super::simple::FeatureWorld;
use crate::block::flags::FLAG_AIR;
use crate::block::{BlockId, BlockState};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::jmath;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::DecodeError;
use crate::worldgen::placement::{DecodeContext, PlacementWorld};
use crate::worldgen::state_provider::ProviderError;

/// The blocks these features name, resolved once.
#[derive(Clone, Copy, Debug)]
pub struct IceBlocks {
    water: BlockId,
    ice: BlockState,
    packed_ice: BlockId,
    blue_ice: BlockState,
    blue_ice_block: BlockId,
    ice_block: BlockId,
    snow: BlockState,
    snow_block_id: BlockId,
    snow_layer: BlockId,
    snow_block: BlockState,
    air: BlockState,
}

impl IceBlocks {
    pub fn new(names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let blocks = names.blocks;
        let get = |name: &str| {
            blocks
                .block_by_name(&format!("minecraft:{name}"))
                .ok_or(DecodeError::Field("ice block"))
        };
        let state = |name: &str| get(name).map(|b| blocks.default_state(b));
        Ok(Self {
            water: get("water")?,
            ice: state("ice")?,
            packed_ice: get("packed_ice")?,
            blue_ice: state("blue_ice")?,
            blue_ice_block: get("blue_ice")?,
            ice_block: get("ice")?,
            snow: state("snow")?,
            snow_block_id: get("snow_block")?,
            snow_layer: get("snow")?,
            snow_block: state("snow_block")?,
            air: state("air")?,
        })
    }
}

fn is_water_at(world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, ProviderError> {
    let state = world.block_state(pos)?;
    let data = world.state_data();
    Ok(matches!(
        data.fluid_name(data.fluid(state).fluid),
        "minecraft:water" | "minecraft:flowing_water"
    ))
}

fn is_air(world: &impl PlacementWorld, state: BlockState) -> bool {
    world.state_data().has(state, FLAG_AIR)
}

fn block_of(world: &impl PlacementWorld, state: BlockState) -> BlockId {
    world.blocks().block(state)
}

/// `Biome.shouldFreeze(level, pos, checkNeighbors)` for the biome at some other position.
pub(super) fn should_freeze(
    registry: &FeatureRegistry,
    blocks: &IceBlocks,
    world: &impl PlacementWorld,
    biome: u32,
    pos: BlockPos,
    check_neighbors: bool,
) -> Result<bool, ProviderError> {
    let sea_level = world.level_generation().sea_level;
    if registry
        .climates()
        .warm_enough_to_rain(biome, pos, sea_level)
    {
        return Ok(false);
    }
    if !world.outside_build_height(pos.y) {
        // A `LiquidBlock` of water: the water block (its fluid is water or flowing water).
        let state = world.block_state(pos)?;
        if block_of(world, state) == blocks.water {
            if !check_neighbors {
                return Ok(true);
            }
            let surrounded = is_water_at(world, pos.offset(-1, 0, 0))?
                && is_water_at(world, pos.offset(1, 0, 0))?
                && is_water_at(world, pos.offset(0, 0, -1))?
                && is_water_at(world, pos.offset(0, 0, 1))?;
            if !surrounded {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// `Biome.shouldSnow`.
fn should_snow(
    registry: &FeatureRegistry,
    blocks: &IceBlocks,
    world: &impl PlacementWorld,
    biome: u32,
    pos: BlockPos,
) -> Result<bool, ProviderError> {
    let sea_level = world.level_generation().sea_level;
    // `getPrecipitationAt`: none without precipitation, snow where it is cold enough.
    if !registry.has_precipitation(biome)
        || !registry
            .climates()
            .cold_enough_to_snow(biome, pos, sea_level)
    {
        return Ok(false);
    }
    if !world.outside_build_height(pos.y) {
        let state = world.block_state(pos)?;
        if (is_air(world, state) || block_of(world, state) == blocks.snow_layer)
            && world.block_survives(blocks.snow, pos)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `SnowAndFreezeFeature.place`.
pub fn freeze_top_layer<W: FeatureWorld>(
    registry: &FeatureRegistry,
    blocks: &IceBlocks,
    world: &mut W,
    origin: BlockPos,
) -> Result<bool, ProviderError> {
    for dx in 0..16 {
        for dz in 0..16 {
            let x = origin.x.wrapping_add(dx);
            let z = origin.z.wrapping_add(dz);
            let y = world.height(Heightmap::MotionBlocking, x, z)?;
            let top = BlockPos::new(x, y, z);
            let below = top.offset(0, -1, 0);
            let biome = world.biome(top)?;
            if should_freeze(registry, blocks, world, biome, below, false)? {
                world.set_block(below, blocks.ice, 2)?;
            }
            if should_snow(registry, blocks, world, biome, top)? {
                world.set_block(top, blocks.snow, 2)?;
                let below_state = world.block_state(below)?;
                let data = world.blocks();
                // `hasProperty(SNOWY)` first: a state without it is left as it is.
                if data.property(data.block(below_state), "snowy").is_some()
                    && let Some(snowy) = data.with_property(below_state, "snowy", "true")
                {
                    world.set_block(below, snowy, 2)?;
                }
            }
        }
    }
    Ok(true)
}

/// `BlueIceFeature.place`.
pub fn blue_ice<W: FeatureWorld>(
    blocks: &IceBlocks,
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
) -> Result<bool, ProviderError> {
    if origin.y > world.level_generation().sea_level - 1 {
        return Ok(false);
    }
    let here = world.block_state(origin)?;
    let under = world.block_state(origin.offset(0, -1, 0))?;
    if block_of(world, here) != blocks.water && block_of(world, under) != blocks.water {
        return Ok(false);
    }
    let mut found = false;
    for direction in Direction::ALL {
        if direction != Direction::Down {
            let n = direction.normal();
            let state = world.block_state(origin.offset(n[0], n[1], n[2]))?;
            if block_of(world, state) == blocks.packed_ice {
                found = true;
                break;
            }
        }
    }
    if !found {
        return Ok(false);
    }
    world.set_block(origin, blocks.blue_ice, 2)?;
    for _ in 0..200 {
        let y_off = random.next_int_bound(5)? - random.next_int_bound(6)?;
        let mut xz_diff = 3;
        if y_off < 2 {
            xz_diff += y_off / 2;
        }
        if xz_diff >= 1 {
            let dx = random.next_int_bound(xz_diff)? - random.next_int_bound(xz_diff)?;
            let dz = random.next_int_bound(xz_diff)? - random.next_int_bound(xz_diff)?;
            let place = origin.offset(dx, y_off, dz);
            let state = world.block_state(place)?;
            let block = block_of(world, state);
            if is_air(world, state)
                || block == blocks.water
                || block == blocks.packed_ice
                || block == blocks.ice_block
            {
                for direction in Direction::ALL {
                    let n = direction.normal();
                    let side = world.block_state(place.offset(n[0], n[1], n[2]))?;
                    if block_of(world, side) == blocks.blue_ice_block {
                        world.set_block(place, blocks.blue_ice, 2)?;
                        break;
                    }
                }
            }
        }
    }
    Ok(true)
}

/// `Mth.ceil(float)`.
fn ceil(value: f32) -> i32 {
    let truncated = value as i32;
    if value > truncated as f32 {
        truncated + 1
    } else {
        truncated
    }
}

/// `Mth.clamp(float, float, float)`.
fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// The iceberg's shape, drawn once.
struct Berg<'a> {
    blocks: &'a IceBlocks,
    main: BlockState,
    origin: BlockPos,
    ellipse: bool,
    ellipse_a: i32,
    ellipse_c: i32,
    angle: f64,
    snow_on_top: bool,
}

/// `IcebergFeature.place`.
pub fn iceberg<W: FeatureWorld>(
    blocks: &IceBlocks,
    main: BlockState,
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
) -> Result<bool, ProviderError> {
    let origin = BlockPos::new(origin.x, world.generation().sea_level, origin.z);
    let snow_on_top = random.next_double() > 0.7;
    let angle = random.next_double() * 2.0 * PI;
    let ellipse_a = 11 - random.next_int_bound(5)?;
    let ellipse_c = 3 + random.next_int_bound(3)?;
    let ellipse = random.next_double() > 0.7;
    let mut over = if ellipse {
        random.next_int_bound(6)? + 6
    } else {
        random.next_int_bound(15)? + 3
    };
    if !ellipse && random.next_double() > 0.9 {
        over += random.next_int_bound(19)? + 7;
    }
    let under = (over + random.next_int_bound(11)?).min(18);
    let width = (over + random.next_int_bound(7)? - random.next_int_bound(5)?).min(11);
    let a = if ellipse { ellipse_a } else { 11 };
    let berg = Berg {
        blocks,
        main,
        origin,
        ellipse,
        ellipse_a,
        ellipse_c,
        angle,
        snow_on_top,
    };

    for xo in -a..a {
        for zo in -a..a {
            for y_off in 0..over {
                let radius = if ellipse {
                    radius_ellipse(y_off, over, width)
                } else {
                    radius_round(random, y_off, over, width)?
                };
                if ellipse || xo < radius {
                    berg.generate_block(world, random, over, xo, y_off, zo, radius, a)?;
                }
            }
        }
    }
    berg.smooth(world, width, over)?;
    for xo in -a..a {
        for zo in -a..a {
            let mut y_off = -1;
            while y_off > -under {
                let new_a = if ellipse {
                    let squared = (y_off * y_off) as f32;
                    ceil(a as f32 * (1.0 - squared / (under as f32 * 8.0)))
                } else {
                    a
                };
                let radius = radius_steep(random, -y_off, under, width)?;
                if xo < radius {
                    berg.generate_block(world, random, under, xo, y_off, zo, radius, new_a)?;
                }
                y_off -= 1;
            }
        }
    }
    let cut_out = if ellipse {
        random.next_double() > 0.1
    } else {
        random.next_double() > 0.7
    };
    if cut_out {
        berg.cut_out(world, random, width, over)?;
    }
    Ok(true)
}

fn radius_round(
    random: &mut impl RandomSource,
    y_off: i32,
    height: i32,
    width: i32,
) -> Result<i32, ProviderError> {
    let k = 3.5f32 - random.next_float();
    let mut scale = (1.0f32 - (y_off * y_off) as f32 / (height as f32 * k)) * width as f32;
    if height > 15 + random.next_int_bound(5)? {
        let temp = if y_off < 3 + random.next_int_bound(6)? {
            y_off / 2
        } else {
            y_off
        };
        scale = (1.0f32 - temp as f32 / (height as f32 * k * 0.4f32)) * width as f32;
    }
    Ok(ceil(scale / 2.0))
}

fn radius_ellipse(y_off: i32, height: i32, width: i32) -> i32 {
    let scale = (1.0f32 - (y_off * y_off) as f32 / (height as f32 * 1.0f32)) * width as f32;
    ceil(scale / 2.0)
}

fn radius_steep(
    random: &mut impl RandomSource,
    y_off: i32,
    height: i32,
    width: i32,
) -> Result<i32, ProviderError> {
    let k = 1.0f32 + random.next_float() / 2.0;
    let scale = (1.0f32 - y_off as f32 / (height as f32 * k)) * width as f32;
    Ok(ceil(scale / 2.0))
}

/// `signedDistanceEllipse` (`Math.pow(x, 2.0)` is `x * x` in HotSpot).
fn distance_ellipse(xo: i32, zo: i32, origin: (i32, i32), a: i32, c: i32, angle: f64) -> f64 {
    let (dx, dz) = (f64::from(xo - origin.0), f64::from(zo - origin.1));
    let first = (dx * jmath::cos(angle) - dz * jmath::sin(angle)) / f64::from(a);
    let second = (dx * jmath::sin(angle) + dz * jmath::cos(angle)) / f64::from(c);
    first * first + second * second - 1.0
}

fn distance_circle(xo: i32, zo: i32, radius: i32, random: &mut impl RandomSource) -> f64 {
    let off = 10.0f32 * clamp(random.next_float(), 0.2, 0.8) / radius as f32;
    f64::from(off) + f64::from(xo * xo) + f64::from(zo * zo) - f64::from(radius * radius)
}

impl Berg<'_> {
    fn is_iceberg(&self, world: &impl PlacementWorld, state: BlockState) -> bool {
        let block = block_of(world, state);
        block == self.blocks.packed_ice
            || block == self.blocks.snow_block_id
            || block == self.blocks.blue_ice_block
    }

    fn ellipse_c_at(&self, y_off: i32, height: i32) -> i32 {
        let mut c = self.ellipse_c;
        if y_off > 0 && height - y_off <= 3 {
            c -= 4 - (height - y_off);
        }
        c
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_block<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        height: i32,
        xo: i32,
        y_off: i32,
        zo: i32,
        radius: i32,
        a: i32,
    ) -> Result<(), ProviderError> {
        let signed = if self.ellipse {
            distance_ellipse(
                xo,
                zo,
                (0, 0),
                a,
                self.ellipse_c_at(y_off, height),
                self.angle,
            )
        } else {
            distance_circle(xo, zo, radius, random)
        };
        if signed < 0.0 {
            let pos = self.origin.offset(xo, y_off, zo);
            let compare = if self.ellipse {
                -0.5
            } else {
                f64::from(-6 - random.next_int_bound(3)?)
            };
            if signed > compare && random.next_double() > 0.9 {
                return Ok(());
            }
            self.set_iceberg_block(world, random, pos, height - y_off, height)?;
        }
        Ok(())
    }

    fn set_iceberg_block<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
        h_diff: i32,
        height: i32,
    ) -> Result<(), ProviderError> {
        let state = world.block_state(pos)?;
        let block = block_of(world, state);
        let blocks = self.blocks;
        if is_air(world, state)
            || block == blocks.snow_block_id
            || block == blocks.ice_block
            || block == blocks.water
        {
            let randomness = !self.ellipse || random.next_double() > 0.05;
            let divisor = if self.ellipse { 3 } else { 2 };
            if self.snow_on_top
                && block != blocks.water
                && f64::from(h_diff)
                    <= f64::from(random.next_int_bound((height / divisor).max(1))?)
                        + f64::from(height) * 0.6
                && randomness
            {
                world.set_block(pos, blocks.snow_block, 3)?;
            } else {
                world.set_block(pos, self.main, 3)?;
            }
        }
        Ok(())
    }

    fn smooth<W: FeatureWorld>(
        &self,
        world: &mut W,
        width: i32,
        height: i32,
    ) -> Result<(), ProviderError> {
        let a = if self.ellipse {
            self.ellipse_a
        } else {
            width / 2
        };
        for x in -a..=a {
            for z in -a..=a {
                for y_off in 0..=height {
                    let pos = self.origin.offset(x, y_off, z);
                    let state = world.block_state(pos)?;
                    let iceberg = self.is_iceberg(world, state);
                    if iceberg || block_of(world, state) == self.blocks.snow_layer {
                        let under = world.block_state(pos.offset(0, -1, 0))?;
                        if is_air(world, under) {
                            world.set_block(pos, self.blocks.air, 3)?;
                            world.set_block(pos.offset(0, 1, 0), self.blocks.air, 3)?;
                        } else if iceberg {
                            let mut counter = 0;
                            for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                                let side = world.block_state(pos.offset(dx, 0, dz))?;
                                if !self.is_iceberg(world, side) {
                                    counter += 1;
                                }
                            }
                            if counter >= 3 {
                                world.set_block(pos, self.blocks.air, 3)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn cut_out<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        width: i32,
        height: i32,
    ) -> Result<(), ProviderError> {
        let sign_x = if random.next_boolean() { -1 } else { 1 };
        let sign_z = if random.next_boolean() { -1 } else { 1 };
        let mut x_off = random.next_int_bound((width / 2 - 2).max(1))?;
        if random.next_boolean() {
            x_off = width / 2 + 1 - random.next_int_bound((width - width / 2 - 1).max(1))?;
        }
        let mut z_off = random.next_int_bound((width / 2 - 2).max(1))?;
        if random.next_boolean() {
            z_off = width / 2 + 1 - random.next_int_bound((width - width / 2 - 1).max(1))?;
        }
        if self.ellipse {
            x_off = random.next_int_bound((self.ellipse_a - 5).max(1))?;
            z_off = x_off;
        }
        let local = (sign_x * x_off, sign_z * z_off);
        let angle = if self.ellipse {
            self.angle + PI / 2.0
        } else {
            random.next_double() * 2.0 * PI
        };
        for y_off in 0..height - 3 {
            let radius = radius_round(random, y_off, height, width)?;
            self.carve(world, radius, y_off, false, angle, local)?;
        }
        let mut y_off = -1;
        // The loop condition draws every time it is tested.
        while y_off > -height + random.next_int_bound(5)? {
            let radius = radius_steep(random, -y_off, height, width)?;
            self.carve(world, radius, y_off, true, angle, local)?;
            y_off -= 1;
        }
        Ok(())
    }

    fn carve<W: FeatureWorld>(
        &self,
        world: &mut W,
        radius: i32,
        y_off: i32,
        under_water: bool,
        angle: f64,
        local: (i32, i32),
    ) -> Result<(), ProviderError> {
        let a = radius + 1 + self.ellipse_a / 3;
        let c = (radius - 3).min(3) + self.ellipse_c / 2 - 1;
        for xo in -a..a {
            for zo in -a..a {
                if distance_ellipse(xo, zo, local, a, c, angle) < 0.0 {
                    let pos = self.origin.offset(xo, y_off, zo);
                    let state = world.block_state(pos)?;
                    if self.is_iceberg(world, state)
                        || block_of(world, state) == self.blocks.snow_block_id
                    {
                        if under_water {
                            let water = world.blocks().default_state(self.blocks.water);
                            world.set_block(pos, water, 3)?;
                        } else {
                            world.set_block(pos, self.blocks.air, 3)?;
                            // `removeFloatingSnowLayer`.
                            let above = pos.offset(0, 1, 0);
                            let up = world.block_state(above)?;
                            if block_of(world, up) == self.blocks.snow_layer {
                                world.set_block(above, self.blocks.air, 3)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
