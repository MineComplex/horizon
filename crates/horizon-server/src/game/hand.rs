//! The local player's hand: clicks and held attack through
//! `crate::hc::first_person` (`Minecraft.startAttack` /
//! `continueAttack`, `MultiPlayerGameMode` block breaking), the arm swing
//! (`LivingEntity.SwingState`) and the first-person equip heights
//! (`FirstPersonHandsAndItems`) with the item swap ticker they read.
use super::minecarts::CartKind;
use crate::hc::{Result, attack_strength::HeldStack, first_person::{Hand, HandWorld, Hit, SwingAnimation, UseResult, swing_duration}, items::ItemStack};

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

impl crate::game::Sim<'_> {
    /// Opening a screen stops breaking before the next game tick (which may pause).
    pub(super) fn stop_mining(&mut self) {
        let mut destroying = std::mem::take(&mut self.clicks.destroying);
        destroying.stop(&mut GameHands {
            game: self,
            error: None,
        });
        self.clicks.destroying = destroying;
        self.attack_clicks = 0;
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
                == crate::hc::blocks::Fluid::Water
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
            || before(self.guest_hit().map(|h| h.1))
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
        let value = crate::hc::attack_strength::BASE_ATTACK_SPEED
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
    pub(super) fn attack_item_info(&self) -> Option<&crate::hc::items::ItemInfo> {
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
        crate::hc::first_person::hands_to_render(
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

/// The game as `first_person`'s world: the crosshair, breaking and attacking.
struct GameHands<'g, 'a> {
    game: &'g mut crate::game::Sim<'a>,
    error: Option<Box<dyn std::error::Error>>,
}
impl GameHands<'_, '_> {
    /// The main-hand item's `kinetic_weapon` and `piercing_weapon`, a spear's.
    fn attack_kinetic(&self) -> Option<crate::hc::spear::Kinetic> {
        self.game
            .held()
            .and_then(|s| crate::hc::spear::Kinetic::of(&s.item))
    }
    fn keep(&mut self, r: Result<()>) {
        if let Err(e) = r
            && self.error.is_none()
        {
            self.error = Some(e);
        }
    }
}
impl HandWorld for GameHands<'_, '_> {
    fn hit(&mut self) -> Hit {
        if self.game.entity_targeted() {
            return Hit::Entity;
        }
        match self.game.pick() {
            Some(h) => Hit::Block {
                pos: h.pos,
                face: match h.face {
                    [0, -1, 0] => 0,
                    [0, 1, 0] => 1,
                    [0, 0, -1] => 2,
                    [0, 0, 1] => 3,
                    [-1, 0, 0] => 4,
                    _ => 5,
                },
            },
            None => Hit::Miss,
        }
    }
    fn is_air(&mut self, pos: [i32; 3]) -> bool {
        let state = self.game.world.block(pos[0], pos[1], pos[2]);
        self.game.res.facts().get(state).name.ends_with("air")
    }
    fn creative(&self) -> bool {
        self.game.creative
    }
    fn spectator(&self) -> bool {
        self.game.spectator()
    }
    fn instabuild(&self) -> bool {
        self.game.creative
    }
    fn hands_busy(&self) -> bool {
        self.game.hands_busy()
    }
    fn using_item(&self) -> bool {
        self.game.using.is_some()
    }
    fn item_enabled(&mut self, _: Hand) -> bool {
        true
    }
    fn held_empty(&mut self, hand: Hand) -> bool {
        match hand {
            Hand::Main => self.game.held().is_none(),
            Hand::Off => self.game.offhand.is_none(),
        }
    }
    fn held_count(&mut self, hand: Hand) -> i32 {
        let stack = match hand {
            Hand::Main => self.game.held(),
            Hand::Off => self.game.offhand.as_ref(),
        };
        stack.map_or(0, |s| s.count as i32)
    }
    fn infinite_materials(&self) -> bool {
        self.game.creative
    }
    fn cannot_attack(&mut self) -> bool {
        let ticker = self.game.hand_strength.ticker;
        self.game.cannot_attack_with_item(ticker, 0)
    }
    fn piercing_weapon(&mut self) -> bool {
        self.attack_kinetic().is_some()
    }
    fn attack_animation(&mut self) -> SwingAnimation {
        self.attack_kinetic()
            .map_or(SwingAnimation::DEFAULT, |k| k.attack_animation())
    }
    fn interact_animation(&mut self, _: Hand) -> SwingAnimation {
        SwingAnimation::DEFAULT
    }
    fn piercing_attack(&mut self, _: SwingAnimation) {
        // The server's part (`PiercingWeapon.attack`), then the client's:
        // the sound `MultiPlayerGameMode.piercingAttack` plays.
        if let Some(sound) = self.game.jab() {
            self.game.play_at_feet(sound);
        }
        self.game.hand_strength.reset_attack();
    }
    fn attack_entity(&mut self) -> bool {
        self.game.attack()
    }
    fn block_action_restricted(&mut self, _: [i32; 3]) -> bool {
        !self.game.may_build()
    }
    fn within_border(&mut self, _: [i32; 3]) -> bool {
        true
    }
    fn attack_block(&mut self, pos: [i32; 3]) {
        let result = self.game.attack_block(pos);
        self.keep(result);
    }
    fn destroy_progress(&mut self, pos: [i32; 3]) -> f32 {
        self.game.destroy_progress(pos)
    }
    fn destroy_block(&mut self, pos: [i32; 3]) -> bool {
        if self.is_air(pos) {
            return false;
        }
        // Item.canDestroyBlock: swords, tridents and maces cannot break
        // blocks in creative.
        let held = self.game.held().map(|s| s.item.to_string());
        if self.game.creative
            && held.is_some_and(|i| {
                i.ends_with("_sword") || i == "minecraft:trident" || i == "minecraft:mace"
            })
        {
            return false;
        }
        let r = self.game.break_block(pos);
        self.keep(r);
        true
    }
    fn main_hand_key(&mut self) -> String {
        self.game.main_hand_key()
    }
    fn destroy_stage(&mut self, _: [i32; 3], _: Option<i32>) {}
    fn swing(&mut self, hand: Hand, animation: SwingAnimation) {
        self.game.swing_hand(hand, animation);
    }
    fn reset_attack_strength(&mut self) {
        self.game.reset_attack_strength();
    }
    fn entity_within_border(&mut self) -> bool {
        true
    }
    fn use_on_entity(&mut self, _: Hand) -> Option<UseResult> {
        None
    }
    fn use_on_block(&mut self, _: Hand) -> UseResult {
        UseResult::Pass
    }
    fn use_item(&mut self, _: Hand) -> UseResult {
        UseResult::Pass
    }
    fn item_used(&mut self, hand: Hand) {
        self.game.hands_used.push(hand);
    }
    fn release_using(&mut self) {}
}
