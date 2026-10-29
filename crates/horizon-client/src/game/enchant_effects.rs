//! Enchantment effects on gameplay, read from the data-driven effect
//! components: efficiency and aqua affinity (`Player.getDestroySpeed`),
//! unbreaking (`minecraft:item_damage`), sharpness/smite/bane and power
//! (`minecraft:damage`), knockback, fire aspect, infinity (`minecraft:ammo_use`),
//! thorns (`minecraft:post_attack`), mending (`minecraft:repair_with_xp`) and
//! the armor attribute effects (respiration, depth strider, ...), Soul
//! Speed's `location_changed` and `tick` effects and the
//! `minecraft:location_changed` disk effect (frost walker).
use horizon_client::items::ItemStack;

impl crate::game::Game<'_> {
    /// `Player.getDestroySpeed` after the tool speed: mining efficiency, haste
    /// and mining fatigue.
    pub(super) fn modify_destroy_speed(&self, mut speed: f32) -> f32 {
        if speed > 1.0 {
            let held = self.inventory[self.selected].as_ref();
            speed += self
                .res
                .stations()
                .enchantments
                .attribute_add(held, "minecraft:mining_efficiency") as f32;
        }
        speed * self.potions.effects.mining_speed_factor()
    }
    /// `Attributes.SUBMERGED_MINING_SPEED` (0.2, raised by aqua affinity).
    pub(super) fn submerged_mining_speed(&self) -> f32 {
        let mut total = 0.0;
        for stack in self.armor.iter().flatten() {
            for (attr, _, amount, op) in self.res.stations().enchantments.attributes(stack) {
                if attr == "minecraft:submerged_mining_speed" && op == "add_multiplied_total" {
                    total += amount;
                }
            }
        }
        (0.2 * (1.0 + total)) as f32
    }
    /// Whether a worn enchantment gives immunity to `#burn_from_stepping`
    /// damage (frost walker's `minecraft:damage_immunity`).
    pub(crate) fn wears_burn_from_stepping_immunity(&self) -> bool {
        self.armor
            .iter()
            .zip(["head", "chest", "legs", "feet"])
            .any(|(stack, slot)| {
                stack
                    .iter()
                    .flat_map(ItemStack::enchantments)
                    .any(|(id, _)| {
                        self.res.stations().enchantments.get(id).is_some_and(|def| {
                            def.effects.get("minecraft:damage_immunity").is_some()
                                && def
                                    .slots
                                    .iter()
                                    .any(|s| s == slot || s == "armor" || s == "any")
                        })
                    })
            })
    }
    /// Enchantment attribute modifiers of worn armor (respiration, depth strider...).
    /// Each piece's modifier id ends with its slot (`EnchantmentAttributeEffect.idForSlot`),
    /// so the same enchantment on two pieces stacks instead of replacing itself.
    pub(super) fn apply_enchantment_attributes(&mut self) {
        let mut wanted: Vec<(String, String, f64, String)> = Vec::new();
        for (stack, slot) in self.armor.iter().zip(["head", "chest", "legs", "feet"]) {
            let Some(stack) = stack else { continue };
            for (attr, id, amount, op) in self.res.stations().enchantments.attributes(stack) {
                wanted.push((attr, format!("{id}/{slot}"), amount, op));
            }
        }
        let Some(p) = self.player.as_mut() else {
            return;
        };
        for (attr, id, _, op) in std::mem::take(&mut self.potions.enchant_modifiers) {
            let _ = p.set_modifier(&attr, &id, 0.0, &op, false);
        }
        for (attr, id, amount, op) in &wanted {
            let _ = p.set_modifier(attr, id, *amount, op, true);
        }
        self.potions.enchant_modifiers = wanted;
    }
}
