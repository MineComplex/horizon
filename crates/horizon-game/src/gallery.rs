//! The model gallery of the test scene (`World::test_scene`): sixteen block states in a row
//! of plinths, the same with or without block models loaded.
use crate::Result;
use horizon_core::position::BlockPos;
use horizon_world::{
    chunk::BlockState,
    registry::{BlockRegistry, BlockStateId},
};

pub type Placement = (&'static str, &'static [(&'static str, &'static str)]);
pub const GALLERY: [Placement; 16] = [
    (
        "oak_stairs",
        &[
            ("facing", "north"),
            ("half", "top"),
            ("shape", "inner_left"),
        ],
    ),
    ("stone_slab", &[("type", "top")]),
    ("anvil", &[("facing", "east")]),
    ("torch", &[]),
    ("lantern", &[]),
    ("oak_fence", &[("north", "true"), ("east", "true")]),
    ("oak_fence_gate", &[("facing", "west"), ("open", "true")]),
    ("iron_bars", &[("north", "true"), ("south", "true")]),
    ("cobweb", &[]),
    ("grindstone", &[("face", "wall"), ("facing", "west")]),
    ("ladder", &[("facing", "west")]),
    ("amethyst_cluster", &[("facing", "east")]),
    ("stone", &[]),
    ("stone", &[]),
    ("stone", &[]),
    ("stone", &[]),
];

pub fn descriptor(name: &str, overrides: &[(&str, &str)]) -> BlockState {
    BlockState {
        name: format!("minecraft:{name}"),
        properties: overrides
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

/// Where each gallery state stands.
pub fn placements(registry: &BlockRegistry) -> Result<Vec<(BlockPos, BlockStateId)>> {
    GALLERY
        .iter()
        .enumerate()
        .map(|(index, (name, properties))| {
            let state = registry.resolve(&descriptor(name, properties))?;
            Ok((position(index), state))
        })
        .collect()
}
/// The gallery's `index`th plinth.
pub fn position(index: usize) -> BlockPos {
    BlockPos::new((index % 4) as i32 * 4 - 7, 4, (index / 4) as i32 * 4 + 14)
}
