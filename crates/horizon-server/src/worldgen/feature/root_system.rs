//! `root_system` (`RootSystemFeature`): the azalea tree's rooted dirt and hanging roots.

use serde_json::value::RawValue;

use super::common::{is_air, relative_by};
use super::select::PlacedRefs;
use super::simple::FeatureWorld;
use super::{FeatureRegistry, PlacedFeatureId};
use crate::block::flags::{FLAG_AIR, FLAG_LEGACY_SOLID, SupportType};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{DecodeError, int_range, object, required};
use crate::worldgen::placement::{BlockPredicate, PlacementWorld};
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

#[derive(Clone, Debug)]
pub struct RootSystem {
    tree: PlacedRefs,
    required_vertical_space_for_tree: i32,
    level_test_distance: i32,
    max_level_deviation: i32,
    root_radius: i32,
    root_replaceable: BiomeSet,
    root_state_provider: BlockStateProvider,
    root_placement_attempts: i32,
    root_column_max_height: i32,
    hanging_root_radius: i32,
    hanging_roots_vertical_span: i32,
    hanging_root_state_provider: BlockStateProvider,
    hanging_root_placement_attempts: i32,
    allowed_vertical_water_for_tree: i32,
    allowed_tree_position: BlockPredicate,
}

impl RootSystem {
    pub fn decode(raw: &RawValue, ctx: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let int = |name: &'static str, low: i32, high: i32| {
            int_range(required(&fields, name)?, name, low, high)
        };
        Ok(Self {
            tree: PlacedRefs::new(required(&fields, "feature")?),
            required_vertical_space_for_tree: int("required_vertical_space_for_tree", 1, 64)?,
            level_test_distance: int("level_test_distance", 0, 16)?,
            max_level_deviation: int("max_level_deviation", 0, 64)?,
            root_radius: int("root_radius", 1, 64)?,
            root_replaceable: ctx
                .names
                .block_set(required(&fields, "root_replaceable")?)?,
            root_state_provider: BlockStateProvider::decode(
                required(&fields, "root_state_provider")?,
                ctx,
            )?,
            root_placement_attempts: int("root_placement_attempts", 1, 256)?,
            root_column_max_height: int("root_column_max_height", 1, 4096)?,
            hanging_root_radius: int("hanging_root_radius", 1, 64)?,
            hanging_roots_vertical_span: int("hanging_roots_vertical_span", 1, 16)?,
            hanging_root_state_provider: BlockStateProvider::decode(
                required(&fields, "hanging_root_state_provider")?,
                ctx,
            )?,
            hanging_root_placement_attempts: int("hanging_root_placement_attempts", 1, 256)?,
            allowed_vertical_water_for_tree: int("allowed_vertical_water_for_tree", 1, 64)?,
            allowed_tree_position: BlockPredicate::decode(
                required(&fields, "allowed_tree_position")?,
                ctx.names,
            )?,
        })
    }

    pub(super) fn refs_mut(&mut self) -> &mut PlacedRefs {
        &mut self.tree
    }

    pub fn placed(&self) -> &[PlacedFeatureId] {
        self.tree.ids()
    }

    /// `RootSystemFeature.place`.
    pub fn place<W: FeatureWorld>(
        &self,
        registry: &FeatureRegistry,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        if !is_air(world, origin)? {
            return Ok(false);
        }
        if let Some(working) = self.place_dirt_and_tree(registry, world, random, origin)? {
            self.place_roots(world, random, origin, working)?;
        }
        Ok(true)
    }

    /// The tree's position when it was placed (the working position after the climb).
    fn place_dirt_and_tree<W: FeatureWorld>(
        &self,
        registry: &FeatureRegistry,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
    ) -> Result<Option<BlockPos>, ProviderError> {
        let mut working = pos;
        for y in 0..self.root_column_max_height {
            working = working.offset(0, 1, 0);
            if world.height(Heightmap::WorldSurface, working.x, working.z)? < working.y {
                return Ok(None);
            }
            if self.allowed_tree_position.test(world, working)?
                && self.space_for_tree(world, working)?
            {
                let below = working.offset(0, -1, 0);
                let below_state = world.block_state(below)?;
                let fluid = world.state_data().fluid(below_state).fluid;
                let lava = matches!(
                    world.state_data().fluid_name(fluid),
                    "minecraft:lava" | "minecraft:flowing_lava"
                );
                if lava || !world.state_data().has(below_state, FLAG_LEGACY_SOLID) {
                    return Ok(None);
                }
                if registry.place_placed(self.tree_id()?, world, random, working)? {
                    self.place_dirt(pos, pos.y + y, world, random)?;
                    return Ok(Some(working));
                }
            }
        }
        Ok(None)
    }

    fn tree_id(&self) -> Result<PlacedFeatureId, ProviderError> {
        self.tree.single()
    }

    fn space_for_tree(
        &self,
        world: &impl PlacementWorld,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let mut column = pos;
        for i in 1..=self.required_vertical_space_for_tree {
            column = column.offset(0, 1, 0);
            let state = world.block_state(column)?;
            if !self.allowed_tree_space(world, state, i) {
                return Ok(false);
            }
        }
        if self.level_test_distance > 0 {
            // `Direction.from2DDataValue(i)`: south, west, north, east.
            for direction in [
                Direction::South,
                Direction::West,
                Direction::North,
                Direction::East,
            ] {
                let corner = relative_by(pos, direction, self.level_test_distance);
                let below = world.block_state(corner.offset(0, -self.max_level_deviation, 0))?;
                let above = world.block_state(corner.offset(0, self.max_level_deviation, 0))?;
                if world.state_data().has(below, FLAG_AIR)
                    || !world.state_data().has(above, FLAG_AIR)
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn allowed_tree_space(
        &self,
        world: &impl PlacementWorld,
        state: crate::block::BlockState,
        blocks_above_origin: i32,
    ) -> bool {
        if world.state_data().has(state, FLAG_AIR) {
            return true;
        }
        let above_ground = blocks_above_origin + 1;
        let fluid = world.state_data().fluid(state).fluid;
        above_ground <= self.allowed_vertical_water_for_tree
            && matches!(
                world.state_data().fluid_name(fluid),
                "minecraft:water" | "minecraft:flowing_water"
            )
    }

    fn place_dirt<W: FeatureWorld>(
        &self,
        origin: BlockPos,
        target_height: i32,
        world: &mut W,
        random: &mut impl RandomSource,
    ) -> Result<(), ProviderError> {
        for y in origin.y..target_height {
            for _ in 0..self.root_placement_attempts {
                let dx = random.next_int_bound(self.root_radius)?
                    - random.next_int_bound(self.root_radius)?;
                let dz = random.next_int_bound(self.root_radius)?
                    - random.next_int_bound(self.root_radius)?;
                let pos = BlockPos::new(origin.x + dx, y, origin.z + dz);
                let state = world.block_state(pos)?;
                if self
                    .root_replaceable
                    .contains(world.blocks().block(state).0)
                {
                    let root = self.root_state_provider.state(world, random, pos)?;
                    world.set_block(pos, root, 2)?;
                }
            }
        }
        Ok(())
    }

    fn place_roots<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        pos: BlockPos,
        _working: BlockPos,
    ) -> Result<(), ProviderError> {
        for _ in 0..self.hanging_root_placement_attempts {
            let r = self.hanging_root_radius;
            let v = self.hanging_roots_vertical_span;
            let dx = random.next_int_bound(r)? - random.next_int_bound(r)?;
            let dy = random.next_int_bound(v)? - random.next_int_bound(v)?;
            let dz = random.next_int_bound(r)? - random.next_int_bound(r)?;
            let target = pos.offset(dx, dy, dz);
            if !is_air(world, target)? {
                continue;
            }
            let state = self
                .hanging_root_state_provider
                .state(world, random, target)?;
            let above = world.block_state(target.offset(0, 1, 0))?;
            if world.block_survives(state, target)?
                && world
                    .state_data()
                    .is_face_sturdy(above, Direction::Down, SupportType::Full)
            {
                world.set_block(target, state, 2)?;
            }
        }
        Ok(())
    }
}
