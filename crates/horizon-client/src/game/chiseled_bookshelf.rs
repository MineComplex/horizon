//! Chiseled bookshelves: `ChiseledBookShelfBlock.useItemOn` / `useWithoutItem`
//! (a book into the slot under the crosshair, or out of it) and
//! `ChiseledBookShelfBlockEntity.updateState`, which also follows the books a
//! hopper or dropper moved (`machines::container_changed`). The slots' books
//! show through the block state's `slot_N_occupied`, which reaches a server's
//! clients with the block update.
use horizon_client::containers::BlockEntity;
use horizon_core::position::BlockPos;
use horizon_world::level::flags;

impl crate::game::Sim<'_> {
    /// `ChiseledBookShelfBlockEntity.updateState`: `slot` becomes the last
    /// one touched, the block state takes the slots' occupancy
    /// (`setBlockAndUpdate`) and `BLOCK_CHANGE` goes out.
    fn chiseled_bookshelf_update_state(&mut self, pos: [i32; 3], slot: usize) {
        let key = (pos[0], pos[1], pos[2]);
        let Some(BlockEntity::ChiseledBookshelf(b)) = self.world.block_entities.get_mut(&key)
        else {
            return;
        };
        b.last_slot = slot as i32;
        let occupied: Vec<bool> = b.items.iter().map(Option::is_some).collect();
        let registry = self.pack.assets.registry;
        let Some(mut state) = registry
            .state(self.world.block(pos[0], pos[1], pos[2]))
            .cloned()
        else {
            return;
        };
        for (i, occupied) in occupied.into_iter().enumerate() {
            state
                .properties
                .insert(format!("slot_{i}_occupied"), occupied.to_string());
        }
        let Some(new) = registry.id(&state) else {
            return;
        };
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| b.set_block(level, ticks, at, new, flags::UPDATE_ALL));
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
    }
    /// The books a hopper or dropper put in or took out: the slot whose
    /// occupancy no longer matches the block state is the one `setItem` or
    /// `removeItem` updated.
    pub(super) fn chiseled_bookshelf_synced(&mut self, pos: (i32, i32, i32)) {
        let Some(BlockEntity::ChiseledBookshelf(b)) = self.world.block_entities.get(&pos) else {
            return;
        };
        let Some(info) = self
            .pack
            .assets
            .registry
            .state(self.world.block(pos.0, pos.1, pos.2))
        else {
            return;
        };
        let moved = (0..6).rev().find(|i| {
            let shown = info
                .properties
                .get(&format!("slot_{i}_occupied"))
                .map(String::as_str);
            b.items[*i].is_some() != (shown == Some("true"))
        });
        if let Some(slot) = moved {
            self.chiseled_bookshelf_update_state([pos.0, pos.1, pos.2], slot);
        }
    }
}
