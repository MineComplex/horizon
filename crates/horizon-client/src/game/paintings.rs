//! Paintings: `HangingEntityItem.useOn` placement with `Painting.create`'s
//! variant choice, `HangingEntity.survives` checks every 100 ticks,
//! breaking by attack (`dropItem`), saving, and `PaintingRenderer` quads.
use super::{Game, view_vector};
use horizon_client::mesher::Vertex;
use std::sync::Arc;

/// A `painting_variant` and whether `#placeable` lists it (in tag order).
#[derive(Clone, Debug)]
pub(super) struct Variant {
    pub id: Arc<str>,
    pub asset: String,
    pub width: i32,
    pub height: i32,
}
/// Variants by id plus the `#minecraft:placeable` list.
#[derive(Default)]
pub(super) struct PaintingVariants {
    pub all: Vec<Variant>,
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
                    asset: v["asset_id"]
                        .as_str()
                        .unwrap_or(name)
                        .trim_start_matches("minecraft:")
                        .to_owned(),
                    width: v["width"].as_i64().unwrap_or(1) as i32,
                    height: v["height"].as_i64().unwrap_or(1) as i32,
                });
            }
        }
        Self { all }
    }
    pub fn get(&self, id: &str) -> Option<&Variant> {
        self.all.iter().find(|v| &*v.id == id)
    }
}
impl Game<'_> {
    /// PaintingRenderer: the variant on the front, the back texture behind and
    /// on the edges.
    pub(super) fn painting_geometry(&self, camera: glam::DVec3, out: &mut Vec<Vertex>) {
        let sprites = &self.gui.atlas.sprites;
        let Some(back) = sprites.get("painting/back").copied() else {
            return;
        };
        for p in &self.paintings {
            let entity_start = out.len();
            // EntityRenderer.shouldRender: the default range and its box,
            // measured from the box's centre (HangingEntity's position).
            let (lo, hi) = p.bounds();
            let (lo, hi) = (glam::DVec3::from(lo), glam::DVec3::from(hi));
            let range = super::entity_culling::default_range(hi - lo, self.view_scale());
            if !self.entity_in_view(camera, (lo + hi) / 2.0, range, lo, hi) {
                continue;
            }
            let Some(v) = self.painting_variants.get(&p.variant) else {
                continue;
            };
            let Some(front) = sprites.get(&format!("painting/{}", v.asset)).copied() else {
                continue;
            };
            let (lo, hi) = p.bounds();
            let c = glam::DVec3::new(
                (lo[0] + hi[0]) / 2.0,
                (lo[1] + hi[1]) / 2.0,
                (lo[2] + hi[2]) / 2.0,
            ) - camera;
            let s = p.step().map(f64::from);
            let l = p.left().map(f64::from);
            let out_dir = glam::DVec3::new(s[0], 0.0, s[2]);
            // The viewer's right is the painting's left.
            let right = glam::DVec3::new(l[0], 0.0, l[2]);
            let up = glam::DVec3::Y;
            let (hw, hh, d) = (
                f64::from(p.width) / 2.0,
                f64::from(p.height) / 2.0,
                1.0 / 32.0,
            );
            let light = self.world.light(p.pos[0], p.pos[1], p.pos[2]);
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let mut quad = |corners: [glam::DVec3; 4], uv: [[f32; 2]; 4], shade: f32| {
                let k = (255.0 * shade) as u32;
                let color = 0xFF00_0000 | k << 16 | k << 8 | k;
                for (p, uv) in corners.iter().zip(uv) {
                    out.push(Vertex {
                        normal: [0; 4],
                        pos: p.as_vec3().to_array(),
                        uv,
                        layer: 0,
                        color,
                        light: packed,

                        glint_uv: [0.0, 0.0],
                    });
                }
            };
            let full = |r: horizon_client::hud::Rect| {
                [[r.u0, r.v1], [r.u1, r.v1], [r.u1, r.v0], [r.u0, r.v0]]
            };
            let f = c + out_dir * d;
            quad(
                [
                    f - right * hw - up * hh,
                    f + right * hw - up * hh,
                    f + right * hw + up * hh,
                    f - right * hw + up * hh,
                ],
                full(front),
                1.0,
            );
            let b = c - out_dir * d;
            quad(
                [
                    b + right * hw - up * hh,
                    b - right * hw - up * hh,
                    b - right * hw + up * hh,
                    b + right * hw + up * hh,
                ],
                full(back),
                0.8,
            );
            // Edges: a one-pixel strip of the back texture.
            let strip = [
                [back.u0, back.v0 + (back.v1 - back.v0) / 16.0],
                [back.u1, back.v0 + (back.v1 - back.v0) / 16.0],
                [back.u1, back.v0],
                [back.u0, back.v0],
            ];
            let (t, bt) = (up * hh, -up * hh);
            quad(
                [
                    b + t + right * hw,
                    b + t - right * hw,
                    f + t - right * hw,
                    f + t + right * hw,
                ],
                strip,
                1.0,
            );
            quad(
                [
                    f + bt + right * hw,
                    f + bt - right * hw,
                    b + bt - right * hw,
                    b + bt + right * hw,
                ],
                strip,
                0.5,
            );
            quad(
                [
                    b - right * hw - up * hh,
                    f - right * hw - up * hh,
                    f - right * hw + up * hh,
                    b - right * hw + up * hh,
                ],
                strip,
                0.6,
            );
            quad(
                [
                    f + right * hw - up * hh,
                    b + right * hw - up * hh,
                    b + right * hw + up * hh,
                    f + right * hw + up * hh,
                ],
                strip,
                0.6,
            );
            self.cull_entity_vertices(out, entity_start);
        }
    }
}
impl crate::game::Game<'_> {
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
}
