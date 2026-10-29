//! Chiseled bookshelves: `ChiseledBookShelfBlock.useItemOn` / `useWithoutItem`
//! (a book into the slot under the crosshair, or out of it) and
//! `ChiseledBookShelfBlockEntity.updateState`, which also follows the books a
//! hopper or dropper moved (`machines::container_changed`). The slots' books
//! show through the block state's `slot_N_occupied`, which reaches a server's
//! clients with the block update.

impl crate::game::Sim<'_> {}
