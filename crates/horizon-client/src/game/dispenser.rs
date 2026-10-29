//! Dispenser behaviours: `DispenserBlock.getDispenseMethod(..).dispense` with
//! every `DispenseItemBehavior` registration of vanilla that Horizon has the
//! entities for. A behaviour returns what is left in the slot and the level
//! event it plays (`DefaultDispenseItemBehavior.dispense` then plays that
//! event and the smoke animation, event 2000).
use super::Game;
use horizon_client::items::ItemStack;
use horizon_core::{
    geometry::Vec3,
    random::{LegacyRandom, RandomSource},
};
use horizon_world::registry::BlockStateId;

thread_local! {
    /// `OptionalDispenseItemBehavior.success` of the registered behaviours
    /// that leave it unset on some paths (brush, TNT, honeycomb, chest).
    /// Vanilla keeps one behaviour instance per item for the whole run, so
    /// there the flag carries over from that item's previous dispense.
    static SUCCESS: std::cell::RefCell<std::collections::HashMap<&'static str, bool>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// `RandomSource.create()`: the unseeded random an entity owns
/// (`RandomSupport.generateUniqueSeed`). Draws of an entity's own random never
/// touch the level's stream.
pub(super) fn entity_random() -> LegacyRandom {
    use std::sync::atomic::{AtomicI64, Ordering};
    static UNIQUIFIER: AtomicI64 = AtomicI64::new(8_682_522_807_148_012);
    const STEP: i64 = 1_181_783_497_276_652_981;
    // seedUniquifier.updateAndGet(s -> s * STEP)
    let mut current = UNIQUIFIER.load(Ordering::Relaxed);
    let seed = loop {
        let next = current.wrapping_mul(STEP);
        match UNIQUIFIER.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break next,
            Err(actual) => current = actual,
        }
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    LegacyRandom::new(seed ^ nanos)
}
/// `new ItemEntity(level, x, y, z, stack)`'s motion, from the item's own random.
pub(super) fn new_item_motion() -> [f64; 3] {
    let mut r = entity_random();
    let x = r.next_double() * 0.2 - 0.1;
    let z = r.next_double() * 0.2 - 0.1;
    [x, 0.2, z]
}

impl Game<'_> {
    // ---- level events (LevelEventHandler) ----

    // ---- DefaultDispenseItemBehavior ----

    // ---- the registry ----

    // ---- entities ----

    // ---- ProjectileDispenseBehavior ----

    // ---- entities placed by items ----

    // ---- buckets ----

    // ---- fire, bone meal and TNT ----

    // ---- blocks placed by dispensers ----

    // ---- bottles, glowstone, shears, brush, honeycomb ----
}
impl crate::game::Sim<'_> {
    /// The same block with other properties (`withProperty`).
    pub(super) fn with_props(
        &self,
        state: BlockStateId,
        props: &[(&str, &str)],
    ) -> Option<BlockStateId> {
        let mut s = self.pack.assets.registry.state(state)?.clone();
        for (k, v) in props {
            s.properties.insert((*k).to_owned(), (*v).to_owned());
        }
        self.pack.assets.registry.resolve(&s).ok()
    }
    /// `LootTable.getRandomItems` of `table` in the context of `parent`: the
    /// draws come from `parent`'s server random sequence when it names one (a
    /// nested `loot_table` entry shares its parent's context), else the level's.
    pub(super) fn sequence_loot(
        &mut self,
        parent: &str,
        table: &str,
        held: Option<&ItemStack>,
    ) -> Vec<ItemStack> {
        match self.res.items().loot_sequence(parent) {
            Some(sequence) => {
                let random = self.rand_sequences.get(&sequence, self.level.seed);
                self.res.items().loot_table_from(table, false, held, random)
            }
            None => self
                .res
                .items()
                .loot_table(table, false, held, &mut self.world.random),
        }
    }
    /// `Entity.spawnAtLocation(level, stack, offset)`: an item at the entity's
    /// feet plus the offset, moving as `new ItemEntity` makes it.
    pub(super) fn spawn_at_location(
        &mut self,
        at: Vec3,
        y_offset: f64,
        stack: ItemStack,
    ) -> [f64; 3] {
        let v = new_item_motion();
        self.spawn_item(stack, [at.x, at.y + y_offset, at.z], v, 10);
        v
    }
}
/// A tipped arrow's lasting effect at an eighth of the potion's duration
/// (`Arrow.doPostHurtEffects`).
pub(super) fn tipped_effect(stack: &ItemStack) -> Option<(&'static str, i32)> {
    stack
        .potion()
        .and_then(horizon_client::brewing::potion)
        .and_then(|def| {
            def.effects
                .iter()
                .find(|(id, _, _)| !horizon_client::brewing::effect(id).is_some_and(|e| e.instant))
                .map(|(id, duration, _)| (*id, (duration / 8).max(1)))
        })
}
