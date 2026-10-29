//! Crossbows (`CrossbowItem`): charging with the start/middle/end sounds
//! (Quick Charge), loading arrows or a held firework rocket into
//! `charged_projectiles` (Multishot loads three), and shooting critical
//! arrows at power 3.15, or rockets at 1.6 for three durability, with the
//! fanned Multishot angles and their shot pitches. `Player.getProjectile`
//! finds the ammo for bows and crossbows.
use super::{Game, shield::OFFHAND_SLOT};
use horizon_client::items::ItemStack;

const CROSSBOW: &str = "minecraft:crossbow";
const FIREWORK_ROCKET: &str = "minecraft:firework_rocket";

/// `ProjectileWeaponItem.ARROW_ONLY`, for the arrows Horizon shoots.
fn is_arrow(stack: &ItemStack) -> bool {
    &*stack.item == "minecraft:arrow"
}

/// `Player.getProjectile`: what `weapon` supports in the off hand, then in
/// the main hand (`getHeldProjectile`; crossbows also take firework rockets
/// there), else the first arrows in the inventory. The slot of the stack.
fn projectile_slot(
    weapon: &str,
    hands: [(usize, Option<&ItemStack>); 2],
    inventory: &[Option<ItemStack>],
) -> Option<usize> {
    let held = |s: &ItemStack| is_arrow(s) || weapon == CROSSBOW && &*s.item == FIREWORK_ROCKET;
    hands
        .into_iter()
        .find_map(|(slot, s)| s.is_some_and(held).then_some(slot))
        .or_else(|| {
            inventory
                .iter()
                .position(|s| s.as_ref().is_some_and(is_arrow))
        })
}

impl Game<'_> {
    pub(super) fn charge_ticks(stack: &ItemStack) -> u32 {
        // crossbow_charge_time: 1.25 s, minus 0.25 s per Quick Charge level.
        let seconds = 1.25 - 0.25 * stack.enchantment_level("minecraft:quick_charge") as f32;
        (seconds.max(0.0) * 20.0).floor() as u32
    }
}
impl crate::game::Game<'_> {
    /// `Player.getProjectile` for `weapon` (`projectile_slot`).
    pub(super) fn projectile_slot(&self, weapon: &str) -> Option<usize> {
        let hands = [OFFHAND_SLOT, self.selected].map(|slot| (slot, self.stack_in(slot)));
        projectile_slot(weapon, hands, &self.inventory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_projectiles_come_before_the_inventory() {
        let stack = |item: &str| Some(ItemStack::new(item, 1));
        let (crossbow, rocket, arrow) = (
            stack(CROSSBOW),
            stack(FIREWORK_ROCKET),
            stack("minecraft:arrow"),
        );
        let mut inventory = vec![None; 36];
        inventory[5] = arrow.clone();
        // The weapon in hotbar slot 0, `off` in the off hand.
        let slot = |weapon, off: &Option<ItemStack>, inventory: &[Option<ItemStack>]| {
            let hands = [(OFFHAND_SLOT, off.as_ref()), (0, crossbow.as_ref())];
            projectile_slot(weapon, hands, inventory)
        };
        // The off hand's rocket loads a crossbow; a bow only takes arrows.
        assert_eq!(slot(CROSSBOW, &rocket, &inventory), Some(OFFHAND_SLOT));
        assert_eq!(slot("minecraft:bow", &rocket, &inventory), Some(5));
        assert_eq!(
            slot("minecraft:bow", &arrow, &inventory),
            Some(OFFHAND_SLOT)
        );
        assert_eq!(slot(CROSSBOW, &None, &inventory), Some(5));
        // A rocket in the inventory is not ammo.
        inventory[5] = rocket.clone();
        assert_eq!(slot(CROSSBOW, &None, &inventory), None);
        // The main hand's rocket, with the crossbow in the off hand.
        let hands = [(OFFHAND_SLOT, crossbow.as_ref()), (3, rocket.as_ref())];
        assert_eq!(projectile_slot(CROSSBOW, hands, &inventory), Some(3));
    }
}
