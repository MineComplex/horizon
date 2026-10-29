//! The local player's hand: clicks and held attack through
//! `horizon_client::first_person` (`Minecraft.startAttack` /
//! `continueAttack`, `MultiPlayerGameMode` block breaking), the arm swing
//! (`LivingEntity.SwingState`) and the first-person equip heights
//! (`FirstPersonHandsAndItems`) with the item swap ticker they read.
use super::{Game, minecarts::CartKind};
use horizon_client::{
    attack_strength::HeldStack,
    first_person::{Hand, SwingAnimation, swing_duration},
    items::ItemStack,
};

/// The off hand's place in `visible_hands` (the main hand uses its hotbar slot).
const OFFHAND: usize = usize::MAX;

/// `AttackRange.isInRange`: float bounds, then inclusive double comparison.
fn attack_range_contains(range: &serde_json::Value, distance: f64, creative: bool) -> bool {
    let value = |key: &str, default: f32| range[key].as_f64().map_or(default, |v| v as f32);
    let (min, max) = if creative {
        (
            value("min_creative_reach", 0.0),
            value("max_creative_reach", 5.0),
        )
    } else {
        (value("min_reach", 0.0), value("max_reach", 3.0))
    };
    let margin = value("hitbox_margin", 0.3);
    distance >= f64::from(min - margin) && distance <= f64::from(max + margin)
}

impl Game<'_> {
    /// `LocalPlayer.tick`: the swing (`LivingEntity.baseTick`), the attack
    /// strength and item swap tickers (`Player.tick`), then
    /// `FirstPersonHandsAndItems.tick`.
    pub(super) fn tick_hand(&mut self) {
        // LocalPlayer.applyInput (the camera is the player's own).
        if self.player.is_some() {
            self.sway.tick(self.pitch, self.yaw);
        }
        self.swing.tick();
        let main = self.inventory[self.selected].clone();
        // Player.tick: a different item resets the attack strength, and
        // detectEquipmentUpdates gives the player its modifiers.
        if self.hand_strength.tick(main.as_ref().map(|s| HeldStack {
            item: s.item.to_string(),
            rest: format!("{} {} {:?}", s.count, s.damage, s.extra),
        })) {
            self.attack_ticks = 0;
        }
        self.attack_item = main.as_ref().map(|s| s.item.clone());
        let next = [(self.selected, main), (OFFHAND, self.offhand.clone())];
        // shouldInstantlyReplaceVisibleItem: the same stack, or an equal one
        // (matchesIgnoringComponents; every item plays the swap animation).
        // A stack that stays in its slot with the same item is the same
        // stack, whatever its count (vanilla shrinks and grows it in place).
        let mut changed = [false; 2];
        for h in 0..2 {
            let same = match (&self.visible_hands[h], &next[h]) {
                ((_, None), (_, None)) => true,
                ((slot, Some(a)), (next_slot, Some(b))) => {
                    a.item == b.item
                        && (slot == next_slot || (a.count == b.count && a.extra == b.extra))
                }
                _ => false,
            };
            if same {
                self.visible_hands[h] = next[h].clone();
            }
            changed[h] = !same;
        }
        let speed = self.main_hand_attack_speed();
        for hand in std::mem::take(&mut self.hands_used) {
            self.heights.item_used(hand);
        }
        let replace = self.heights.tick(
            self.hands_busy(),
            self.hand_strength.swap_scale(1.0, speed),
            changed,
        );
        for h in 0..2 {
            if replace[h] {
                self.visible_hands[h] = next[h].clone();
            }
        }
    }
}
impl crate::game::Game<'_> {
    /// Opening a screen stops breaking before the next game tick (which may pause).
    pub(super) fn stop_mining(&mut self) {
        self.attack_clicks = 0;
        self.request_remote_mining_stop();
        self.mining = None;
    }
    /// `LivingEntity.swing` with `getModifiedSwingDuration`.
    pub(super) fn swing_hand(&mut self, hand: Hand, animation: SwingAnimation) {
        let effects = &self.potions.effects;
        let dig_speed = effects
            .amplifier("haste")
            .max(effects.amplifier("conduit_power"));
        let fatigue = effects.amplifier("mining_fatigue");
        let duration = swing_duration(animation, dig_speed, fatigue);
        if self.swing.start_if_able(hand, animation, duration) {
            self.swings_started += 1;
        }
    }
    /// `swing(hand)` for the hand that acts, with the default animation.
    pub(super) fn swing_arm(&mut self) {
        let hand = self.hand;
        self.swing_hand(hand, SwingAnimation::DEFAULT);
    }
    /// `Player.resetAttackStrengthTicker`.
    pub(super) fn reset_attack_strength(&mut self) {
        self.attack_ticks = 0;
        self.hand_strength.reset();
    }
    /// `BlockBehaviour.getDestroyProgress` with `Player.getDestroySpeed`.
    pub(super) fn destroy_progress(&self, pos: [i32; 3]) -> f32 {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let facts = self.res.facts().get(state);
        if facts.hardness == -1.0 {
            return 0.0;
        }
        let held = self.held();
        let mut speed = self.res.items().mining_speed(held, &facts.name);
        speed = self.modify_destroy_speed(speed);
        if let Some(eye) = self.eye_position()
            && self
                .res
                .facts()
                .get(self.world.block(
                    eye[0].floor() as i32,
                    eye[1].floor() as i32,
                    eye[2].floor() as i32,
                ))
                .fluid
                == horizon_client::blocks::Fluid::Water
        {
            speed *= self.submerged_mining_speed();
        }
        if self.player.as_ref().is_some_and(|p| !p.ground.on_ground) {
            speed /= 5.0;
        }
        let harvest = !facts.requires_tool || self.res.items().correct_for_drops(held, &facts.name);
        speed / facts.hardness / if harvest { 30.0 } else { 100.0 }
    }
    /// A key for the main-hand stack (`destroyingItem`): its item and components.
    pub(super) fn main_hand_key(&self) -> String {
        self.held()
            .map_or(String::new(), |s| format!("{} {:?}", s.item, s.extra))
    }
    /// Whether an attack would hit an entity rather than the block behind
    /// it: the crosshair's `EntityHitResult`.
    pub(super) fn entity_targeted(&self) -> bool {
        let before = |d: Option<f64>| d.is_some_and(|d| self.before_block(d));
        before(self.painting_hit().map(|h| h.1))
            || before(self.frame_hit().map(|h| h.1))
            || before(self.armor_stand_hit().map(|h| h.1))
            || before(self.cushion_hit().map(|h| h.1))
            || before(self.boat_hit().map(|h| h.1))
            || before(self.cart_hit().map(|h| h.1))
            || before(self.dragon_hit().map(|h| h.1))
            || before(self.mob_hit().map(|h| h.1))
            || before(self.redirectable_hit().map(|h| h.1))
    }
    /// `LocalPlayer.isHandsBusy`: paddling a boat.
    fn hands_busy(&self) -> bool {
        self.riding
            .is_some_and(|id| self.boats.iter().any(|b| b.id == id))
            && [
                winit::keyboard::KeyCode::KeyW,
                winit::keyboard::KeyCode::KeyS,
                winit::keyboard::KeyCode::KeyA,
                winit::keyboard::KeyCode::KeyD,
            ]
            .iter()
            .any(|k| self.keys.contains(k))
    }
    /// `Minecraft.crosshairPickEntity` when it is a `LivingEntity`: whether
    /// it is alive (the nearest entity under the crosshair, before any block).
    pub(super) fn crosshair_living(&self) -> Option<bool> {
        let nearest = |hits: &[Option<f64>]| hits.iter().flatten().copied().reduce(f64::min);
        let living = nearest(&[
            self.mob_hit().map(|h| h.1),
            self.armor_stand_hit().map(|h| h.1),
        ])?;
        let other = nearest(&[
            self.painting_hit().map(|h| h.1),
            self.frame_hit().map(|h| h.1),
            self.boat_hit().map(|h| h.1),
            self.cart_hit().map(|h| h.1),
            self.dragon_hit().map(|h| h.1),
        ]);
        if !self.before_block(living) || other.is_some_and(|o| o <= living) {
            return None;
        }
        let active = self
            .using
            .and_then(|(slot, _)| self.inventory.get(slot).and_then(Option::as_ref))
            .or_else(|| self.held());
        if let Some(stack) = active {
            let components = self
                .pack
                .item_renderer
                .models
                .components(&horizon_client::item_render::stack_view(stack));
            if let Some(range) = components.get("minecraft:attack_range")
                && !attack_range_contains(range, living, self.creative)
            {
                return None;
            }
        }
        // Only alive mobs are hit, so a picked living entity is alive.
        Some(true)
    }
    /// `Hud.canRenderCrosshairForSpectator` for the nearest hit in reach: an
    /// entity that is a `MenuProvider` (a chest or hopper minecart, a chest
    /// boat), or a block whose `showAsInteractableInSpectatorMode` holds.
    pub(super) fn spectator_crosshair(&self) -> bool {
        let cart = self.cart_hit().map(|(i, d)| {
            let kind = &self.carts[i].kind;
            (d, matches!(kind, CartKind::Chest | CartKind::Hopper))
        });
        let boat = self.boat_hit().map(|(i, d)| (d, self.boats[i].chest));
        let others = [
            self.mob_hit().map(|h| h.1),
            self.armor_stand_hit().map(|h| h.1),
            self.cushion_hit().map(|h| h.1),
            self.painting_hit().map(|h| h.1),
            self.frame_hit().map(|h| h.1),
            self.dragon_hit().map(|h| h.1),
        ];
        let nearest = cart
            .into_iter()
            .chain(boat)
            .chain(others.into_iter().flatten().map(|d| (d, false)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((distance, menu)) = nearest
            && self.before_block(distance)
        {
            return menu;
        }
        let Some(hit) = self.pick() else {
            return false;
        };
        let name = &*self
            .res
            .facts()
            .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]))
            .name;
        let name = name.trim_start_matches("minecraft:");
        match name {
            // BlockBehaviour.getMenuProvider and `instanceof Portal`.
            "crafting_table" | "enchanting_table" | "anvil" | "chipped_anvil" | "damaged_anvil"
            | "stonecutter" | "loom" | "cartography_table" | "grindstone" | "smithing_table"
            | "barrel" | "furnace" | "blast_furnace" | "smoker" | "brewing_stand" | "hopper"
            | "dispenser" | "dropper" | "crafter" | "lectern" | "beacon" | "nether_portal"
            | "end_portal" | "end_gateway" => true,
            // ChestBlock.getMenuProvider: none while a redstone conductor
            // sits on top of either half (a sitting cat aside).
            "chest" | "trapped_chest" => {
                let key = (hit.pos[0], hit.pos[1], hit.pos[2]);
                let blocked = |p: (i32, i32, i32)| {
                    let above = self.world.block(p.0, p.1 + 1, p.2);
                    self.res.behaviours().facts(above).flags
                        & horizon_world::level::data::state_flags::CONDUCTOR
                        != 0
                };
                !blocked(key) && self.chest_other_half(key).is_none_or(|o| !blocked(o))
            }
            _ => name.ends_with("shulker_box"),
        }
    }
    /// `Attributes.ATTACK_SPEED` with the main-hand item's modifier and the
    /// Haste and Mining Fatigue modifiers (`ADD_MULTIPLIED_TOTAL`), clamped to
    /// the attribute's range.
    pub(super) fn main_hand_attack_speed(&self) -> f64 {
        let value = horizon_client::attack_strength::BASE_ATTACK_SPEED
            + self
                .attack_item_info()
                .map_or(0.0, |i| f64::from(i.attack_speed));
        self.potions
            .effects
            .modifiers()
            .into_iter()
            .filter(|m| m.0 == "minecraft:attack_speed")
            .fold(value, |v, m| v * (1.0 + m.2))
            .clamp(0.0, 1024.0)
    }
    /// The item whose attack damage and speed modifiers the player has (`attack_item`).
    pub(super) fn attack_item_info(&self) -> Option<&horizon_client::items::ItemInfo> {
        self.res.items().get(self.attack_item.as_deref()?)
    }
    /// `evaluateWhichHandsToRender`: which of the main and off hand show.
    pub(super) fn hands_to_render(&self) -> [bool; 2] {
        let held = |slot| {
            self.stack_in(slot)
                .map(|s: &ItemStack| (&*s.item, s.data().is_some_and(|d| !d.charged.is_empty())))
        };
        let using = self.used_hand().zip(
            self.using
                .and_then(|(slot, _)| self.stack_in(slot))
                .map(|s| &*s.item),
        );
        horizon_client::first_person::hands_to_render(
            [held(self.selected), held(super::shield::OFFHAND_SLOT)],
            using,
        )
    }
    /// The visible main- and off-hand items.
    pub(super) fn visible_hand_items(&self) -> [Option<ItemStack>; 2] {
        [
            self.visible_hands[0].1.clone(),
            self.visible_hands[1].1.clone(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_range_uses_inclusive_float_bounds() {
        let range = serde_json::json!({
            "min_reach": 2.0, "max_reach": 4.5,
            "min_creative_reach": 2.0, "max_creative_reach": 6.5,
            "hitbox_margin": 0.125,
        });
        for (creative, min, max) in [(false, 1.875, 4.625), (true, 1.875, 6.625)] {
            for distance in [min, max] {
                assert!(attack_range_contains(&range, distance, creative));
            }
            for distance in [min - 0.0001, max + 0.0001] {
                assert!(!attack_range_contains(&range, distance, creative));
            }
        }
        let defaults = serde_json::json!({});
        for (creative, reach) in [(false, 3.0f32), (true, 5.0f32)] {
            let max = f64::from(reach + 0.3f32);
            assert!(attack_range_contains(&defaults, max, creative));
            assert!(!attack_range_contains(&defaults, max + 1e-8, creative));
        }
    }
}
