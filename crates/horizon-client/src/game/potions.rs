//! Potions and status effects in the game: drinking (`PotionItem` via
//! `Consumable`), throwing splash/lingering potions (`ThrownSplashPotion`),
//! filling glass bottles (`BottleItem`), the per-tick effect application,
//! effect attribute modifiers and the HUD effect icons (`Hud.extractEffects`).
use super::{Game, Screen, item_draw::owner_at};
use horizon_assets::item_model::DisplayContext;
use horizon_client::{
    block_entity_render::vertex_light, effects::PlayerEffects, hud::HudBatch, hud_layout,
    hud_overlays, inventory_effects, items::ItemStack, mesher::Vertex,
};
use horizon_core::geometry::Vec3;

/// A thrown splash or lingering potion in flight.
#[derive(Clone, Debug)]
pub(super) struct ThrownPotion {
    /// Its entity id once a server's clients track it (0 until then).
    id: u32,
    stack: ItemStack,
    /// The position before the last tick, for drawing between ticks.
    previous: Vec3,
    position: Vec3,
    velocity: Vec3,
    age: u32,
}
/// Effects, thrown potions and the attribute modifiers last applied.
#[derive(Default)]
pub(super) struct PotionState {
    pub(super) effects: PlayerEffects,
    thrown: Vec<ThrownPotion>,
    /// The area effect clouds a server tracks (game/remote_projectiles.rs).
    pub(super) clouds: Vec<super::remote_projectiles::RemoteCloud>,
    applied: Vec<(&'static str, &'static str, f64, &'static str)>,
    /// Armor the protection table was computed for.
    protection_for: Option<Vec<Option<ItemStack>>>,

    /// Enchantment attribute modifiers on the player: (attribute, id, amount, operation).
    pub(super) enchant_modifiers: Vec<(String, String, f64, String)>,
}

impl PotionState {
    /// Thrown potions in flight as (entity type, position, motion), for the
    /// dispenser parity fixture.
    pub(super) fn thrown_entities(&self) -> Vec<(&'static str, Vec3, Vec3)> {
        self.thrown
            .iter()
            .map(|t| (thrown_kind(&t.stack), t.position, t.velocity))
            .collect()
    }
    pub(super) fn clear_thrown(&mut self) {
        self.thrown.clear();
    }
    /// `handleAddEntity` for a thrown potion or bottle on a server's client:
    /// drawn where the server says (its item comes in its entity data).
    pub(super) fn add_remote_thrown(&mut self, id: u32, kind: &str, at: Vec3, velocity: Vec3) {
        self.thrown.retain(|t| t.id != id);
        let item = match kind {
            "minecraft:lingering_potion" | "minecraft:experience_bottle" => kind,
            _ => "minecraft:splash_potion",
        };
        self.thrown.push(ThrownPotion {
            id,
            stack: ItemStack::new(item, 1),
            previous: at,
            position: at,
            velocity,
            age: 0,
        });
    }
    /// ThrowableItemProjectile.DATA_ITEM_STACK: the potion it shows.
    pub(super) fn remote_thrown_item(&mut self, id: u32, stack: ItemStack) {
        if let Some(t) = self.thrown.iter_mut().find(|t| t.id == id) {
            t.stack = stack;
        }
    }
    /// A server's move: thrown potions have no interpolation handler, so
    /// `moveOrInterpolateTo` puts them there at once. False when `id` isn't one.
    pub(super) fn move_remote_thrown(&mut self, id: u32, at: Vec3) -> bool {
        let Some(t) = self.thrown.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        t.position = at;
        true
    }
    /// `handleSetEntityMotion` for a thrown potion. False when `id` isn't one.
    pub(super) fn remote_thrown_motion(&mut self, id: u32, velocity: Vec3) -> bool {
        let Some(t) = self.thrown.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        t.velocity = velocity;
        true
    }
    pub(super) fn remove_remote_thrown(&mut self, id: u32) {
        self.thrown.retain(|t| t.id != id);
    }
    pub(super) fn has_thrown(&self, id: u32) -> bool {
        self.thrown.iter().any(|t| t.id == id)
    }
}

/// The entity type of a thrown potion or bottle.
fn thrown_kind(stack: &ItemStack) -> &'static str {
    match &*stack.item {
        "minecraft:lingering_potion" => "minecraft:lingering_potion",
        "minecraft:experience_bottle" => "minecraft:experience_bottle",
        _ => "minecraft:splash_potion",
    }
}

impl Game<'_> {
    /// `ThrownItemRenderer` for thrown splash and lingering potions and
    /// experience bottles: the item (GROUND) facing the camera at scale 1,
    /// lit at the light probe (the eye, 0.85 of the 0.25 height).
    pub(super) fn thrown_potion_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        let orientation = glam::Mat4::from_quat(self.camera_orientation());
        for t in &self.potions.thrown {
            let previous = glam::DVec3::new(t.previous.x, t.previous.y, t.previous.z);
            let position = glam::DVec3::new(t.position.x, t.position.y, t.position.z);
            // ThrowableProjectile.shouldRenderAtSqrDistance: hidden for its
            // first two ticks within 3.5 blocks (not in the thrower's face).
            if t.age < 2 && position.distance_squared(camera) < 12.25 {
                continue;
            }
            if !self.standing_entity_in_view(
                camera,
                super::entity_culling::at(t.previous),
                super::entity_culling::at(t.position),
                alpha,
                0.25,
                0.25,
                64.0,
                0.0,
            ) {
                continue;
            }
            let p = previous.lerp(position, alpha);
            let owner = owner_at(p.to_array(), 0.0, &t.stack.item);
            let state = self.resolve_item(&t.stack, DisplayContext::Ground, Some(&owner), 0);
            if state.is_empty() {
                continue;
            }
            let pose = glam::Mat4::from_translation((p - camera).as_vec3()) * orientation;
            let light = self.world.light(
                p.x.floor() as i32,
                (p.y + 0.2125).floor() as i32,
                p.z.floor() as i32,
            );
            self.submit_item_world(&state, pose, vertex_light(light), out);
        }
    }
    /// `Hud.extractEffects`: icons top-right, beneficial row then harmful row;
    /// every screen but the inventories keeps them.
    pub(super) fn draw_effect_icons(&self, batch: &mut HudBatch) {
        if self.potions.effects.list.is_empty() || self.screen_shows_active_effects(batch) {
            return;
        }
        let icons: Vec<hud_overlays::EffectIcon> = self
            .potions
            .effects
            .list
            .iter()
            .map(hud_overlays::EffectIcon::from)
            .collect();
        let draws = hud_overlays::effects(batch.width as i32, false, &icons);
        hud_layout::paint(batch, &self.front.gui.atlas, &draws);
    }
    /// `EffectsInInventory`: panels to the right of survival/creative inventory.
    pub(super) fn draw_inventory_effects(
        &self,
        batch: &mut HudBatch,
        origin: (f32, f32),
        image_width: i32,
    ) {
        let panels = inventory_effects::panels(
            batch.width as i32,
            [origin.0 as i32, origin.1 as i32],
            image_width,
            &self.potions.effects.list,
            self.clock.rate(),
            &|key| self.pack.names.display_get(key),
            &|text| self.front.gui.atlas.font.width(text).ceil() as i32,
        );
        if let Some(lines) = inventory_effects::paint(
            batch,
            &self.front.gui.atlas,
            &panels,
            [self.front.cursor.0 as i32, self.front.cursor.1 as i32],
        ) {
            self.draw_tooltip_lines(batch, &lines, self.front.cursor.0, self.front.cursor.1);
        }
    }
}
impl crate::game::Sim<'_> {
    /// A client's own effects (`LivingEntity.tickEffects` on the client,
    /// `MobEffectInstance.tickClient`): they count down, and the server
    /// removes them. Their modifiers and flags move the predicted player.
    pub(super) fn tick_client_effects(&mut self) {
        for e in &mut self.potions.effects.list {
            e.tick_client();
        }
        self.apply_effect_status();
    }
    /// The effects' attribute modifiers, armor protection and status flags
    /// on `self.player`.
    pub(super) fn apply_effect_status(&mut self) {
        let modifiers = self.potions.effects.modifiers();
        if modifiers != self.potions.applied
            && let Some(p) = self.player.as_mut()
        {
            for (attr, id, _, op) in &self.potions.applied {
                let _ = p.set_modifier(attr, id, 0.0, op, false);
            }
            for (attr, id, amount, op) in &modifiers {
                let _ = p.set_modifier(attr, id, *amount, op, true);
            }
            self.potions.applied = modifiers;
        }
        if let Some(p) = self.player.as_mut()
            && self.potions.effects.amplifier("absorption").is_none()
            && p.health.absorption() > 0.0
        {
            let _ = p.set_health(0.0, true);
        }
        // EnchantmentHelper.getDamageProtection per damage type, recomputed on armor change.
        let armor: Vec<Option<ItemStack>> = self.armor.to_vec();
        if self.potions.protection_for.as_ref() != Some(&armor) {
            let mut table = Vec::new();
            let e = &self.res.stations().enchantments;
            let mut random = horizon_core::random::LegacyRandom::new(0);
            for (_, def) in self.motion.health.damage.entries() {
                let ctx = horizon_client::enchant::EffectContext {
                    items: Some(self.res.items()),
                    damage_type: Some(&def.name),
                    this: Some("minecraft:player"),
                    ..Default::default()
                };
                let total: f32 = armor
                    .iter()
                    .flatten()
                    .map(|s| {
                        e.modify_value(
                            Some(s),
                            "minecraft:damage_protection",
                            &ctx,
                            &mut random,
                            0.0,
                        )
                    })
                    .sum();
                if total > 0.0 {
                    table.push((def.name.clone(), total));
                }
            }
            self.potions.protection_for = Some(armor);
            if let Some(p) = self.player.as_mut() {
                p.status.protection = table;
            }
        }
        let mut status = self.potions.effects.status();
        if let Some(p) = self.player.as_mut() {
            status.protection = std::mem::take(&mut p.status.protection);
            p.status = status;
        }
    }
    /// `ThrowableProjectile.tick`'s `applyGravity` and `applyInertia`, which
    /// come before the move: gravity 0.05 (0.07 for an experience bottle),
    /// then drag 0.99 (0.8 in water).
    fn thrown_potion_motion(&self, t: &mut ThrownPotion) {
        let gravity = if &*t.stack.item == "minecraft:experience_bottle" {
            0.07
        } else {
            0.05
        };
        let at = t.position;
        let water = self
            .res
            .facts()
            .get(self.world.block(
                at.x.floor() as i32,
                at.y.floor() as i32,
                at.z.floor() as i32,
            ))
            .fluid
            == horizon_client::blocks::Fluid::Water;
        let inertia = if water { 0.8 } else { 0.99 };
        t.velocity = Vec3::new(
            t.velocity.x * inertia,
            (t.velocity.y - gravity) * inertia,
            t.velocity.z * inertia,
        );
    }
    /// `ThrowableProjectile.tick` on a server's client: it falls and moves
    /// by itself between the server's updates and stops at a block; only
    /// the server breaks it.
    pub(super) fn step_remote_thrown(&mut self) {
        let mut thrown = std::mem::take(&mut self.potions.thrown);
        for t in &mut thrown {
            t.previous = t.position;
            t.age += 1;
            self.thrown_potion_motion(t);
            let next = Vec3::new(
                t.position.x + t.velocity.x,
                t.position.y + t.velocity.y,
                t.position.z + t.velocity.z,
            );
            let state = self.world.block(
                next.x.floor() as i32,
                next.y.floor() as i32,
                next.z.floor() as i32,
            );
            if self.res.facts().get(state).full_collision {
                t.velocity = Vec3::ZERO;
            } else {
                t.position = next;
            }
        }
        self.potions.thrown = thrown;
    }
    /// `Screen.showsActiveEffects`: the survival and creative inventories
    /// draw their own effect list when `EffectsInInventory.canSeeEffects`
    /// (32 or more pixels right of the screen).
    fn screen_shows_active_effects(&self, batch: &HudBatch) -> bool {
        let (ox, image_width) = match self.screen {
            Screen::Inventory
                if self.container.is_none() && self.craft_size == 2 && !self.mount_active() =>
            {
                (self.inventory_origin(batch).0, 176)
            }
            Screen::Creative => (
                crate::game::Game::creative_origin(batch).0,
                horizon_client::creative::Creative::WIDTH as i32,
            ),
            _ => return false,
        };
        batch.width as i32 - (ox as i32 + image_width + 2) >= 32
    }
    pub(super) fn effects_json(&self) -> serde_json::Value {
        serde_json::json!({
            "active": self.potions.effects.list.iter().map(|e| serde_json::json!({ "id": e.id, "duration": e.duration, "amplifier": e.amplifier })).collect::<Vec<_>>(),
            "thrown": self.potions.thrown.iter().map(|t| [t.position.x, t.position.y, t.position.z]).collect::<Vec<_>>(),
        })
    }
}
