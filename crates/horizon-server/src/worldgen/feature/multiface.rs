//! `multiface_growth` (`MultifaceGrowthFeature`) for the two `MultifaceSpreadeableBlock`s, glow
//! lichen and sculk vein, with `MultifaceSpreader`'s world generation paths.

use serde_json::value::RawValue;

use super::common::{HORIZONTAL, face_name, is_block, relative, shuffle};
use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::{FLAG_AIR, FLAG_REPLACEABLE, SupportType};
use crate::block::survival::can_attach_to;
use crate::core::geometry::{Axis, Direction};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, boolean, float, int_range, object, optional, required};
use crate::worldgen::placement::{DecodeContext, PlacementWorld};
use crate::worldgen::state_provider::{ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

#[derive(Clone, Debug)]
pub struct MultifaceGrowth {
    block: BlockState,
    /// Sculk vein: its spreader config also refuses sculk and fire and takes replaceable blocks.
    sculk: bool,
    search_range: i32,
    directions: Vec<Direction>,
    chance_of_spreading: f32,
    can_be_placed_on: BiomeSet,
    fire: BiomeSet,
}

/// `MultifaceSpreader.SpreadType`.
#[derive(Clone, Copy)]
pub(super) enum Spread {
    SamePosition,
    SamePlane,
    WrapAround,
}

/// `MultifaceSpreader.DEFAULT_SPREAD_ORDER`.
pub(super) const DEFAULT_SPREAD_ORDER: [Spread; 3] =
    [Spread::SamePosition, Spread::SamePlane, Spread::WrapAround];

impl MultifaceGrowth {
    /// The sculk vein's spreader, which the sculk patch drives.
    pub(super) fn sculk_vein(names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let blocks = names.blocks;
        let id = blocks
            .block_by_name("minecraft:sculk_vein")
            .ok_or(DecodeError::Field("sculk vein"))?;
        Ok(Self {
            block: blocks.default_state(id),
            sculk: true,
            search_range: 10,
            directions: Vec::new(),
            chance_of_spreading: 0.5,
            can_be_placed_on: BiomeSet::new(Vec::new()),
            fire: names.block_tag("minecraft:fire")?,
        })
    }

    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let names = ctx.names;
        let name = crate::worldgen::codec::identifier(&crate::worldgen::codec::string(required(
            &fields, "block",
        )?)?)?;
        // The `MultifaceSpreadeableBlock`s of 26.3.
        let sculk = match name.as_str() {
            "minecraft:glow_lichen" => false,
            "minecraft:sculk_vein" => true,
            _ => return Err(DecodeError::Field("multiface spreadeable block")),
        };
        let flag = |key: &str| -> Result<bool, DecodeError> {
            optional(&fields, key).map_or(Ok(false), boolean)
        };
        let mut directions = Vec::new();
        if flag("can_place_on_ceiling")? {
            directions.push(Direction::Up);
        }
        if flag("can_place_on_floor")? {
            directions.push(Direction::Down);
        }
        if flag("can_place_on_wall")? {
            directions.extend(HORIZONTAL);
        }
        let blocks = names.blocks;
        let id = blocks
            .block_by_name(&name)
            .ok_or(DecodeError::Field("multiface block"))?;
        Ok(Self {
            block: blocks.default_state(id),
            sculk,
            search_range: optional(&fields, "search_range")
                .map_or(Ok(10), |r| int_range(r, "search_range", 1, 64))?,
            directions,
            chance_of_spreading: optional(&fields, "chance_of_spreading").map_or(Ok(0.5), float)?,
            can_be_placed_on: names.block_set(required(&fields, "can_be_placed_on")?)?,
            fire: names.block_tag("minecraft:fire")?,
        })
    }

    fn air_or_water(&self, world: &impl PlacementWorld, state: BlockState) -> bool {
        world.state_data().has(state, FLAG_AIR) || is_block(world, state, "minecraft:water")
    }

    fn is_this(&self, world: &impl PlacementWorld, state: BlockState) -> bool {
        world.blocks().block(state) == world.blocks().block(self.block)
    }

    fn has_face(&self, world: &impl PlacementWorld, state: BlockState, face: Direction) -> bool {
        world.blocks().value(state, face_name(face)) == Some("true")
    }

    /// `MultifaceGrowthFeature.place`.
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let here = world.block_state(origin)?;
        if !self.air_or_water(world, here) {
            return Ok(false);
        }
        let mut directions = self.directions.clone();
        shuffle(&mut directions, random)?;
        if self.place_growth(world, random, origin, here, &directions)? {
            return Ok(true);
        }
        for &search in &directions {
            let mut placement: Vec<Direction> = (self.directions.iter().copied())
                .filter(|d| *d != search.opposite())
                .collect();
            shuffle(&mut placement, random)?;
            for _ in 0..self.search_range {
                // `setWithOffset(origin, searchDirection)` is the same neighbour every time.
                let pos = relative(origin, search);
                let state = world.block_state(pos)?;
                if !self.air_or_water(world, state) && !self.is_this(world, state) {
                    break;
                }
                if self.place_growth(world, random, pos, state, &placement)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// `placeGrowthIfPossible`.
    fn place_growth<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
        old: BlockState,
        directions: &[Direction],
    ) -> Result<bool, ProviderError> {
        for &direction in directions {
            let neighbour = world.block_state(relative(pos, direction))?;
            let block = world.blocks().block(neighbour).0;
            if !self.can_be_placed_on.contains(block) {
                continue;
            }
            let Some(state) = self.state_for_placement(world, old, pos, direction)? else {
                return Ok(false);
            };
            world.set_block(pos, state, 3)?;
            world.mark_postprocessing(pos)?;
            if random.next_float() < self.chance_of_spreading {
                self.spread_toward_random_direction(world, state, pos, direction, random)?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// `MultifaceBlock.isValidStateForPlacement`.
    pub(super) fn valid_for_placement(
        &self,
        world: &impl PlacementWorld,
        old: BlockState,
        pos: BlockPos,
        direction: Direction,
    ) -> Result<bool, ProviderError> {
        if self.is_this(world, old) && self.has_face(world, old, direction) {
            return Ok(false);
        }
        let neighbour = world.block_state(relative(pos, direction))?;
        can_attach_to(world.state_data(), neighbour, direction)
            .ok_or(ProviderError::Invalid("a dynamic shape"))
    }

    /// `MultifaceBlock.getStateForPlacement(old, level, pos, direction)`.
    pub(super) fn state_for_placement(
        &self,
        world: &impl PlacementWorld,
        old: BlockState,
        pos: BlockPos,
        direction: Direction,
    ) -> Result<Option<BlockState>, ProviderError> {
        if !self.valid_for_placement(world, old, pos, direction)? {
            return Ok(None);
        }
        let blocks = world.blocks();
        let base = if self.is_this(world, old) {
            old
        } else {
            let fluid = world.state_data().fluid(old);
            let water_source = fluid.amount == 8
                && world.state_data().fluid_name(fluid.fluid) == "minecraft:water";
            if water_source {
                blocks
                    .with_property(self.block, "waterlogged", "true")
                    .ok_or(ProviderError::Invalid("no waterlogged"))?
            } else {
                self.block
            }
        };
        blocks
            .with_property(base, face_name(direction), "true")
            .map(Some)
            .ok_or(ProviderError::Invalid("a growth without its face"))
    }

    /// `MultifaceSpreader.spreadFromFaceTowardRandomDirection(.., postProcess = true)`.
    fn spread_toward_random_direction<W: FeatureWorld>(
        &self,
        world: &mut W,
        state: BlockState,
        pos: BlockPos,
        face: Direction,
        random: &mut impl RandomSource,
    ) -> Result<(), ProviderError> {
        let mut all = Direction::ALL;
        shuffle(&mut all, random)?;
        for direction in all {
            if self.spread_toward(world, state, pos, face, direction, &DEFAULT_SPREAD_ORDER)? {
                break;
            }
        }
        Ok(())
    }

    /// `MultifaceSpreader.spreadAll(state, level, pos, true)` with the given spread types: the
    /// number of growths placed.
    pub(super) fn spread_all<W: FeatureWorld>(
        &self,
        world: &mut W,
        state: BlockState,
        pos: BlockPos,
        kinds: &[Spread],
    ) -> Result<i64, ProviderError> {
        let mut count = 0;
        for face in Direction::ALL {
            // `canSpreadFrom`.
            let other_source = self.sculk && !self.is_this(world, state);
            if !(other_source || self.has_face(world, state, face)) {
                continue;
            }
            for direction in Direction::ALL {
                if self.spread_toward(world, state, pos, face, direction, kinds)? {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// The block's default state.
    pub(super) fn block_state(&self) -> BlockState {
        self.block
    }

    pub(super) fn has_face_of(
        &self,
        world: &impl PlacementWorld,
        state: BlockState,
        face: Direction,
    ) -> bool {
        self.has_face(world, state, face)
    }

    pub(super) fn is_vein_block(&self, world: &impl PlacementWorld, state: BlockState) -> bool {
        self.is_this(world, state)
    }

    /// `spreadFromFaceTowardDirection`: whether a growth was placed.
    fn spread_toward<W: FeatureWorld>(
        &self,
        world: &mut W,
        state: BlockState,
        pos: BlockPos,
        from_face: Direction,
        direction: Direction,
        kinds: &[Spread],
    ) -> Result<bool, ProviderError> {
        if axis(direction) == axis(from_face) {
            return Ok(false);
        }
        // The sculk vein's spreader takes any other block as a source; a vein needs the face.
        let other_source = self.sculk && !self.is_this(world, state);
        let allowed = other_source
            || (self.has_face(world, state, from_face) && !self.has_face(world, state, direction));
        if !allowed {
            return Ok(false);
        }
        for &kind in kinds {
            let (target, face) = match kind {
                Spread::SamePosition => (pos, direction),
                Spread::SamePlane => (relative(pos, direction), from_face),
                Spread::WrapAround => (
                    relative(relative(pos, direction), from_face),
                    direction.opposite(),
                ),
            };
            if !self.can_spread_into(world, pos, target, face)? {
                continue;
            }
            // `spreadToFace`: the state for the old state at the target; a refusal ends the try.
            let old = world.block_state(target)?;
            let Some(new) = self.state_for_placement(world, old, target, face)? else {
                return Ok(false);
            };
            world.mark_postprocessing(target)?;
            return Ok(world.set_block(target, new, 2)?);
        }
        Ok(false)
    }

    /// `SpreadConfig.canSpreadInto`.
    fn can_spread_into(
        &self,
        world: &impl PlacementWorld,
        source: BlockPos,
        target: BlockPos,
        face: Direction,
    ) -> Result<bool, ProviderError> {
        let existing = world.block_state(target)?;
        let replaceable = if self.sculk {
            let against = world.block_state(relative(target, face))?;
            if is_block(world, against, "minecraft:sculk")
                || is_block(world, against, "minecraft:sculk_catalyst")
                || is_block(world, against, "minecraft:moving_piston")
            {
                return Ok(false);
            }
            if manhattan(source, target) == 2 {
                let neighbour = relative(source, face.opposite());
                let state = world.block_state(neighbour)?;
                if world
                    .state_data()
                    .is_face_sturdy(state, face, SupportType::Full)
                {
                    return Ok(false);
                }
            }
            let fluid = world.state_data().fluid(existing);
            let fluid_ok =
                fluid.fluid == 0 || world.state_data().fluid_name(fluid.fluid) == "minecraft:water";
            fluid_ok
                && !self.fire.contains(world.blocks().block(existing).0)
                && (world.state_data().has(existing, FLAG_REPLACEABLE)
                    || self.default_replaceable(world, existing))
        } else {
            self.default_replaceable(world, existing)
        };
        Ok(replaceable && self.valid_for_placement(world, existing, target, face)?)
    }

    /// `DefaultSpreaderConfig.stateCanBeReplaced`.
    fn default_replaceable(&self, world: &impl PlacementWorld, state: BlockState) -> bool {
        let data = world.state_data();
        let fluid = data.fluid(state);
        data.has(state, FLAG_AIR)
            || self.is_this(world, state)
            || (is_block(world, state, "minecraft:water") && fluid.amount == 8)
    }
}

fn axis(direction: Direction) -> Axis {
    match direction {
        Direction::Down | Direction::Up => Axis::Y,
        Direction::North | Direction::South => Axis::Z,
        Direction::West | Direction::East => Axis::X,
    }
}

/// `Vec3i.distManhattan`, including its float accumulation.
fn manhattan(a: BlockPos, b: BlockPos) -> i32 {
    let d = |a: i32, b: i32| b.wrapping_sub(a).wrapping_abs() as f32;
    (d(a.x, b.x) + d(a.y, b.y) + d(a.z, b.z)) as i32
}
