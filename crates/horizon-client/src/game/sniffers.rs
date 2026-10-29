//! Sniffers on the game side: the egg a bred pair lays
//! (`Sniffer.spawnChildFromBreeding`), the snifflet that hatches from an
//! egg block (`SnifferEggBlock.tick`) and the dust and sound of digging.
use horizon_client::{items::ItemStack, mobs::Mob};
use horizon_core::random::RandomSource;

impl crate::game::Sim<'_> {
    /// The egg item at the parent (`Sniffer.spawnChildFromBreeding`; the
    /// brain already played the plop and dropped the experience).
    pub(super) fn lay_sniffer_egg(&mut self, parent: &Mob) {
        let at = [parent.position.x, parent.position.y, parent.position.z];
        // new ItemEntity(level, x, y, z, stack): a little random push.
        let vx = self.world.random.next_double() * 0.2 - 0.1;
        let vz = self.world.random.next_double() * 0.2 - 0.1;
        self.spawn_item(
            ItemStack::new("minecraft:sniffer_egg", 1),
            at,
            [vx, 0.2, vz],
            10,
        );
    }
}
