//! The attribute instances a server syncs for the living entities it tracks
//! (`ClientboundUpdateAttributesPacket`, lan/attributes.rs): the ones
//! vanilla's `AttributeMap` would have created by then, since only those are
//! sent, never the type's other defaults.
use super::Game;
use horizon_client::{
    lan::attributes::operation_named,
    mobs::{
        Mob,
        finalize::{self, Attr, Op}}};

/// `EquipmentSlotGroup.test`: whether a modifier's slot group covers a slot.
fn slot_in_group(group: &str, slot: &str) -> bool {
    group == slot
        || group == "any"
        || (group == "hand" && matches!(slot, "mainhand" | "offhand"))
        || (group == "armor" && matches!(slot, "feet" | "legs" | "chest" | "head" | "body"))
}

impl crate::game::Sim<'_> {
    /// `getAttributeValue(KNOCKBACK_RESISTANCE)` of a mob: its own value plus
    /// the modifiers of the items it wears or holds in matching slots.
    pub(super) fn mob_knockback_resistance(&self, m: &Mob) -> f64 {
        let e = &m.extra;
        let slots = [
            ("mainhand", &e.main_hand),
            ("offhand", &e.off_hand),
            ("feet", &e.feet),
            ("legs", &e.legs),
            ("chest", &e.chest),
            ("head", &e.head),
        ];
        let mut extra = Vec::new();
        for (slot, item) in slots {
            let Some(info) = item.as_ref().and_then(|i| self.res.items().get(i)) else {
                continue;
            };
            for (attribute, _, amount, op, group) in &info.modifiers {
                if attribute == "minecraft:knockback_resistance" && slot_in_group(group, slot) {
                    let op = match operation_named(op) {
                        1 => Op::AddMultipliedBase,
                        2 => Op::AddMultipliedTotal,
                        _ => Op::AddValue,
                    };
                    extra.push((*amount, op));
                }
            }
        }
        finalize::value_with(m, Attr::KnockbackResistance, &extra)
    }
}

#[cfg(test)]
mod tests {
    use super::slot_in_group;

    #[test]
    fn slot_groups_cover_their_slots() {
        assert!(slot_in_group("head", "head"));
        assert!(slot_in_group("armor", "chest"));
        assert!(!slot_in_group("armor", "mainhand"));
        assert!(slot_in_group("hand", "offhand"));
        assert!(slot_in_group("any", "feet"));
        assert!(!slot_in_group("mainhand", "offhand"));
    }
}
