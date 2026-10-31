//! `WorldCarver` (26.3 registers two types: `minecraft:cave`, which `nether_cave` and
//! `cave_extra_underground` are configurations of, and `minecraft:canyon`): the configuration the
//! `worldgen/carver` registry holds and the carving paths, with the random draws of
//! `CaveWorldCarver` and `CanyonWorldCarver` in their order.
//!
//! Copied from Horizon's `horizon-world::worldgen::carver`, rewritten over the server's pieces:
//! the mask is a plain [`CarvingMask`], vanilla's `Mth.sin`/`cos` table is
//! `core::geometry::trig`, and a draw from an empty range (only a datapack makes one) is an
//! error where Java throws.

use std::f32::consts::{PI, TAU};

use serde_json::value::RawValue;

use super::float_provider::FloatProvider;
use super::mask::CarvingMask;
use crate::core::geometry::trig::{cos, sin};
use crate::core::position::ChunkPos;
use crate::util::random::{InvalidBound, LegacyRandom, RandomSource};
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::codec::{DecodeError, boolean, float, int_range, object, optional, required};
use crate::worldgen::placement::provider::{HeightProvider, IntProvider, type_of};

/// `WorldCarver.getRange()`: how many chunks away a carver starts and still reaches.
pub const RANGE: i32 = 4;

/// `Codec.floatRange(0, 1)`.
fn probability(fields: &crate::worldgen::codec::Object) -> Result<f32, DecodeError> {
    let value = float(required(fields, "probability")?)?;
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(DecodeError::Field("carver probability"))
    }
}

/// `CaveWorldCarver`.
#[derive(Clone, Debug)]
pub struct CaveCarver {
    probability: f32,
    y: HeightProvider,
    count: IntProvider,
    thickness: FloatProvider,
    weird_thickness_bias: bool,
    room_vertical_radius_multiplier: FloatProvider,
    horizontal_radius_multiplier: FloatProvider,
    vertical_radius_multiplier: FloatProvider,
    start_vertical_radius_multiplier: FloatProvider,
    floor_level: FloatProvider,
}

/// `CanyonWorldCarver.Shape`.
#[derive(Clone, Debug)]
pub struct CanyonShape {
    distance_factor: FloatProvider,
    thickness: FloatProvider,
    width_smoothness: i32,
    horizontal_radius_factor: FloatProvider,
    vertical_radius_default_factor: f32,
    vertical_radius_center_factor: f32,
    y_scale: FloatProvider,
}

/// `CanyonWorldCarver`.
#[derive(Clone, Debug)]
pub struct CanyonCarver {
    probability: f32,
    y: HeightProvider,
    vertical_rotation: FloatProvider,
    shape: CanyonShape,
}

/// A decoded `worldgen/carver` element.
#[derive(Clone, Debug)]
pub enum WorldCarver {
    Cave(CaveCarver),
    Canyon(CanyonCarver),
}

impl WorldCarver {
    /// `WorldCarver.DIRECT_CODEC`: dispatched on `type`.
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let provider = |name| required(&fields, name).and_then(FloatProvider::decode);
        match type_of(&fields)?.as_str() {
            "minecraft:cave" => Ok(Self::Cave(CaveCarver {
                probability: probability(&fields)?,
                y: HeightProvider::decode(required(&fields, "y")?)?,
                count: IntProvider::decode_within(required(&fields, "count")?, 0, i32::MAX)?,
                thickness: FloatProvider::decode_at_least(required(&fields, "thickness")?, 0.0)?,
                weird_thickness_bias: optional(&fields, "weird_thickness_bias")
                    .map_or(Ok(false), boolean)?,
                room_vertical_radius_multiplier: provider("room_vertical_radius_multiplier")?,
                horizontal_radius_multiplier: provider("horizontal_radius_multiplier")?,
                vertical_radius_multiplier: provider("vertical_radius_multiplier")?,
                start_vertical_radius_multiplier: optional(
                    &fields,
                    "start_vertical_radius_multiplier",
                )
                .map_or(Ok(FloatProvider::Constant(1.0)), FloatProvider::decode)?,
                floor_level: FloatProvider::decode_within(
                    required(&fields, "floor_level")?,
                    -1.0,
                    1.0,
                )?,
            })),
            "minecraft:canyon" => {
                let shape = object(required(&fields, "shape")?)?;
                let shape_provider = |name| required(&shape, name).and_then(FloatProvider::decode);
                let shape_float = |name| required(&shape, name).and_then(float);
                Ok(Self::Canyon(CanyonCarver {
                    probability: probability(&fields)?,
                    y: HeightProvider::decode(required(&fields, "y")?)?,
                    vertical_rotation: provider("vertical_rotation")?,
                    shape: CanyonShape {
                        distance_factor: shape_provider("distance_factor")?,
                        thickness: shape_provider("thickness")?,
                        width_smoothness: int_range(
                            required(&shape, "width_smoothness")?,
                            "width_smoothness",
                            1,
                            i32::MAX,
                        )?,
                        horizontal_radius_factor: shape_provider("horizontal_radius_factor")?,
                        vertical_radius_default_factor: shape_float(
                            "vertical_radius_default_factor",
                        )?,
                        vertical_radius_center_factor: shape_float(
                            "vertical_radius_center_factor",
                        )?,
                        y_scale: shape_provider("y_scale")?,
                    },
                }))
            }
            _ => Err(DecodeError::Field("carver type")),
        }
    }

    /// `isStartChunk`: one float against the carver's probability.
    pub fn is_start_chunk(&self, random: &mut impl RandomSource) -> bool {
        let probability = match self {
            Self::Cave(cave) => cave.probability,
            Self::Canyon(canyon) => canyon.probability,
        };
        random.next_float() <= probability
    }

    /// `carve`: the paths that start in `source` and reach `chunk`, into `mask`.
    pub fn carve(
        &self,
        context: WorldGenerationContext,
        random: &mut impl RandomSource,
        chunk: ChunkPos,
        source: ChunkPos,
        mask: &mut CarvingMask,
    ) -> Result<(), InvalidBound> {
        match self {
            Self::Cave(cave) => cave.carve(context, random, chunk, source, mask),
            Self::Canyon(canyon) => canyon.carve(context, random, chunk, source, mask),
        }
    }
}

/// `WorldCarver.canReach`: whether the path, `current_step` of `total_steps` along at `(x, z)`,
/// can still come within reach of the chunk.
fn can_reach(
    chunk: ChunkPos,
    x: f64,
    z: f64,
    current_step: i32,
    total_steps: i32,
    thickness: f32,
) -> bool {
    let x_mid = f64::from(middle_block(chunk.x));
    let z_mid = f64::from(middle_block(chunk.z));
    let (xd, zd) = (x - x_mid, z - z_mid);
    let remaining = f64::from(total_steps.wrapping_sub(current_step));
    let rr = f64::from(thickness + 2.0 + 16.0);
    xd * xd + zd * zd - remaining * remaining <= rr * rr
}

/// `ChunkPos.getMiddleBlockX`.
fn middle_block(chunk: i32) -> i32 {
    chunk.wrapping_shl(4).wrapping_add(8)
}

/// `Mth.floor`.
fn floor(value: f64) -> i32 {
    value.floor() as i32
}

/// `WorldCarver.carveEllipsoid`: every block of the chunk inside the ellipsoid the skip checker
/// does not skip, from the top of the mask down.
fn carve_ellipsoid(
    chunk: ChunkPos,
    [x, y, z]: [f64; 3],
    horizontal_radius: f64,
    vertical_radius: f64,
    mask: &mut CarvingMask,
    mut should_skip: impl FnMut(f64, f64, f64, i32) -> bool,
) {
    let max_delta = 16.0 + horizontal_radius * 2.0;
    if (x - f64::from(middle_block(chunk.x))).abs() > max_delta
        || (z - f64::from(middle_block(chunk.z))).abs() > max_delta
    {
        return;
    }
    let (min_x, min_z) = (chunk.x.wrapping_shl(4), chunk.z.wrapping_shl(4));
    let min_x_index = floor(x - horizontal_radius)
        .wrapping_sub(min_x)
        .wrapping_sub(1)
        .max(0);
    let max_x_index = floor(x + horizontal_radius).wrapping_sub(min_x).min(15);
    let min_y = floor(y - vertical_radius).wrapping_sub(1).max(mask.min_y());
    let max_y = floor(y + vertical_radius).wrapping_add(1).min(mask.max_y());
    let min_z_index = floor(z - horizontal_radius)
        .wrapping_sub(min_z)
        .wrapping_sub(1)
        .max(0);
    let max_z_index = floor(z + horizontal_radius).wrapping_sub(min_z).min(15);
    for x_index in min_x_index..=max_x_index {
        let xd = (f64::from(min_x.wrapping_add(x_index)) + 0.5 - x) / horizontal_radius;
        for z_index in min_z_index..=max_z_index {
            let zd = (f64::from(min_z.wrapping_add(z_index)) + 0.5 - z) / horizontal_radius;
            if xd * xd + zd * zd >= 1.0 {
                continue;
            }
            for world_y in (min_y + 1..=max_y).rev() {
                let yd = (f64::from(world_y) - 0.5 - y) / vertical_radius;
                if !should_skip(xd, yd, zd, world_y) {
                    mask.carve(x_index, world_y, z_index);
                }
            }
        }
    }
}

/// `Mth.sin(float)`/`Mth.cos` of a float angle, as the float the table holds.
fn sin_f(angle: f32) -> f32 {
    sin(f64::from(angle))
}

fn cos_f(angle: f32) -> f32 {
    cos(f64::from(angle))
}

impl CaveCarver {
    fn carve(
        &self,
        context: WorldGenerationContext,
        random: &mut impl RandomSource,
        chunk: ChunkPos,
        source: ChunkPos,
        mask: &mut CarvingMask,
    ) -> Result<(), InvalidBound> {
        // `SectionPos.sectionToBlockCoord(getRange() * 2 - 1)`.
        let max_distance = (RANGE * 2 - 1) << 4;
        let cave_count = self.count.sample(random)?;
        for _ in 0..cave_count {
            let x = f64::from(
                source
                    .x
                    .wrapping_shl(4)
                    .wrapping_add(random.next_int_bound(16)?),
            );
            let y = f64::from(self.y.sample(random, context)?);
            let z = f64::from(
                source
                    .z
                    .wrapping_shl(4)
                    .wrapping_add(random.next_int_bound(16)?),
            );
            let horizontal_multiplier = f64::from(self.horizontal_radius_multiplier.sample(random));
            let vertical_multiplier = f64::from(self.vertical_radius_multiplier.sample(random));
            let start_vertical_multiplier =
                f64::from(self.start_vertical_radius_multiplier.sample(random));
            let floor_level = f64::from(self.floor_level.sample(random));
            let mut tunnels = 1;
            if random.next_int_bound(4)? == 0 {
                let y_scale = f64::from(self.room_vertical_radius_multiplier.sample(random));
                let thickness = 1.0 + random.next_float() * 6.0;
                // `createRoom`.
                let horizontal_radius = 1.5 + f64::from(sin_f(PI / 2.0) * thickness);
                let vertical_radius = horizontal_radius * y_scale;
                carve_ellipsoid(
                    chunk,
                    [x + 1.0, y, z],
                    horizontal_radius,
                    vertical_radius,
                    mask,
                    |xd, yd, zd, _| should_skip(xd, yd, zd, floor_level),
                );
                tunnels += random.next_int_bound(4)?;
            }
            for _ in 0..tunnels {
                let horizontal_rotation = random.next_float() * TAU;
                let vertical_rotation = (random.next_float() - 0.5) / 4.0;
                let thickness = self.thickness(random)?;
                let distance = max_distance - random.next_int_bound(max_distance / 4)?;
                let tunnel = Tunnel {
                    chunk,
                    horizontal_multiplier,
                    vertical_multiplier,
                    floor_level,
                    distance,
                };
                tunnel.create(
                    random.next_long(),
                    [x, y, z],
                    thickness,
                    horizontal_rotation,
                    vertical_rotation,
                    0,
                    start_vertical_multiplier,
                    mask,
                )?;
            }
        }
        Ok(())
    }

    fn thickness(&self, random: &mut impl RandomSource) -> Result<f32, InvalidBound> {
        let mut thickness = self.thickness.sample(random);
        if self.weird_thickness_bias && random.next_int_bound(10)? == 0 {
            thickness *= random.next_float() * random.next_float() * 3.0 + 1.0;
        }
        Ok(thickness)
    }
}

/// `CaveWorldCarver.shouldSkip`.
fn should_skip(xd: f64, yd: f64, zd: f64, floor_level: f64) -> bool {
    yd <= floor_level || xd * xd + yd * yd + zd * zd >= 1.0
}

/// What the tunnels of one cave share (the arguments `createTunnel` passes on unchanged).
struct Tunnel {
    chunk: ChunkPos,
    horizontal_multiplier: f64,
    vertical_multiplier: f64,
    /// The cave's `floorLevel`, for the skip checker.
    floor_level: f64,
    distance: i32,
}

impl Tunnel {
    /// `createTunnel`: the winding path from `step`, which may split in two once.
    #[allow(clippy::too_many_arguments)]
    fn create(
        &self,
        seed: i64,
        [mut x, mut y, mut z]: [f64; 3],
        thickness: f32,
        mut horizontal_rotation: f32,
        mut vertical_rotation: f32,
        step: i32,
        y_scale: f64,
        mask: &mut CarvingMask,
    ) -> Result<(), InvalidBound> {
        let dist = self.distance;
        let mut random = LegacyRandom::new(seed);
        let split_point = random.next_int_bound(dist / 2)? + dist / 4;
        let steep = random.next_int_bound(6)? == 0;
        let (mut y_rota, mut x_rota) = (0.0f32, 0.0f32);
        for current_step in step..dist {
            let horizontal_radius =
                1.5 + f64::from(sin_f(PI * current_step as f32 / dist as f32) * thickness);
            let vertical_radius = horizontal_radius * y_scale;
            let cos_x = cos_f(vertical_rotation);
            x += f64::from(cos_f(horizontal_rotation) * cos_x);
            y += f64::from(sin_f(vertical_rotation));
            z += f64::from(sin_f(horizontal_rotation) * cos_x);
            vertical_rotation *= if steep { 0.92 } else { 0.7 };
            vertical_rotation += x_rota * 0.1;
            horizontal_rotation += y_rota * 0.1;
            x_rota *= 0.9;
            y_rota *= 0.75;
            x_rota += (random.next_float() - random.next_float()) * random.next_float() * 2.0;
            y_rota += (random.next_float() - random.next_float()) * random.next_float() * 4.0;
            if current_step == split_point && thickness > 1.0 {
                for sign in [-1.0f32, 1.0] {
                    let seed = random.next_long();
                    let thickness = random.next_float() * 0.5 + 0.5;
                    self.create(
                        seed,
                        [x, y, z],
                        thickness,
                        horizontal_rotation + sign * (PI / 2.0),
                        vertical_rotation / 3.0,
                        current_step,
                        1.0,
                        mask,
                    )?;
                }
                return Ok(());
            }
            if random.next_int_bound(4)? != 0 {
                if !can_reach(self.chunk, x, z, current_step, dist, thickness) {
                    return Ok(());
                }
                let floor_level = self.floor_level;
                carve_ellipsoid(
                    self.chunk,
                    [x, y, z],
                    horizontal_radius * self.horizontal_multiplier,
                    vertical_radius * self.vertical_multiplier,
                    mask,
                    |xd, yd, zd, _| should_skip(xd, yd, zd, floor_level),
                );
            }
        }
        Ok(())
    }
}

impl CanyonCarver {
    fn carve(
        &self,
        context: WorldGenerationContext,
        random: &mut impl RandomSource,
        chunk: ChunkPos,
        source: ChunkPos,
        mask: &mut CarvingMask,
    ) -> Result<(), InvalidBound> {
        let max_distance = (RANGE * 2 - 1) * 16;
        let x = f64::from(
            source
                .x
                .wrapping_shl(4)
                .wrapping_add(random.next_int_bound(16)?),
        );
        let y = f64::from(self.y.sample(random, context)?);
        let z = f64::from(
            source
                .z
                .wrapping_shl(4)
                .wrapping_add(random.next_int_bound(16)?),
        );
        let horizontal_rotation = random.next_float() * TAU;
        let vertical_rotation = self.vertical_rotation.sample(random);
        let y_scale = f64::from(self.shape.y_scale.sample(random));
        let thickness = self.shape.thickness.sample(random);
        let distance = (max_distance as f32 * self.shape.distance_factor.sample(random)) as i32;
        self.do_carve(
            context,
            chunk,
            random.next_long(),
            [x, y, z],
            thickness,
            [horizontal_rotation, vertical_rotation],
            distance,
            y_scale,
            mask,
        )
    }

    /// `doCarve`.
    #[allow(clippy::too_many_arguments)]
    fn do_carve(
        &self,
        context: WorldGenerationContext,
        chunk: ChunkPos,
        seed: i64,
        [mut x, mut y, mut z]: [f64; 3],
        thickness: f32,
        [mut horizontal_rotation, mut vertical_rotation]: [f32; 2],
        distance: i32,
        y_scale: f64,
        mask: &mut CarvingMask,
    ) -> Result<(), InvalidBound> {
        let shape = &self.shape;
        let mut random = LegacyRandom::new(seed);
        // `initWidthFactors`.
        let mut width_factor_per_height = Vec::with_capacity(context.height.max(0) as usize);
        let mut width_factor = 1.0f32;
        for y_index in 0..context.height {
            if y_index == 0 || random.next_int_bound(shape.width_smoothness)? == 0 {
                width_factor = 1.0 + random.next_float() * random.next_float();
            }
            width_factor_per_height.push(width_factor * width_factor);
        }
        let (mut y_rota, mut x_rota) = (0.0f32, 0.0f32);
        for current_step in 0..distance {
            let mut horizontal_radius =
                1.5 + f64::from(sin_f(current_step as f32 * PI / distance as f32) * thickness);
            let vertical_radius = horizontal_radius * y_scale;
            horizontal_radius *= f64::from(shape.horizontal_radius_factor.sample(&mut random));
            // `updateVerticalRadius`.
            let vertical_multiplier =
                1.0 - (0.5 - current_step as f32 / distance as f32).abs() * 2.0;
            let factor = shape.vertical_radius_default_factor
                + shape.vertical_radius_center_factor * vertical_multiplier;
            let vertical_radius = f64::from(factor)
                * vertical_radius
                * f64::from(random.next_float() * (1.0 - 0.75) + 0.75);
            let xc = cos_f(vertical_rotation);
            let xs = sin_f(vertical_rotation);
            x += f64::from(cos_f(horizontal_rotation) * xc);
            y += f64::from(xs);
            z += f64::from(sin_f(horizontal_rotation) * xc);
            vertical_rotation *= 0.7;
            vertical_rotation += x_rota * 0.05;
            horizontal_rotation += y_rota * 0.05;
            x_rota *= 0.8;
            y_rota *= 0.5;
            x_rota += (random.next_float() - random.next_float()) * random.next_float() * 2.0;
            y_rota += (random.next_float() - random.next_float()) * random.next_float() * 4.0;
            if random.next_int_bound(4)? != 0 {
                if !can_reach(chunk, x, z, current_step, distance, thickness) {
                    return Ok(());
                }
                carve_ellipsoid(
                    chunk,
                    [x, y, z],
                    horizontal_radius,
                    vertical_radius,
                    mask,
                    |xd, yd, zd, world_y| {
                        // `shouldSkip`: the width factor of the layer below the block.
                        let index = world_y.wrapping_sub(context.min_y).wrapping_sub(1);
                        (xd * xd + zd * zd) * f64::from(width_factor_per_height[index as usize])
                            + yd * yd / 6.0
                            >= 1.0
                    },
                );
            }
        }
        Ok(())
    }
}
