//! The totem of undying (`LivingEntity.checkTotemDeathProtection` with the
//! totem's `death_protection` component, entity event 35 on the client).
use crate::hc::effects::EffectInstance;

const TOTEM: &str = "minecraft:totem_of_undying";

/// `EntityEvent.TOTEM_OF_UNDYING`.
pub(super) const TOTEM_EVENT: i8 = 35;

impl crate::game::Server<'_> {
    /// Whether either hand holds a totem, for the next hurt: the window's
    /// player once a tick, a server's player for each of its turns.
    pub(super) fn update_totem_ready(&mut self) {
        let ready = self.held().is_some_and(|s| &*s.item == TOTEM)
            || self.offhand.as_ref().is_some_and(|s| &*s.item == TOTEM);
        if let Some(p) = self.player.as_mut() {
            p.health.totem_ready = ready;
        }
    }
    /// The server's half once a totem fired: use it up (main hand first),
    /// count it, set health to 1, then clear and apply its effects. Returns
    /// the player's body, for entity event 35, which the caller broadcasts.
    pub(super) fn apply_totem(&mut self) -> Option<Body> {
        let p = self.player.as_mut()?;
        if !p.health.totem_used {
            return None;
        }
        p.health.totem_used = false;
        let _ = p.set_health(1.0, false);
        let body = Body {
            at: [p.position.x, p.position.y, p.position.z],
        };
        let slot = if self.held().is_some_and(|s| &*s.item == TOTEM) {
            Some(self.selected)
        } else {
            None
        };
        let cell = match slot {
            Some(i) => &mut self.inventory[i],
            None => &mut self.offhand,
        };
        if let Some(s) = cell.as_mut() {
            s.count -= 1;
            if s.count == 0 {
                *cell = None;
            }
        }
        self.stats.add("used", TOTEM, 1);
        self.trigger_item(
            "minecraft:used_totem",
            &crate::hc::items::ItemStack::new(TOTEM, 1),
        );
        // ItemStack.causeUseVibration: the totem's use effects make one.
        let ctx = self.player_context(None);
        self.post_game_event("minecraft:item_interact_finish", body.at, ctx);
        // DeathProtection.TOTEM_EFFECTS: clear, then regeneration II, absorption II, fire resistance.
        self.potions.effects.clear();
        for (id, ticks, amp) in [
            ("regeneration", 900, 1),
            ("absorption", 100, 1),
            ("fire_resistance", 800, 0),
        ] {
            if let Some(e) = crate::hc::brewing::effect(id) {
                self.add_player_effect(EffectInstance::new(e.id, ticks, amp));
            }
        }
        Some(body)
    }
}

/// Where an entity event's entity stands and its size.
#[derive(Clone, Copy)]
pub(super) struct Body {
    pub at: [f64; 3],
}
