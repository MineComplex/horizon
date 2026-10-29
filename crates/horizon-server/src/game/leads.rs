//! Leads on the game side: putting them on and taking them off mobs
//! (`Entity.interact`'s leash part), fence knots (`LeadItem.useOn` and
//! `LeashFenceKnotEntity.interact`), loading and saving who holds what,
//! breaking, and drawing the rope (`LeashFeatureRenderer`) and the knot.
use crate::game::keys::KeyCode;
use crate::hc::{items::ItemStack, mobs::leash::{self, Holder, Pending}};
use horizon_world::nbt::Tag;

/// `Vec3.yRot(angle)`.
fn rot_y(v: glam::DVec3, angle: f64) -> glam::DVec3 {
    let (s, c) = (angle.sin(), angle.cos());
    glam::DVec3::new(v.x * c + v.z * s, v.y, v.z * c - v.x * s)
}
fn pack_light(light: u8) -> u32 {
    (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8)
}
fn abgr(r: f32, g: f32, b: f32) -> u32 {
    let c = |v: f32| (v * 255.0) as u32 & 255;
    0xFF00_0000 | c(b) << 16 | c(g) << 8 | c(r)
}
impl crate::game::Sim<'_> {
    fn lead_sound(&mut self, sound: &str, at: [f64; 3]) {
        self.sounds
            .play(&format!("minecraft:item.lead.{sound}"), Some(at), 1.0, 1.0);
    }
    /// `Leashable.canHaveALeashAttachedTo` for the mob at `index` and a
    /// holder whose box centre is `center`: within snapping range and
    /// leashable.
    fn can_attach_lead(&self, index: usize, center: glam::DVec3) -> bool {
        let m = &self.mobs[index];
        if !leash::can_be_leashed(m) || !m.alive() {
            return false;
        }
        let b = m.bounds();
        let (lo, hi) = (b.min_position(), b.max_position());
        let mine = glam::DVec3::new(
            (lo.x + hi.x) / 2.0,
            (lo.y + hi.y) / 2.0,
            (lo.z + hi.z) / 2.0,
        );
        mine.distance(center) <= leash::SNAP
    }
    fn player_center(&self) -> glam::DVec3 {
        self.player.as_ref().map_or(glam::DVec3::ZERO, |p| {
            glam::DVec3::new(
                p.position.x,
                p.position.y + f64::from(p.dimensions.height) / 2.0,
                p.position.z,
            )
        })
    }
    /// The mobs on the player's lead within 32 blocks of a point.
    fn player_leashed(&self, around: glam::DVec3) -> Vec<usize> {
        (0..self.mobs.len())
            .filter(|&i| {
                let m = &self.mobs[i];
                m.extra.leash == Some(Holder::Player)
                    && (m.position.x - around.x).abs() <= 16.0
                    && (m.position.y - around.y).abs() <= 16.0
                    && (m.position.z - around.z).abs() <= 16.0
            })
            .collect()
    }
    /// `Entity.spawnAtLocation(level, Items.LEAD)`: the item entity draws its
    /// motion from its own random, not the level's.
    fn drop_lead_item(&mut self, at: [f64; 3]) {
        let v = super::dispenser::new_item_motion();
        self.spawn_item(ItemStack::new("minecraft:lead", 1), at, v, 10);
    }
    /// `Leashable.dropLeash` / `removeLeash`: the lead falls as an item.
    pub(super) fn break_leash(&mut self, id: u32, drop: bool, sound: bool) {
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        if crate::hc::mobs::leash::remove(m).is_none() {
            return;
        }
        // Leashable.tickLeash gates a dying entity's lead drop on entity_drops.
        let drop = drop && (m.alive() || self.world_data.rules.bool("entity_drops"));
        let at = [m.position.x, m.position.y, m.position.z];
        if sound {
            self.lead_sound("break", at);
        }
        if drop {
            self.drop_lead_item(at);
        }
    }
    /// Holders read from a save become real once the entity is found; a
    /// holder that never shows up drops the lead after 100 ticks.
    pub(super) fn tick_leads(&mut self) {
        let player_uuid = self.level.player_uuid.unwrap_or([0, 0, 0, 1]);
        for i in 0..self.mobs.len() {
            let Some(pending) = self.mobs[i].extra.leash_pending else {
                continue;
            };
            let holder = match pending {
                Pending::Knot(p) => Some(Holder::Knot(p)),
                Pending::Uuid(u) if u == player_uuid => Some(Holder::Player),
                Pending::Uuid(u) => self.mob_by_uuid(u).map(Holder::Mob),
            };
            let m = &mut self.mobs[i];
            if let Some(h) = holder {
                if h == Holder::Player {
                    m.extra.leash_player = Some(player_uuid);
                }
                m.extra.leash = Some(h);
                m.extra.leash_pending = None;
            } else if m.age > 100 {
                m.extra.leash_pending = None;
                let at = [m.position.x, m.position.y, m.position.z];
                self.drop_lead_item(at);
            }
        }
    }
    /// The `leash` tag of a mob about to be saved.
    pub(super) fn write_leash(&mut self, tag: &mut Tag, index: usize) {
        let Tag::Compound(map) = tag else { return };
        map.remove(&"leash".into());
        let m = &self.mobs[index];
        let holder = m.extra.leash;
        let pending = m.extra.leash_pending;
        let uuid = match (holder, pending) {
            (Some(Holder::Player), _) => Some(self.level.player_uuid.unwrap_or([0, 0, 0, 1])),
            (Some(Holder::Mob(id)), _) => Some(self.mob_uuid(id)),
            (_, Some(Pending::Uuid(u))) => Some(u),
            _ => None,
        };
        let value = match (holder, pending) {
            (Some(h), _) => leash::to_nbt(h, uuid),
            (None, Some(Pending::Knot(p))) => leash::to_nbt(Holder::Knot(p), None),
            (None, Some(Pending::Uuid(u))) => leash::to_nbt(Holder::Player, Some(u)),
            _ => None,
        };
        if let Some(v) = value {
            map.insert("leash".into(), v);
        }
    }
    /// `Entity.interact`'s leash part for a mob. Returns whether it was used.
    pub(super) fn interact_leash(&mut self, index: usize, item: &str) -> bool {
        let sneaking = self.keys.contains(&KeyCode::ShiftLeft);
        let id = self.mobs[index].id;
        let at = self.mobs[index].center();
        if !self.mobs[index].alive() {
            return false;
        }
        // Sneaking with mobs in hand: tie them to this one.
        if sneaking && leash::can_be_leashed(&self.mobs[index]) && !self.mobs[index].is_baby() {
            let b = self.mobs[index].bounds();
            let (lo, hi) = (b.min_position(), b.max_position());
            let center = glam::DVec3::new(
                (lo.x + hi.x) / 2.0,
                (lo.y + hi.y) / 2.0,
                (lo.z + hi.z) / 2.0,
            );
            let held = self.player_leashed(center);
            let mut any = false;
            for j in held {
                if j != index && self.can_attach_lead(j, center) {
                    self.mobs[j].extra.leash = Some(Holder::Mob(id));
                    any = true;
                }
            }
            if any {
                self.lead_sound("tied", at);
                return true;
            }
        }
        if item == "minecraft:shears" {
            let mut dropped = false;
            if self.mobs[index].extra.leash.is_some() {
                self.break_leash(id, true, false);
                dropped = true;
            }
            let others: Vec<u32> = self
                .mobs
                .iter()
                .filter(|m| m.extra.leash == Some(Holder::Mob(id)))
                .map(|m| m.id)
                .collect();
            for o in others {
                self.break_leash(o, true, false);
                dropped = true;
            }
            if dropped {
                self.sounds
                    .play("minecraft:item.shears.snip", Some(at), 1.0, 1.0);
                self.damage_held_tool();
                return true;
            }
        }
        if self.mobs[index].extra.leash == Some(Holder::Player) {
            let creative = self.creative;
            self.break_leash(id, !creative, false);
            self.lead_sound("untied", at);
            return true;
        }
        if item == "minecraft:lead" {
            let center = self.player_center();
            if self.can_attach_lead(index, center) {
                if self.mobs[index].extra.leash.is_some() {
                    self.break_leash(id, true, false);
                }
                let holder = self.acting_player_uuid();
                self.mobs[index].extra.leash = Some(Holder::Player);
                self.mobs[index].extra.leash_player = Some(holder);
                self.lead_sound("tied", at);
                // The lead is always used up.
                if let Some(stack) = self.held_mut().as_mut() {
                    stack.count -= 1;
                    if stack.count == 0 {
                        *self.held_mut() = None;
                    }
                }
                return true;
            }
        }
        false
    }
    /// `LeadItem.useOn` and `LeashFenceKnotEntity.interact` on a fence.
    pub(super) fn interact_knot(&mut self) -> bool {
        let Some(hit) = self.pick() else {
            return false;
        };
        let name = self
            .res
            .facts()
            .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]));
        if !self
            .res
            .items()
            .block_tags
            .get("minecraft:fences")
            .is_some_and(|s| s.contains(&*name.name))
        {
            return false;
        }
        let knot_pos = hit.pos;
        let knot = leash::knot_state(knot_pos).center;
        let center = glam::DVec3::new(knot.x, knot.y, knot.z);
        let at = [center.x, center.y, center.z];
        let existing: Vec<usize> = (0..self.mobs.len())
            .filter(|&i| self.mobs[i].extra.leash == Some(Holder::Knot(knot_pos)))
            .collect();
        let held_item = self.held().map(|s| s.item.to_string());
        let sneaking = self.keys.contains(&KeyCode::ShiftLeft);
        // Shears snip every lead on the knot.
        if held_item.as_deref() == Some("minecraft:shears") && !existing.is_empty() {
            for i in existing {
                let id = self.mobs[i].id;
                self.break_leash(id, true, false);
            }
            self.sounds
                .play("minecraft:item.shears.snip", Some(at), 1.0, 1.0);
            self.damage_held_tool();
            return true;
        }
        let in_hand = self.player_leashed(center);
        // The knot is only made for the lead item, or when mobs already hang on it.
        if held_item.as_deref() != Some("minecraft:lead")
            && existing.is_empty()
            && in_hand.is_empty()
        {
            return false;
        }
        let mut attached = false;
        for j in in_hand {
            if self.can_attach_lead(j, center) {
                self.mobs[j].extra.leash = Some(Holder::Knot(knot_pos));
                attached = true;
            }
        }
        if !attached && !sneaking {
            let player = self.player_center();
            for i in existing {
                if self.can_attach_lead(i, player) {
                    self.mobs[i].extra.leash = Some(Holder::Player);
                    attached = true;
                }
            }
        }
        if attached {
            self.lead_sound("tied", at);
            self.swing_arm();
        }
        attached
    }
}
