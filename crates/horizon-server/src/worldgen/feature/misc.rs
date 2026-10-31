//! Small features: `spring_feature`, `block_pile`, `vines`, `underwater_magma`, `bamboo` and
//! `random_neighbor_spread` (`levelgen.feature`).

use serde_json::value::RawValue;

use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::{FLAG_AIR, SupportType};
use crate::block::survival::{can_attach_to, shape_fills_face};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{
    DecodeError, boolean, float, int_range, object, optional, required, string,
};
use crate::worldgen::placement::provider::type_of;
use crate::worldgen::placement::{BlockPredicate, DecodeContext, IntProvider, PlacementWorld};
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

/// A feature of this module.
#[derive(Clone, Debug)]
pub enum Misc {
    Spring {
        /// `FluidState.createLegacyBlock()`.
        block: BlockState,
        /// The fluid ticked after the block is placed (`StateFluid::fluid`).
        fluid: u8,
        requires_block_below: bool,
        rock_count: i32,
        hole_count: i32,
        valid_blocks: BiomeSet,
    },
    BlockPile(BlockStateProvider),
    Vines,
    UnderwaterMagma {
        floor_search_range: i32,
        radius: i32,
        probability: f32,
    },
    Bamboo {
        probability: f32,
        podzol_replaceable: BiomeSet,
    },
    RandomNeighborSpread {
        block: BlockStateProvider,
        accepted_neighbors: BiomeSet,
        can_replace: BlockPredicate,
        attempts: IntProvider,
        xz_offset: IntProvider,
        y_offset: IntProvider,
    },
}

/// `FluidState.CODEC` (`{"id": fluid, "properties": {...}}`) as `createLegacyBlock()`: the fluid's
/// block with `LiquidBlock.LEVEL` = `8 - min(amount, 8) + (falling ? 8 : 0)`.
fn legacy_block(
    raw: &RawValue,
    names: &DecodeContext<'_>,
) -> Result<(BlockState, u8), DecodeError> {
    let fields = object(raw)?;
    let id = crate::worldgen::codec::identifier(&string(required(&fields, "id")?)?)?;
    let properties: std::collections::BTreeMap<String, String> = optional(&fields, "properties")
        .map_or(Ok(Default::default()), |raw| {
            serde_json::from_str(raw.get())
        })?;
    let (block, amount) = match id.as_str() {
        "minecraft:water" => ("minecraft:water", 8),
        "minecraft:lava" => ("minecraft:lava", 8),
        "minecraft:flowing_water" | "minecraft:flowing_lava" => {
            let level: i32 = (properties.get("level"))
                .and_then(|v| v.parse().ok())
                .ok_or(DecodeError::Field("fluid level"))?;
            (
                if id.ends_with("water") {
                    "minecraft:water"
                } else {
                    "minecraft:lava"
                },
                level,
            )
        }
        _ => return Err(DecodeError::Field("fluid")),
    };
    let falling = properties.get("falling").is_some_and(|v| v == "true");
    // `FlowingFluid.getLegacyLevel`: a source is level 0.
    let source = matches!(id.as_str(), "minecraft:water" | "minecraft:lava");
    let level = if source {
        0
    } else {
        8 - amount.min(8) + if falling { 8 } else { 0 }
    };
    let blocks = names.blocks;
    let block = blocks
        .block_by_name(block)
        .ok_or(DecodeError::Field("fluid block"))?;
    let state = blocks
        .with_property(blocks.default_state(block), "level", &level.to_string())
        .ok_or(DecodeError::Field("fluid level"))?;
    // The still fluid of the block (what `state.getType()` is for a source).
    let fluid = crate::worldgen::placement::FLUIDS
        .iter()
        .position(|f| {
            *f == if id.contains("lava") {
                "minecraft:lava"
            } else {
                "minecraft:water"
            }
        })
        .ok_or(DecodeError::Field("fluid"))? as u8;
    Ok((state, fluid))
}

impl Misc {
    pub fn decode(
        kind: &str,
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        let fields = object(raw)?;
        debug_assert_eq!(type_of(&fields)?, kind);
        let names = ctx.names;
        Ok(Some(match kind {
            "minecraft:spring_feature" => {
                let (block, fluid) = legacy_block(required(&fields, "state")?, names)?;
                Self::Spring {
                    block,
                    fluid,
                    requires_block_below: optional(&fields, "requires_block_below")
                        .map_or(Ok(true), boolean)?,
                    rock_count: optional(&fields, "rock_count")
                        .map_or(Ok(4), |r| int_range(r, "rock_count", i32::MIN, i32::MAX))?,
                    hole_count: optional(&fields, "hole_count")
                        .map_or(Ok(1), |r| int_range(r, "hole_count", i32::MIN, i32::MAX))?,
                    valid_blocks: names.block_set(required(&fields, "valid_blocks")?)?,
                }
            }
            "minecraft:block_pile" => Self::BlockPile(BlockStateProvider::decode(
                required(&fields, "state_provider")?,
                ctx,
            )?),
            "minecraft:vines" => Self::Vines,
            "minecraft:underwater_magma" => Self::UnderwaterMagma {
                floor_search_range: int_range(
                    required(&fields, "floor_search_range")?,
                    "floor_search_range",
                    0,
                    512,
                )?,
                radius: int_range(
                    required(&fields, "placement_radius_around_floor")?,
                    "placement_radius_around_floor",
                    0,
                    64,
                )?,
                probability: unit(required(
                    &fields,
                    "placement_probability_per_valid_position",
                )?)?,
            },
            "minecraft:bamboo" => Self::Bamboo {
                probability: unit(required(&fields, "probability")?)?,
                podzol_replaceable: names
                    .block_tag("minecraft:beneath_bamboo_podzol_replaceable")?,
            },
            "minecraft:random_neighbor_spread" => Self::RandomNeighborSpread {
                block: BlockStateProvider::decode(required(&fields, "block")?, ctx)?,
                accepted_neighbors: names.block_set(required(&fields, "accepted_neighbors")?)?,
                can_replace: BlockPredicate::decode(required(&fields, "can_replace")?, names)?,
                attempts: IntProvider::decode_within(required(&fields, "attempts")?, 1, 3000)?,
                xz_offset: IntProvider::decode_within(required(&fields, "xz_offset")?, -16, 16)?,
                y_offset: IntProvider::decode_within(required(&fields, "y_offset")?, -16, 16)?,
            },
            _ => return Ok(None),
        }))
    }

    pub fn place<W: FeatureWorld, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match self {
            Self::Spring {
                block,
                fluid,
                requires_block_below,
                rock_count,
                hole_count,
                valid_blocks,
            } => spring(
                world,
                origin,
                *block,
                *fluid,
                *requires_block_below,
                *rock_count,
                *hole_count,
                valid_blocks,
            ),
            Self::BlockPile(provider) => block_pile(world, random, origin, provider),
            Self::Vines => vines(world, origin),
            Self::UnderwaterMagma {
                floor_search_range,
                radius,
                probability,
            } => underwater_magma(
                world,
                random,
                origin,
                *floor_search_range,
                *radius,
                *probability,
            ),
            Self::Bamboo {
                probability,
                podzol_replaceable,
            } => bamboo(world, random, origin, *probability, podzol_replaceable),
            Self::RandomNeighborSpread {
                block,
                accepted_neighbors,
                can_replace,
                attempts,
                xz_offset,
                y_offset,
            } => random_neighbor_spread(
                world,
                random,
                origin,
                block,
                accepted_neighbors,
                can_replace,
                attempts,
                xz_offset,
                y_offset,
            ),
        }
    }
}

/// `Codec.floatRange(0, 1)`.
fn unit(raw: &RawValue) -> Result<f32, DecodeError> {
    let value = float(raw)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(DecodeError::Field("probability"));
    }
    Ok(value)
}

fn is_air(world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, ProviderError> {
    let state = world.block_state(pos)?;
    Ok(world.state_data().has(state, FLAG_AIR))
}

fn is_in(
    world: &impl PlacementWorld,
    pos: BlockPos,
    set: &BiomeSet,
) -> Result<bool, ProviderError> {
    let state = world.block_state(pos)?;
    Ok(set.contains(world.blocks().block(state).0))
}

#[allow(clippy::too_many_arguments)]
fn spring<W: FeatureWorld>(
    world: &mut W,
    origin: BlockPos,
    block: BlockState,
    fluid: u8,
    requires_block_below: bool,
    rock_count: i32,
    hole_count: i32,
    valid: &BiomeSet,
) -> Result<bool, ProviderError> {
    let at = |dx, dy, dz| origin.offset(dx, dy, dz);
    if !is_in(world, at(0, 1, 0), valid)? {
        return Ok(false);
    }
    if requires_block_below && !is_in(world, at(0, -1, 0), valid)? {
        return Ok(false);
    }
    let current = world.block_state(origin)?;
    if !world.state_data().has(current, FLAG_AIR)
        && !valid.contains(world.blocks().block(current).0)
    {
        return Ok(false);
    }
    let sides = [(-1, 0, 0), (1, 0, 0), (0, 0, -1), (0, 0, 1), (0, -1, 0)];
    let mut rocks = 0;
    for (dx, dy, dz) in sides {
        rocks += i32::from(is_in(world, at(dx, dy, dz), valid)?);
    }
    let mut holes = 0;
    for (dx, dy, dz) in sides {
        holes += i32::from(is_air(world, at(dx, dy, dz))?);
    }
    if rocks == rock_count && holes == hole_count {
        world.set_block(origin, block, 2)?;
        world.schedule_fluid_tick(origin, fluid, 0)?;
        return Ok(true);
    }
    Ok(false)
}

fn block_pile<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    provider: &BlockStateProvider,
) -> Result<bool, ProviderError> {
    if origin.y < world.level_generation().min_y + 5 {
        return Ok(false);
    }
    let xr = 2 + random.next_int_bound(2)?;
    let zr = 2 + random.next_int_bound(2)?;
    // `BlockPos.betweenClosed`: x fastest, then y, then z.
    for dz in -zr..=zr {
        for dy in 0..=1 {
            for dx in -xr..=xr {
                let pos = origin.offset(dx, dy, dz);
                let (xd, zd) = (origin.x - pos.x, origin.z - pos.z);
                // The second draw only when the first test fails.
                if (xd * xd + zd * zd) as f32
                    <= random.next_float() * 10.0 - random.next_float() * 6.0
                    || random.next_float() < 0.031
                {
                    try_place_pile(world, random, pos, provider)?;
                }
            }
        }
    }
    Ok(true)
}

fn try_place_pile<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    pos: BlockPos,
    provider: &BlockStateProvider,
) -> Result<(), ProviderError> {
    if !is_air(world, pos)? {
        return Ok(());
    }
    let below = pos.offset(0, -1, 0);
    let below_state = world.block_state(below)?;
    let path = world
        .blocks()
        .block_by_name("minecraft:dirt_path")
        .is_some_and(|b| world.blocks().block(below_state) == b);
    let may_place = if path {
        random.next_boolean()
    } else {
        (world.state_data()).is_face_sturdy(below_state, Direction::Up, SupportType::Full)
    };
    if may_place {
        let state = provider.state(world, random, pos)?;
        world.set_block(pos, state, 260)?;
    }
    Ok(())
}

fn vines<W: FeatureWorld>(world: &mut W, origin: BlockPos) -> Result<bool, ProviderError> {
    if !is_air(world, origin)? {
        return Ok(false);
    }
    for direction in Direction::ALL {
        if direction == Direction::Down {
            continue;
        }
        let n = direction.normal();
        let neighbour = world.block_state(origin.offset(n[0], n[1], n[2]))?;
        let attach = can_attach_to(world.state_data(), neighbour, direction)
            .ok_or(ProviderError::Invalid("a dynamic shape"))?;
        if attach {
            let blocks = world.blocks();
            let vine = blocks
                .block_by_name("minecraft:vine")
                .ok_or(ProviderError::Invalid("no vine block"))?;
            let face = match direction {
                Direction::Up => "up",
                Direction::North => "north",
                Direction::South => "south",
                Direction::West => "west",
                _ => "east",
            };
            let state = blocks
                .with_property(blocks.default_state(vine), face, "true")
                .ok_or(ProviderError::Invalid("a vine without its face"))?;
            world.set_block(origin, state, 2)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_water(world: &impl PlacementWorld, state: BlockState) -> bool {
    let blocks = world.blocks();
    blocks
        .block_by_name("minecraft:water")
        .is_some_and(|water| blocks.block(state) == water)
}

fn underwater_magma<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    range: i32,
    radius: i32,
    probability: f32,
) -> Result<bool, ProviderError> {
    // `Column.scan` for the water column: the floor is the first non-water state below.
    let inside = |world: &W, pos: BlockPos| -> Result<bool, ProviderError> {
        Ok(is_water(world, world.block_state(pos)?))
    };
    if !inside(world, origin)? {
        return Ok(false);
    }
    let mut y = origin.y;
    let mut i = 1;
    while i < range && inside(world, BlockPos::new(origin.x, y, origin.z))? {
        y -= 1;
        i += 1;
    }
    if inside(world, BlockPos::new(origin.x, y, origin.z))? {
        return Ok(false);
    }
    let floor = BlockPos::new(origin.x, y, origin.z);
    let magma = {
        let blocks = world.blocks();
        let block = blocks
            .block_by_name("minecraft:magma_block")
            .ok_or(ProviderError::Invalid("no magma block"))?;
        blocks.default_state(block)
    };
    let mut placed = false;
    // `BlockPos.betweenClosedStream`: x fastest, then y, then z; the filters run per position.
    for dz in -radius..=radius {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let pos = floor.offset(dx, dy, dz);
                if random.next_float() < probability && valid_magma_position(world, pos)? {
                    world.set_block(pos, magma, 2)?;
                    placed = true;
                }
            }
        }
    }
    Ok(placed)
}

/// `UnderwaterMagmaFeature.isValidPlacement`.
fn valid_magma_position(world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, ProviderError> {
    let state = world.block_state(pos)?;
    if is_water(world, state) || world.state_data().has(state, FLAG_AIR) {
        return Ok(false);
    }
    if visible_from_outside(world, pos.offset(0, -1, 0), Direction::Up)? {
        return Ok(false);
    }
    for direction in [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ] {
        let n = direction.normal();
        if visible_from_outside(world, pos.offset(n[0], n[1], n[2]), direction.opposite())? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `!Block.isShapeFullBlock(state.getFaceOcclusionShape(covered))`.
fn visible_from_outside(
    world: &impl PlacementWorld,
    pos: BlockPos,
    covered: Direction,
) -> Result<bool, ProviderError> {
    let state = world.block_state(pos)?;
    Ok(!shape_fills_face(
        world.state_data().occlusion_shape(state),
        covered,
    ))
}

fn bamboo<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    probability: f32,
    podzol_replaceable: &BiomeSet,
) -> Result<bool, ProviderError> {
    if !is_air(world, origin)? {
        return Ok(false);
    }
    let (default, trunk, final_large, top_large, top_small, podzol) = {
        let blocks = world.blocks();
        let bamboo = blocks
            .block_by_name("minecraft:bamboo")
            .ok_or(ProviderError::Invalid("no bamboo block"))?;
        let with = |state, name, value| {
            blocks
                .with_property(state, name, value)
                .ok_or(ProviderError::Invalid("a bamboo without its property"))
        };
        let default = blocks.default_state(bamboo);
        let trunk = with(
            with(with(default, "age", "1")?, "leaves", "none")?,
            "stage",
            "0",
        )?;
        let podzol = blocks
            .block_by_name("minecraft:podzol")
            .ok_or(ProviderError::Invalid("no podzol block"))?;
        (
            default,
            trunk,
            with(with(trunk, "leaves", "large")?, "stage", "1")?,
            with(trunk, "leaves", "large")?,
            with(trunk, "leaves", "small")?,
            blocks.default_state(podzol),
        )
    };
    let mut top = origin;
    if world.block_survives(default, origin)? {
        let height = random.next_int_bound(12)? + 5;
        if random.next_float() < probability {
            let r = random.next_int_bound(4)? + 1;
            for xx in origin.x - r..=origin.x + r {
                for zz in origin.z - r..=origin.z + r {
                    let (xd, zd) = (xx - origin.x, zz - origin.z);
                    if xd * xd + zd * zd <= r * r {
                        let y = world.height(Heightmap::WorldSurface, xx, zz)? - 1;
                        let pos = BlockPos::new(xx, y, zz);
                        if is_in(world, pos, podzol_replaceable)? {
                            world.set_block(pos, podzol, 2)?;
                        }
                    }
                }
            }
        }
        let mut i = 0;
        while i < height && is_air(world, top)? {
            world.set_block(top, trunk, 2)?;
            top = top.offset(0, 1, 0);
            i += 1;
        }
        if top.y - origin.y >= 3 {
            world.set_block(top, final_large, 2)?;
            top = top.offset(0, -1, 0);
            world.set_block(top, top_large, 2)?;
            top = top.offset(0, -1, 0);
            world.set_block(top, top_small, 2)?;
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn random_neighbor_spread<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    block: &BlockStateProvider,
    accepted: &BiomeSet,
    can_replace: &BlockPredicate,
    attempts: &IntProvider,
    xz_offset: &IntProvider,
    y_offset: &IntProvider,
) -> Result<bool, ProviderError> {
    let state = block.state(world, random, origin)?;
    world.set_block(origin, state, 2)?;
    let attempts = attempts.sample(random)?;
    for _ in 0..attempts {
        // `setWithOffset(origin, xz, y, xz)`: the arguments are drawn in this order.
        let dx = xz_offset.sample(random)?;
        let dy = y_offset.sample(random)?;
        let dz = xz_offset.sample(random)?;
        let pos = origin.offset(dx, dy, dz);
        if can_replace.test(world, pos)? {
            let mut neighbours = 0;
            for direction in Direction::ALL {
                let n = direction.normal();
                if is_in(world, pos.offset(n[0], n[1], n[2]), accepted)? {
                    neighbours += 1;
                }
                if neighbours > 1 {
                    break;
                }
            }
            if neighbours == 1 {
                let state = block.state(world, random, pos)?;
                world.set_block(pos, state, 2)?;
            }
        }
    }
    Ok(true)
}
