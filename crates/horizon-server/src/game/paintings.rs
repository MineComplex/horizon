//! Paintings: `HangingEntityItem.useOn` placement with `Painting.create`'s
//! variant choice, `HangingEntity.survives` checks every 100 ticks,
//! breaking by attack (`dropItem`), saving, and `PaintingRenderer` quads.
use super::{Hit, view_vector};
use crate::hc::{entities::Painting, items::ItemStack};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::RandomSource,
};
use std::sync::Arc;

/// A `painting_variant` and whether `#placeable` lists it (in tag order).
#[derive(Clone, Debug)]
pub(super) struct Variant {
    pub id: Arc<str>,
    pub width: i32,
    pub height: i32,
}
/// Variants by id plus the `#minecraft:placeable` list.
#[derive(Default)]
pub(super) struct PaintingVariants {
    pub all: Vec<Variant>,
    pub placeable: Vec<Arc<str>>,
}
impl PaintingVariants {
    pub fn load(root: &std::path::Path) -> Self {
        let data = root.join("data/minecraft");
        let mut all = Vec::new();
        if let Ok(entries) = std::fs::read_dir(data.join("painting_variant")) {
            for e in entries.flatten() {
                let path = e.path();
                let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    continue;
                };
                all.push(Variant {
                    id: Arc::from(format!("minecraft:{name}")),
                    width: v["width"].as_i64().unwrap_or(1) as i32,
                    height: v["height"].as_i64().unwrap_or(1) as i32,
                });
            }
        }
        let placeable = std::fs::read(data.join("tags/painting_variant/placeable.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v["values"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(Arc::from))
            .collect();
        Self { all, placeable }
    }
    pub fn get(&self, id: &str) -> Option<&Variant> {
        self.all.iter().find(|v| &*v.id == id)
    }
}
fn overlaps(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])) -> bool {
    (0..3).all(|i| a.0[i] < b.1[i] && b.0[i] < a.1[i])
}
impl crate::game::Server<'_> {
    /// `HangingEntity.survives`: no block collision in the box, solid blocks
    /// (or diodes) behind it and no other painting overlapping it.
    pub(super) fn painting_survives(&self, p: &Painting) -> bool {
        let (lo, hi) = p.bounds();
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
        // calculateSupportBox: moved half a block into the wall, deflated.
        let s = p.step().map(f64::from);
        let (slo, shi) = (
            std::array::from_fn::<f64, 3, _>(|i| lo[i] - s[i] * 0.5 + 1.0e-7),
            std::array::from_fn::<f64, 3, _>(|i| hi[i] - s[i] * 0.5 - 1.0e-7),
        );
        for x in slo[0].floor() as i32..=shi[0].floor() as i32 {
            for y in slo[1].floor() as i32..=shi[1].floor() as i32 {
                for z in slo[2].floor() as i32..=shi[2].floor() as i32 {
                    let f = self.res.facts().get(self.world.block(x, y, z));
                    if !(f.legacy_solid || f.has_class("DiodeBlock")) {
                        return false;
                    }
                }
            }
        }
        !self
            .paintings
            .iter()
            .any(|o| o.id != p.id && overlaps(o.bounds(), (lo, hi)))
    }
    /// `HangingEntityItem.useOn` for a painting against `hit`'s face.
    pub(super) fn place_painting(&mut self, hit: &Hit, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:painting" || hit.face[1] != 0 {
            return false;
        }
        let facing = match hit.face {
            [0, 0, 1] => 0,
            [-1, 0, 0] => 1,
            [0, 0, -1] => 2,
            _ => 3,
        };
        let pos = [
            hit.pos[0] + hit.face[0],
            hit.pos[1],
            hit.pos[2] + hit.face[2],
        ];
        self.next_entity += 1;
        let mut candidate = Painting {
            id: self.next_entity,
            pos,
            facing,
            variant: Arc::from(""),
            width: 1,
            height: 1,
            check_interval: 0,
        };
        // Painting.create: the largest placeable variants that survive, one at random.
        let mut fitting: Vec<Variant> = Vec::new();
        for id in self.painting_variants.placeable.clone() {
            let Some(v) = self.painting_variants.get(&id).cloned() else {
                continue;
            };
            (candidate.width, candidate.height) = (v.width, v.height);
            if self.painting_survives(&candidate) {
                fitting.push(v);
            }
        }
        let Some(largest) = fitting.iter().map(|v| v.width * v.height).max() else {
            return false;
        };
        fitting.retain(|v| v.width * v.height >= largest);
        let pick = self
            .world
            .random
            .next_int_bound(fitting.len() as i32)
            .unwrap_or(0) as usize;
        let v = &fitting[pick];
        (candidate.variant, candidate.width, candidate.height) = (v.id.clone(), v.width, v.height);
        let (lo, hi) = candidate.bounds();
        let at = [
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ];
        self.sounds
            .play("minecraft:entity.painting.place", Some(at), 1.0, 1.0);
        self.paintings.push(candidate);
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
        if !self.creative {
            self.consume_held();
        }
        true
    }
    /// The painting under the crosshair within reach and its distance.
    pub(super) fn painting_hit(&self) -> Option<(usize, f64)> {
        let eye = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        let mut best: Option<(usize, f64)> = None;
        for (i, p) in self.paintings.iter().enumerate() {
            let (lo, hi) = p.bounds();
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
    /// `Painting.hurtServer` -> kill and `dropItem`.
    pub(super) fn break_painting(&mut self, index: usize, by_player: bool) {
        let p = self.paintings.remove(index);
        let (lo, hi) = p.bounds();
        let at = [
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ];
        self.world.modified.insert((p.pos[0] >> 4, p.pos[2] >> 4));
        if !self.world_data.rules.bool("entity_drops") {
            return;
        }
        self.sounds
            .play("minecraft:entity.painting.break", Some(at), 1.0, 1.0);
        if by_player && self.creative {
            return;
        }
        // HangingEntity.spawnAtLocation: 0.15 out from the wall.
        let s = p.step().map(f64::from);
        let stack = ItemStack {
            item: Arc::from("minecraft:painting"),
            count: 1,
            damage: 0,
            extra: None,
        };
        self.spawn_item(
            stack,
            [at[0] + s[0] * 0.15, at[1], at[2] + s[2] * 0.15],
            [0.0; 3],
            10,
        );
    }
    /// BlockAttachedEntity.tick: check survival every 100 ticks.
    pub(super) fn tick_paintings(&mut self) {
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        let mut i = 0;
        while i < self.paintings.len() {
            let (lo, hi) = self.paintings[i].bounds();
            let centre = [0, 2].map(|k| ((lo[k] + hi[k]) / 2.0).floor() as i32);
            if !ticking(centre[0], centre[1]) {
                i += 1;
                continue;
            }
            self.paintings[i].check_interval += 1;
            if self.paintings[i].check_interval >= 100 {
                self.paintings[i].check_interval = 0;
                let p = self.paintings[i].clone();
                if !self.painting_survives(&p) {
                    self.break_painting(i, false);
                    continue;
                }
            }
            i += 1;
        }
    }
}
