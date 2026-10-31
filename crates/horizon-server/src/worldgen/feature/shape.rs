//! `BlockState.updateShape` for the plants a tree's edge touches, which is all that
//! `StructureTemplate.updateShapeAtEdge` changes in the decoration oracle's chunks. The block
//! behaviour port (WP6) owns the rest of `updateShape` (leaves, fences, walls...); this covers the
//! classes whose answer follows from `canSurvive` alone, and vines:
//!
//! - `VegetationBlock.updateShape`: air where the plant can no longer survive (every block whose
//!   `canSurvive` rule is a `TagBelow`: grass, ferns, flowers, saplings, bushes...);
//! - `DoublePlantBlock.updateShape`: air for a half whose other half is gone or replaced, and for a
//!   lower half that can no longer survive.
//!
//! - `VineBlock.updateShape`: the faces that lost their support dropped, air when none is left.
//!
//! A plant's own `canSurvive` is `BlockSurvival`'s, through [`PlacementWorld::block_survives`].

use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::survival::can_attach_to;
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::worldgen::placement::{PlacementWorld, WorldError};

/// Whether the state's `canSurvive` is `VegetationBlock`'s (a `TagBelow` rule) or a double plant's.
enum Plant {
    Vegetation,
    Double,
    Vine,
}

fn plant_of(world: &impl PlacementWorld, state: BlockState) -> Option<Plant> {
    let data = world.state_data();
    let rule = data.survival_rule(state)?;
    let name = &data.survival_rule_names()[rule];
    if &**name == "DoublePlant" {
        Some(Plant::Double)
    } else if &**name == "Vine" {
        Some(Plant::Vine)
    } else if name.starts_with("TagBelow") {
        Some(Plant::Vegetation)
    } else {
        None
    }
}

/// `state.updateShape(level, pos, direction, neighbour_pos, neighbour_state)`: air, or `state`.
fn update_shape(
    world: &impl PlacementWorld,
    state: BlockState,
    pos: BlockPos,
    direction: Direction,
    neighbour: BlockState,
) -> Result<BlockState, WorldError> {
    let Some(plant) = plant_of(world, state) else {
        return Ok(state);
    };
    let air = BlockState::AIR;
    match plant {
        Plant::Vegetation => Ok(if world.block_survives(state, pos)? {
            state
        } else {
            air
        }),
        Plant::Vine => {
            if direction == Direction::Down {
                return Ok(state);
            }
            let updated = vine_faces(world, state, pos)?;
            let blocks = world.blocks();
            let any = FACES
                .iter()
                .any(|(name, _)| blocks.value(updated, name) == Some("true"));
            Ok(if any { updated } else { air })
        }
        Plant::Double => {
            let blocks = world.blocks();
            let lower = blocks.value(state, "half") == Some("lower");
            let same_block = blocks.block(neighbour) == blocks.block(state);
            let neighbour_half = blocks.value(neighbour, "half");
            let other_half_gone = matches!(direction, Direction::Up | Direction::Down)
                && lower == (direction == Direction::Up)
                && !(same_block && neighbour_half != blocks.value(state, "half"));
            if other_half_gone {
                return Ok(air);
            }
            if lower && direction == Direction::Down && !world.block_survives(state, pos)? {
                return Ok(air);
            }
            Ok(state)
        }
    }
}

const FACES: [(&str, Direction); 5] = [
    ("up", Direction::Up),
    ("north", Direction::North),
    ("south", Direction::South),
    ("west", Direction::West),
    ("east", Direction::East),
];

/// `VineBlock.getUpdatedState`: each face the vine has is kept only where something supports it
/// (a block to attach to, or a vine above with the same face).
fn vine_faces(
    world: &impl PlacementWorld,
    state: BlockState,
    pos: BlockPos,
) -> Result<BlockState, WorldError> {
    let blocks = world.blocks();
    let above = world.block_state(pos.offset(0, 1, 0))?;
    let vine_above = blocks.block(above) == blocks.block(state);
    let mut state = state;
    for (name, direction) in FACES {
        if blocks.value(state, name) != Some("true") {
            continue;
        }
        let supported = if direction == Direction::Up {
            // `getUpdatedState` asks for the block above with DOWN.
            can_attach_to(world.state_data(), above, Direction::Down)
        } else {
            let n = direction.normal();
            let side = world.block_state(pos.offset(n[0], n[1], n[2]))?;
            can_attach_to(world.state_data(), side, direction)
                .map(|attach| attach || (vine_above && blocks.value(above, name) == Some("true")))
        }
        .ok_or(WorldError::Unsupported("a dynamic shape"))?;
        state = blocks
            .with_property(state, name, if supported { "true" } else { "false" })
            .unwrap_or(state);
    }
    Ok(state)
}

/// `StructureTemplate.updateShapeAtEdge` for one face: the voxel at `pos` and its neighbour in
/// `direction` are each told about the other's state.
pub fn update_shape_at_face<W: FeatureWorld>(
    world: &mut W,
    pos: BlockPos,
    direction: Direction,
    flags: i32,
) -> Result<(), WorldError> {
    let n = direction.normal();
    let neighbour_pos = pos.offset(n[0], n[1], n[2]);
    let state = world.block_state(pos)?;
    let neighbour = world.block_state(neighbour_pos)?;
    let new_state = update_shape(world, state, pos, direction, neighbour)?;
    if new_state != state {
        world.set_block(pos, new_state, flags & !1)?;
    }
    let new_neighbour = update_shape(
        world,
        neighbour,
        neighbour_pos,
        direction.opposite(),
        new_state,
    )?;
    if new_neighbour != neighbour {
        world.set_block(neighbour_pos, new_neighbour, flags & !1)?;
    }
    Ok(())
}
