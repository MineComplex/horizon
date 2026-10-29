//! The creaking heart block entity in the game: `mobs::creaking_heart`
//! ticks each heart (spawning, letting go of and bleeding for its creaking)
//! and this applies what it reports to the world, the mobs and the speakers.
use horizon_client::containers::BlockEntity;
use horizon_core::position::BlockPos;

/// Comparator input of a heart (`getAnalogOutputSignal`).
pub(super) fn heart_signal(world: &horizon_client::world::World, pos: BlockPos) -> Option<i32> {
    match world.block_entities.get(&(pos.x, pos.y, pos.z))? {
        BlockEntity::CreakingHeart(h) => Some(h.output_signal),
        _ => None,
    }
}

impl crate::game::Sim<'_> {}
