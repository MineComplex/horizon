//! `minecraft:ore` (`OreFeature`, with `AbstractOreFeature.canPlaceOre`): an interpolated chain of
//! spheres written straight into the chunk sections through `BulkSectionAccess`, so no heightmap,
//! light or post-processing follows a write. Adapted from Horizon's `ore_feature`.
//!
//! `BulkSectionAccess` only caches section lookups (and pins them against a concurrent unload);
//! here a read or write names the section by position through [`FeatureWorld`].

use serde_json::value::RawValue;

use super::FeatureWorld;
use super::rule_test::RuleTest;
use crate::block::BlockState;
use crate::block::flags::FLAG_AIR;
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::jmath;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{DecodeError, Object, float, int_range, object, required};
use crate::worldgen::placement::DecodeContext;
use crate::worldgen::state_provider::ProviderError;

/// `AbstractOreFeature`'s fields: the targets (`BlockReplacement`s), the size and the chance of
/// discarding a block that touches air.
#[derive(Clone, Debug)]
pub struct OreConfig {
    pub targets: Vec<(RuleTest, BlockState)>,
    pub size: i32,
    pub discard_chance_on_air_exposure: f32,
}

impl OreConfig {
    /// `AbstractOreFeature.makeCodec`: targets, size 0..=64, the air exposure chance 0..=1.
    pub(crate) fn decode(fields: &Object, names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let targets: Vec<Box<RawValue>> = serde_json::from_str(required(fields, "targets")?.get())?;
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
        let chance = float(required(fields, "discard_chance_on_air_exposure")?)?;
        if !(0.0..=1.0).contains(&chance) {
            return Err(DecodeError::Field("discard_chance_on_air_exposure"));
        }
        Ok(Self {
            targets,
            size: int_range(required(fields, "size")?, "size", 0, 64)?,
            discard_chance_on_air_exposure: chance,
        })
    }
}

/// What `place` hands `doPlace`: the chain's ends and the box it may touch.
struct Area {
    x0: f64,
    x1: f64,
    z0: f64,
    z1: f64,
    y0: f64,
    y1: f64,
    start: [i32; 3],
    size_xz: i32,
    size_y: i32,
}

/// `OreFeature.place`.
pub fn place<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    config: &OreConfig,
) -> Result<bool, ProviderError> {
    let size = config.size;
    let dir = random.next_float() * std::f32::consts::PI;
    let spread_xy = size as f32 / 8.0;
    let max_radius = ((size as f32 / 16.0 * 2.0 + 1.0) / 2.0).ceil() as i32;
    let spread = f64::from(spread_xy);
    let (sin, cos) = (jmath::sin(f64::from(dir)), jmath::cos(f64::from(dir)));
    let x0 = f64::from(origin.x) + sin * spread;
    let x1 = f64::from(origin.x) - sin * spread;
    let z0 = f64::from(origin.z) + cos * spread;
    let z1 = f64::from(origin.z) - cos * spread;
    let y0 = f64::from(
        origin
            .y
            .wrapping_add(random.next_int_bound(3)?)
            .wrapping_sub(2),
    );
    let y1 = f64::from(
        origin
            .y
            .wrapping_add(random.next_int_bound(3)?)
            .wrapping_sub(2),
    );
    let spread_ceil = spread_xy.ceil() as i32;
    let x_start = origin.x.wrapping_sub(spread_ceil).wrapping_sub(max_radius);
    let y_start = origin.y.wrapping_sub(2).wrapping_sub(max_radius);
    let z_start = origin.z.wrapping_sub(spread_ceil).wrapping_sub(max_radius);
    let size_xz = 2 * (spread_ceil + max_radius);
    let size_y = 2 * (2 + max_radius);

    for x in x_start..=x_start.wrapping_add(size_xz) {
        for z in z_start..=z_start.wrapping_add(size_xz) {
            if y_start <= world.height(Heightmap::OceanFloorWg, x, z)? {
                let area = Area {
                    x0,
                    x1,
                    z0,
                    z1,
                    y0,
                    y1,
                    start: [x_start, y_start, z_start],
                    size_xz,
                    size_y,
                };
                return do_place(world, random, config, &area);
            }
        }
    }
    Ok(false)
}

/// `OreFeature.doPlace`.
fn do_place<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    config: &OreConfig,
    area: &Area,
) -> Result<bool, ProviderError> {
    let size = config.size as usize;
    let [x_start, y_start, z_start] = area.start;
    let (size_xz, size_y) = (area.size_xz, area.size_y);
    let mut data = vec![[0.0_f64; 4]; size];
    for (i, entry) in data.iter_mut().enumerate() {
        let step = i as f32 / config.size as f32;
        let alpha = f64::from(step);
        let xx = area.x0 + alpha * (area.x1 - area.x0);
        let yy = area.y0 + alpha * (area.y1 - area.y0);
        let zz = area.z0 + alpha * (area.z1 - area.z0);
        let ss = random.next_double() * f64::from(config.size) / 16.0;
        let sine = crate::core::geometry::trig::sin(f64::from(std::f32::consts::PI * step));
        let r = (f64::from(sine + 1.0) * ss + 1.0) / 2.0;
        *entry = [xx, yy, zz, r];
    }
    // A sphere swallowed by a bigger one is dropped.
    for i1 in 0..size.saturating_sub(1) {
        if data[i1][3] <= 0.0 {
            continue;
        }
        for i2 in i1 + 1..size {
            if data[i2][3] <= 0.0 {
                continue;
            }
            let dx = data[i1][0] - data[i2][0];
            let dy = data[i1][1] - data[i2][1];
            let dz = data[i1][2] - data[i2][2];
            let dr = data[i1][3] - data[i2][3];
            if dr * dr > dx * dx + dy * dy + dz * dz {
                if dr > 0.0 {
                    data[i2][3] = -1.0;
                } else {
                    data[i1][3] = -1.0;
                }
            }
        }
    }

    let mut tested: Vec<u64> = Vec::new();
    let mut placed = 0;
    for [xx, yy, zz, r] in data {
        if r < 0.0 {
            continue;
        }
        let x_min = ((xx - r).floor() as i32).max(x_start);
        let y_min = ((yy - r).floor() as i32).max(y_start);
        let z_min = ((zz - r).floor() as i32).max(z_start);
        let x_max = ((xx + r).floor() as i32).max(x_min);
        let y_max = ((yy + r).floor() as i32).max(y_min);
        let z_max = ((zz + r).floor() as i32).max(z_min);
        for x in x_min..=x_max {
            let xd = (f64::from(x) + 0.5 - xx) / r;
            if xd * xd >= 1.0 {
                continue;
            }
            for y in y_min..=y_max {
                let yd = (f64::from(y) + 0.5 - yy) / r;
                if xd * xd + yd * yd >= 1.0 {
                    continue;
                }
                for z in z_min..=z_max {
                    let zd = (f64::from(z) + 0.5 - zz) / r;
                    if xd * xd + yd * yd + zd * zd >= 1.0 || world.outside_build_height(y) {
                        continue;
                    }
                    // `BitSet.get/set`: a negative index throws.
                    let index = i64::from(x.wrapping_sub(x_start))
                        + i64::from(y.wrapping_sub(y_start)) * i64::from(size_xz)
                        + i64::from(z.wrapping_sub(z_start))
                            * i64::from(size_xz)
                            * i64::from(size_y);
                    let index = usize::try_from(index)
                        .map_err(|_| ProviderError::Invalid("ore bit set index is negative"))?;
                    let (word, bit) = (index / 64, 1_u64 << (index % 64));
                    if tested.len() <= word {
                        tested.resize(word + 1, 0);
                    }
                    if tested[word] & bit != 0 {
                        continue;
                    }
                    tested[word] |= bit;
                    let pos = BlockPos::new(x, y, z);
                    if !world.ensure_can_write(pos)? {
                        continue;
                    }
                    let Some(state) = world.section_block(pos)? else {
                        continue;
                    };
                    for (test, replacement) in &config.targets {
                        if can_place_ore(world, random, config, state, test, pos)? {
                            world.set_section_block(pos, *replacement)?;
                            placed += 1;
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(placed > 0)
}

/// `AbstractOreFeature.canPlaceOre`; the neighbours are read through the sections (air outside).
fn can_place_ore<W: FeatureWorld>(
    world: &W,
    random: &mut impl RandomSource,
    config: &OreConfig,
    state: BlockState,
    test: &RuleTest,
    pos: BlockPos,
) -> Result<bool, ProviderError> {
    if !test.test(world.blocks(), state, pos, random) {
        return Ok(false);
    }
    if should_skip_air_check(random, config.discard_chance_on_air_exposure) {
        return Ok(true);
    }
    for direction in Direction::ALL {
        let n = direction.normal();
        let neighbour = world
            .section_block(pos.offset(n[0], n[1], n[2]))?
            .unwrap_or(BlockState::AIR);
        if world.state_data().has(neighbour, FLAG_AIR) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `AbstractOreFeature.shouldSkipAirCheck`.
fn should_skip_air_check(random: &mut impl RandomSource, discard: f32) -> bool {
    discard <= 0.0 || (discard < 1.0 && random.next_float() >= discard)
}
