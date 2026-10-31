//! `monster_room` (`MonsterRoomFeature`, the dungeon).
//!
//! The chest's loot table seed and the spawner's mob are block entity data, which the proto chunk
//! keeps nowhere yet; their draws are made (`setBlockEntityLootTable`'s `nextLong`, the mob's
//! `nextInt(4)`) so the random stays in step.

use super::common::{HORIZONTAL, default_state, is_block, relative};
use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::{
    FLAG_HAS_BLOCK_ENTITY, FLAG_LEGACY_SOLID, FLAG_RANDOMIZABLE_CONTAINER, FLAG_SOLID_RENDER,
};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::DecodeError;
use crate::worldgen::placement::{DecodeContext, PlacementWorld};
use crate::worldgen::state_provider::ProviderError;
use crate::worldgen::structure::BiomeSet;

#[derive(Clone, Debug)]
pub struct MonsterRoom {
    cave_air: BlockState,
    cobblestone: BlockState,
    mossy: BlockState,
    chest: BlockState,
    spawner: BlockState,
    cannot_replace: BiomeSet,
}

impl MonsterRoom {
    pub fn new(names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let state = |name| default_state(names.blocks, name);
        Ok(Self {
            cave_air: state("minecraft:cave_air")?,
            cobblestone: state("minecraft:cobblestone")?,
            mossy: state("minecraft:mossy_cobblestone")?,
            chest: state("minecraft:chest")?,
            spawner: state("minecraft:spawner")?,
            cannot_replace: names.block_tag("minecraft:features_cannot_replace")?,
        })
    }

    fn solid(world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        Ok(world.state_data().has(state, FLAG_LEGACY_SOLID))
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

    /// `MonsterRoomFeature.place`.
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let xr = random.next_int_bound(2)? + 2;
        let (min_x, max_x) = (-xr - 1, xr + 1);
        let zr = random.next_int_bound(2)? + 2;
        let (min_z, max_z) = (-zr - 1, zr + 1);
        let mut holes = 0;
        for dx in min_x..=max_x {
            for dy in -1..=4 {
                for dz in min_z..=max_z {
                    let pos = origin.offset(dx, dy, dz);
                    let solid = Self::solid(world, pos)?;
                    if (dy == -1 || dy == 4) && !solid {
                        return Ok(false);
                    }
                    if (dx == min_x || dx == max_x || dz == min_z || dz == max_z)
                        && dy == 0
                        && super::common::is_air(world, pos)?
                        && super::common::is_air(world, pos.offset(0, 1, 0))?
                    {
                        holes += 1;
                    }
                }
            }
        }
        if !(1..=5).contains(&holes) {
            return Ok(false);
        }
        let min_y = world.min_y();
        for dx in min_x..=max_x {
            for dy in (-1..=3).rev() {
                for dz in min_z..=max_z {
                    let pos = origin.offset(dx, dy, dz);
                    let wall = world.block_state(pos)?;
                    let on_wall = dx == min_x
                        || dy == -1
                        || dz == min_z
                        || dx == max_x
                        || dy == 4
                        || dz == max_z;
                    if on_wall {
                        if pos.y >= min_y && !Self::solid(world, pos.offset(0, -1, 0))? {
                            world.set_block(pos, self.cave_air, 2)?;
                        } else if world.state_data().has(wall, FLAG_LEGACY_SOLID)
                            && !is_block(world, wall, "minecraft:chest")
                        {
                            if dy == -1 && random.next_int_bound(4)? != 0 {
                                self.safe_set(world, pos, self.mossy)?;
                            } else {
                                self.safe_set(world, pos, self.cobblestone)?;
                            }
                        }
                    } else if !is_block(world, wall, "minecraft:chest")
                        && !is_block(world, wall, "minecraft:spawner")
                    {
                        self.safe_set(world, pos, self.cave_air)?;
                    }
                }
            }
        }
        for _ in 0..2 {
            for _ in 0..3 {
                let x = origin.x.wrapping_add(random.next_int_bound(xr * 2 + 1)?) - xr;
                let z = origin.z.wrapping_add(random.next_int_bound(zr * 2 + 1)?) - zr;
                let chest_pos = BlockPos::new(x, origin.y, z);
                if !super::common::is_air(world, chest_pos)? {
                    continue;
                }
                let mut walls = 0;
                for direction in HORIZONTAL {
                    if Self::solid(world, relative(chest_pos, direction))? {
                        walls += 1;
                    }
                }
                if walls == 1 {
                    let chest = self.reorient(world, chest_pos)?;
                    self.safe_set(world, chest_pos, chest)?;
                    // `setBlockEntityLootTable`: a chest's block entity takes a loot table seed.
                    let there = world.block_state(chest_pos)?;
                    if world.state_data().has(there, FLAG_RANDOMIZABLE_CONTAINER) {
                        random.next_long();
                    }
                    break;
                }
            }
        }
        self.safe_set(world, origin, self.spawner)?;
        let there = world.block_state(origin)?;
        if is_block(world, there, "minecraft:spawner")
            && world.state_data().has(there, FLAG_HAS_BLOCK_ENTITY)
        {
            // `Util.getRandom(MOBS, random)`.
            random.next_int_bound(4)?;
        }
        Ok(true)
    }

    /// `StructurePiece.reorient` for the chest.
    fn reorient(
        &self,
        world: &impl PlacementWorld,
        pos: BlockPos,
    ) -> Result<BlockState, ProviderError> {
        let data = world.state_data();
        let mut connected = None;
        for direction in HORIZONTAL {
            let state = world.block_state(relative(pos, direction))?;
            if is_block(world, state, "minecraft:chest") {
                return Ok(self.chest);
            }
            if data.has(state, FLAG_SOLID_RENDER) {
                if connected.is_some() {
                    connected = None;
                    break;
                }
                connected = Some(direction);
            }
        }
        let facing = |direction: Direction| {
            world
                .blocks()
                .with_property(self.chest, "facing", super::common::face_name(direction))
                .ok_or(ProviderError::Invalid("a chest without facing"))
        };
        if let Some(direction) = connected {
            return facing(direction.opposite());
        }
        let solid = |direction: Direction| -> Result<bool, ProviderError> {
            let state = world.block_state(relative(pos, direction))?;
            Ok(data.has(state, FLAG_SOLID_RENDER))
        };
        // The chest's default `facing` is north.
        let mut lock = Direction::North;
        if solid(lock)? {
            lock = lock.opposite();
        }
        if solid(lock)? {
            lock = clockwise(lock);
        }
        if solid(lock)? {
            lock = lock.opposite();
        }
        facing(lock)
    }
}

/// `Direction.getClockWise` around the Y axis.
fn clockwise(direction: Direction) -> Direction {
    match direction {
        Direction::North => Direction::East,
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        other => other,
    }
}
