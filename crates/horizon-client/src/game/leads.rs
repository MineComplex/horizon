//! Leads on the game side: putting them on and taking them off mobs
//! (`Entity.interact`'s leash part), fence knots (`LeadItem.useOn` and
//! `LeashFenceKnotEntity.interact`), loading and saving who holds what,
//! breaking, and drawing the rope (`LeashFeatureRenderer`) and the knot.
use super::Game;
use horizon_client::{
    mesher::Vertex,
    mobs::leash::{self, Holder, Pending},
};
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
impl Game<'_> {
    /// `LeashFeatureRenderer` for every lead and `LeashKnotRenderer` for the knots.
    pub(super) fn leash_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let atlas = &self.gui.atlas;
        let white = atlas.font.white();
        let uv = [
            [white.u0, white.v1],
            [white.u1, white.v1],
            [white.u1, white.v0],
            [white.u0, white.v0],
        ];
        let lerp3 = |a: horizon_core::geometry::Vec3, b: horizon_core::geometry::Vec3| {
            glam::DVec3::new(
                a.x + (b.x - a.x) * alpha,
                a.y + (b.y - a.y) * alpha,
                a.z + (b.z - a.z) * alpha,
            )
        };
        let mut knots: Vec<[i32; 3]> = Vec::new();
        for m in &self.mobs {
            let Some(holder) = m.extra.leash else {
                continue;
            };
            let body = f64::from(
                m.previous_body_yaw
                    + horizon_client::mobs::living::wrap(m.body_yaw - m.previous_body_yaw)
                        * alpha as f32,
            )
            .to_radians();
            let offset = leash::leash_offset(m);
            let offset = rot_y(glam::DVec3::from_array(offset), -body);
            let start = lerp3(m.previous, m.position) + offset;
            let end = match holder {
                Holder::Knot(p) => {
                    if !knots.contains(&p) {
                        knots.push(p);
                    }
                    glam::DVec3::new(
                        f64::from(p[0]) + 0.5,
                        f64::from(p[1]) + 0.375 + 0.2,
                        f64::from(p[2]) + 0.5,
                    )
                }
                Holder::Mob(id) => {
                    let Some(o) = self.mobs.iter().find(|o| o.id == id) else {
                        continue;
                    };
                    lerp3(o.previous, o.position) + glam::DVec3::new(0.0, o.eye_height() * 0.7, 0.0)
                }
                Holder::Player => {
                    let Some(p) = self.player.as_ref() else {
                        continue;
                    };
                    self.player_rope_hold(p, alpha)
                }
            };
            let light = |p: glam::DVec3| {
                self.world
                    .light(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
            };
            // Block and sky light averaged between the two ends.
            let (a, b) = (light(start), light(end));
            let packed = pack_light((((a >> 4) + (b >> 4)) / 2) << 4 | (((a & 15) + (b & 15)) / 2));
            let d = end - start;
            let k = 1.0 / (d.x * d.x + d.z * d.z).sqrt().max(1e-6) * 0.05 / 2.0;
            let (dx_off, dz_off) = (d.z * k, d.x * k);
            let point = |step: i32, fudge: f64| {
                let progress = f64::from(step) / 24.0;
                let y = if d.y > 0.0 {
                    d.y * progress * progress
                } else {
                    d.y - d.y * (1.0 - progress) * (1.0 - progress)
                };
                let base = start + glam::DVec3::new(d.x * progress, y, d.z * progress);
                (
                    base + glam::DVec3::new(-dx_off, fudge, dz_off),
                    base + glam::DVec3::new(dx_off, 0.05 - fudge, -dz_off),
                )
            };
            for (fudge, backwards) in [(0.05, false), (0.0, true)] {
                for step in 0..24 {
                    let shade = |s: i32| {
                        let dark = s % 2 == i32::from(backwards);
                        let f = if dark { 0.7 } else { 1.0 };
                        abgr(0.5 * f, 0.4 * f, 0.3 * f)
                    };
                    let (a0, b0) = point(step, fudge);
                    let (a1, b1) = point(step + 1, fudge);
                    let color = shade(step);
                    // Both faces: the leash is not culled.
                    for quad in [[a0, b0, b1, a1], [a1, b1, b0, a0]] {
                        for (v, uv) in quad.iter().zip(uv) {
                            out.push(Vertex {
                                normal: [0; 4],
                                pos: (*v - camera).as_vec3().to_array(),
                                uv,
                                layer: 0,
                                color,
                                light: packed,

                                glint_uv: [0.0, 0.0],
                            });
                        }
                    }
                }
            }
        }
        // The knots: a 6 by 8 by 6 pixel block on the fence post.
        static KNOT: std::sync::OnceLock<Option<horizon_client::mobs::Part>> =
            std::sync::OnceLock::new();
        let model = KNOT.get_or_init(|| {
            horizon_client::mobs::load_model(&self.options.resources, "minecraft:leash_knot#main")
        });
        let Some(model) = model else { return };
        let Some(r) = atlas.sprites.get("entity/lead_knot/lead_knot").copied() else {
            return;
        };
        for p in knots {
            let state = leash::knot_state(p);
            let at =
                glam::DVec3::new(state.position.x, state.position.y, state.position.z) - camera;
            let base = glam::Mat4::from_translation(at.as_vec3())
                * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0));
            let poses = horizon_client::mobs::anim::Poses::new(model);
            let light = pack_light(self.world.light(p[0], p[1], p[2]));
            horizon_client::mobs::render::draw(
                model,
                base,
                &poses,
                [r.u0, r.v0, r.u1, r.v1],
                0xFFFF_FFFF,
                light,
                0,
                out,
            );
        }
    }
}
impl crate::game::Game<'_> {
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
    /// `Player.getRopeHoldPosition` (and `LocalPlayer`'s in first person).
    fn player_rope_hold(
        &self,
        p: &horizon_client::motion::PlayerMotion,
        alpha: f64,
    ) -> glam::DVec3 {
        let pos = glam::DVec3::new(p.position.x, p.position.y, p.position.z);
        let _ = alpha;
        if self.camera_type.first_person() {
            let (yaw, pitch) = (
                f64::from(self.yaw.to_radians()),
                f64::from(self.pitch.to_radians()),
            );
            let v = glam::DVec3::new(-0.39, -0.6, 0.3);
            // Vec3.xRot(-pitch) then yRot(-yaw).
            let (s, c) = ((-pitch).sin(), (-pitch).cos());
            let v = glam::DVec3::new(v.x, v.y * c + v.z * s, v.z * c - v.y * s);
            let eye = pos + glam::DVec3::new(0.0, f64::from(p.dimensions.eye_height), 0.0);
            return rot_y(v, -yaw) + eye;
        }
        let yaw = f64::from(self.yaw.to_radians());
        let crouching = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
        let v = glam::DVec3::new(
            -0.22,
            f64::from(p.dimensions.height) - 1.0,
            if crouching { -0.2 } else { 0.07 },
        );
        pos + rot_y(v, -yaw)
    }
}
