//! Shields and other `blocks_attacks` items: raising them (`ShieldItem.use`,
//! from either hand), `LivingEntity.getItemBlockingWith` and
//! `applyItemBlocking` inputs, and what follows a block: item damage,
//! the block sound, attacker knockback (`blockedByItem`) and axes disabling
//! the shield (`Player.blockUsingItem`, `BlocksAttacks.disable`).
use horizon_client::items::ItemStack;

/// `Inventory.SLOT_OFFHAND`: the `using` slot of an off-hand item.
pub(super) const OFFHAND_SLOT: usize = 40;

impl crate::game::Game<'_> {
    /// The stack in an inventory slot, the off hand at `OFFHAND_SLOT`.
    pub(super) fn stack_in(&self, slot: usize) -> Option<&ItemStack> {
        if slot == OFFHAND_SLOT {
            self.offhand.as_ref()
        } else {
            self.inventory.get(slot)?.as_ref()
        }
    }
    pub(super) fn stack_in_mut(&mut self, slot: usize) -> &mut Option<ItemStack> {
        if slot == OFFHAND_SLOT {
            &mut self.offhand
        } else {
            &mut self.inventory[slot]
        }
    }
    pub(super) fn blocks_attacks(
        &self,
        stack: &ItemStack,
    ) -> Option<&horizon_client::items::BlocksAttacks> {
        self.res.items().get(&stack.item)?.blocks_attacks.as_ref()
    }
    /// `ShieldItem.use` (ItemUtils.startUsingInstantly) for the hand at `slot`;
    /// false when that hand holds no blocking item or it is on cooldown.
    pub(super) fn raise_shield(&mut self, slot: usize) -> bool {
        let Some(stack) = self.stack_in(slot) else {
            return false;
        };
        if self.blocks_attacks(stack).is_none() {
            return false;
        }
        if self.shield_cooldown == 0 && self.using.is_none() {
            self.using = Some((slot, 0));
        }
        true
    }
}
