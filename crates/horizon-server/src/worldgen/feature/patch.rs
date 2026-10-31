//! `vegetation_patch` and `waterlogged_vegetation_patch` (`VegetationPatchFeature`,
//! `WaterloggedVegetationPatchFeature`): a patch of ground on the floor or ceiling of a cave (or
//! anywhere), with vegetation placed on it.

use serde_json::value::RawValue;

use super::select::PlacedRefs;
use super::simple::FeatureWorld;
use super::tree::PosSet;
use super::{FeatureRegistry, PlacedFeatureId};
use crate::block::flags::{FLAG_AIR, SupportType};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, float, int_range, object, required, string};
use crate::worldgen::placement::{IntProvider, PlacementWorld};
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

#[derive(Clone, Debug)]
pub struct VegetationPatch {
    waterlogged: bool,
    replaceable: BiomeSet,
    ground_state: BlockStateProvider,
    vegetation: PlacedRefs,
    /// `CaveSurface.getDirection`: down for a floor, up for a ceiling.
    surface: Direction,
    depth: IntProvider,
    extra_bottom_block_chance: f32,
    vertical_range: i32,
    vegetation_chance: f32,
    xz_radius: IntProvider,
    extra_edge_column_chance: f32,
}

fn unit(raw: &RawValue) -> Result<f32, DecodeError> {
    let value = float(raw)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(DecodeError::Field("chance"));
    }
    Ok(value)
}

impl VegetationPatch {
    pub fn decode(
        kind: &str,
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        let waterlogged = match kind {
            "minecraft:vegetation_patch" => false,
            "minecraft:waterlogged_vegetation_patch" => true,
            _ => return Ok(None),
        };
        let fields = object(raw)?;
        Ok(Some(Self {
            waterlogged,
            replaceable: ctx.names.block_set(required(&fields, "replaceable")?)?,
            ground_state: BlockStateProvider::decode(required(&fields, "ground_state")?, ctx)?,
            vegetation: PlacedRefs::new(required(&fields, "vegetation_feature")?),
            surface: match string(required(&fields, "surface")?)?.as_str() {
                "floor" => Direction::Down,
                "ceiling" => Direction::Up,
                _ => return Err(DecodeError::Field("surface")),
            },
            depth: IntProvider::decode_within(required(&fields, "depth")?, 1, 128)?,
            extra_bottom_block_chance: unit(required(&fields, "extra_bottom_block_chance")?)?,
            vertical_range: int_range(
                required(&fields, "vertical_range")?,
                "vertical_range",
                1,
                256,
            )?,
            vegetation_chance: unit(required(&fields, "vegetation_chance")?)?,
            xz_radius: IntProvider::decode(required(&fields, "xz_radius")?)?,
            extra_edge_column_chance: unit(required(&fields, "extra_edge_column_chance")?)?,
        }))
    }

    pub(super) fn refs_mut(&mut self) -> &mut PlacedRefs {
        &mut self.vegetation
    }

    pub fn placed(&self) -> &[PlacedFeatureId] {
        self.vegetation.ids()
    }

    pub fn place<W: FeatureWorld, R: RandomSource>(
        &self,
        registry: &FeatureRegistry,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let x_radius = self.xz_radius.sample(random)? + 1;
        let z_radius = self.xz_radius.sample(random)? + 1;
        let mut surface = self.place_ground_patch(world, random, origin, x_radius, z_radius)?;
        if self.waterlogged {
            surface = self.waterlog(world, &surface)?;
        }
        // `distributeVegetation`, over the set in its hash order.
        let empty = surface.is_empty();
        for pos in surface.to_vec() {
            if self.vegetation_chance > 0.0 && random.next_float() < self.vegetation_chance {
                let pos = BlockPos::new(pos[0], pos[1], pos[2]);
                self.place_vegetation(registry, world, random, pos)?;
            }
        }
        Ok(!empty)
    }

    fn is_air(world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        Ok(world.state_data().has(state, FLAG_AIR))
    }

    fn place_ground_patch<W: FeatureWorld, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
        x_radius: i32,
        z_radius: i32,
    ) -> Result<PosSet, ProviderError> {
        let inwards = self.surface.normal();
        let outwards = self.surface.opposite();
        let up_step = outwards.normal();
        let mut surface = PosSet::default();
        for dx in -x_radius..=x_radius {
            let x_edge = dx == -x_radius || dx == x_radius;
            for dz in -z_radius..=z_radius {
                let z_edge = dz == -z_radius || dz == z_radius;
                let edge = x_edge || z_edge;
                let corner = x_edge && z_edge;
                let edge_not_corner = edge && !corner;
                if corner
                    || (edge_not_corner
                        && (self.extra_edge_column_chance == 0.0
                            || random.next_float() > self.extra_edge_column_chance))
                {
                    continue;
                }
                let mut pos = origin.offset(dx, 0, dz);
                let mut offset = 0;
                while Self::is_air(world, pos)? && offset < self.vertical_range {
                    pos = pos.offset(inwards[0], inwards[1], inwards[2]);
                    offset += 1;
                }
                let mut offset = 0;
                while !Self::is_air(world, pos)? && offset < self.vertical_range {
                    pos = pos.offset(up_step[0], up_step[1], up_step[2]);
                    offset += 1;
                }
                let mut below = pos.offset(inwards[0], inwards[1], inwards[2]);
                let below_state = world.block_state(below)?;
                if Self::is_air(world, pos)?
                    && world.state_data().is_face_sturdy(
                        below_state,
                        self.surface.opposite(),
                        SupportType::Full,
                    )
                {
                    let mut depth = self.depth.sample(random)?;
                    if self.extra_bottom_block_chance > 0.0
                        && random.next_float() < self.extra_bottom_block_chance
                    {
                        depth += 1;
                    }
                    let ground_pos = below;
                    if self.place_ground(world, random, &mut below, depth)? {
                        surface.insert([ground_pos.x, ground_pos.y, ground_pos.z]);
                    }
                }
            }
        }
        Ok(surface)
    }

    fn place_ground<W: FeatureWorld, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        below: &mut BlockPos,
        depth: i32,
    ) -> Result<bool, ProviderError> {
        let step = self.surface.normal();
        for i in 0..depth {
            let to_place = self.ground_state.state(world, random, *below)?;
            let below_state = world.block_state(*below)?;
            let blocks = world.blocks();
            if blocks.block(to_place) != blocks.block(below_state) {
                if !self.replaceable.contains(blocks.block(below_state).0) {
                    return Ok(i != 0);
                }
                world.set_block(*below, to_place, 2)?;
                *below = below.offset(step[0], step[1], step[2]);
            }
        }
        Ok(true)
    }

    /// `WaterloggedVegetationPatchFeature.placeGroundPatch`: the ground not exposed on a side or
    /// below becomes water, and those positions are what the vegetation goes on.
    fn waterlog<W: FeatureWorld>(
        &self,
        world: &mut W,
        surface: &PosSet,
    ) -> Result<PosSet, ProviderError> {
        let mut water = PosSet::default();
        for pos in surface.to_vec() {
            let pos = BlockPos::new(pos[0], pos[1], pos[2]);
            let mut exposed = false;
            for direction in [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
                Direction::Down,
            ] {
                let n = direction.normal();
                let state = world.block_state(pos.offset(n[0], n[1], n[2]))?;
                if !world.state_data().is_face_sturdy(
                    state,
                    direction.opposite(),
                    SupportType::Full,
                ) {
                    exposed = true;
                    break;
                }
            }
            if !exposed {
                water.insert([pos.x, pos.y, pos.z]);
            }
        }
        let water_state = {
            let blocks = world.blocks();
            let block = blocks
                .block_by_name("minecraft:water")
                .ok_or(ProviderError::Invalid("no water block"))?;
            blocks.default_state(block)
        };
        for pos in water.to_vec() {
            world.set_block(BlockPos::new(pos[0], pos[1], pos[2]), water_state, 2)?;
        }
        Ok(water)
    }

    fn place_vegetation<W: FeatureWorld, R: RandomSource>(
        &self,
        registry: &FeatureRegistry,
        world: &mut W,
        random: &mut R,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let id = match self.vegetation.ids() {
            [id] => *id,
            _ => {
                return Err(ProviderError::Invalid(
                    "a vegetation feature that is not one",
                ));
            }
        };
        let up = self.surface.opposite().normal();
        if !self.waterlogged {
            return registry.place_placed(id, world, random, pos.offset(up[0], up[1], up[2]));
        }
        // The waterlogged patch places one block down, so that the vegetation lands on the water.
        let below = pos.offset(0, -1, 0);
        if !registry.place_placed(id, world, random, below.offset(up[0], up[1], up[2]))? {
            return Ok(false);
        }
        let placed = world.block_state(pos)?;
        let blocks = world.blocks();
        if blocks.value(placed, "waterlogged") == Some("false")
            && let Some(wet) = blocks.with_property(placed, "waterlogged", "true")
        {
            world.set_block(pos, wet, 2)?;
        }
        Ok(true)
    }
}
