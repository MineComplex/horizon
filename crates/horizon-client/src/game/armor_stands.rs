//! Armor stands: `ArmorStandItem.useOn` placement (snapped to 45 degrees),
//! falling, `interactAt` equipment swaps by clicked height, the two-hit break
//! of `hurtServer` (creative breaks at once), and `ArmorStandRenderer` with
//! the default pose, the hit wobble and humanoid armor layers.
use super::{Game, view_vector};
use horizon_client::{
    entities::ArmorStand,
    items::ItemStack,
    mesher::Vertex,
    mobs::{Mob, Part, anim::Poses},
    trims::{TrimOverride, TrimRegistry, TrimTexture},
};
use std::{collections::HashMap, path::PathBuf};

/// `EquipmentLayerRenderer.getColorForLayer`: a dyeable layer (one with a
/// colour when undyed) takes the stack's `minecraft:dyed_color`, else that
/// colour; any other layer stays white, dyed or not. The result is in the
/// renderer's byte order (R and B swapped).
fn layer_color(undyed: Option<u32>, stack: Option<&ItemStack>) -> u32 {
    let dyed = stack
        .and_then(|s| s.data())
        .and_then(|d| d.dyed_color)
        .map(|c| c as u32 & 0xFF_FFFF);
    undyed.map_or(0xFFFF_FFFF, |undyed| {
        let c = dyed.unwrap_or(undyed);
        0xFF00_0000 | (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16) & 0xFF
    })
}

/// Armor stand models and the humanoid layers of every equipment asset.
pub(super) struct ArmorStandAssets {
    pub main: Option<Part>,
    pub small: Option<Part>,
    /// (slot model suffix, small) -> model.
    pub layers: HashMap<(&'static str, bool), Part>,
    /// The player's armour models by slot (helmet, chestplate, leggings, boots).
    pub player: HashMap<&'static str, Part>,
    /// asset id -> layer kind (humanoid / humanoid_leggings) -> (texture, undyed colour).
    pub equipment: HashMap<String, HashMap<String, (String, Option<u32>)>>,
    /// asset id -> `trim_overrides`.
    pub trim_overrides: HashMap<String, Vec<TrimOverride>>,
    pub trims: TrimRegistry,
    /// The resources the trim textures are made from.
    pub root: PathBuf,
}
impl ArmorStandAssets {
    pub fn load(
        root: &std::path::Path,
        document: Option<&horizon_client::mobs::EntityModelDocument>,
    ) -> Self {
        let load = |key: &str| document.and_then(|document| document.model(key));
        let mut layers = HashMap::new();
        let mut player = HashMap::new();
        for slot in ["helmet", "chestplate", "leggings", "boots"] {
            if let Some(part) = load(&format!("minecraft:player#{slot}")) {
                player.insert(slot, part);
            }
            for (small, model) in [(false, "armor_stand"), (true, "armor_stand_small")] {
                if let Some(part) = load(&format!("minecraft:{model}#{slot}")) {
                    layers.insert((slot, small), part);
                }
            }
        }
        if let Some(part) = load("minecraft:elytra#main") {
            player.insert("elytra", part);
        }
        let mut equipment = HashMap::new();
        let mut trim_overrides = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(root.join("assets/minecraft/equipment")) {
            for e in entries.flatten() {
                let path = e.path();
                let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(v) =
                    std::fs::read(&path).map(|b| serde_json::from_slice::<serde_json::Value>(&b))
                else {
                    continue;
                };
                let Ok(v) = v else { continue };
                let mut kinds = HashMap::new();
                for kind in ["humanoid", "humanoid_leggings", "wings"] {
                    if let Some(layer) = v["layers"][kind].get(0) {
                        let tex = layer["texture"]
                            .as_str()
                            .unwrap_or(name)
                            .trim_start_matches("minecraft:");
                        let undyed = layer["dyeable"]["color_when_undyed"]
                            .as_i64()
                            .map(|c| c as u32 & 0xFF_FFFF);
                        kinds.insert(
                            kind.to_owned(),
                            (format!("entity/equipment/{kind}/{tex}"), undyed),
                        );
                    }
                }
                trim_overrides.insert(format!("minecraft:{name}"), TrimOverride::list(&v));
                equipment.insert(format!("minecraft:{name}"), kinds);
            }
        }
        Self {
            main: load("minecraft:armor_stand#main"),
            small: load("minecraft:armor_stand_small#main"),
            layers,
            player,
            equipment,
            trim_overrides,
            trims: TrimRegistry::load(root),
            root: root.to_owned(),
        }
    }
    /// Trim data of reloaded resources (the models stay).
    pub fn reload_trims(&mut self, root: &std::path::Path) {
        let fresh = Self::load(root, None);
        self.trim_overrides = fresh.trim_overrides;
        self.trims = fresh.trims;
        self.root = fresh.root;
    }
    /// `TrimTextureKey`: the trim texture of `stack` on its humanoid
    /// layer (`humanoid` or `humanoid_leggings`), if it has a trim.
    fn trim_texture(&self, asset: &str, kind: &str, stack: &ItemStack) -> Option<TrimTexture> {
        let (material, pattern) = stack.extra.as_ref()?.trim.as_ref()?;
        let overrides = self
            .trim_overrides
            .get(asset)
            .map_or(&[][..], Vec::as_slice);
        TrimTexture::resolve(&self.trims, overrides, material, pattern, kind)
    }
}
impl Game<'_> {
    /// `HumanoidArmorLayer` for the player: (model, texture rect, colour) of
    /// each worn piece, head to feet.
    pub(super) fn player_armor_layers(&self) -> Vec<(&Part, [f32; 4], u32)> {
        self.stack_armor_layers(&self.armor)
    }
    /// `HumanoidArmorLayer` for worn stacks (head to feet), trims included.
    pub(super) fn stack_armor_layers(
        &self,
        armor: &[Option<ItemStack>; 4],
    ) -> Vec<(&Part, [f32; 4], u32)> {
        self.armor_layers(
            armor
                .each_ref()
                .map(|s| s.as_ref().map(|s| (&*s.item, Some(s)))),
        )
    }
    /// `HumanoidArmorLayer` for another player's model: the armour it wears
    /// (`extra.head` .. `extra.feet`).
    pub(super) fn worn_armor_layers(&self, m: &Mob) -> Vec<(&Part, [f32; 4], u32)> {
        let e = &m.extra;
        self.armor_layers(
            [&e.head, &e.chest, &e.legs, &e.feet].map(|s| s.as_deref().map(|i| (i, None))),
        )
    }
    /// The layers of the armour items `worn` (with their stacks when known,
    /// for trims), head to feet.
    fn armor_layers(
        &self,
        worn: [Option<(&str, Option<&ItemStack>)>; 4],
    ) -> Vec<(&Part, [f32; 4], u32)> {
        let assets = &self.armor_stand_assets;
        let mut out = Vec::new();
        for (item, layer) in worn
            .into_iter()
            .zip(["helmet", "chestplate", "leggings", "boots"])
        {
            let Some((item, stack)) = item else {
                continue;
            };
            let Some(asset) = self
                .res
                .items()
                .get(item)
                .and_then(|i| i.equip_asset.clone())
            else {
                continue;
            };
            let kind = if layer == "leggings" {
                "humanoid_leggings"
            } else {
                "humanoid"
            };
            let Some((texture, undyed)) = assets.equipment.get(&asset).and_then(|k| k.get(kind))
            else {
                continue;
            };
            let (Some(r), Some(part)) = (
                self.gui.atlas.sprites.get(texture.as_str()),
                assets.player.get(layer),
            ) else {
                continue;
            };
            let color = layer_color(*undyed, stack);
            out.push((part, [r.u0, r.v0, r.u1, r.v1], color));
            // EquipmentLayerRenderer: the trim layer over the armour.
            if let Some(r) = stack
                .and_then(|s| assets.trim_texture(&asset, kind, s))
                .and_then(|t| self.gui.atlas.sprites.get(&t.sprite()))
            {
                out.push((part, [r.u0, r.v0, r.u1, r.v1], 0xFFFF_FFFF));
            }
        }
        out
    }
    /// `PalettedTextureManager.getOrPrepare` for the trims drawn this frame
    /// (the player's armour and the armour stands'): each generated texture
    /// goes into an atlas trim slot, a free one, else the least recently
    /// drawn.
    pub(super) fn prepare_trims(&mut self) {
        use horizon_client::hud::TRIM_SLOTS;
        let assets = &self.armor_stand_assets;
        let items = self.res.items();
        let mut wanted: Vec<TrimTexture> = Vec::new();
        let stacks = self
            .armor
            .iter()
            .zip(
                ["helmet", "chestplate", "leggings", "boots"]
                    .into_iter()
                    .cycle(),
            )
            .chain(self.armor_stands.iter().flat_map(|a| {
                [5usize, 4, 3, 2]
                    .map(|slot| &a.equipment[slot])
                    .into_iter()
                    .zip(["helmet", "chestplate", "leggings", "boots"])
            }));
        for (stack, layer) in stacks {
            let Some(stack) = stack.as_ref().filter(|s| s.extra.is_some()) else {
                continue;
            };
            let Some(asset) = items
                .get(&stack.item)
                .and_then(|i| i.equip_asset.as_deref())
            else {
                continue;
            };
            let kind = if layer == "leggings" {
                "humanoid_leggings"
            } else {
                "humanoid"
            };
            if let Some(t) = assets.trim_texture(asset, kind, stack)
                && !wanted.contains(&t)
            {
                wanted.push(t);
            }
        }
        for t in wanted {
            let name = t.sprite();
            // Entries the atlas no longer has (rebuilt by a resource reload) are free.
            let sprites = &self.gui.atlas.sprites;
            self.trim_slots.retain(|(n, _)| sprites.contains_key(n));
            if let Some(i) = self.trim_slots.iter().position(|e| e.0 == name) {
                let e = self.trim_slots.remove(i);
                self.trim_slots.push(e);
                continue;
            }
            let Some((w, h, rgba)) = t.generate(&self.armor_stand_assets.root) else {
                continue;
            };
            let used: Vec<usize> = self.trim_slots.iter().map(|e| e.1).collect();
            let slot = match (0..TRIM_SLOTS).find(|i| !used.contains(i)) {
                Some(free) => free,
                None => {
                    let (old, slot) = self.trim_slots.remove(0);
                    self.gui.atlas.sprites.remove(&old);
                    slot
                }
            };
            let Some(area) = self
                .gui
                .atlas
                .sprites
                .get(&format!("horizon/trim{slot}"))
                .copied()
            else {
                continue;
            };
            if w > area.w || h > area.h {
                continue;
            }
            let (aw, ah) = (self.gui.atlas.width as f32, self.gui.atlas.height as f32);
            let (x, y) = ((area.u0 * aw).round(), (area.v0 * ah).round());
            self.map_patches.push(horizon_client::font::Patch {
                x: x as u32,
                y: y as u32,
                width: w,
                height: h,
                rgba,
            });
            let rect = horizon_client::hud::Rect {
                u0: area.u0,
                v0: area.v0,
                u1: (x + w as f32) / aw,
                v1: (y + h as f32) / ah,
                w,
                h,
            };
            self.gui.atlas.sprites.insert(name.clone(), rect);
            self.trim_slots.push((name, slot));
        }
    }
    /// ArmorStandRenderer: the stand with its default pose, then armor layers.
    pub(super) fn armor_stand_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        let assets = &self.armor_stand_assets;
        let sprites = &self.gui.atlas.sprites;
        let uv_of = |name: &str| sprites.get(name).map(|r| [r.u0, r.v0, r.u1, r.v1]);
        let Some(wood) = uv_of("entity/armorstand/armorstand") else {
            return;
        };
        for a in &self.armor_stands {
            let entity_start = out.len();
            if a.invisible {
                continue;
            }
            // ArmorStand.shouldRenderAtSqrDistance: 256 times the box's mean
            // edge (4 for a marker's empty box), whatever the entity distance.
            // LivingEntityRenderer grows the culling box by 0.5 for a dragon head.
            let (width, height) = match (a.marker, a.small) {
                (true, _) => (0.0, 0.0),
                (false, true) => (0.25, 0.9875),
                (false, false) => (0.5, 1.975),
            };
            let edge = (width + height + width) / 3.0 * 4.0;
            // 26.3 compares the size with RenderSystem.DEFAULT_DEPTH_CLEAR_VALUE,
            // which is 0.0 (reverse Z; the bytecode is `dconst_0`): an empty
            // marker box falls back to 4, so a visible marker stand draws.
            // Holograms stay hidden through the invisible flag instead.
            let range = if edge == 0.0 { 4.0 } else { edge } * 64.0;
            let dragon = a.equipment[5]
                .as_ref()
                .is_some_and(|i| &*i.item == "minecraft:dragon_head");
            let position = super::entity_culling::at(a.position);
            if !self.standing_entity_in_view(
                camera,
                position,
                position,
                alpha,
                width,
                height,
                range,
                if dragon { 0.5 } else { 0.0 },
            ) {
                continue;
            }
            let Some(model) = (if a.small { &assets.small } else { &assets.main }) else {
                continue;
            };
            let p = glam::DVec3::new(a.position.x, a.position.y, a.position.z) - camera;
            // setupRotations: 180 - body yaw, plus the wobble for 5 ticks after a hit.
            let wiggle = (self.world.game_time - a.last_hit) as f32 + alpha as f32;
            let wobble = if wiggle < 5.0 {
                (wiggle / 1.5 * std::f32::consts::PI).sin() * 3.0
            } else {
                0.0
            };
            let base = glam::Mat4::from_translation(p.as_vec3())
                * glam::Mat4::from_rotation_y((180.0 - a.yaw + wobble).to_radians())
                * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0))
                * glam::Mat4::from_translation(glam::Vec3::new(0.0, -1.501, 0.0));
            let mut poses = Poses::new(model);
            // ArmorStandModel.setupAnim: the stand's pose and part visibility.
            apply_pose(&mut poses, &a.pose);
            // Item layers pose from the hidden arms too (`visible` only skips drawing).
            let posed = poses.clone();
            let mut hide = |name: &str| {
                if poses.has(name) {
                    let pose = poses.get(name);
                    pose[6] = 0.0;
                    pose[7] = 0.0;
                    pose[8] = 0.0;
                }
            };
            if !a.show_arms {
                hide("left_arm");
                hide("right_arm");
            }
            if a.no_base_plate {
                hide("base_plate");
            }
            let light = self.world.light(
                a.position.x.floor() as i32,
                (a.position.y + 0.5).floor() as i32,
                a.position.z.floor() as i32,
            );
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            horizon_client::mobs::render::draw(
                model,
                base,
                &poses,
                wood,
                0xFFFF_FFFF,
                packed,
                0,
                out,
            );
            self.armor_stand_items(a, model, base, &posed, packed, out);
            // HumanoidArmorLayer: head, chest, legs, feet.
            for (slot, layer) in [
                (5usize, "helmet"),
                (4, "chestplate"),
                (3, "leggings"),
                (2, "boots"),
            ] {
                let Some(stack) = &a.equipment[slot] else {
                    continue;
                };
                let Some(asset) = self
                    .res
                    .items()
                    .get(&stack.item)
                    .and_then(|i| i.equip_asset.clone())
                else {
                    continue;
                };
                let kind = if layer == "leggings" {
                    "humanoid_leggings"
                } else {
                    "humanoid"
                };
                let Some((texture, undyed)) =
                    assets.equipment.get(&asset).and_then(|k| k.get(kind))
                else {
                    continue;
                };
                let (Some(uv), Some(part)) = (uv_of(texture), assets.layers.get(&(layer, a.small)))
                else {
                    continue;
                };
                let color = layer_color(*undyed, Some(stack));
                // ArmorStandRenderer's HumanoidArmorLayer: the armour glint
                // when `stack.hasFoil()`, as on a living entity.
                let item_glint = self
                    .res
                    .items()
                    .enchantments
                    .props
                    .get(&stack.item)
                    .is_some_and(|p| p.glint);
                let overlay = if stack.has_foil(item_glint) {
                    horizon_client::mobs::render::ARMOR_GLINT
                } else {
                    0
                };
                // ArmorStandArmorModel.setupAnim: the same pose as the stand.
                let mut layer_poses = Poses::new(part);
                apply_pose(&mut layer_poses, &a.pose);
                horizon_client::mobs::render::draw(
                    part,
                    base,
                    &layer_poses,
                    uv,
                    color,
                    packed,
                    overlay,
                    out,
                );
                // EquipmentLayerRenderer: the trim layer over the armour.
                if let Some(trim) = assets
                    .trim_texture(&asset, kind, stack)
                    .and_then(|t| uv_of(&t.sprite()))
                {
                    horizon_client::mobs::render::draw(
                        part,
                        base,
                        &layer_poses,
                        trim,
                        0xFFFF_FFFF,
                        packed,
                        overlay,
                        out,
                    );
                }
            }
            self.cull_entity_vertices(out, entity_start);
        }
    }
}
impl crate::game::Game<'_> {
    /// The armor stand under the crosshair, its distance and the hit point.
    pub(super) fn armor_stand_hit(&self) -> Option<(usize, f64, [f64; 3])> {
        let eye = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        let mut best: Option<(usize, f64, [f64; 3])> = None;
        for (i, a) in self.armor_stands.iter().enumerate() {
            if a.marker {
                continue;
            }
            let b = a.bounds();
            let (lo, hi) = (b.min_position(), b.max_position());
            let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
            let (mut t0, mut t1) = (0.0f64, reach);
            let mut hit = true;
            for k in 0..3 {
                if dir[k].abs() < 1e-9 {
                    hit &= eye[k] >= lo[k] && eye[k] <= hi[k];
                    continue;
                }
                let (mut ta, mut tb) = ((lo[k] - eye[k]) / dir[k], (hi[k] - eye[k]) / dir[k]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
            }
            if hit && t0 <= t1 && best.is_none_or(|b| t0 < b.1) {
                best = Some((i, t0, std::array::from_fn(|k| eye[k] + dir[k] * t0)));
            }
        }
        best
    }
}
/// The `Pose` rotations on the parts of an armor stand model or its armor
/// (`ArmorStandArmorModel.setupAnim`); the body also turns the sticks.
fn apply_pose(poses: &mut Poses, pose: &[[f32; 3]; 6]) {
    let parts: [&[&str]; 6] = [
        &["head"],
        &[
            "body",
            "right_body_stick",
            "left_body_stick",
            "shoulder_stick",
        ],
        &["left_arm"],
        &["right_arm"],
        &["left_leg"],
        &["right_leg"],
    ];
    for (names, rotation) in parts.into_iter().zip(pose) {
        for name in names {
            if poses.has(name) {
                let part = poses.get(name);
                part[3] = rotation[0].to_radians();
                part[4] = rotation[1].to_radians();
                part[5] = rotation[2].to_radians();
            }
        }
    }
}
impl Game<'_> {
    /// `ItemInHandLayer` and `CustomHeadLayer` of an armour stand: the hand
    /// items in the third-person contexts, and a non-armour head item on the
    /// head (`translateToHead`: down a quarter, turned, 0.625 scale).
    fn armor_stand_items(
        &self,
        a: &ArmorStand,
        model: &Part,
        base: glam::Mat4,
        poses: &Poses,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        use horizon_assets::item_model::DisplayContext;
        use horizon_client::mobs::render::part_matrix;
        let owner = super::item_draw::owner_at(
            [a.position.x, a.position.y, a.position.z],
            a.yaw,
            "minecraft:armor_stand",
        );
        // `useBabyOffset`: small stands use the baby hand offsets.
        let (ox, oy, oz) = if a.small {
            (0.0, 1.0, -4.5)
        } else {
            (1.0, 2.0, -10.0)
        };
        for (slot, left, context) in [
            (0usize, false, DisplayContext::ThirdPersonRightHand),
            (1, true, DisplayContext::ThirdPersonLeftHand),
        ] {
            let Some(stack) = &a.equipment[slot] else {
                continue;
            };
            let arm = if left { "left_arm" } else { "right_arm" };
            let Some(hand) = part_matrix(model, base, poses, arm, 0.0) else {
                continue;
            };
            let pose = hand
                * glam::Mat4::from_rotation_x((-90.0f32).to_radians())
                * glam::Mat4::from_rotation_y(std::f32::consts::PI)
                * glam::Mat4::from_translation(glam::Vec3::new(
                    if left { -ox } else { ox } / 16.0,
                    oy / 16.0,
                    oz / 16.0,
                ));
            let seed = a.id as i32 + if left { 1 } else { 2 };
            let state = self.resolve_item(stack, context, Some(&owner), seed);
            self.submit_item_world(&state, pose, light, out);
        }
        // `CustomHeadLayer`: a worn skull draws the skull model.
        if let Some(stack) = &a.equipment[5]
            && let Some(kind) = skull_kind(&stack.item)
            && let Some(head) = part_matrix(model, base, poses, "head", 0.0)
        {
            let pose = head * glam::Mat4::from_scale(glam::Vec3::splat(1.1875));
            let special = horizon_assets::item_model::SpecialModel {
                kind: if kind == "player" {
                    horizon_assets::item_model::SpecialKind::PlayerHead
                } else {
                    horizon_assets::item_model::SpecialKind::Head {
                        kind: kind.to_owned(),
                        texture: None,
                        animation: 0.0,
                    }
                },
            };
            let mut sink = horizon_client::item_render::WorldSink {
                out,
                light,
                lights: Some(horizon_client::block_entity_render::diffuse_lights(
                    self.dim == super::Dim::Nether,
                )),
                glint: self.glint(),
            };
            let profile = stack
                .extra
                .as_deref()
                .and_then(|d| d.other.iter().find(|(k, _)| k == "minecraft:profile"))
                .map(|(_, t)| horizon_client::scoreboard::tag_to_json(t))
                .unwrap_or(serde_json::Value::Null);
            self.pack.item_renderer.submit_special(
                &special,
                &profile,
                false,
                pose,
                &self.gui.atlas.sprites,
                &self.item_host(),
                &mut sink,
            );
            return;
        }
        // A head item that is not worn armour.
        if let Some(stack) = &a.equipment[5]
            && self
                .res
                .items()
                .get(&stack.item)
                .is_none_or(|i| i.equip_asset.is_none())
            && let Some(head) = part_matrix(model, base, poses, "head", 0.0)
        {
            let pose = head
                * glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.25, 0.0))
                * glam::Mat4::from_rotation_y(std::f32::consts::PI)
                * glam::Mat4::from_scale(glam::Vec3::new(0.625, -0.625, -0.625));
            // `updateForLiving`: seeded by the entity id plus HEAD's ordinal.
            let state =
                self.resolve_item(stack, DisplayContext::Head, Some(&owner), a.id as i32 + 5);
            self.submit_item_world(&state, pose, light, out);
        }
    }
}
/// `AbstractSkullBlock.getType` of a skull item.
pub(super) fn skull_kind(item: &str) -> Option<&'static str> {
    Some(match item {
        "minecraft:skeleton_skull" => "skeleton",
        "minecraft:wither_skeleton_skull" => "wither_skeleton",
        "minecraft:zombie_head" => "zombie",
        "minecraft:creeper_head" => "creeper",
        "minecraft:dragon_head" => "dragon",
        "minecraft:piglin_head" => "piglin",
        "minecraft:player_head" => "player",
        _ => return None,
    })
}

#[cfg(test)]
mod layer_color_tests {
    use super::*;
    #[test]
    fn a_dyed_stack_overrides_the_undyed_leather_colour() {
        let mut dyed = ItemStack::new("minecraft:leather_helmet", 1);
        dyed.data_mut().dyed_color = Some(0x1E_90FF);
        // Dyed blue, R and B swapped into the renderer's order, opaque.
        assert_eq!(layer_color(Some(0xA06540), Some(&dyed)), 0xFFFF_901E);
        // An undyed stack keeps the asset's brown.
        let plain = ItemStack::new("minecraft:leather_helmet", 1);
        assert_eq!(layer_color(Some(0xA06540), Some(&plain)), 0xFF40_65A0);
        // No dye and no undyed colour: white.
        assert_eq!(layer_color(None, Some(&plain)), 0xFFFF_FFFF);
        // A layer that isn't dyeable stays white even on a dyed stack.
        assert_eq!(layer_color(None, Some(&dyed)), 0xFFFF_FFFF);
    }
}
