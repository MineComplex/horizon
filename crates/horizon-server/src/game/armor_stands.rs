//! Armor stands: `ArmorStandItem.useOn` placement (snapped to 45 degrees),
//! falling, `interactAt` equipment swaps by clicked height, the two-hit break
//! of `hurtServer` (creative breaks at once), and `ArmorStandRenderer` with
//! the default pose, the hit wobble and humanoid armor layers.
use super::view_vector;
use crate::hc::{entities::ArmorStand, items::ItemStack, mobs::{Mob, Part, anim::Poses}};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::RandomSource,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

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

impl crate::game::Sim<'_> {
    /// `ArmorStandItem.useOn`.
    pub(super) fn place_armor_stand(&mut self, hit: &super::Hit, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:armor_stand" || hit.face == [0, -1, 0] {
            return false;
        }
        // BlockPlaceContext: a replaceable clicked block is replaced.
        let clicked = self
            .res
            .facts()
            .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]));
        let pos = if clicked.replaceable {
            hit.pos
        } else {
            std::array::from_fn(|i| hit.pos[i] + hit.face[i])
        };
        let foot = Vec3::new(
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        );
        let yaw =
            (((self.yaw - 180.0 + 180.0).rem_euclid(360.0) - 180.0 + 22.5) / 45.0).floor() * 45.0;
        let stand = ArmorStand::new(self.next_entity + 1, foot, yaw);
        let b = stand.bounds();
        // Touching the floor is not a collision.
        let (lo, hi) = (b.min_position(), b.max_position());
        let e = 1.0e-7;
        let bounds = Aabb::new(
            Vec3::new(lo.x + e, lo.y + e, lo.z + e),
            Vec3::new(hi.x - e, hi.y - e, hi.z - e),
        );
        let mut scene = crate::hc::collisions::SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        if !scene.unobstructed(bounds).unwrap_or(false)
            || self
                .armor_stands
                .iter()
                .any(|o| overlaps(o.bounds(), bounds))
        {
            return false;
        }
        self.next_entity += 1;
        self.sounds.play(
            "minecraft:entity.armor_stand.place",
            Some([foot.x, foot.y, foot.z]),
            0.75,
            0.8,
        );
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
        self.armor_stands.push(stand);
        if !self.creative {
            self.consume_held();
        }
        true
    }
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
    fn equipment_slot(&self, stack: &ItemStack) -> usize {
        match self
            .res
            .items()
            .get(&stack.item)
            .and_then(|i| i.equip_slot.as_deref())
        {
            Some("feet") => 2,
            Some("legs") => 3,
            Some("chest") => 4,
            Some("head") => 5,
            Some("offhand") => 1,
            _ => 0,
        }
    }
    /// `ArmorStand.interactAt`.
    pub(super) fn interact_armor_stand(&mut self) -> bool {
        let Some((i, d, point)) = self.armor_stand_hit() else {
            return false;
        };
        if !self.before_block(d) {
            return false;
        }
        let held = self.held().cloned();
        if held
            .as_ref()
            .is_some_and(|h| &*h.item == "minecraft:name_tag")
        {
            return false;
        }
        let a = &self.armor_stands[i];
        let slot = match &held {
            Some(h) => {
                let slot = self.equipment_slot(h);
                if slot < 2 && !a.show_arms {
                    return true;
                }
                slot
            }
            None => {
                // getClickedSlot by the height of the click.
                let y = (point[1] - a.position.y) / if a.small { 0.5 } else { 1.0 };
                let has = |s: usize| a.equipment[s].is_some();
                let small = a.small;
                if y >= 0.1 && y < 0.1 + if small { 0.8 } else { 0.45 } && has(2) {
                    2
                } else if y >= 0.9 + if small { 0.3 } else { 0.0 }
                    && y < 0.9 + if small { 1.0 } else { 0.7 }
                    && has(4)
                {
                    4
                } else if y >= 0.4 && y < 0.4 + if small { 1.0 } else { 0.8 } && has(3) {
                    3
                } else if y >= 1.6 && has(5) {
                    5
                } else if !has(0) && has(1) {
                    1
                } else {
                    0
                }
            }
        };
        let current = self.armor_stands[i].equipment[slot].clone();
        if held.is_none() && current.is_none() {
            return false;
        }
        // ArmorStand.swapItem.
        match held {
            // A player with infinite materials puts a copy of one on an empty
            // slot and keeps the stack.
            Some(h) if self.creative && current.is_none() => {
                self.armor_stands[i].equipment[slot] = Some(ItemStack { count: 1, ..h });
            }
            Some(h) if h.count > 1 => {
                if current.is_some() {
                    return true;
                }
                self.armor_stands[i].equipment[slot] = Some(ItemStack {
                    count: 1,
                    ..h.clone()
                });
                *self.held_mut() = Some(ItemStack {
                    count: h.count - 1,
                    ..h
                });
            }
            h => {
                self.armor_stands[i].equipment[slot] = h;
                *self.held_mut() = current;
            }
        }
        let p = self.armor_stands[i].position;
        self.world
            .modified
            .insert(((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4));
        self.swing_arm();
        true
    }
    /// `ArmorStand.hurtServer` from a player attack.
    pub(super) fn attack_armor_stand(&mut self) -> bool {
        let Some((i, d, _)) = self.armor_stand_hit() else {
            return false;
        };
        if !self.before_block(d) || self.mob_hit().is_some_and(|m| m.1 < d) {
            return false;
        }
        let time = self.world.game_time;
        let at = self.armor_stands[i].position;
        let at = [at.x, at.y, at.z];
        if self.creative {
            self.armor_stands.remove(i);
            self.sounds
                .play("minecraft:entity.armor_stand.break", Some(at), 1.0, 1.0);
            return true;
        }
        if time - self.armor_stands[i].last_hit > 5 {
            // First hit: entity event 32, the wobble and the hit sound.
            self.armor_stands[i].last_hit = time;
            self.sounds
                .play("minecraft:entity.armor_stand.hit", Some(at), 0.3, 1.0);
            return true;
        }
        // brokenByPlayer: the stand item, then brokenByAnything: the equipment.
        let a = self.armor_stands.remove(i);
        self.sounds
            .play("minecraft:entity.armor_stand.break", Some(at), 1.0, 1.0);
        let center = [at[0], at[1] + if a.small { 0.4875 } else { 0.9875 }, at[2]];
        let stand = ItemStack {
            item: Arc::from("minecraft:armor_stand"),
            count: 1,
            damage: 0,
            extra: None,
        };
        self.spawn_item(stand, center, [0.0; 3], 10);
        for stack in a.equipment.into_iter().flatten() {
            let r = &mut self.world.random;
            let v = [
                f64::from(r.next_float()) * 0.2 - 0.1,
                0.2,
                f64::from(r.next_float()) * 0.2 - 0.1,
            ];
            self.spawn_item(stack, center, v, 10);
        }
        self.world
            .modified
            .insert(((at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4));
        true
    }
    pub(super) fn tick_armor_stands(&mut self) {
        let mut scene = crate::hc::collisions::SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        let mut dead = Vec::new();
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        for (i, a) in self.armor_stands.iter_mut().enumerate() {
            if !ticking(a.position.x.floor() as i32, a.position.z.floor() as i32) {
                continue;
            }
            a.tick(&mut scene);
            if !a.environment_tick(&self.world, self.res.facts(), self.pack.assets.registry) {
                dead.push(i);
            }
        }
        for i in dead.into_iter().rev() {
            let a = self.armor_stands.remove(i);
            let at = [a.position.x, a.position.y, a.position.z];
            let (pos, drops) = burnt_armor_stand_equipment(a);
            for stack in drops {
                self.pop_resource(pos, stack);
            }
            self.sounds
                .play("minecraft:entity.armor_stand.break", Some(at), 1.0, 1.0);
            self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
        }
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
fn overlaps(a: Aabb, b: Aabb) -> bool {
    let (a0, a1, b0, b1) = (
        a.min_position(),
        a.max_position(),
        b.min_position(),
        b.max_position(),
    );
    a0.x < b1.x && b0.x < a1.x && a0.y < b1.y && b0.y < a1.y && a0.z < b1.z && b0.z < a1.z
}
impl crate::game::Sim<'_> {
    /// `Equippable.swapWithEquipmentSlot`: right-clicking armor swaps it into
    /// its slot (the worn piece comes back to the hand).
    pub(super) fn equip_from_hand(&mut self, stack: &ItemStack) -> bool {
        let Some(info) = self.res.items().get(&stack.item) else {
            return false;
        };
        let (index, slot) = match info.equip_slot.as_deref() {
            Some("head") => (0, "head"),
            Some("chest") => (1, "chest"),
            Some("legs") => (2, "legs"),
            Some("feet") => (3, "feet"),
            _ => return false,
        };
        if !info.swappable {
            return false;
        }
        let current = self.armor[index].clone();
        // PREVENT_ARMOR_CHANGE (Curse of Binding) outside creative, or the same item.
        if current.as_ref().is_some_and(|c| {
            (c.enchantment_level("minecraft:binding_curse") > 0 && !self.creative)
                || c.same_item_same_components(stack)
        }) {
            return true;
        }
        let equipped = ItemStack {
            count: 1,
            ..stack.clone()
        };
        self.armor[index] = Some(equipped.clone());
        // Player.setItemSlot: onEquipItem plays the equip sound.
        self.player_equipped(slot, Some(&equipped));
        if !self.creative {
            let mut held = stack.clone();
            held.count -= 1;
            if held.count == 0 {
                // The worn piece comes back to the hand.
                *self.held_mut() = current;
            } else {
                *self.held_mut() = Some(held);
                if let Some(old) = current
                    && let Some(rest) = self.give(old)
                    && let Some(eye) = self.eye_position()
                {
                    self.spawn_item(rest, eye, [0.0; 3], 40);
                }
            }
        }
        self.apply_equipment();
        self.swing_arm();
        true
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

fn burnt_armor_stand_equipment(a: ArmorStand) -> ([i32; 3], Vec<ItemStack>) {
    let pos = [
        a.position.x.floor() as i32,
        a.position.y.floor() as i32 + 1,
        a.position.z.floor() as i32,
    ];
    let drops = a
        .equipment
        .into_iter()
        .flatten()
        .filter(|stack| stack.enchantment_level("minecraft:vanishing_curse") == 0)
        .collect();
    (pos, drops)
}
