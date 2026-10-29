//! Block entities the block behaviour removed on the client. What they spill
//! (`preRemoveSideEffects`) and what a block drops are the server's.
use super::{GameLevel, Screen};
use horizon_client::containers::BlockEntity;
use horizon_core::position::BlockPos;
use horizon_world::registry::BlockStateId;

#[derive(Default)]
pub(super) struct Spill {
    /// Block entities the behaviour removed, for the game's own follow-ups
    /// (open screens, playing songs).
    pub(super) removed: Vec<([i32; 3], BlockEntity)>,
}

/// The block entity at `pos` Horizon keeps as it was loaded, if any
/// (campfires, decorated pots, chiseled bookshelves, shelves).
pub(super) fn saved_entity(
    world: &horizon_client::world::World,
    pos: [i32; 3],
) -> Option<horizon_world::nbt::Tag> {
    use horizon_world::nbt::Tag;
    let saved = world
        .chunks
        .get(&(pos[0] >> 4, pos[2] >> 4))?
        .saved
        .as_ref()?;
    saved
        .block_entities
        .iter()
        .find(|t| {
            t.get("x") == Some(&Tag::Int(pos[0]))
                && t.get("y") == Some(&Tag::Int(pos[1]))
                && t.get("z") == Some(&Tag::Int(pos[2]))
        })
        .cloned()
}

impl GameLevel<'_> {
    /// `removeBlockEntity` for the block entity at `pos`. Its side effects
    /// (`preRemoveSideEffects`: the contents spilling, a record popping) are
    /// the server's (`LevelChunk.setBlockState` runs them only there).
    pub(super) fn remove_entity(
        &mut self,
        pos: BlockPos,
        _old: BlockStateId,
        _side_effects: bool,
    ) -> u8 {
        let key = (pos.x, pos.y, pos.z);
        self.remove_saved_entity(pos);
        self.world.block_names.remove(&key);
        if let Some(entity) = self.world.block_entities.remove(&key) {
            self.spill.removed.push(([pos.x, pos.y, pos.z], entity));
        }
        0
    }
    /// Forget the block entity Horizon keeps as it was loaded (campfires,
    /// decorated pots, chiseled bookshelves, shelves) at `pos`.
    fn remove_saved_entity(&mut self, pos: BlockPos) {
        use horizon_world::nbt::Tag;
        let at = |t: &Tag| {
            let int = |k: &str| match t.get(k) {
                Some(Tag::Int(v)) => Some(*v),
                _ => None,
            };
            int("x") == Some(pos.x) && int("y") == Some(pos.y) && int("z") == Some(pos.z)
        };
        let Some(chunk) = self.world.chunks.get_mut(&(pos.x >> 4, pos.z >> 4)) else {
            return;
        };
        let Some(index) = chunk
            .saved
            .as_ref()
            .and_then(|saved| saved.block_entities.iter().position(at))
        else {
            return;
        };
        let chunk = std::sync::Arc::make_mut(chunk);
        if let Some(saved) = chunk.saved.as_mut() {
            std::sync::Arc::make_mut(saved).block_entities.remove(index);
        }
    }
}

impl crate::game::Sim<'_> {
    /// Follow up on the block entities the block behaviour removed: a song
    /// stops, an open container's screen closes.
    pub(super) fn flush_spill(&mut self) {
        let spill = std::mem::take(&mut self.spill);
        for (pos, entity) in spill.removed {
            let key = (pos[0], pos[1], pos[2]);
            if matches!(entity, BlockEntity::Jukebox(_)) {
                self.jukebox_removed(pos);
            }
            if self.container == Some(key) {
                self.container_opener_event(key, false);
                self.container = None;
                self.screen = Screen::Playing;
            }
        }
    }
}
