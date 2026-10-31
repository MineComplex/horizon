//! The Nether features: `huge_fungus` (`HugeFungusFeature`), `netherrack_replace_blobs`
//! (`ReplaceBlobsFeature`) and `delta_feature` (`DeltaFeature`).

use serde_json::value::RawValue;

use super::common::{default_state, is_block, relative};
use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::{FLAG_AIR, FLAG_REPLACEABLE};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, boolean, object, optional, required};
use crate::worldgen::placement::{BlockPredicate, DecodeContext, IntProvider, PlacementWorld};
use crate::worldgen::state_provider::{ProviderContext, ProviderError};

/// A feature of this module.
#[derive(Clone, Debug)]
pub enum Nether {
    HugeFungus(Box<HugeFungus>),
    ReplaceBlobs(ReplaceBlobs),
    Delta(Delta),
}

impl Nether {
    pub fn decode(
        kind: &str,
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        let fields = object(raw)?;
        let names = ctx.names;
        Ok(Some(match kind {
            "minecraft:huge_fungus" => Self::HugeFungus(Box::new(HugeFungus {
                valid_base: names.block_state(required(&fields, "valid_base_block")?)?,
                stem: names.block_state(required(&fields, "stem_state")?)?,
                hat: names.block_state(required(&fields, "hat_state")?)?,
                decor: names.block_state(required(&fields, "decor_state")?)?,
                replaceable: BlockPredicate::decode(
                    required(&fields, "replaceable_blocks")?,
                    names,
                )?,
                planted: optional(&fields, "planted").map_or(Ok(false), boolean)?,
                air: default_state(names.blocks, "minecraft:air")?,
                weeping_vines: default_state(names.blocks, "minecraft:weeping_vines")?,
                weeping_vines_plant: default_state(names.blocks, "minecraft:weeping_vines_plant")?,
            })),
            "minecraft:netherrack_replace_blobs" => Self::ReplaceBlobs(ReplaceBlobs {
                target: names.block_state(required(&fields, "target")?)?,
                state: names.block_state(required(&fields, "state")?)?,
                radius: IntProvider::decode_within(required(&fields, "radius")?, 0, 12)?,
            }),
            "minecraft:delta_feature" => Self::Delta(Delta::decode(&fields, names)?),
            _ => return Ok(None),
        }))
    }

    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match self {
            Self::HugeFungus(f) => f.place(world, random, origin),
            Self::ReplaceBlobs(f) => f.place(world, random, origin),
            Self::Delta(f) => f.place(world, random, origin),
        }
    }
}

/// `Mth.nextInt(random, min, max)`.
fn next_int(random: &mut impl RandomSource, min: i32, max: i32) -> Result<i32, ProviderError> {
    Ok(if min >= max {
        min
    } else {
        random.next_int_bound(max - min + 1)? + min
    })
}

#[derive(Clone, Debug)]
pub struct HugeFungus {
    valid_base: BlockState,
    stem: BlockState,
    hat: BlockState,
    decor: BlockState,
    replaceable: BlockPredicate,
    planted: bool,
    air: BlockState,
    weeping_vines: BlockState,
    weeping_vines_plant: BlockState,
}

impl HugeFungus {
    /// `HugeFungusFeature.place`.
    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let below = world.block_state(origin.offset(0, -1, 0))?;
        if world.blocks().block(below) != world.blocks().block(self.valid_base) {
            return Ok(false);
        }
        let mut total_height = next_int(random, 4, 13)?;
        if random.next_int_bound(12)? == 0 {
            total_height *= 2;
        }
        if !self.planted {
            // `chunkGenerator.getGenDepth()`: the generator's height, which `generation()` carries
            // (the level's and the generator's, the smaller).
            let max_height = world.generation().height;
            if origin.y + total_height + 1 >= max_height {
                return Ok(false);
            }
        }
        let is_huge = !self.planted && random.next_float() < 0.06;
        // `Block.UPDATE_NONE`.
        world.set_block(origin, self.air, 260)?;
        self.place_stem(world, random, origin, total_height, is_huge)?;
        self.place_hat(world, random, origin, total_height, is_huge)?;
        Ok(true)
    }

    fn replaceable<W: FeatureWorld>(
        &self,
        world: &W,
        pos: BlockPos,
        check_plants: bool,
    ) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        Ok(world.state_data().has(state, FLAG_REPLACEABLE)
            || (check_plants && self.replaceable.test(world, pos)?))
    }

    /// `level.destroyBlock(pos, true)`: the block goes, leaving its fluid (drops are not made).
    fn destroy<W: FeatureWorld>(&self, world: &mut W, pos: BlockPos) -> Result<(), ProviderError> {
        let state = world.block_state(pos)?;
        if world.state_data().has(state, FLAG_AIR) {
            return Ok(());
        }
        if world.state_data().fluid(state).fluid != 0 {
            return Err(ProviderError::Invalid("a planted fungus over a fluid"));
        }
        world.set_block(pos, self.air, 3)?;
        Ok(())
    }

    fn place_stem<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
        total_height: i32,
        is_huge: bool,
    ) -> Result<(), ProviderError> {
        let radius = i32::from(is_huge);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let corner = is_huge && dx.abs() == radius && dz.abs() == radius;
                for dy in 0..total_height {
                    let pos = origin.offset(dx, dy, dz);
                    if !self.replaceable(world, pos, true)? {
                        continue;
                    }
                    if self.planted {
                        let below = world.block_state(pos.offset(0, -1, 0))?;
                        if !world.state_data().has(below, FLAG_AIR) {
                            self.destroy(world, pos)?;
                        }
                        world.set_block(pos, self.stem, 3)?;
                    } else if corner {
                        if random.next_float() < 0.1 {
                            world.set_block(pos, self.stem, 3)?;
                        }
                    } else {
                        world.set_block(pos, self.stem, 3)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn place_hat<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
        total_height: i32,
        is_huge: bool,
    ) -> Result<(), ProviderError> {
        let place_vines = is_block(world, self.hat, "minecraft:nether_wart_block");
        let hat_height = (random.next_int_bound(1 + total_height / 3)? + 5).min(total_height);
        let hat_start_y = total_height - hat_height;
        for dy in hat_start_y..=total_height {
            let mut radius = if dy < total_height - random.next_int_bound(3)? {
                2
            } else {
                1
            };
            if hat_height > 8 && dy < hat_start_y + 4 {
                radius = 3;
            }
            if is_huge {
                radius += 1;
            }
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let edge_x = dx == -radius || dx == radius;
                    let edge_z = dz == -radius || dz == radius;
                    let inside = !edge_x && !edge_z && dy != total_height;
                    let corner = edge_x && edge_z;
                    let hat_bottom = dy < hat_start_y + 3;
                    let pos = origin.offset(dx, dy, dz);
                    if !self.replaceable(world, pos, false)? {
                        continue;
                    }
                    if self.planted {
                        let below = world.block_state(pos.offset(0, -1, 0))?;
                        if !world.state_data().has(below, FLAG_AIR) {
                            self.destroy(world, pos)?;
                        }
                    }
                    let vines = |chance: f32| if place_vines { chance } else { 0.0 };
                    if hat_bottom {
                        if !inside {
                            self.place_hat_drop_block(world, random, pos, place_vines)?;
                        }
                    } else if inside {
                        self.place_hat_block(world, random, pos, 0.1, 0.2, vines(0.1))?;
                    } else if corner {
                        self.place_hat_block(world, random, pos, 0.01, 0.7, vines(0.083))?;
                    } else {
                        self.place_hat_block(world, random, pos, 5.0e-4, 0.98, vines(0.07))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn place_hat_block<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
        decor_probability: f32,
        hat_probability: f32,
        vines_probability: f32,
    ) -> Result<(), ProviderError> {
        if random.next_float() < decor_probability {
            world.set_block(pos, self.decor, 3)?;
        } else if random.next_float() < hat_probability {
            world.set_block(pos, self.hat, 3)?;
            if random.next_float() < vines_probability {
                self.try_place_weeping_vines(world, random, pos)?;
            }
        }
        Ok(())
    }

    fn place_hat_drop_block<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
        place_vines: bool,
    ) -> Result<(), ProviderError> {
        let below = world.block_state(pos.offset(0, -1, 0))?;
        if world.blocks().block(below) == world.blocks().block(self.hat) {
            world.set_block(pos, self.hat, 3)?;
        } else if f64::from(random.next_float()) < 0.15 {
            world.set_block(pos, self.hat, 3)?;
            if place_vines && random.next_int_bound(11)? == 0 {
                self.try_place_weeping_vines(world, random, pos)?;
            }
        }
        Ok(())
    }

    fn try_place_weeping_vines<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        hat_pos: BlockPos,
    ) -> Result<(), ProviderError> {
        let place = relative(hat_pos, Direction::Down);
        if !super::common::is_air(world, place)? {
            return Ok(());
        }
        let mut goal = next_int(random, 1, 5)?;
        if random.next_int_bound(7)? == 0 {
            goal *= 2;
        }
        // `placeWeepingVinesColumn(level, random, pos, goal, 23, 25)`.
        let mut pos = place;
        for height in 0..=goal {
            if super::common::is_air(world, pos)? {
                if height == goal || !super::common::is_air(world, pos.offset(0, -1, 0))? {
                    let age = next_int(random, 23, 25)?;
                    let head = world
                        .blocks()
                        .with_property(self.weeping_vines, "age", &age.to_string())
                        .ok_or(ProviderError::Invalid("weeping vines age"))?;
                    world.set_block(pos, head, 2)?;
                    break;
                }
                world.set_block(pos, self.weeping_vines_plant, 2)?;
            }
            pos = pos.offset(0, -1, 0);
        }
        Ok(())
    }
}

/// `BlockPos.withinClippedManhattan(center, rx, ry, rz)` as Leaf iterates it: by distance from the
/// centre, then ascending x, ascending y, the positive z before its mirror.
fn within_clipped_manhattan(center: BlockPos, rx: i32, ry: i32, rz: i32) -> Vec<BlockPos> {
    let mut out = Vec::new();
    for depth in 0..=rx.max(ry).max(rz) {
        let max_x = rx.min(depth);
        for x in -max_x..=max_x {
            let max_y = ry.min(depth - x.abs());
            for y in -max_y..=max_y {
                let z = depth - x.abs() - y.abs();
                if z > rz {
                    continue;
                }
                out.push(center.offset(x, y, z));
                if z != 0 {
                    out.push(center.offset(x, y, -z));
                }
            }
        }
    }
    out
}

fn manhattan(a: BlockPos, b: BlockPos) -> i32 {
    let d = |a: i32, b: i32| b.wrapping_sub(a).wrapping_abs() as f32;
    (d(a.x, b.x) + d(a.y, b.y) + d(a.z, b.z)) as i32
}

#[derive(Clone, Debug)]
pub struct ReplaceBlobs {
    target: BlockState,
    state: BlockState,
    radius: IntProvider,
}

impl ReplaceBlobs {
    /// `ReplaceBlobsFeature.place`.
    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let target = world.blocks().block(self.target);
        let min_y = world.min_y();
        let level = world.level_generation();
        let max_y = level.min_y.wrapping_add(level.height).wrapping_sub(1);
        // `clamp(Axis.Y, minY + 1, maxY)`: `Mth.clamp` with inverted bounds takes the minimum first.
        let mut cursor = BlockPos::new(
            origin.x,
            if origin.y < min_y + 1 {
                min_y + 1
            } else {
                origin.y.min(max_y)
            },
            origin.z,
        );
        let center = loop {
            if cursor.y <= min_y + 1 {
                return Ok(false);
            }
            if world.blocks().block(world.block_state(cursor)?) == target {
                break cursor;
            }
            cursor = cursor.offset(0, -1, 0);
        };
        let rx = self.radius.sample(random)?;
        let ry = self.radius.sample(random)?;
        let rz = self.radius.sample(random)?;
        let maximum = rx.max(ry.max(rz));
        let mut replaced = false;
        for pos in within_clipped_manhattan(center, rx, ry, rz) {
            if manhattan(pos, center) > maximum {
                break;
            }
            let state = world.block_state(pos)?;
            if world.blocks().block(state) == target {
                world.set_block(pos, self.state, 3)?;
                replaced = true;
            }
        }
        Ok(replaced)
    }
}

#[derive(Clone, Debug)]
pub struct Delta {
    contents: BlockState,
    rim: BlockState,
    size: IntProvider,
    rim_size: IntProvider,
    cannot_replace: Vec<crate::block::BlockId>,
}

impl Delta {
    fn decode(
        fields: &crate::worldgen::codec::Object,
        names: &DecodeContext<'_>,
    ) -> Result<Self, DecodeError> {
        let cannot_replace = [
            "minecraft:bedrock",
            "minecraft:nether_bricks",
            "minecraft:nether_brick_fence",
            "minecraft:nether_brick_stairs",
            "minecraft:nether_wart",
            "minecraft:chest",
            "minecraft:spawner",
        ]
        .iter()
        .map(|name| {
            names
                .blocks
                .block_by_name(name)
                .ok_or(DecodeError::Field("block"))
        })
        .collect::<Result<_, _>>()?;
        Ok(Self {
            contents: names.block_state(required(fields, "contents")?)?,
            rim: names.block_state(required(fields, "rim")?)?,
            size: IntProvider::decode_within(required(fields, "size")?, 0, 16)?,
            rim_size: IntProvider::decode_within(required(fields, "rim_size")?, 0, 16)?,
            cannot_replace,
        })
    }

    /// `DeltaFeature.place`.
    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let mut any = false;
        let spawn_rim = random.next_double() < 0.9;
        let rim_x = if spawn_rim {
            self.rim_size.sample(random)?
        } else {
            0
        };
        let rim_z = if spawn_rim {
            self.rim_size.sample(random)?
        } else {
            0
        };
        let has_rim = spawn_rim && rim_x != 0 && rim_z != 0;
        let radius_x = self.size.sample(random)?;
        let radius_z = self.size.sample(random)?;
        let limit = radius_x.max(radius_z);
        for pos in within_clipped_manhattan(origin, radius_x, 0, radius_z) {
            if manhattan(pos, origin) > limit {
                break;
            }
            if self.is_clear(world, pos)? {
                if has_rim {
                    any = true;
                    world.set_block(pos, self.rim, 3)?;
                }
                let shifted = pos.offset(rim_x, 0, rim_z);
                if self.is_clear(world, shifted)? {
                    any = true;
                    world.set_block(shifted, self.contents, 3)?;
                }
            }
        }
        Ok(any)
    }

    fn is_clear(&self, world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        let block = world.blocks().block(state);
        if block == world.blocks().block(self.contents) || self.cannot_replace.contains(&block) {
            return Ok(false);
        }
        for direction in Direction::ALL {
            let air = world
                .state_data()
                .has(world.block_state(relative(pos, direction))?, FLAG_AIR);
            if (air && direction != Direction::Up) || (!air && direction == Direction::Up) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
