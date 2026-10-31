//! `lake` (`LakeFeature`, deprecated in vanilla but in ~55 overworld biomes).

use serde_json::value::RawValue;

use super::simple::FeatureWorld;
use super::terrain::mark_above;
use super::{FeatureRegistry, ice};
use crate::block::BlockState;
use crate::block::flags::{FLAG_AIR, FLAG_LEGACY_SOLID, FLAG_LIQUID};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, object, required};
use crate::worldgen::placement::BlockPredicate;
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext, ProviderError};

#[derive(Clone, Debug)]
pub struct Lake {
    fluid: BlockStateProvider,
    barrier: BlockStateProvider,
    can_place_feature: BlockPredicate,
    can_replace_with_air_or_fluid: BlockPredicate,
    can_replace_with_barrier: BlockPredicate,
    cave_air: BlockState,
    ice: ice::IceBlocks,
    ice_state: BlockState,
}

impl Lake {
    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let names = ctx.names;
        let blocks = names.blocks;
        let state = |name: &str| {
            blocks
                .block_by_name(name)
                .map(|b| blocks.default_state(b))
                .ok_or(DecodeError::Field("lake block"))
        };
        Ok(Self {
            fluid: BlockStateProvider::decode(required(&fields, "fluid")?, ctx)?,
            barrier: BlockStateProvider::decode(required(&fields, "barrier")?, ctx)?,
            can_place_feature: BlockPredicate::decode(
                required(&fields, "can_place_feature")?,
                names,
            )?,
            can_replace_with_air_or_fluid: BlockPredicate::decode(
                required(&fields, "can_replace_with_air_or_fluid")?,
                names,
            )?,
            can_replace_with_barrier: BlockPredicate::decode(
                required(&fields, "can_replace_with_barrier")?,
                names,
            )?,
            cave_air: state("minecraft:cave_air")?,
            ice: ice::IceBlocks::new(names)?,
            ice_state: state("minecraft:ice")?,
        })
    }

    /// `LakeFeature.place`.
    pub fn place<W: FeatureWorld>(
        &self,
        registry: &FeatureRegistry,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        if origin.y <= world.min_y() + 4 {
            return Ok(false);
        }
        let origin = origin.offset(-8, -4, -8);
        let mut grid = [false; 2048];
        let spots = random.next_int_bound(4)? + 4;
        for _ in 0..spots {
            let xr = random.next_double() * 6.0 + 3.0;
            let yr = random.next_double() * 4.0 + 2.0;
            let zr = random.next_double() * 6.0 + 3.0;
            let xp = random.next_double() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0;
            let yp = random.next_double() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0;
            let zp = random.next_double() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0;
            for xx in 1..15 {
                for zz in 1..15 {
                    for yy in 1..7 {
                        let xd = (f64::from(xx) - xp) / (xr / 2.0);
                        let yd = (f64::from(yy) - yp) / (yr / 2.0);
                        let zd = (f64::from(zz) - zp) / (zr / 2.0);
                        if xd * xd + yd * yd + zd * zd < 1.0 {
                            grid[index(xx, zz, yy)] = true;
                        }
                    }
                }
            }
        }

        let fluid = self.fluid.state(world, random, origin)?;
        for xx in 0..16 {
            for zz in 0..16 {
                for yy in 0..8 {
                    if edge(&grid, xx, zz, yy) {
                        let state = world.block_state(origin.offset(xx, yy, zz))?;
                        let data = world.state_data();
                        if yy >= 4 && data.has(state, FLAG_LIQUID) {
                            return Ok(false);
                        }
                        if yy < 4 && !data.has(state, FLAG_LEGACY_SOLID) && state != fluid {
                            return Ok(false);
                        }
                        if !self
                            .can_place_feature
                            .test(world, origin.offset(xx, yy, zz))?
                        {
                            return Ok(false);
                        }
                    }
                }
            }
        }

        for xx in 0..16 {
            for zz in 0..16 {
                for yy in 0..8 {
                    if grid[index(xx, zz, yy)] {
                        let place = origin.offset(xx, yy, zz);
                        if self.can_replace_with_air_or_fluid.test(world, place)? {
                            let air = yy >= 4;
                            world.set_block(place, if air { self.cave_air } else { fluid }, 2)?;
                            if air {
                                world.schedule_tick(place, 0)?;
                                mark_above(world, place)?;
                            }
                        }
                    }
                }
            }
        }

        let barrier = self.barrier.state(world, random, origin)?;
        if !world.state_data().has(barrier, FLAG_AIR) {
            for xx in 0..16 {
                for zz in 0..16 {
                    for yy in 0..8 {
                        if edge(&grid, xx, zz, yy) && (yy < 4 || random.next_int_bound(2)? != 0) {
                            let at = origin.offset(xx, yy, zz);
                            let state = world.block_state(at)?;
                            if world.state_data().has(state, FLAG_LEGACY_SOLID)
                                && self.can_replace_with_barrier.test(world, at)?
                            {
                                world.set_block(at, barrier, 2)?;
                                mark_above(world, at)?;
                            }
                        }
                    }
                }
            }
        }

        let data = world.state_data();
        let water = matches!(
            data.fluid_name(data.fluid(fluid).fluid),
            "minecraft:water" | "minecraft:flowing_water"
        );
        if water {
            for xx in 0..16 {
                for zz in 0..16 {
                    let at = origin.offset(xx, 4, zz);
                    let biome = world.biome(at)?;
                    if ice::should_freeze(registry, &self.ice, world, biome, at, false)?
                        && self.can_replace_with_air_or_fluid.test(world, at)?
                    {
                        world.set_block(at, self.ice_state, 2)?;
                    }
                }
            }
        }
        Ok(true)
    }
}

fn index(xx: i32, zz: i32, yy: i32) -> usize {
    ((xx * 16 + zz) * 8 + yy) as usize
}

/// A cell outside the lake with one inside next to it.
fn edge(grid: &[bool; 2048], xx: i32, zz: i32, yy: i32) -> bool {
    !grid[index(xx, zz, yy)]
        && (xx < 15 && grid[index(xx + 1, zz, yy)]
            || xx > 0 && grid[index(xx - 1, zz, yy)]
            || zz < 15 && grid[index(xx, zz + 1, yy)]
            || zz > 0 && grid[index(xx, zz - 1, yy)]
            || yy < 7 && grid[index(xx, zz, yy + 1)]
            || yy > 0 && grid[index(xx, zz, yy - 1)])
}
