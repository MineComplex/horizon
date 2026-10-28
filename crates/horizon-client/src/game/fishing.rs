//! Fishing: `FishingRodItem.use` (cast / reel in), the `FishingHook` (flight,
//! bobbing, lure and bite timers, open water, hooking mobs, retrieving the
//! `gameplay/fishing` loot) and `FishingHookRenderer` (bobber and line).
use super::{Game, view_vector};
use horizon_client::advancements::EntityView;
use horizon_client::{
    gizmos::Line,
    items::ItemStack,
    mesher::Vertex,
    
    pose::Pose};
use horizon_core::{
    geometry::{ Vec3, trig},
    random::RandomSource};
use horizon_protocol::entity_data::Value;
use std::sync::Arc;


#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Flying,
    Bobbing,
    Hooked(u32),
}
pub(super) struct Hook {
    /// Its entity id, which a server's players track it under.
    id: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
    state: State,
    on_ground: bool,
    nibble: i32,
    time_until_lured: i32,
    time_until_hooked: i32,
    open_water: bool,
    luck: i32,
}
/// A player a hook can catch (`canHitEntity`): their entity id, which
/// guest they are (none: the host) and their box.
pub(super) struct HookTarget {
    id: u32,
}
const SIZE: f64 = 0.25;
/// RandomSource.triangle(mode, deviation).
fn triangle(r: &mut impl RandomSource, mode: f64, deviation: f64) -> f64 {
    mode + deviation * (f64::from(r.next_float()) - f64::from(r.next_float()))
}
impl Game<'_> {
    /// FishingHookRenderer: the bobbers (this player's and the others' a
    /// server tracks), camera-facing half-block quads centred on the hooks.
    pub(super) fn fishing_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let hooks: Vec<glam::DVec3> = self
            .hook_position(alpha)
            .into_iter()
            .chain(self.remote_hook_ends(alpha).into_iter().map(|(h, _)| h))
            .collect();
        if hooks.is_empty() {
            return;
        }
        let Some(r) = self
            .front
            .gui
            .atlas
            .sprites
            .get("entity/fishing/fishing_hook")
            .copied()
        else {
            return;
        };
        let forward = glam::DVec3::from_array(view_vector(self.yaw, self.pitch));
        let right = forward.cross(glam::DVec3::Y).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let s = 0.25;
        let uv = [[r.u0, r.v1], [r.u1, r.v1], [r.u1, r.v0], [r.u0, r.v0]];
        for p in hooks {
            let light =
                self.world
                    .light(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let corners = [
                p - right * s - up * s,
                p + right * s - up * s,
                p + right * s + up * s,
                p - right * s + up * s,
            ];
            for (v, uv) in corners.iter().zip(uv) {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: (*v - camera).as_vec3().to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: packed,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
    }
}
impl crate::game::Sim<'_> {
    /// `FishingRodItem.use`.
    pub(super) fn use_fishing_rod(&mut self, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:fishing_rod" {
            return false;
        }
        let Some(p) = self.player.as_ref() else {
            return false;
        };
        let at = [p.position.x, p.position.y, p.position.z];
        let pitch = 0.4 / (self.world.random.next_float() * 0.4 + 0.8);
        if self.hook.is_some() {
            let damage = self.retrieve_hook(stack);
            self.damage_held(damage);
            self.sounds.play(
                "minecraft:entity.fishing_bobber.retrieve",
                Some(at),
                1.0,
                pitch,
            );
        } else {
            self.sounds.play(
                "minecraft:entity.fishing_bobber.throw",
                Some(at),
                0.5,
                pitch,
            );
            // FishingHook(player, level, luck, lureSpeed)
            let lure = stack.enchantment_level("minecraft:lure") as i32 * 5 * 20;
            let luck = stack.enchantment_level("minecraft:luck_of_the_sea") as i32;
            let (yaw, pitch) = (self.yaw, self.pitch);
            let y_cos = (-yaw.to_radians() - std::f32::consts::PI).cos();
            let y_sin = (-yaw.to_radians() - std::f32::consts::PI).sin();
            let x_cos = -(-pitch.to_radians()).cos();
            let x_sin = (-pitch.to_radians()).sin();
            let eye_y = p.position.y + f64::from(p.dimensions.eye_height);
            let start = Vec3::new(
                p.position.x - f64::from(y_sin) * 0.3,
                eye_y,
                p.position.z - f64::from(y_cos) * 0.3,
            );
            let mut v = Vec3::new(
                f64::from(-y_sin),
                f64::from((-(x_sin / x_cos)).clamp(-5.0, 5.0)),
                f64::from(-y_cos),
            );
            let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
            let r = &mut self.world.random;
            v = Vec3::new(
                v.x * (0.6 / len + triangle(r, 0.5, 0.0103365)),
                v.y * (0.6 / len + triangle(r, 0.5, 0.0103365)),
                v.z * (0.6 / len + triangle(r, 0.5, 0.0103365)),
            );
            // A spawned hook's start position is its feet: centre the box on the eye.
            let start = Vec3::new(start.x, start.y - SIZE / 2.0, start.z);
            self.next_entity += 1;
            self.hook = Some(Hook {
                id: self.next_entity,
                position: start,
                previous: start,
                state: State::Flying,
                on_ground: false,
                nibble: 0,
                time_until_lured: 0,
                time_until_hooked: 0,
                open_water: true,
                luck,
            });
        }
        self.swing_arm();
        true
    }
    /// `FishingHook.retrieve`: the rod damage it costs.
    fn retrieve_hook(&mut self, rod: &ItemStack) -> u32 {
        let Some(hook) = self.hook.take() else {
            return 0;
        };
        let Some(p) = self.player.as_ref() else {
            return 0;
        };
        let owner = p.position;
        let mut damage = 0;
        if let State::Hooked(id) = hook.state {
            // pullEntity: 0.1 of the way to the owner.
            let d = Vec3::new(
                owner.x - hook.position.x,
                owner.y - hook.position.y,
                owner.z - hook.position.z,
            );
            let pulled = |v: Vec3| Vec3::new(v.x + d.x * 0.1, v.y + d.y * 0.1, v.z + d.z * 0.1);
            let mut hooked_item = false;
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                m.velocity = pulled(m.velocity);
            } else if let Some(item) = self.dropped.iter_mut().find(|i| i.id == id) {
                item.velocity = pulled(item.velocity);
                hooked_item = true;
            } else if self.creatures.hook_targets.iter().any(|t| t.id == id) {
                // A player: the hub moves them when the tick ends.
                self.creatures
                    .hook_pulls
                    .push((id, Vec3::new(d.x * 0.1, d.y * 0.1, d.z * 0.1)));
            }
            // CriteriaTriggers.FISHING_ROD_HOOKED with no items.
            let hooked = self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map(|m| self.mob_entity(m));
            if let Some(hooked) = hooked {
                self.trigger_fishing_rod_hooked(rod, &hooked, &[]);
            }
            // `hookedIn instanceof ItemEntity ? 3 : 5`.
            damage = if hooked_item { 3 } else { 5 };
        } else if hook.nibble > 0 {
            let biome = self.biome_name_at(hook.position);
            let Some(loot) = horizon_client::structure_blocks::loot_data(&self.options.resources)
            else {
                return damage;
            };
            // `withLuck(this.luck + owner.getLuck())`: the rod's Luck of the Sea
            // plus the owner's luck attribute (the Luck and Bad Luck effects).
            let owner_luck = self
                .player
                .as_mut()
                .and_then(|p| p.attribute("minecraft:luck").ok())
                .unwrap_or(0.0) as f32;
            let mut random = self.world.random.clone();
            let items = loot.roll(
                "minecraft:gameplay/fishing",
                &mut random,
                hook.luck as f32 + owner_luck,
                biome.as_deref(),
                Some(hook.open_water),
            );
            self.world.random = random;
            let mut caught = Vec::new();
            for stack in items {
                // FishingHook.retrieve: fish_caught for each catch in #minecraft:fishes.
                if self
                    .res
                    .items()
                    .item_tags
                    .get("minecraft:fishes")
                    .is_some_and(|t| t.contains(stack.item.as_str()))
                {
                    self.stats.custom("fish_caught", 1);
                }
                let mut item = ItemStack {
                    item: Arc::from(stack.item.as_str()),
                    count: stack.count.max(1) as u32,
                    damage: stack.damage.max(0) as u32,
                    extra: None,
                };
                for (id, level) in &stack.enchantments {
                    if &*item.item == "minecraft:enchanted_book" {
                        horizon_client::item_data::set_level(
                            &mut item.data_mut().stored_enchantments,
                            id,
                            (*level).max(0) as u32,
                        );
                    } else {
                        item.enchant(id, (*level).max(0) as u32);
                    }
                }
                if let Some(potion) = &stack.potion {
                    item = item.with_potion(potion);
                }
                caught.push(item.clone());
                let (dx, dy, dz) = (
                    owner.x - hook.position.x,
                    owner.y - hook.position.y,
                    owner.z - hook.position.z,
                );
                let v = [
                    dx * 0.1,
                    dy * 0.1 + (dx * dx + dy * dy + dz * dz).sqrt().sqrt() * 0.08,
                    dz * 0.1,
                ];
                self.spawn_item(
                    item,
                    [hook.position.x, hook.position.y, hook.position.z],
                    v,
                    0,
                );
                let xp = self.world.random.next_int_bound(6).unwrap_or(0) as u32 + 1;
                self.spawn_orbs([owner.x, owner.y + 0.5, owner.z + 0.5], xp);
            }
            // CriteriaTriggers.FISHING_ROD_HOOKED: the hook itself is the entity.
            let bobber = EntityView {
                kind: "minecraft:fishing_bobber".into(),
                position: [hook.position.x, hook.position.y, hook.position.z],
                on_ground: hook.on_ground,
                fishing_open_water: Some(hook.open_water),
                ..Default::default()
            };
            self.trigger_fishing_rod_hooked(rod, &bobber, &caught);
            damage = 1;
        }
        if hook.on_ground {
            damage = 2;
        }
        damage
    }
    pub(super) fn biome_name_at(&self, p: Vec3) -> Option<String> {
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let c = self.world.chunks.get(&(x >> 4, z >> 4))?;
        Some(
            self.res.biomes().list[c.biome(x, y, z) as usize]
                .name
                .clone(),
        )
    }
    /// `FishingRodCast`: the stack in the arm holding the rod
    /// (`FishingHookRenderer.getHoldingArm`) while the hook is out.
    pub(super) fn cast_rod(&self) -> Option<&ItemStack> {
        self.hook.as_ref()?;
        if self.main_hand_rod() {
            self.held()
        } else {
            self.offhand.as_ref()
        }
    }
    pub(super) fn main_hand_rod(&self) -> bool {
        self.held()
            .is_some_and(|s| &*s.item == "minecraft:fishing_rod")
    }
    /// The hook's position this frame.
    fn hook_position(&self, alpha: f64) -> Option<glam::DVec3> {
        let h = self.hook.as_ref()?;
        let lerp = |a: f64, b: f64| a + (b - a) * alpha;
        Some(glam::DVec3::new(
            lerp(h.previous.x, h.position.x),
            lerp(h.previous.y, h.position.y),
            lerp(h.previous.z, h.position.z),
        ))
    }
    /// FishingHookRenderer: each line, 16 black `lines` segments sagging
    /// from a quarter block above the hook to the rod in its owner's hand,
    /// `width` pixels wide (`Window.getAppropriateLineWidth`).
    pub(super) fn fishing_line(&self, alpha: f64, size: [u32; 2]) -> Vec<Line> {
        let own = self
            .hook_position(alpha)
            .zip(self.rod_hand_position(alpha, size));
        let width = horizon_client::gfx::outline_width(size[0]);
        let mut lines = Vec::new();
        for (hook, hand) in own.into_iter().chain(self.remote_hook_ends(alpha)) {
            let offset = (hand - (hook + glam::DVec3::new(0.0, 0.25, 0.0))).as_vec3();
            let point = |a: f32| {
                let y = offset.y * (a * a + a) * 0.5 + 0.25;
                hook + glam::Vec3::new(offset.x * a, y, offset.z * a).as_dvec3()
            };
            lines.extend((0..16).map(|i| Line {
                start: point(i as f32 / 16.0).to_array(),
                end: point((i + 1) as f32 / 16.0).to_array(),
                color: 0xFF00_0000,
                width,
            }));
        }
        lines
    }
    /// `FishingHookRenderer.getPlayerHandPos`: on the near plane in first
    /// person, beside the body otherwise.
    fn rod_hand_position(&self, alpha: f64, size: [u32; 2]) -> Option<glam::DVec3> {
        let p = self.player.as_ref()?;
        let right_arm = self.main_hand_rod() != self.settings.main_hand_left;
        let invert: f32 = if right_arm { 1.0 } else { -1.0 };
        let swing = trig::sin(f64::from(
            self.swing.animation(alpha as f32).sqrt() * std::f32::consts::PI,
        ));
        let feet = self.previous_position.lerp(p.position, alpha);
        let eye = glam::DVec3::new(feet.x, feet.y + f64::from(p.dimensions.eye_height), feet.z);
        if self.camera_type.first_person() {
            let fov = self.settings.fov as i32 as f32;
            // Camera.getNearPlane(fov).getPointOnPlane(invert * 0.525, -0.1).
            let aspect = f64::from(size[0]) / f64::from(size[1].max(1));
            let height =
                (f64::from(fov * (std::f64::consts::PI / 180.0) as f32) / 2.0).tan() * 0.05;
            let forward = glam::DVec3::from_array(view_vector(self.yaw, self.pitch));
            let left = -forward.cross(glam::DVec3::Y).normalize_or_zero();
            let up = forward.cross(left).normalize_or_zero();
            let point = forward * 0.05 + up * (height * -0.1)
                - left * (height * aspect * f64::from(invert * 0.525));
            let point = point * (960.0 / f64::from(fov));
            // Vec3.yRot(swing * 0.5), then xRot(-swing * 0.7).
            let (s, c) = (
                f64::from(trig::sin(f64::from(swing * 0.5))),
                f64::from(trig::cos(f64::from(swing * 0.5))),
            );
            let point = glam::DVec3::new(
                point.x * c + point.z * s,
                point.y,
                point.z * c - point.x * s,
            );
            let (s, c) = (
                f64::from(trig::sin(f64::from(-swing * 0.7))),
                f64::from(trig::cos(f64::from(-swing * 0.7))),
            );
            let point = glam::DVec3::new(
                point.x,
                point.y * c + point.z * s,
                point.z * c - point.y * s,
            );
            Some(eye + point)
        } else {
            let model = self.player_model.as_ref()?;
            let body =
                horizon_client::camera::lerp(alpha as f32, model.previous_body_yaw, model.body_yaw);
            let angle = f64::from(body * (std::f64::consts::PI / 180.0) as f32);
            let (sin, cos) = (f64::from(trig::sin(angle)), f64::from(trig::cos(angle)));
            let right = f64::from(invert) * 0.35;
            let crouch = if p.pose == Pose::Crouching {
                -0.1875
            } else {
                0.0
            };
            Some(
                eye + glam::DVec3::new(
                    -cos * right - sin * 0.8,
                    crouch - 0.45,
                    -sin * right + cos * 0.8,
                ),
            )
        }
    }
}
impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// The hooks drawn here, with their owners' ids (0 for this player's own).
    pub(super) fn hooks_status(&self) -> Vec<serde_json::Value> {
        let at = |p: Vec3| [p.x, p.y, p.z];
        let own = self.hook.iter().map(|h| (h.id, 0, h.position));
        let others = self
            .remote_hooks
            .iter()
            .map(|h| (h.id, h.owner, h.position));
        own.chain(others)
            .map(|(id, owner, p)| {
                serde_json::json!({"id": id, "kind": "minecraft:fishing_bobber", "owner": owner, "position": at(p)})
            })
            .collect()
    }
    /// The hook for `horizon-ctl status`.
    pub(super) fn hook_status(&self) -> Option<serde_json::Value> {
        let h = self.hook.as_ref()?;
        Some(serde_json::json!({
            "state": format!("{:?}", h.state),
            "nibble": h.nibble,
            "lured_in": h.time_until_lured,
            "hooked_in": h.time_until_hooked,
            "open_water": h.open_water,
        }))
    }
}

/// A hook a server tracks to this client (`FishingHook.recreateFromPacket`)
/// that another player cast: drawn where the server puts it, its line
/// running to that player's hand.
pub(super) struct RemoteHook {
    id: u32,
    owner: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
}

impl Hook {
    /// This client's own hook as its server tracks it: drawn, never ticked here.
    fn remote(id: u32, at: Vec3) -> Self {
        Self {
            id,
            position: at,
            previous: at,
            state: State::Flying,
            on_ground: false,
            nibble: 0,
            time_until_lured: 0,
            time_until_hooked: 0,
            open_water: true,
            luck: 0,
        }
    }
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// `handleAddEntity` for a fishing bobber: this player's own becomes its
    /// hook (`Player.fishing`), another player's is drawn on its line to them.
    pub(super) fn spawn_remote_hook(&mut self, id: u32, at: Vec3, owner: i32) {
        let own = self.remote.as_ref().map(|r| r.entity_id);
        if own == Some(owner) {
            self.hook = Some(Hook::remote(id, at));
        } else {
            self.remote_hooks.push(RemoteHook {
                id,
                owner: owner as u32,
                position: at,
                previous: at,
            });
        }
    }
    /// One tick of interpolation for the hooks a server tracks.
    pub(super) fn step_remote_hooks(&mut self, mut step: impl FnMut(u32, Vec3) -> Vec3) {
        if self.remote.is_none() {
            return;
        }
        if let Some(h) = self.hook.as_mut() {
            h.previous = h.position;
            h.position = step(h.id, h.position);
        }
        for h in &mut self.remote_hooks {
            h.previous = h.position;
            h.position = step(h.id, h.position);
        }
    }
    /// `FishingHook.onSyncedDataUpdated`: this player's hook is biting
    /// (DATA_BITING), which `status` shows as a nibble.
    pub(super) fn remote_hook_data(&mut self, id: u32, values: &[(u8, i32, Value)]) {
        let Some(h) = self
            .hook
            .as_mut()
            .filter(|h| h.id == id && self.remote.is_some())
        else {
            return;
        };
        for (index, _, value) in values {
            if let (9, Value::Bool(biting)) = (index, value) {
                h.nibble = i32::from(*biting);
            }
        }
    }
    pub(super) fn remove_remote_hook(&mut self, id: u32) {
        if self.remote.is_some() && self.hook.as_ref().is_some_and(|h| h.id == id) {
            self.hook = None;
        }
        self.remote_hooks.retain(|h| h.id != id);
    }
    pub(super) fn clear_remote_hooks(&mut self) {
        if self.remote.is_some() {
            self.hook = None;
        }
        self.remote_hooks.clear();
    }
    /// Other players' hooks: the bobber and its line to the owner's hand
    /// (`FishingHookRenderer.getPlayerHandPos` in third person), as
    /// (hook, hand) this frame.
    fn remote_hook_ends(&self, alpha: f64) -> Vec<(glam::DVec3, glam::DVec3)> {
        let lerp = |a: Vec3, b: Vec3| {
            glam::DVec3::new(
                a.x + (b.x - a.x) * alpha,
                a.y + (b.y - a.y) * alpha,
                a.z + (b.z - a.z) * alpha,
            )
        };
        self.remote_hooks
            .iter()
            .filter_map(|h| {
                let owner = self.mobs.iter().find(|m| m.id == h.owner)?;
                let feet = lerp(owner.previous, owner.position);
                let body = horizon_client::camera::lerp(
                    alpha as f32,
                    owner.previous_body_yaw,
                    owner.body_yaw,
                );
                let angle = f64::from(body * (std::f64::consts::PI / 180.0) as f32);
                let (sin, cos) = (f64::from(trig::sin(angle)), f64::from(trig::cos(angle)));
                // The right arm; a crouching owner's eyes are lower.
                let eye = owner.eye_y() - owner.position.y;
                let hand = feet
                    + glam::DVec3::new(
                        -cos * 0.35 - sin * 0.8,
                        eye - 0.45,
                        -sin * 0.35 + cos * 0.8,
                    );
                Some((lerp(h.previous, h.position), hand))
            })
            .collect()
    }
}
