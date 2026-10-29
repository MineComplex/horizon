//! Decorated pots, which Horizon keeps as loaded (the chunk's saved block
//! entities): the loot table a trial chamber pot holds, rolled into its item
//! the first time the item is read (`RandomizableContainer.unpackLootTable`),
//! and `DecoratedPotBlock.useItemOn` / `useWithoutItem`.
use horizon_client::{random_sequences::RandomSequences, save};
use horizon_core::random::LegacyRandom;
use horizon_world::{loot::LootData, nbt::Tag};

/// `RandomizableContainer.unpackLootTable(null)` on a pot's saved tag: the
/// loot table, if any, is removed and `LootTable.fill` puts its roll in
/// `item`. The roll draws from `LootTableSeed`, else the table's random
/// sequence, else the level random (`LootContext.Builder.create`).
pub(super) fn unpack_loot(
    tag: &mut Tag,
    loot: Option<&LootData>,
    random: &mut LegacyRandom,
    sequences: &mut RandomSequences,
    world_seed: i64,
) {
    let Tag::Compound(map) = tag else {
        return;
    };
    let Some(Tag::String(table)) = map.remove(&"LootTable".into()) else {
        return;
    };
    let seed = match map.remove(&"LootTableSeed".into()) {
        Some(Tag::Long(seed)) => seed,
        _ => 0,
    };
    map.remove(&"item".into());
    let Some(loot) = loot else {
        return;
    };
    let table = table.to_string_lossy();
    // No origin biome: the pot tables have no location checks.
    let slots = if seed != 0 {
        loot.fill(&table, seed, 1, None)
    } else if let Some(sequence) = loot.random_sequence(&table) {
        loot.fill_with(&table, sequences.get(sequence, world_seed), 1, None)
    } else {
        loot.fill_with(&table, random, 1, None)
    };
    if let Some(stack) = slots.into_iter().next().flatten() {
        let item = horizon_client::structure_blocks::loot_item_with_components(&stack);
        map.insert("item".into(), save::item_to_nbt(&item));
    }
}
