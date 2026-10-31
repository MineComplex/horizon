//! `sculk_patch` (`SculkPatchFeature`): the world generation `SculkSpreader` with the charge
//! cursors and the default, sculk and sculk vein behaviours.

use serde_json::value::RawValue;

use super::common::{face_name, relative, shuffle};
use super::multiface::{DEFAULT_SPREAD_ORDER, MultifaceGrowth, Spread};
use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::{FLAG_AIR, FLAG_COLLISION_FULL_BLOCK, SupportType};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, int_range, object, required};
use crate::worldgen::placement::PlacementWorld;
use crate::worldgen::state_provider::{ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

// `SculkSpreader.createWorldGenSpreader`.
const GROWTH_SPAWN_COST: i32 = 50;
const NO_GROWTH_RADIUS: i32 = 1;
const CHARGE_DECAY_RATE: i32 = 5;
const ADDITIONAL_DECAY_RATE: i32 = 10;
const MAX_CURSORS: usize = 32;

/// `SculkBehaviour` of a block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    Default,
    Sculk,
    Vein,
}

struct Cursor {
    pos: BlockPos,
    charge: i32,
    update_delay: i32,
    decay_delay: i32,
    facings: Option<Vec<Direction>>,
}

#[derive(Clone, Debug)]
pub struct SculkPatch {
    charge_count: i32,
    amount_per_charge: i32,
    spread_attempts: i32,
    growth_rounds: i32,
    spread_rounds: i32,
    sculk: BlockState,
    shrieker: BlockState,
    sensor: BlockState,
    water: BlockState,
    air: BlockState,
    /// The vein's `MultifaceSpreader`.
    vein: MultifaceGrowth,
    replaceable_world_gen: BiomeSet,
    replaceable: BiomeSet,
    inhibitors: BiomeSet,
}

impl SculkPatch {
    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let names = ctx.names;
        let int = |name: &'static str, low: i32, high: i32| {
            int_range(required(&fields, name)?, name, low, high)
        };
        let state = |name: &str| super::common::default_state(names.blocks, name);
        let shrieker = names
            .blocks
            .with_property(state("minecraft:sculk_shrieker")?, "can_summon", "true")
            .ok_or(DecodeError::Field("sculk shrieker can_summon"))?;
        Ok(Self {
            charge_count: int("charge_count", 1, 32)?,
            amount_per_charge: int("amount_per_charge", 1, 500)?,
            spread_attempts: int("spread_attempts", 1, 64)?,
            growth_rounds: int("growth_rounds", 0, 8)?,
            spread_rounds: int("spread_rounds", 0, 8)?,
            sculk: state("minecraft:sculk")?,
            shrieker,
            sensor: state("minecraft:sculk_sensor")?,
            water: state("minecraft:water")?,
            air: state("minecraft:air")?,
            vein: MultifaceGrowth::sculk_vein(names)?,
            replaceable_world_gen: names.block_tag("minecraft:sculk_replaceable_world_gen")?,
            replaceable: names.block_tag("minecraft:sculk_replaceable")?,
            inhibitors: names.block_tag("minecraft:sculk_growth_inhibitors")?,
        })
    }

    fn behaviour(&self, world: &impl PlacementWorld, state: BlockState) -> Behaviour {
        let block = world.blocks().block(state);
        if block == world.blocks().block(self.sculk) {
            Behaviour::Sculk
        } else if self.vein.is_vein_block(world, state) {
            Behaviour::Vein
        } else {
            Behaviour::Default
        }
    }

    fn faces(&self, world: &impl PlacementWorld, state: BlockState) -> Vec<Direction> {
        Direction::ALL
            .into_iter()
            .filter(|&d| self.vein.has_face_of(world, state, d))
            .collect()
    }

    /// `SculkPatchFeature.place`.
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        if !self.can_spread_from(world, origin)? {
            return Ok(false);
        }
        let mut cursors: Vec<Cursor> = Vec::new();
        for round in 0..self.spread_rounds + self.growth_rounds {
            for _ in 0..self.charge_count {
                // `addCursors(origin, amountPerCharge)`.
                let mut charge = self.amount_per_charge;
                while charge > 0 {
                    let current = charge.min(1000);
                    if cursors.len() < MAX_CURSORS {
                        cursors.push(Cursor {
                            pos: origin,
                            charge: current,
                            update_delay: 0,
                            decay_delay: 1,
                            facings: None,
                        });
                    }
                    charge -= current;
                }
            }
            let spread_veins = round < self.spread_rounds;
            for _ in 0..self.spread_attempts {
                cursors = self.update_cursors(world, random, origin, cursors, spread_veins)?;
            }
            cursors.clear();
        }
        Ok(true)
    }

    fn can_spread_from(
        &self,
        world: &impl PlacementWorld,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let start = world.block_state(origin)?;
        if self.behaviour(world, start) != Behaviour::Default {
            return Ok(true);
        }
        let data = world.state_data();
        let fluid = data.fluid(start);
        let water_source = super::common::is_block(world, start, "minecraft:water")
            && fluid.amount == 8
            && !fluid.falling;
        if !data.has(start, FLAG_AIR) && !water_source {
            return Ok(false);
        }
        for direction in Direction::ALL {
            let pos = relative(origin, direction);
            let state = world.block_state(pos)?;
            if full_collision(world, state) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `SculkSpreader.updateCursors`.
    fn update_cursors<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
        cursors: Vec<Cursor>,
        spread_veins: bool,
    ) -> Result<Vec<Cursor>, ProviderError> {
        let mut processed: Vec<Cursor> = Vec::new();
        for mut cursor in cursors {
            let distance = (cursor.pos.x.wrapping_sub(origin.x).wrapping_abs())
                .max(cursor.pos.y.wrapping_sub(origin.y).wrapping_abs())
                .max(cursor.pos.z.wrapping_sub(origin.z).wrapping_abs());
            if distance > 1024 {
                continue;
            }
            self.update(world, random, origin, &mut cursor, spread_veins)?;
            if cursor.charge > 0 {
                // World generation never merges cursors.
                processed.push(cursor);
            }
        }
        Ok(processed)
    }

    /// `ChargeCursor.update` (world generation).
    fn update<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
        cursor: &mut Cursor,
        spread_veins: bool,
    ) -> Result<(), ProviderError> {
        if cursor.charge <= 0 {
            return Ok(());
        }
        if cursor.update_delay > 0 {
            cursor.update_delay -= 1;
            return Ok(());
        }
        let mut state = world.block_state(cursor.pos)?;
        let mut behaviour = self.behaviour(world, state);
        if spread_veins
            && self.attempt_spread_vein(
                world,
                behaviour,
                cursor.pos,
                state,
                cursor.facings.as_deref(),
            )?
            && behaviour != Behaviour::Sculk
        {
            state = world.block_state(cursor.pos)?;
            behaviour = self.behaviour(world, state);
        }
        cursor.charge =
            self.attempt_use_charge(world, random, origin, behaviour, cursor, spread_veins)?;
        if cursor.charge <= 0 {
            self.on_discharged(world, behaviour, state, cursor.pos)?;
            return Ok(());
        }
        match self.valid_movement(world, random, cursor.pos, origin)? {
            Some(target) => {
                self.on_discharged(world, behaviour, state, cursor.pos)?;
                cursor.pos = target;
                state = world.block_state(target)?;
            }
            None => {
                self.on_discharged(world, behaviour, state, cursor.pos)?;
                cursor.charge = 0;
                return Ok(());
            }
        }
        if self.behaviour(world, state) != Behaviour::Default {
            // `MultifaceBlock.availableFaces`.
            cursor.facings = Some(self.faces(world, state));
        }
        cursor.decay_delay = match behaviour {
            Behaviour::Default => (cursor.decay_delay - 1).max(0),
            _ => 1,
        };
        cursor.update_delay = 1;
        Ok(())
    }

    /// `SculkBehaviour.attemptSpreadVein`.
    fn attempt_spread_vein<W: FeatureWorld>(
        &self,
        world: &mut W,
        behaviour: Behaviour,
        pos: BlockPos,
        state: BlockState,
        facings: Option<&[Direction]>,
    ) -> Result<bool, ProviderError> {
        if behaviour == Behaviour::Default {
            match facings {
                None => {
                    let current = world.block_state(pos)?;
                    return Ok(self.vein.spread_all(
                        world,
                        current,
                        pos,
                        &[Spread::SamePosition],
                    )? > 0);
                }
                Some(faces) if !faces.is_empty() => {
                    let data = world.state_data();
                    let fluid = data.fluid(state);
                    let water = data.fluid_name(fluid.fluid) == "minecraft:water";
                    return if data.has(state, FLAG_AIR) || water {
                        self.regrow(world, pos, state, faces)
                    } else {
                        Ok(false)
                    };
                }
                Some(_) => {}
            }
        }
        Ok(self
            .vein
            .spread_all(world, state, pos, &DEFAULT_SPREAD_ORDER)?
            > 0)
    }

    /// `SculkVeinBlock.regrow`.
    fn regrow<W: FeatureWorld>(
        &self,
        world: &mut W,
        pos: BlockPos,
        existing: BlockState,
        faces: &[Direction],
    ) -> Result<bool, ProviderError> {
        let blocks = world.blocks();
        let mut state = self.vein.block_state();
        let mut any = false;
        for &face in faces {
            let neighbour = world.block_state(relative(pos, face))?;
            let attach = crate::block::survival::can_attach_to(world.state_data(), neighbour, face)
                .ok_or(ProviderError::Invalid("a dynamic shape"))?;
            if attach {
                state = blocks
                    .with_property(state, face_name(face), "true")
                    .ok_or(ProviderError::Invalid("sculk vein face"))?;
                any = true;
            }
        }
        if !any {
            return Ok(false);
        }
        if world.state_data().fluid(existing).fluid != 0 {
            state = blocks
                .with_property(state, "waterlogged", "true")
                .ok_or(ProviderError::Invalid("sculk vein waterlogged"))?;
        }
        world.set_block(pos, state, 3)?;
        Ok(true)
    }

    /// `attemptUseCharge` of the cursor's block.
    fn attempt_use_charge<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
        behaviour: Behaviour,
        cursor: &Cursor,
        spread_veins: bool,
    ) -> Result<i32, ProviderError> {
        let charge = cursor.charge;
        match behaviour {
            Behaviour::Default => Ok(if cursor.decay_delay > 0 { charge } else { 0 }),
            Behaviour::Vein => {
                if spread_veins && self.place_sculk(world, random, cursor.pos)? {
                    Ok(charge - 1)
                } else if random.next_int_bound(CHARGE_DECAY_RATE)? == 0 {
                    Ok((charge as f32 * 0.5f32).floor() as i32)
                } else {
                    Ok(charge)
                }
            }
            Behaviour::Sculk => {
                if charge == 0 || random.next_int_bound(CHARGE_DECAY_RATE)? != 0 {
                    return Ok(charge);
                }
                let pos = cursor.pos;
                let d = |a: i32, b: i32| f64::from(a) - f64::from(b);
                let (dx, dy, dz) = (d(pos.x, origin.x), d(pos.y, origin.y), d(pos.z, origin.z));
                let dist_sqr = dx * dx + dy * dy + dz * dz;
                let close = dist_sqr < f64::from(NO_GROWTH_RADIUS * NO_GROWTH_RADIUS);
                if !close && self.can_place_growth(world, pos)? {
                    if random.next_int_bound(GROWTH_SPAWN_COST)? < charge {
                        let above = pos.offset(0, 1, 0);
                        let mut growth = if random.next_int_bound(11)? == 0 {
                            self.shrieker
                        } else {
                            self.sensor
                        };
                        // `hasProperty(WATERLOGGED)` and a non-empty fluid at the growth.
                        let at = world.block_state(above)?;
                        if world.state_data().fluid(at).fluid != 0
                            && let Some(wet) =
                                world.blocks().with_property(growth, "waterlogged", "true")
                        {
                            growth = wet;
                        }
                        world.set_block(above, growth, 3)?;
                    }
                    return Ok(0.max(charge - GROWTH_SPAWN_COST));
                }
                if random.next_int_bound(ADDITIONAL_DECAY_RATE)? != 0 {
                    return Ok(charge);
                }
                let penalty = if close {
                    1
                } else {
                    // `getDecayPenalty`.
                    let outer = dist_sqr.sqrt() as f32 - NO_GROWTH_RADIUS as f32;
                    let outer = outer * outer;
                    let reach = (24 - NO_GROWTH_RADIUS) * (24 - NO_GROWTH_RADIUS);
                    let factor = 1.0f32.min(outer / reach as f32);
                    1.max((charge as f32 * factor * 0.5f32) as i32)
                };
                Ok(charge - penalty)
            }
        }
    }

    /// `SculkBlock.canPlaceGrowth`.
    fn can_place_growth(
        &self,
        world: &impl PlacementWorld,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let above = world.block_state(pos.offset(0, 1, 0))?;
        let data = world.state_data();
        let fluid = data.fluid(above);
        let open = data.has(above, FLAG_AIR)
            || (super::common::is_block(world, above, "minecraft:water")
                && data.fluid_name(fluid.fluid) == "minecraft:water");
        if !open {
            return Ok(false);
        }
        let mut matched = 0;
        // `findBlocksIn(pos + (-4, 0, -4), pos + (4, 2, 4))`: x fastest, then y, then z.
        for dz in -4..=4 {
            for dy in 0..=2 {
                for dx in -4..=4 {
                    let state = world.block_state(pos.offset(dx, dy, dz))?;
                    if self.inhibitors.contains(world.blocks().block(state).0) {
                        matched += 1;
                        if matched >= 3 {
                            return Ok(false);
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    /// `SculkVeinBlock.attemptPlaceSculk`.
    fn place_sculk<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        let mut directions = Direction::ALL;
        shuffle(&mut directions, random)?;
        for support in directions {
            if !self.vein.has_face_of(world, state, support) {
                continue;
            }
            let support_pos = relative(pos, support);
            let support_state = world.block_state(support_pos)?;
            if !self
                .replaceable_world_gen
                .contains(world.blocks().block(support_state).0)
            {
                continue;
            }
            world.set_block(support_pos, self.sculk, 3)?;
            self.vein
                .spread_all(world, self.sculk, support_pos, &DEFAULT_SPREAD_ORDER)?;
            let skip = support.opposite();
            for direction in Direction::ALL {
                if direction == skip {
                    continue;
                }
                let vein_pos = relative(support_pos, direction);
                let possible = world.block_state(vein_pos)?;
                if self.vein.is_vein_block(world, possible) {
                    self.on_discharged(world, Behaviour::Vein, possible, vein_pos)?;
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// `SculkVeinBlock.onDischarged`.
    fn on_discharged<W: FeatureWorld>(
        &self,
        world: &mut W,
        behaviour: Behaviour,
        state: BlockState,
        pos: BlockPos,
    ) -> Result<(), ProviderError> {
        if behaviour != Behaviour::Vein || !self.vein.is_vein_block(world, state) {
            return Ok(());
        }
        let mut state = state;
        for direction in Direction::ALL {
            if self.vein.has_face_of(world, state, direction) {
                let neighbour = world.block_state(relative(pos, direction))?;
                if world.blocks().block(neighbour) == world.blocks().block(self.sculk) {
                    state = world
                        .blocks()
                        .with_property(state, face_name(direction), "false")
                        .ok_or(ProviderError::Invalid("sculk vein face"))?;
                }
            }
        }
        if !Direction::ALL
            .into_iter()
            .any(|d| self.vein.has_face_of(world, state, d))
        {
            let here = world.block_state(pos)?;
            state = if world.state_data().fluid(here).fluid == 0 {
                self.air
            } else {
                self.water
            };
        }
        world.set_block(pos, state, 3)?;
        Ok(())
    }

    /// `ChargeCursor.getValidMovementPos`.
    fn valid_movement(
        &self,
        world: &impl PlacementWorld,
        random: &mut impl RandomSource,
        pos: BlockPos,
        origin: BlockPos,
    ) -> Result<Option<BlockPos>, ProviderError> {
        // `NON_CORNER_NEIGHBOURS` in `betweenClosed` order (x fastest, then y, then z).
        let mut offsets = Vec::with_capacity(18);
        for z in -1..=1 {
            for y in -1..=1 {
                for x in -1..=1 {
                    if (x == 0 || y == 0 || z == 0) && (x, y, z) != (0, 0, 0) {
                        offsets.push([x, y, z]);
                    }
                }
            }
        }
        shuffle(&mut offsets, random)?;
        let mut best = pos;
        for o in offsets {
            let neighbour = pos.offset(o[0], o[1], o[2]);
            // `canMoveToPos` (world generation).
            let dx = origin.x.wrapping_sub(neighbour.x);
            let dz = origin.z.wrapping_sub(neighbour.z);
            if dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) > 144 {
                continue;
            }
            let transferee = world.block_state(neighbour)?;
            if self.behaviour(world, transferee) == Behaviour::Default
                || !self.unobstructed_move(world, pos, neighbour)?
            {
                continue;
            }
            best = neighbour;
            // `SculkVeinBlock.hasSubstrateAccess`.
            if self.vein.is_vein_block(world, transferee) {
                for direction in Direction::ALL {
                    if self.vein.has_face_of(world, transferee, direction) {
                        let next = world.block_state(relative(neighbour, direction))?;
                        if self.replaceable.contains(world.blocks().block(next).0) {
                            return Ok(Some(neighbour));
                        }
                    }
                }
            }
        }
        Ok((best != pos).then_some(best))
    }

    fn unobstructed_move(
        &self,
        world: &impl PlacementWorld,
        from: BlockPos,
        to: BlockPos,
    ) -> Result<bool, ProviderError> {
        let manhattan = {
            let d = |a: i32, b: i32| b.wrapping_sub(a).wrapping_abs() as f32;
            (d(from.x, to.x) + d(from.y, to.y) + d(from.z, to.z)) as i32
        };
        if manhattan == 1 {
            return Ok(true);
        }
        let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
        let dir_x = if dx < 0 {
            Direction::West
        } else {
            Direction::East
        };
        let dir_y = if dy < 0 {
            Direction::Down
        } else {
            Direction::Up
        };
        let dir_z = if dz < 0 {
            Direction::North
        } else {
            Direction::South
        };
        let (a, b) = if dx == 0 {
            (dir_y, dir_z)
        } else if dy == 0 {
            (dir_x, dir_z)
        } else {
            (dir_x, dir_y)
        };
        Ok(self.unobstructed(world, from, a)? || self.unobstructed(world, from, b)?)
    }

    fn unobstructed(
        &self,
        world: &impl PlacementWorld,
        from: BlockPos,
        direction: Direction,
    ) -> Result<bool, ProviderError> {
        let test = relative(from, direction);
        let state = world.block_state(test)?;
        Ok(!world
            .state_data()
            .is_face_sturdy(state, direction.opposite(), SupportType::Full))
    }
}

/// `state.isCollisionShapeFullBlock(level, pos)`: the cached collision shape is the full cube.
fn full_collision(world: &impl PlacementWorld, state: BlockState) -> bool {
    world.state_data().has(state, FLAG_COLLISION_FULL_BLOCK)
}
