//! Enchantment effects on gameplay, read from the data-driven effect
//! components: efficiency and aqua affinity (`Player.getDestroySpeed`),
//! unbreaking (`minecraft:item_damage`), sharpness/smite/bane and power
//! (`minecraft:damage`), knockback, fire aspect, infinity (`minecraft:ammo_use`),
//! thorns (`minecraft:post_attack`), mending (`minecraft:repair_with_xp`) and
//! the armor attribute effects (respiration, depth strider, ...), Soul
//! Speed's `location_changed` and `tick` effects and the
//! `minecraft:location_changed` disk effect (frost walker).
use super::Game;
use horizon_client::{
    enchant::{EffectContext, level_value},
    items::ItemStack};
use horizon_core::random::RandomSource;


impl Game<'_> {}
impl crate::game::Sim<'_> {
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
    /// `EnchantmentHelper.processDurabilityChange` (unbreaking) for `amount` points.
    pub(super) fn durability_loss(&mut self, stack: &ItemStack, amount: u32) -> u32 {
        let e = &self.res.stations().enchantments;
        if !e.has_component(stack, "minecraft:item_damage") {
            return amount;
        }
        let ctx = EffectContext {
            tool: Some(&stack.item),
            items: Some(self.res.items()),
            ..Default::default()
        };
        e.modify_value(
            Some(stack),
            "minecraft:item_damage",
            &ctx,
            &mut self.world.random,
            amount as f32,
        )
        .max(0.0) as u32
    }
    /// `getEnchantedDamage - base`: damage bonus of the held weapon against a mob type.
    pub(super) fn enchant_damage_bonus(&mut self, target: &str) -> f32 {
        let Some(held) = self.held().cloned() else {
            return 0.0;
        };
        let ctx = EffectContext {
            this: Some(target),
            attacker: Some("minecraft:player"),
            direct_attacker: Some("minecraft:player"),
            is_direct: true,
            damage_type: Some("minecraft:player_attack"),
            items: Some(self.res.items()),
            ..Default::default()
        };
        self.res.stations().enchantments.modify_value(
            Some(&held),
            "minecraft:damage",
            &ctx,
            &mut self.world.random,
            0.0,
        )
    }
    /// Strength / weakness on the attack damage attribute.
    pub(super) fn effect_attack_bonus(&self) -> f32 {
        let fx = &self.potions.effects;
        fx.amplifier("strength")
            .map_or(0.0, |a| 3.0 * (a + 1) as f32)
            - fx.amplifier("weakness")
                .map_or(0.0, |a| 4.0 * (a + 1) as f32)
    }
    /// `Player.attack`'s extra knockback: `getKnockback` (the weapon's knockback
    /// enchantment, halved) plus 0.5 for a sprinting full-strength hit, and
    /// whether the hit is that knockback attack.
    pub(super) fn extra_knockback(&mut self, target_kind: &str, scale: f32) -> (f32, bool) {
        let knockback_attack = scale > 0.9 && self.player.as_ref().is_some_and(|p| p.sprinting);
        let enchanted = match self.held().cloned() {
            Some(held) => {
                let ctx = EffectContext {
                    this: Some(target_kind),
                    attacker: Some("minecraft:player"),
                    direct_attacker: Some("minecraft:player"),
                    is_direct: true,
                    items: Some(self.res.items()),
                    ..Default::default()
                };
                self.res.stations().enchantments.modify_value(
                    Some(&held),
                    "minecraft:knockback",
                    &ctx,
                    &mut self.world.random,
                    0.0,
                )
            }
            None => 0.0,
        };
        let bonus = if knockback_attack { 0.5 } else { 0.0 };
        (enchanted / 2.0 + bonus, knockback_attack)
    }
    /// `causeExtraKnockback`'s direction, `(sin yRot, -cos yRot)` from Mth's
    /// sine table: the target flies where the attacker looks, not away from
    /// it (straight along an axis when the attacker faces one).
    pub(super) fn extra_knockback_direction(&self) -> (f64, f64) {
        use horizon_core::geometry::trig;
        let yaw = f64::from(self.yaw * trig::DEG_TO_RAD);
        (f64::from(trig::sin(yaw)), -f64::from(trig::cos(yaw)))
    }
    /// The attacker's side of `causeExtraKnockback` as the attacking client
    /// sees it: the server's player stops sprinting, and its flag reaches the
    /// client. (The server's 0.6 speed cut never does: the client's own
    /// `Player.attack` simulates no hurt, so it skips the knockback.)
    pub(super) fn stop_sprint_after_extra_knockback(&mut self) {
        if let Some(p) = self.player.as_mut() {
            let _ = p.set_sprinting(false);
        }
    }
    /// After a successful melee hit: fire aspect and wind burst.
    pub(super) fn post_attack(&mut self, mob: usize) {
        let Some(held) = self.held().cloned() else {
            return;
        };
        let Some(m) = self.mobs.get(mob) else { return };
        let kind = m.kind.id.clone();
        let ctx = EffectContext {
            this: Some(&kind),
            attacker: Some("minecraft:player"),
            direct_attacker: Some("minecraft:player"),
            is_direct: true,
            items: Some(self.res.items()),
            ..Default::default()
        };
        // minecraft:post_attack effects of the attacker's weapon on the victim:
        // ignite (fire aspect) and apply_mob_effect (bane of arthropods).
        let mut ignite = 0.0f32;
        let mut mob_effects = Vec::new();
        for (id, level) in held.enchantments() {
            let Some(def) = self.res.stations().enchantments.get(id) else {
                continue;
            };
            for effect in def.effects["minecraft:post_attack"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let e = &effect["effect"];
                let kind_of = e["type"].as_str();
                if effect["enchanted"] != "attacker"
                    || effect["affected"] != "victim"
                    || !matches!(
                        kind_of,
                        Some("minecraft:ignite" | "minecraft:apply_mob_effect")
                    )
                {
                    continue;
                }
                let mut random = self.world.random.clone();
                let ok = effect.get("requirements").is_none_or(|r| {
                    self.res
                        .stations()
                        .enchantments
                        .condition(r, *level, &ctx, &mut random)
                });
                self.world.random = random;
                if !ok {
                    continue;
                }
                if kind_of == Some("minecraft:ignite") {
                    ignite =
                        ignite.max(horizon_client::enchant::level_value(&e["duration"], *level));
                } else {
                    mob_effects.push((e.clone(), *level));
                }
            }
        }
        if ignite > 0.0
            && let Some(m) = self.mobs.get_mut(mob)
            && !m.kind.flags.fire_immune
        {
            m.ignite_for_ticks((ignite * 20.0) as i32);
        }
        for (effect, level) in mob_effects {
            self.apply_mob_effect(mob, &effect, level);
        }
        self.post_attack_bursts(&held, &kind);
    }
    /// `EnchantmentHelper.doPostPiercingAttackEffects`: the
    /// `minecraft:post_piercing_attack` effects of the weapon in hand after a
    /// jab (Lunge's dash).
    pub(super) fn post_piercing_attack(&mut self) {
        let Some(held) = self.held().cloned() else {
            return;
        };
        let mut effects = Vec::new();
        for (id, level) in held.enchantments() {
            let Some(def) = self.res.stations().enchantments.get(id) else {
                continue;
            };
            for entry in def.effects["minecraft:post_piercing_attack"]
                .as_array()
                .into_iter()
                .flatten()
            {
                effects.push((*level, entry["effect"].clone()));
            }
        }
        // Lunge's requirements: not riding, gliding or in water, and a
        // player outside creative with at least 7 food.
        let Some(p) = self.player.as_ref() else {
            return;
        };
        if effects.is_empty()
            || self.riding.is_some()
            || p.fall_flying
            || p.fluids.was_touching_water
            || !(self.creative || p.health.food.food >= 7)
        {
            return;
        }
        for (level, effect) in effects {
            self.apply_piercing_effect(&effect, level);
        }
    }
    /// One `post_piercing_attack` effect on the player: `AllOf`,
    /// `ChangeItemDamage`, `ApplyExhaustion`, `ApplyEntityImpulse` and
    /// `PlaySoundEffect`.
    fn apply_piercing_effect(&mut self, effect: &serde_json::Value, level: u32) {
        let amount = || level_value(&effect["amount"], level);
        match effect["type"].as_str() {
            Some("minecraft:all_of") => {
                for e in effect["effects"].as_array().into_iter().flatten() {
                    self.apply_piercing_effect(e, level);
                }
            }
            Some("minecraft:change_item_damage") => self.damage_held(amount() as u32),
            Some("minecraft:apply_exhaustion") => {
                if let Some(p) = self.player.as_mut() {
                    p.cause_food_exhaustion(amount());
                }
            }
            // The direction turned by the look (`Entity.getLookQuaternion`:
            // pitch about x, then yaw about y), scaled by coordinate_scale
            // and the magnitude, added to the velocity (the guest's client
            // is told of it by `guest_hit_by`).
            Some("minecraft:apply_impulse") => {
                let vec3 = |key: &str| [0, 1, 2].map(|i| effect[key][i].as_f64().unwrap_or(0.0));
                let (d, scale) = (vec3("direction"), vec3("coordinate_scale"));
                let magnitude = f64::from(level_value(&effect["magnitude"], level));
                let (yaw, pitch) = (
                    f64::from(self.yaw).to_radians(),
                    f64::from(self.pitch).to_radians(),
                );
                let (y1, z1) = (
                    d[1] * pitch.cos() - d[2] * pitch.sin(),
                    d[1] * pitch.sin() + d[2] * pitch.cos(),
                );
                let impulse = horizon_core::geometry::Vec3::new(
                    (d[0] * yaw.cos() - z1 * yaw.sin()) * scale[0] * magnitude,
                    y1 * scale[1] * magnitude,
                    (d[0] * yaw.sin() + z1 * yaw.cos()) * scale[2] * magnitude,
                );
                if let Some(p) = self.player.as_mut() {
                    p.velocity = p.velocity.add_vector(impulse);
                }
            }
            // The sound for the level (a list picks by level), heard by all.
            Some("minecraft:play_sound") => {
                let sounds: Vec<&str> = match &effect["sound"] {
                    serde_json::Value::Array(list) => {
                        list.iter().filter_map(|s| s.as_str()).collect()
                    }
                    one => one.as_str().into_iter().collect(),
                };
                let index = (level as usize).saturating_sub(1);
                if let Some(sound) = sounds.get(index.min(sounds.len().saturating_sub(1))) {
                    let float = |key: &str| effect[key].as_f64().unwrap_or(1.0) as f32;
                    let at = self
                        .player
                        .as_ref()
                        .map(|p| [p.position.x, p.position.y, p.position.z]);
                    self.sounds.play(sound, at, float("volume"), float("pitch"));
                }
            }
            _ => {}
        }
    }
    /// `ApplyMobEffect.apply`: one of `to_apply` (a single id or a list) for a
    /// random duration and amplifier between the level-based bounds.
    fn apply_mob_effect(&mut self, mob: usize, effect: &serde_json::Value, level: u32) {
        use horizon_client::enchant::level_value;
        let Some(m) = self.mobs.get_mut(mob) else {
            return;
        };
        let ids: Vec<&str> = match &effect["to_apply"] {
            serde_json::Value::String(id) => vec![id.as_str()],
            serde_json::Value::Array(list) => list.iter().filter_map(|v| v.as_str()).collect(),
            _ => Vec::new(),
        };
        if ids.is_empty() {
            return;
        }
        // HolderSet.getRandomElement, then Mth.randomBetween for both values.
        let pick = m.random.next_int_bound(ids.len() as i32).unwrap_or(0) as usize;
        let mut between = |min: &str, max: &str| {
            let (lo, hi) = (
                level_value(&effect[min], level),
                level_value(&effect[max], level),
            );
            m.random.next_float() * (hi - lo) + lo
        };
        let ticks = (between("min_duration", "max_duration") * 20.0).round() as i32;
        let amplifier = (between("min_amplifier", "max_amplifier").round() as i32).max(0);
        if let Some(id) = super::potions::mob_effect_id(ids[pick]) {
            m.add_effect(id, ticks, amplifier);
        }
    }
    /// `minecraft:post_attack` explode effects on the attacker (wind burst):
    /// a burst at the player's feet that only knocks back and triggers
    /// blocks (`ExplodeEffect` with `block_interaction: trigger`).
    fn post_attack_bursts(&mut self, held: &ItemStack, victim: &str) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        // EntityFlagsPredicate.isFlying: gliding or flying.
        let motion = (p.fall_distance, p.fall_flying || p.flying);
        let feet = [p.position.x, p.position.y, p.position.z];
        let ctx = EffectContext {
            this: Some(victim),
            attacker: Some("minecraft:player"),
            direct_attacker: Some("minecraft:player"),
            is_direct: true,
            items: Some(self.res.items()),
            direct_motion: Some(motion),
            ..Default::default()
        };
        let mut bursts = Vec::new();
        for (id, level) in held.enchantments() {
            let Some(def) = self.res.stations().enchantments.get(id) else {
                continue;
            };
            for effect in def.effects["minecraft:post_attack"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let e = &effect["effect"];
                if effect["enchanted"] != "attacker"
                    || effect["affected"] != "attacker"
                    || e["type"] != "minecraft:explode"
                    || e["block_interaction"] != "trigger"
                {
                    continue;
                }
                let ok = effect.get("requirements").is_none_or(|r| {
                    self.res.stations().enchantments.condition(
                        r,
                        *level,
                        &ctx,
                        &mut self.world.random,
                    )
                });
                if !ok {
                    continue;
                }
                let radius = horizon_client::enchant::level_value(&e["radius"], *level).max(0.0);
                // ExplosionDamageCalculator.getKnockbackMultiplier: 1 unless given.
                let knockback = if e["knockback_multiplier"].is_null() {
                    1.0
                } else {
                    horizon_client::enchant::level_value(&e["knockback_multiplier"], *level)
                };
                bursts.push((radius, knockback));
            }
        }
        for (radius, knockback) in bursts {
            let _ = self.wind_burst(feet, radius, knockback, false, false);
        }
    }
    /// Thorns on worn armor when the mob `attacker` hits the player: the
    /// `affected: attacker` effect of `Enchantment.doPostAttack` hurts the
    /// entity that caused the hit.
    pub(super) fn thorns(&mut self, attacker: u32) {
        let Some(index) = self.mobs.iter().position(|m| m.id == attacker && m.alive()) else {
            return;
        };
        for damage in self.thorns_damage() {
            let m = &mut self.mobs[index];
            m.hurt(damage, None, &mut self.world.random);
            let center = [m.position.x, m.position.y, m.position.z];
            self.sounds
                .play("minecraft:enchant.thorns.hit", Some(center), 1.0, 1.0);
        }
    }
    /// The victim's side of thorns: for each worn piece whose chance comes up,
    /// the damage to deal the attacker (`DamageEntity` min 1, max 5) and the
    /// piece's wear (`ChangeItemDamage` 2).
    pub(super) fn thorns_damage(&mut self) -> Vec<f32> {
        let mut hits = Vec::new();
        for slot in 0..4 {
            let Some(stack) = self.armor[slot].clone() else {
                continue;
            };
            let level = stack.enchantment_level("minecraft:thorns");
            if level == 0 {
                continue;
            }
            let Some(def) = self.res.stations().enchantments.get("minecraft:thorns") else {
                continue;
            };
            let effect = def.effects["minecraft:post_attack"][0].clone();
            let ctx = EffectContext {
                items: Some(self.res.items()),
                ..Default::default()
            };
            let mut random = self.world.random.clone();
            let ok = self.res.stations().enchantments.condition(
                &effect["requirements"],
                level,
                &ctx,
                &mut random,
            );
            self.world.random = random;
            if !ok {
                continue;
            }
            hits.push(1.0 + self.world.random.next_float() * 4.0);
            // ChangeItemDamage -> ItemStack.hurtAndBreak: unbreaking applies.
            let loss = self.durability_loss(&stack, 2);
            if let Some(s) = self.armor[slot].as_mut() {
                s.damage += loss;
                if self
                    .res
                    .items()
                    .get(&s.item)
                    .is_some_and(|i| s.damage >= i.max_damage)
                {
                    self.armor[slot] = None;
                }
            }
        }
        hits
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


