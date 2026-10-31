//! Small helpers the cave, nether and end features share.

use crate::block::BlockState;
use crate::block::flags::FLAG_AIR;
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::{InvalidBound, RandomSource};
use crate::worldgen::placement::PlacementWorld;
use crate::worldgen::state_provider::ProviderError;

/// `pos.relative(direction)`.
pub(super) fn relative(pos: BlockPos, direction: Direction) -> BlockPos {
    let n = direction.normal();
    pos.offset(n[0], n[1], n[2])
}

/// `pos.relative(direction, distance)`.
pub(super) fn relative_by(pos: BlockPos, direction: Direction, distance: i32) -> BlockPos {
    let n = direction.normal();
    pos.offset(n[0] * distance, n[1] * distance, n[2] * distance)
}

/// The blockstate property of a face (`MultifaceBlock.getFaceProperty`).
pub(super) fn face_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

/// `Direction.Plane.HORIZONTAL` in iteration order.
pub(super) const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// `Util.shuffle`: Fisher-Yates from the end with `nextInt(i)`.
pub(super) fn shuffle<T>(
    values: &mut [T],
    random: &mut impl RandomSource,
) -> Result<(), InvalidBound> {
    for i in (2..=values.len()).rev() {
        let swap = random.next_int_bound(i as i32)? as usize;
        values.swap(i - 1, swap);
    }
    Ok(())
}

pub(super) fn is_air(world: &impl PlacementWorld, pos: BlockPos) -> Result<bool, ProviderError> {
    let state = world.block_state(pos)?;
    Ok(world.state_data().has(state, FLAG_AIR))
}

/// Whether the state's block is the named one.
pub(super) fn is_block(world: &impl PlacementWorld, state: BlockState, name: &str) -> bool {
    let blocks = world.blocks();
    blocks
        .block_by_name(name)
        .is_some_and(|b| blocks.block(state) == b)
}

/// The block's default state; the name must exist in the table.
pub(super) fn default_state(
    blocks: &crate::block::BlockStates,
    name: &str,
) -> Result<BlockState, crate::worldgen::codec::DecodeError> {
    blocks
        .block_by_name(name)
        .map(|b| blocks.default_state(b))
        .ok_or(crate::worldgen::codec::DecodeError::Field("block"))
}

/// `Column`: the floor and ceiling a `Column.scan` found.
#[derive(Clone, Copy, Debug)]
pub(super) struct Column {
    pub floor: Option<i32>,
    pub ceiling: Option<i32>,
}

impl Column {
    /// `getHeight`: only a range with both edges has one.
    pub fn height(&self) -> Option<i32> {
        match (self.floor, self.ceiling) {
            (Some(floor), Some(ceiling)) => Some(ceiling - floor - 1),
            _ => None,
        }
    }
}

/// `Column.scan(level, pos, searchRange, insideColumn, validEdge)`.
pub(super) fn scan_column<W: PlacementWorld>(
    world: &W,
    pos: BlockPos,
    range: i32,
    inside: impl Fn(&W, BlockState) -> bool,
    edge: impl Fn(&W, BlockState) -> bool,
) -> Result<Option<Column>, ProviderError> {
    if !inside(world, world.block_state(pos)?) {
        return Ok(None);
    }
    let scan = |step: i32| -> Result<Option<i32>, ProviderError> {
        let mut p = pos;
        let mut i = 1;
        while i < range && inside(world, world.block_state(p)?) {
            p = p.offset(0, step, 0);
            i += 1;
        }
        Ok(edge(world, world.block_state(p)?).then_some(p.y))
    };
    let ceiling = scan(1)?;
    let floor = scan(-1)?;
    Ok(Some(Column { floor, ceiling }))
}

/// `state.isAir() || state.is(Blocks.WATER)`.
pub(super) fn empty_or_water(world: &impl PlacementWorld, state: BlockState) -> bool {
    world.state_data().has(state, FLAG_AIR) || is_block(world, state, "minecraft:water")
}

/// `Mth.floor(double)`.
pub(super) fn floor(value: f64) -> i32 {
    let i = value as i32;
    if value < f64::from(i) { i - 1 } else { i }
}
