//! Item frames and glow item frames: `ItemFrameItem` placement on any face,
//! `ItemFrame.survives`, `interact` (put an item in, then rotate it),
//! `hurtServer` (take the item out, then break the frame) and
//! `ItemFrameRenderer` (the frame model and the item at a 45 degree step).
use super::{Hit, view_vector};
use horizon_assets::{bake::bake_model, model::CuboidModel, resolve::ModelLibrary};
use crate::hc::{entities::ItemFrame, items::ItemStack, mesher::Vertex};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
    random::RandomSource,
};
use std::sync::Arc;

/// A baked frame model: (corners in block units, unit UVs, sprite) per quad.
pub(super) type FrameModel = Vec<([[f32; 3]; 4], [[f32; 2]; 4], String)>;

/// Bake `block/<name>` (item_frame / glow_item_frame) from the resources.
pub(super) fn bake_frame(root: &std::path::Path, name: &str) -> FrameModel {
    let mut models = std::collections::BTreeMap::new();
    let mut pending = vec![format!("minecraft:block/{name}")];
    while let Some(model) = pending.pop() {
        let Some((ns, path)) = model.split_once(':') else {
            continue;
        };
        let Ok(text) =
            std::fs::read_to_string(root.join(format!("assets/{ns}/models/{path}.json")))
        else {
            continue;
        };
        let Ok(m) = CuboidModel::from_json(&text) else {
            continue;
        };
        if let Some(parent) = &m.parent {
            pending.push(parent.clone());
        }
        models.insert(model, m);
    }
    let Ok(library) = ModelLibrary::new(models) else {
        return Vec::new();
    };
    let Ok(resolved) = library.resolve(&format!("minecraft:block/{name}")) else {
        return Vec::new();
    };
    let Ok(quads) = bake_model(&resolved) else {
        return Vec::new();
    };
    let unit = if quads
        .iter()
        .flat_map(|q| q.positions)
        .any(|p| p.iter().any(|v| *v > 1.5))
    {
        16.0
    } else {
        1.0
    };
    quads
        .into_iter()
        .map(|q| {
            (
                q.positions.map(|p| p.map(|v| v / unit)),
                q.uv,
                q.material
                    .sprite
                    .trim_start_matches("minecraft:")
                    .to_owned(),
            )
        })
        .collect()
}
fn facing_of(face: [i32; 3]) -> u8 {
    match face {
        [0, -1, 0] => 0,
        [0, 1, 0] => 1,
        [0, 0, -1] => 2,
        [0, 0, 1] => 3,
        [-1, 0, 0] => 4,
        _ => 5,
    }
}
fn sound(f: &ItemFrame, what: &str) -> String {
    let prefix = if f.glow {
        "glow_item_frame"
    } else {
        "item_frame"
    };
    format!("minecraft:entity.{prefix}.{what}")
}
fn center(f: &ItemFrame) -> [f64; 3] {
    let (lo, hi) = f.bounds();
    std::array::from_fn(|i| (lo[i] + hi[i]) / 2.0)
}
impl crate::game::Sim<'_> {
    /// `ItemFrame.survives` (not fixed): no collision, a solid block behind
    /// (or a diode for wall frames), no overlapping hanging entity.
    fn frame_survives(&self, f: &ItemFrame) -> bool {
        if f.fixed {
            return true;
        }
        let (lo, hi) = f.bounds();
        let bounds = Aabb::new(
            Vec3::new(lo[0], lo[1], lo[2]),
            Vec3::new(hi[0], hi[1], hi[2]),
        );
        let mut scene = crate::hc::collisions::SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        if !scene.unobstructed(bounds).unwrap_or(false) {
            return false;
        }
        let s = f.step();
        let behind = self.res.facts().get(self.world.block(
            f.pos[0] - s[0],
            f.pos[1] - s[1],
            f.pos[2] - s[2],
        ));
        let supported = behind.legacy_solid || (s[1] == 0 && behind.has_class("DiodeBlock"));
        // canCoexist(true): only hanging entities facing the same way block it.
        let overlaps = |o: ([f64; 3], [f64; 3])| (0..3).all(|i| o.0[i] < hi[i] && lo[i] < o.1[i]);
        supported
            && !self
                .item_frames
                .iter()
                .any(|o| o.id != f.id && o.facing == f.facing && overlaps(o.bounds()))
    }
    /// `ItemFrameItem.useOn`.
    pub(super) fn place_item_frame(&mut self, hit: &Hit, stack: &ItemStack) -> bool {
        let glow = match &*stack.item {
            "minecraft:item_frame" => false,
            "minecraft:glow_item_frame" => true,
            _ => return false,
        };
        let pos = std::array::from_fn(|i| hit.pos[i] + hit.face[i]);
        let frame = ItemFrame {
            id: self.next_entity + 1,
            pos,
            facing: facing_of(hit.face),
            glow,
            item: None,
            rotation: 0,
            drop_chance: 1.0,
            fixed: false,
            invisible: false,
            check_interval: 0,
        };
        if !self.frame_survives(&frame) {
            return false;
        }
        self.next_entity += 1;
        self.sounds
            .play(&sound(&frame, "place"), Some(center(&frame)), 1.0, 1.0);
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
        self.item_frames.push(frame);
        if !self.creative {
            self.consume_held();
        }
        true
    }
    /// The frame under the crosshair within reach and its distance.
    pub(super) fn frame_hit(&self) -> Option<(usize, f64)> {
        let eye = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        let mut best: Option<(usize, f64)> = None;
        for (i, f) in self.item_frames.iter().enumerate() {
            let (lo, hi) = f.bounds();
            let (mut t0, mut t1) = (0.0f64, reach);
            let mut hit = true;
            for a in 0..3 {
                if dir[a].abs() < 1e-9 {
                    hit &= eye[a] >= lo[a] && eye[a] <= hi[a];
                    continue;
                }
                let (mut ta, mut tb) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
            }
            if hit && t0 <= t1 && best.is_none_or(|b| t0 < b.1) {
                best = Some((i, t0));
            }
        }
        best
    }
    /// Nearer than the targeted block?
    pub(super) fn before_block(&self, distance: f64) -> bool {
        self.pick().is_none_or(|h| {
            let eye = self.eye_position().unwrap_or_default();
            (0..3)
                .map(|a| (h.point[a] - eye[a]).powi(2))
                .sum::<f64>()
                .sqrt()
                > distance
        })
    }
    /// `ItemFrame.interact` for the frame under the crosshair.
    pub(super) fn interact_frame(&mut self) -> bool {
        let Some((i, d)) = self.frame_hit() else {
            return false;
        };
        if !self.before_block(d) || self.item_frames[i].fixed {
            return false;
        }
        let at = center(&self.item_frames[i]);
        if self.item_frames[i].item.is_none() {
            let Some(held) = self.held().cloned() else {
                return false;
            };
            self.item_frames[i].item = Some(ItemStack { count: 1, ..held });
            self.sounds
                .play(&sound(&self.item_frames[i], "add_item"), Some(at), 1.0, 1.0);
            if !self.creative {
                self.consume_held();
            }
        } else {
            let f = &mut self.item_frames[i];
            f.rotation = (f.rotation + 1) % 8;
            let s = sound(f, "rotate_item");
            self.sounds.play(&s, Some(at), 1.0, 1.0);
        }
        let p = self.item_frames[i].pos;
        self.world.modified.insert((p[0] >> 4, p[2] >> 4));
        self.frame_output_changed(p);
        self.swing_arm();
        true
    }
    /// `ItemFrame.hurtServer` from the player: the item comes out first.
    pub(super) fn attack_frame(&mut self) -> bool {
        let Some((i, d)) = self.frame_hit() else {
            return false;
        };
        if !self.before_block(d) || self.mob_hit().is_some_and(|m| m.1 < d) {
            return false;
        }
        if self.item_frames[i].fixed {
            return true;
        }
        let with_frame = self.item_frames[i].item.is_none();
        let at = center(&self.item_frames[i]);
        let f = if with_frame {
            self.item_frames.remove(i)
        } else {
            self.item_frames[i].clone()
        };
        self.sounds.play(
            &sound(&f, if with_frame { "break" } else { "remove_item" }),
            Some(at),
            1.0,
            1.0,
        );
        if !with_frame {
            self.item_frames[i].item = None;
        }
        self.drop_frame_items(&f, with_frame, true);
        self.world.modified.insert((f.pos[0] >> 4, f.pos[2] >> 4));
        if f.item.is_some() {
            self.frame_output_changed(f.pos);
        }
        true
    }
    /// `ItemFrame.setItem` / `setRotation`: `updateNeighbourForOutputSignal`, so
    /// the comparators reading the frame take its new signal.
    fn frame_output_changed(&mut self, pos: [i32; 3]) {
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| b.update_neighbour_for_output_signal(level, ticks, at));
    }
    /// `ItemFrame.dropItem`: the frame (when breaking) and the held item.
    fn drop_frame_items(&mut self, f: &ItemFrame, with_frame: bool, by_player: bool) {
        if !self.world_data.rules.bool("entity_drops") || (by_player && self.creative) {
            return;
        }
        let s = f.step().map(f64::from);
        let c = center(f);
        let at = [c[0] + s[0] * 0.15, c[1], c[2] + s[2] * 0.15];
        if with_frame {
            let frame = ItemStack {
                item: Arc::from(f.kind()),
                count: 1,
                damage: 0,
                extra: None,
            };
            self.spawn_item(frame, at, [0.0; 3], 10);
        }
        if let Some(item) = f.item.clone()
            && self.world.random.next_float() < f.drop_chance
        {
            self.spawn_item(item, at, [0.0; 3], 10);
        }
    }
    /// BlockAttachedEntity.tick for frames.
    pub(super) fn tick_item_frames(&mut self) {
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        let mut i = 0;
        while i < self.item_frames.len() {
            let at = center(&self.item_frames[i]);
            if !ticking(at[0].floor() as i32, at[2].floor() as i32) {
                i += 1;
                continue;
            }
            self.item_frames[i].check_interval += 1;
            if self.item_frames[i].check_interval >= 100 {
                self.item_frames[i].check_interval = 0;
                let f = self.item_frames[i].clone();
                if !self.frame_survives(&f) {
                    self.item_frames.remove(i);
                    self.sounds
                        .play(&sound(&f, "break"), Some(center(&f)), 1.0, 1.0);
                    self.drop_frame_items(&f, true, false);
                    if f.item.is_some() {
                        self.frame_output_changed(f.pos);
                    }
                    continue;
                }
            }
            i += 1;
        }
    }
}
