//! Block classifications and helpers shared by tree, fallen-tree and other vegetation features.
//! Built once per registry from the block tags, names, properties and the state data.

use super::hash_order::{JavaHashMap, JavaKey};
use crate::block::flags::{
    FLAG_AIR, FLAG_LEGACY_SOLID, FLAG_LIQUID, FLAG_REPLACEABLE, FLAG_SOLID_RENDER,
};
use crate::block::{BlockId, BlockState, BlockStateData, BlockStates};
use crate::core::geometry::Direction;
use crate::util::random::{InvalidBound, RandomSource};
use crate::worldgen::codec::DecodeError;

pub(crate) const AIR: u32 = 1;
pub(crate) const LEAVES: u32 = 1 << 1;
pub(crate) const REPLACEABLE_BY_TREES: u32 = 1 << 2;
pub(crate) const LOGS: u32 = 1 << 3;
pub(crate) const PREVENTS_LEAF_DECAY: u32 = 1 << 4;
pub(crate) const VINE: u32 = 1 << 5;
pub(crate) const WATER_BLOCK: u32 = 1 << 6;
pub(crate) const WATER_SOURCE: u32 = 1 << 7;
pub(crate) const WATER_FLUID: u32 = 1 << 8;
pub(crate) const REPLACEABLE: u32 = 1 << 9;
pub(crate) const SOLID_RENDER: u32 = 1 << 10;
pub(crate) const SHELF_MUSHROOM: u32 = 1 << 11;
pub(crate) const PERSISTENT: u32 = 1 << 12;
pub(crate) const WATERLOGGABLE: u32 = 1 << 13;
pub(crate) const SOLID: u32 = 1 << 14;
pub(crate) const LAVA_FLUID: u32 = 1 << 15;
pub(crate) const ANY_FLUID: u32 = 1 << 16;
pub(crate) const FULL_AXIS: u32 = 1 << 17;
pub(crate) const MUSHROOM_REPLACEABLE: u32 = 1 << 18;
pub(crate) const FIRE: u32 = 1 << 19;
pub(crate) const SCULK_BLOCKER: u32 = 1 << 20;
pub(crate) const SCULK_VEIN: u32 = 1 << 21;
pub(crate) const LIQUID: u32 = 1 << 22;
pub(crate) const FEATURES_CANNOT_REPLACE: u32 = 1 << 23;
pub(crate) const CHEST: u32 = 1 << 24;
pub(crate) const SPAWNER: u32 = 1 << 25;
/// Any source fluid state (water or lava), including waterlogged blocks.
pub(crate) const FLUID_SOURCE: u32 = 1 << 26;
/// Fluid amount 8 (sources and falling fluids).
pub(crate) const FLUID_FULL: u32 = 1 << 27;
pub(crate) const BASE_STONE_OVERWORLD: u32 = 1 << 28;

/// Immutable per-state facts. Missing tags or blocks are empty classifications.
pub struct TreeBlocks {
    flags: Vec<u32>,
    /// `LeavesBlock.DISTANCE` (1..=7) values; 0 when absent.
    distance: Vec<u8>,
}
impl std::fmt::Debug for TreeBlocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeBlocks")
            .field("states", &self.flags.len())
            .finish_non_exhaustive()
    }
}
impl TreeBlocks {
    pub fn build(
        blocks: &BlockStates,
        data: &BlockStateData,
        block_tags: &[(String, Vec<i32>)],
    ) -> Self {
        let count = blocks.state_count() as usize;
        let mut flags = vec![0u32; count];
        let mut distance = vec![0u8; count];
        let mark_block = |block: u32, flag: u32, flags: &mut Vec<u32>| {
            for state in blocks.states(BlockId(block)) {
                flags[state.0 as usize] |= flag;
            }
        };
        for (tag, flag) in [
            ("minecraft:leaves", LEAVES),
            ("minecraft:replaceable_by_trees", REPLACEABLE_BY_TREES),
            ("minecraft:logs", LOGS),
            ("minecraft:prevents_nearby_leaf_decay", PREVENTS_LEAF_DECAY),
            ("minecraft:replaceable_by_mushrooms", MUSHROOM_REPLACEABLE),
            ("minecraft:fire", FIRE),
            ("minecraft:base_stone_overworld", BASE_STONE_OVERWORLD),
            ("minecraft:features_cannot_replace", FEATURES_CANNOT_REPLACE),
        ] {
            if let Some((_, ids)) = block_tags.iter().find(|(t, _)| t == tag) {
                for &id in ids {
                    mark_block(id as u32, flag, &mut flags);
                }
            }
        }
        for (name, flag) in [
            ("minecraft:vine", VINE),
            ("minecraft:water", WATER_BLOCK),
            ("minecraft:shelf_mushroom", SHELF_MUSHROOM),
            ("minecraft:sculk", SCULK_BLOCKER),
            ("minecraft:sculk_catalyst", SCULK_BLOCKER),
            ("minecraft:moving_piston", SCULK_BLOCKER),
            ("minecraft:sculk_vein", SCULK_VEIN),
            ("minecraft:chest", CHEST),
            ("minecraft:spawner", SPAWNER),
        ] {
            if let Some(block) = blocks.block_by_name(name) {
                mark_block(block.0, flag, &mut flags);
            }
        }
        for index in 0..count {
            let state = BlockState(index as u32);
            let flag = &mut flags[index];
            if data.has(state, FLAG_AIR) {
                *flag |= AIR;
            }
            if data.has(state, FLAG_REPLACEABLE) {
                *flag |= REPLACEABLE;
            }
            if data.has(state, FLAG_SOLID_RENDER) {
                *flag |= SOLID_RENDER;
            }
            if data.has(state, FLAG_LEGACY_SOLID) {
                *flag |= SOLID;
            }
            if data.has(state, FLAG_LIQUID) {
                *flag |= LIQUID;
            }
            let fluid = data.fluid(state);
            let name = data.fluid_name(fluid.fluid);
            match name {
                "minecraft:water" => *flag |= WATER_SOURCE | WATER_FLUID | ANY_FLUID,
                "minecraft:flowing_water" => *flag |= WATER_FLUID | ANY_FLUID,
                "minecraft:lava" | "minecraft:flowing_lava" => *flag |= LAVA_FLUID | ANY_FLUID,
                _ => {}
            }
            // `FluidState.isSource`: the still fluids are sources.
            if matches!(name, "minecraft:water" | "minecraft:lava") {
                *flag |= FLUID_SOURCE;
            }
            if fluid.amount == 8 {
                *flag |= FLUID_FULL;
            }
            if blocks.value(state, "persistent") == Some("true") {
                *flag |= PERSISTENT;
            }
            let block = blocks.block(state);
            if let Some(p) = blocks.property(block, "waterlogged")
                && blocks.property_values(p).len() == 2
            {
                *flag |= WATERLOGGABLE;
            }
            if let Some(p) = blocks.property(block, "axis") {
                let values = blocks.property_values(p);
                if values.len() == 3
                    && ["x", "y", "z"]
                        .iter()
                        .all(|a| values.iter().any(|v| &**v == *a))
                {
                    *flag |= FULL_AXIS;
                }
            }
            if let Some(p) = blocks.property(block, "distance") {
                let values = blocks.property_values(p);
                if values.len() == 7 && (1..=7).all(|n| values.iter().any(|v| **v == n.to_string()))
                {
                    distance[index] = blocks
                        .value(state, "distance")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                }
            }
        }
        Self { flags, distance }
    }
    pub(crate) fn has(&self, state: BlockState, flag: u32) -> bool {
        self.flags
            .get(state.0 as usize)
            .is_some_and(|value| value & flag != 0)
    }
    pub(crate) fn is_air(&self, state: BlockState) -> bool {
        self.has(state, AIR)
    }
    /// TreeFeature.validTreePos: air or replaceable by trees.
    pub(crate) fn valid_tree_pos(&self, state: BlockState) -> bool {
        self.has(state, AIR | REPLACEABLE_BY_TREES)
    }
    /// LeavesBlock.getOptionalDistanceAt.
    pub(crate) fn leaf_distance(&self, state: BlockState) -> Option<i32> {
        if self.has(state, PREVENTS_LEAF_DECAY) {
            return Some(0);
        }
        match self.distance.get(state.0 as usize) {
            Some(0) | None => None,
            Some(d) => Some(i32::from(*d)),
        }
    }
    /// BlockState.trySetValue(RotatedPillarBlock.AXIS, axis).
    pub(crate) fn try_axis(
        &self,
        blocks: &BlockStates,
        state: BlockState,
        axis: Axis,
    ) -> BlockState {
        if self.has(state, FULL_AXIS) {
            blocks
                .with_property(state, "axis", axis.name())
                .unwrap_or(state)
        } else {
            state
        }
    }
    /// Set WATERLOGGED when the state has that property; otherwise unchanged.
    pub(crate) fn waterlogged(
        &self,
        blocks: &BlockStates,
        state: BlockState,
        value: bool,
    ) -> BlockState {
        if self.has(state, WATERLOGGABLE) {
            blocks
                .with_property(state, "waterlogged", if value { "true" } else { "false" })
                .unwrap_or(state)
        } else {
            state
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
    pub(crate) fn of(direction: Direction) -> Self {
        match direction {
            Direction::West | Direction::East => Self::X,
            Direction::Down | Direction::Up => Self::Y,
            Direction::North | Direction::South => Self::Z,
        }
    }
}

/// Direction.Plane.HORIZONTAL order.
pub(crate) const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];
pub(crate) fn clockwise(direction: Direction) -> Direction {
    match direction {
        Direction::North => Direction::East,
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        other => other,
    }
}
pub(crate) fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}
pub(crate) fn decode_direction(name: &str) -> Result<Direction, DecodeError> {
    Direction::ALL
        .into_iter()
        .find(|d| direction_name(*d) == name)
        .ok_or(DecodeError::Field("direction"))
}
pub(crate) fn offset(p: [i32; 3], dx: i32, dy: i32, dz: i32) -> [i32; 3] {
    [
        p[0].wrapping_add(dx),
        p[1].wrapping_add(dy),
        p[2].wrapping_add(dz),
    ]
}
pub(crate) fn relative(p: [i32; 3], direction: Direction, distance: i32) -> [i32; 3] {
    let n = direction.normal();
    offset(
        p,
        n[0].wrapping_mul(distance),
        n[1].wrapping_mul(distance),
        n[2].wrapping_mul(distance),
    )
}
/// Vec3i.distManhattan, including its float accumulation.
pub(crate) fn dist_manhattan(a: [i32; 3], b: [i32; 3]) -> i32 {
    let d = |i: usize| b[i].wrapping_sub(a[i]).wrapping_abs() as f32;
    (d(0) + d(1) + d(2)) as i32
}

/// A position as `java.util.HashSet<BlockPos>` hashes it (`Vec3i.hashCode`). Two positions with
/// the same hash in a tree bin would be ordered by identity hash codes in vanilla, which cannot be
/// reproduced: they are ordered by position instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JavaPos([i32; 3]);

impl JavaKey for JavaPos {
    fn java_hash(&self) -> i32 {
        let [x, y, z] = self.0;
        y.wrapping_add(z.wrapping_mul(31))
            .wrapping_mul(31)
            .wrapping_add(x)
    }
    fn tie_break(&self, other: &Self) -> std::cmp::Ordering {
        let key = |p: &Self| (p.0[1], p.0[2], p.0[0]);
        match key(self).cmp(&key(other)) {
            std::cmp::Ordering::Equal => std::cmp::Ordering::Less,
            ordering => ordering,
        }
    }
}

/// A set of positions that iterates the way a `java.util.HashSet<BlockPos>` does, which decides
/// the order of the random draws made over a tree's trunks and leaves ([`JavaHashMap`] models the
/// buckets, resizes and tree bins).
#[derive(Default)]
pub(crate) struct PosSet(JavaHashMap<JavaPos, ()>);

impl PosSet {
    pub(crate) fn insert(&mut self, p: [i32; 3]) -> bool {
        self.0.put(JavaPos(p), ()).is_none()
    }
    pub(crate) fn contains(&self, p: [i32; 3]) -> bool {
        self.0.get(&JavaPos(p)).is_some()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Iteration order of the set.
    pub(crate) fn to_vec(&self) -> Vec<[i32; 3]> {
        self.0.entries().map(|(k, _)| k.0).collect()
    }
    /// `iterator().next()` followed by `iterator.remove()`.
    pub(crate) fn pop_first(&mut self) -> Option<[i32; 3]> {
        let mut cursor = self.0.cursor();
        let first = self.0.next_entry(&mut cursor).ok().flatten()?.0.0;
        self.0.remove_current(&mut cursor).ok()?;
        Some(first)
    }
}

/// Util.shuffle (Fisher-Yates from the end with nextInt(i)).
pub(crate) fn shuffle<T>(
    values: &mut [T],
    random: &mut impl RandomSource,
) -> Result<(), InvalidBound> {
    for i in (2..=values.len()).rev() {
        let swap = random.next_int_bound(i as i32)? as usize;
        values.swap(i - 1, swap);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pos_set_iterates_in_hash_bucket_order() {
        let mut set = PosSet::default();
        let inserted = [[0, 64, 1], [1, 64, 0], [0, 65, 0], [0, 64, 0]];
        for p in inserted {
            set.insert(p);
        }
        // `BlockPos.hashCode` spread by `h ^ (h >>> 16)` into 16 buckets; ties keep insertion order.
        let mut expected = inserted.to_vec();
        expected.sort_by_key(|p| {
            let h = (p[1] + p[2] * 31) * 31 + p[0];
            (h as u32 ^ (h as u32 >> 16)) & 15
        });
        assert_eq!(set.to_vec(), expected);
        assert_eq!(set.pop_first(), Some(expected[0]));
        assert!(!set.contains(expected[0]));
    }
}
