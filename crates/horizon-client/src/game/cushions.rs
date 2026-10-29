//! Cushions: `CushionItem.useOn` placement on the top face of a block,
//! `Cushion.canBePlacedAt`/`survives`, `Cushion.interact` (sit),
//! `BlockAttachedEntity.hurtServer` (break, dropping the matching colour),
//! `BlockAttachedEntity.tick` and `CushionRenderer`.
//!
//! A cushion is a `BlockAttachedEntity`, not a block: it has no block state,
//! only an entity whose `Pos` is the click point's height. Placement uses
//! outline bounds, collision shapes and suffocation predicates.
use super::{Game, view_vector};
use horizon_client::mobs::anim::Poses;
use horizon_core::geometry::{Aabb, Vec3};

/// `Direction.fromYRot(yaw).toYRot()`: the nearest 90 degree step.
pub(crate) fn snap_yaw(yaw: f32) -> f32 {
    (((f64::from(yaw) / 90.0 + 0.5).floor() as i64) & 3) as f32 * 90.0
}

fn aabb(bounds: ([f64; 3], [f64; 3])) -> Aabb {
    Aabb::new(
        Vec3::new(bounds.0[0], bounds.0[1], bounds.0[2]),
        Vec3::new(bounds.1[0], bounds.1[1], bounds.1[2]),
    )
}

impl Game<'_> {
    /// `CushionRenderer`: the coloured model, rotated to the nearest
    /// direction, with its bottom on the cushion's position.
    pub(super) fn cushion_geometry(
        &self,
        camera: glam::DVec3,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) {
        let Some(model) = self.cushion_model.as_ref() else {
            return;
        };
        let sprites = &self.front.gui.atlas.sprites;
        for c in &self.cushions {
            let entity_start = out.len();
            let a = aabb(c.bounds());
            let (lo, hi) = (a.min_position(), a.max_position());
            let (lo, hi) = (
                glam::DVec3::new(lo.x, lo.y, lo.z),
                glam::DVec3::new(hi.x, hi.y, hi.z),
            );
            let range = super::entity_culling::default_range(hi - lo, self.view_scale());
            if !self.entity_in_view(camera, (lo + hi) / 2.0, range, lo, hi) {
                continue;
            }
            let Some(uv) = sprites
                .get(&format!("entity/cushion/{}_cushion", c.color_name()))
                .map(|r| [r.u0, r.v0, r.u1, r.v1])
            else {
                continue;
            };
            let p = glam::DVec3::new(c.position.x, c.position.y, c.position.z) - camera;
            let base = glam::Mat4::from_translation(p.as_vec3())
                * glam::Mat4::from_rotation_y((180.0 - snap_yaw(c.yaw)).to_radians())
                * glam::Mat4::from_rotation_x(std::f32::consts::PI)
                * glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.25, 0.0));
            // `EntityRenderer.getPackedLightCoords` probes at the eye height.
            let l = self.world.light(
                c.position.x.floor() as i32,
                (c.position.y + 0.2125).floor() as i32,
                c.position.z.floor() as i32,
            );
            let packed = (u32::from(l >> 4) * 16) | ((u32::from(l & 15) * 16) << 8);
            horizon_client::mobs::render::draw(
                model,
                base,
                &Poses::new(model),
                uv,
                0xFFFF_FFFF,
                packed,
                0,
                out,
            );
            self.cull_entity_vertices(out, entity_start);
        }
    }
}
impl crate::game::Sim<'_> {
    /// The cushion under the crosshair, its distance and the hit point.
    pub(super) fn cushion_hit(&self) -> Option<(usize, f64, [f64; 3])> {
        let eye = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        let mut best: Option<(usize, f64, [f64; 3])> = None;
        for (i, c) in self.cushions.iter().enumerate() {
            let Some(d) = super::ray_box(eye, dir, reach, aabb(c.bounds())) else {
                continue;
            };
            if best.is_none_or(|b| d < b.1) {
                best = Some((i, d, std::array::from_fn(|k| eye[k] + dir[k] * d)));
            }
        }
        best
    }
}
