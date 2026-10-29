//! Item frames and glow item frames: `ItemFrameItem` placement on any face,
//! `ItemFrame.survives`, `interact` (put an item in, then rotate it),
//! `hurtServer` (take the item out, then break the frame) and
//! `ItemFrameRenderer` (the frame model and the item at a 45 degree step).
use super::{Game, view_vector};
use horizon_assets::{bake::bake_model, model::CuboidModel, resolve::ModelLibrary};
use horizon_client::{items::ItemStack, mesher::Vertex};

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
impl Game<'_> {
    /// ItemFrameRenderer: the frame model, then the item half size on its face.
    pub(super) fn frame_geometry(&mut self, camera: glam::DVec3, out: &mut Vec<Vertex>) {
        let sprites = &self.front.gui.atlas.sprites;
        let mut maps: Vec<(i32, glam::Mat4, u32)> = Vec::new();
        let mut items: Vec<(ItemStack, u32, glam::Mat4, u8)> = Vec::new();
        // ItemFrame.shouldRenderAtSqrDistance: 1024 blocks at a view scale
        // of 1.
        let range = 16.0 * 64.0 * self.view_scale();
        for f in &self.item_frames {
            let entity_start = out.len();
            let map = f.item.as_ref().and_then(super::maps::map_id);
            // EntityRenderer.shouldRender: its box (a block wide with a map),
            // measured from the box's centre.
            let (lo, hi) = f.bounds();
            let (mut lo, mut hi) = (glam::DVec3::from(lo), glam::DVec3::from(hi));
            if map.is_some() {
                let s = f.step();
                for i in 0..3 {
                    if s[i] == 0 {
                        let c = (lo[i] + hi[i]) / 2.0;
                        (lo[i], hi[i]) = (c - 0.5, c + 0.5);
                    }
                }
            }
            if !self.entity_in_view(camera, (lo + hi) / 2.0, range, lo, hi) {
                continue;
            }
            let s = f.step();
            // Pose: to the block centre, then X/Y rotation by facing.
            let (x_rot, y_rot) = if s[1] == 0 {
                let to_y = match f.facing {
                    3 => 0.0,
                    4 => 90.0,
                    2 => 180.0,
                    _ => 270.0,
                };
                (0.0f64, 180.0f64 - to_y)
            } else {
                (-90.0 * f64::from(s[1]), 180.0)
            };
            let pose = glam::DMat4::from_translation(
                glam::DVec3::new(
                    f64::from(f.pos[0]) + 0.5,
                    f64::from(f.pos[1]) + 0.5,
                    f64::from(f.pos[2]) + 0.5,
                ) - camera,
            ) * glam::DMat4::from_rotation_x(x_rot.to_radians())
                * glam::DMat4::from_rotation_y(y_rot.to_radians());
            let light = self.world.light(f.pos[0], f.pos[1], f.pos[2]);
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            // ItemFrameRenderer.getBlockLightLevel: a glow frame's model is at least 5 bright.
            let frame_light = if f.glow {
                (light & 0xF0) | (light & 15).max(5)
            } else {
                light
            };
            let frame_packed =
                (u32::from(frame_light >> 4) * 16) | ((u32::from(frame_light & 15) * 16) << 8);
            let mut quad =
                |m: &glam::DMat4, corners: [[f64; 3]; 4], uv: [[f32; 2]; 4], light: u32| {
                    // The frame model is drawn with the entity shader, lit by its face normal.
                    let at = corners.map(|c| m.transform_point3(glam::DVec3::from_array(c)));
                    let n = (at[1] - at[0]).cross(at[2] - at[0]).normalize_or_zero();
                    let normal = [0, 1, 2].map(|k| (n[k].clamp(-1.0, 1.0) * 127.0) as i8);
                    for (p, uv) in at.into_iter().zip(uv) {
                        out.push(Vertex {
                            normal: [normal[0], normal[1], normal[2], -127],
                            pos: p.as_vec3().to_array(),
                            uv,
                            layer: 0,
                            color: 0xFFFF_FFFF,
                            light,

                            glint_uv: [0.0, 0.0],
                        });
                    }
                };
            let map_uv = |r: &horizon_client::hud::Rect, uv: [[f32; 2]; 4]| {
                uv.map(|[u, v]| [r.u0 + u * (r.u1 - r.u0), r.v0 + v * (r.v1 - r.v0)])
            };
            if !f.invisible {
                let model = match (f.glow, map.is_some()) {
                    (true, true) => &self.glow_map_frame_model,
                    (false, true) => &self.map_frame_model,
                    (true, false) => &self.glow_frame_model,
                    (false, false) => &self.frame_model,
                };
                for (corners, uv, sprite) in model {
                    let Some(r) = sprites.get(sprite) else {
                        continue;
                    };
                    quad(
                        &pose,
                        corners.map(|c| {
                            [
                                f64::from(c[0]) - 0.5,
                                f64::from(c[1]) - 0.5,
                                f64::from(c[2]) - 0.5,
                            ]
                        }),
                        map_uv(r, *uv),
                        frame_packed,
                    );
                }
            }
            self.cull_entity_vertices(out, entity_start);
            if let Some(id) = map {
                // ItemFrameRenderer: the map fills the face, in 90-degree steps.
                let depth = if f.invisible { 0.5 } else { 0.4375 };
                // A glow frame's map is 15728850 = 0xF000D2 (sky coordinate 240,
                // block coordinate 210), not full bright.
                let item_light = if f.glow { 240 | 210 << 8 } else { packed };
                let m = pose
                    * glam::DMat4::from_translation(glam::DVec3::new(0.0, 0.0, depth))
                    * glam::DMat4::from_rotation_z(
                        (f64::from(f.rotation % 4 * 2) * 45.0).to_radians(),
                    )
                    * glam::DMat4::from_rotation_z(std::f64::consts::PI)
                    * glam::DMat4::from_scale(glam::DVec3::splat(0.0078125))
                    * glam::DMat4::from_translation(glam::DVec3::new(-64.0, -64.0, -1.0));
                maps.push((id, m.as_mat4(), item_light));
                continue;
            }
            let Some(stack) = f.item.clone() else {
                continue;
            };
            // `GlowItemFrame` items render full bright (15728880).
            let item_light = if f.glow { 0xFF } else { light };
            let depth = if f.invisible { 0.5 } else { 0.4375 };
            let m = pose
                * glam::DMat4::from_translation(glam::DVec3::new(0.0, 0.0, depth))
                * glam::DMat4::from_rotation_z((f64::from(f.rotation) * 45.0).to_radians())
                * glam::DMat4::from_scale(glam::DVec3::splat(0.5));
            items.push((stack, f.id, m.as_mat4(), item_light));
        }
        // `extractRenderState`: FIXED, seeded by the frame entity.
        for (stack, id, m, light) in items {
            let entity_start = out.len();
            let owner = super::item_draw::owner_at([0.0; 3], 0.0, "minecraft:item_frame");
            let state = self.resolve_item(
                &stack,
                horizon_assets::item_model::DisplayContext::Fixed,
                Some(&owner),
                id as i32,
            );
            self.submit_item_world(
                &state,
                m,
                horizon_client::block_entity_render::vertex_light(light),
                out,
            );
            self.cull_entity_vertices(out, entity_start);
        }
        for (id, m, light) in maps {
            let entity_start = out.len();
            self.map_vertices(id, true, m, light, out);
            self.cull_entity_vertices(out, entity_start);
        }
    }
}
impl crate::game::Sim<'_> {
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
}
