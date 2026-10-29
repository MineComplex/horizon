//! The End: End portal frames filled with eyes of ender, End portal and End
//! gateway travel, the obsidian arrival platform, the exit portal and the End
//! sky. The dragon fight itself lives in game/dragon.rs: the podium is
//! inactive until the dragon dies.
use super::{Dim, Game};
use horizon_client::{
    environment::Environment,
    items::ItemStack,
    mesher::Vertex,
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::random::RandomSource;

#[derive(Default)]
pub(super) struct EndState {
    /// Thrown eyes of ender in flight.
    eyes: Vec<EyeOfEnder>,
    flash: EndFlash,
}

/// EndFlashState: a purple flash in the End sky every 600 ticks that also
/// lights the world (the lightmap's sky factor).
#[derive(Default)]
struct EndFlash {
    intensity: f32,
    old_intensity: f32,
    x_angle: f32,
    y_angle: f32,
}
impl EndFlash {
    fn intensity(&self, partial: f32) -> f32 {
        self.old_intensity + (self.intensity - self.old_intensity) * partial
    }
}

impl EndState {
    /// `handleAddEntity` for an eye of ender on a server's client: it flies
    /// where the server says.
    pub(super) fn add_remote_eye(&mut self, id: u32, at: [f64; 3], velocity: [f64; 3]) {
        self.eyes.retain(|e| e.id != id);
        let at = glam::DVec3::from_array(at);
        self.eyes.push(EyeOfEnder {
            id,
            position: at,
            previous: at,
            velocity: glam::DVec3::from_array(velocity),
            life: 0,
        });
    }
    /// `EyeOfEnder.tick` on a server's client: it moves by its last known
    /// motion; only the server steers it.
    pub(super) fn step_remote_eyes(&mut self) {
        for e in &mut self.eyes {
            e.previous = e.position;
            e.life += 1;
            e.position += e.velocity;
        }
    }
    /// A server's move: an eye has no interpolation handler, so
    /// `moveOrInterpolateTo` puts it there at once. False when `id` isn't one.
    pub(super) fn move_remote_eye(&mut self, id: u32, at: [f64; 3]) -> bool {
        let Some(e) = self.eyes.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        e.position = glam::DVec3::from_array(at);
        true
    }
    /// `handleSetEntityMotion` for an eye. False when `id` isn't one.
    pub(super) fn remote_eye_motion(&mut self, id: u32, velocity: [f64; 3]) -> bool {
        let Some(e) = self.eyes.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        e.velocity = glam::DVec3::from_array(velocity);
        true
    }
    pub(super) fn remove_remote_eye(&mut self, id: u32) {
        self.eyes.retain(|e| e.id != id);
    }
    pub(super) fn clear_remote_eyes(&mut self) {
        self.eyes.clear();
    }
    /// Where each eye is, for `status`.
    pub(super) fn eye_positions(&self) -> impl Iterator<Item = (u32, [f64; 3])> {
        self.eyes.iter().map(|e| (e.id, e.position.to_array()))
    }
}

/// The EyeOfEnder entity.
struct EyeOfEnder {
    /// Its entity id once a server's clients track it (0 until then).
    id: u32,
    position: glam::DVec3,
    previous: glam::DVec3,
    velocity: glam::DVec3,
    life: u32,
}

impl Game<'_> {
    /// SkyRenderer.renderEndFlash, drawn after the renderer's resident sky mesh.
    pub(super) fn end_flash_geometry(&self) -> Vec<Vertex> {
        let mut out = Vec::new();
        // The old combined sky/flash draw also skipped the flash without a sky sprite.
        if !self
            .front
            .gui
            .atlas
            .sprites
            .contains_key("environment/end_sky")
        {
            return out;
        }
        // SkyRenderer.renderEndFlash: the end_flash sprite at height 100, scale 60.
        let intensity = self.end.flash.intensity(self.clock.fraction() as f32);
        if intensity > 1e-5
            && let Some(r) = self
                .front
                .gui
                .atlas
                .sprites
                .get("environment/celestial/end_flash")
                .copied()
        {
            let flash = &self.end.flash;
            let pose = glam::Mat4::from_rotation_y((180.0 - flash.y_angle).to_radians())
                * glam::Mat4::from_rotation_x((-90.0 - flash.x_angle).to_radians());
            let level = (intensity.clamp(0.0, 1.0) * 255.0) as u32;
            let color = level << 24 | level << 16 | level << 8 | level;
            let corners = [
                (-1.0, -1.0, [r.u0, r.v0]),
                (1.0, -1.0, [r.u1, r.v0]),
                (1.0, 1.0, [r.u1, r.v1]),
                (-1.0, 1.0, [r.u0, r.v1]),
            ];
            for (x, z, uv) in corners {
                let p = pose.transform_point3(glam::Vec3::new(x * 60.0, 100.0, z * 60.0));
                out.push(Vertex {
                    normal: [0; 4],
                    pos: p.to_array(),
                    uv,
                    layer: 2,
                    color,
                    light: 0,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
        out
    }

    /// LightmapRenderStateExtractor: End flashes add to the sky light factor
    /// unless Hide Lightning Flashes is on (the sky sprite is not gated), a
    /// third of it under a boss bar's world fog.
    pub(super) fn end_flash_sky(&self, sky: &mut horizon_client::environment::Sky, partial: f64) {
        if self.dim == Dim::End && !self.settings.file.bool("hideLightningFlashes") {
            let intensity = self.end.flash.intensity(partial as f32);
            sky.sky_light_factor += if self.front.boss_overlay.should_create_world_fog() {
                intensity / 3.0
            } else {
                intensity
            };
        }
    }

    /// End crystals (on the spikes, or placed in any dimension) and thrown
    /// eyes of ender, as camera-relative entity geometry (End portals and
    /// gateways are block entities).
    pub(super) fn end_geometry(&self, camera: glam::DVec3, out: &mut Vec<Vertex>) {
        self.end_crystal_geometry(camera, out);
        self.eye_geometry(camera, out);
    }

    /// EndCrystalRenderer and EndCrystalModel for the end crystals, and the
    /// beam to a crystal's target (the spikes being summoned back).
    fn end_crystal_geometry(&self, camera: glam::DVec3, out: &mut Vec<Vertex>) {
        let (Some(model), Some(r)) = (
            self.crystal_model.as_ref(),
            self.front
                .gui
                .atlas
                .sprites
                .get("entity/end_crystal/end_crystal")
                .copied(),
        ) else {
            return;
        };
        let uv = [r.u0, r.v0, r.u1, r.v1];
        let partial = self.clock.fraction() as f32;
        for crystal in &self.crystals {
            let feet = glam::DVec3::new(crystal.position.x, crystal.position.y, crystal.position.z);
            let rel = feet - camera;
            if rel.length_squared() > 256.0 * 256.0 {
                continue;
            }
            let age = crystal.time as f32 + partial;
            let light = self.world.light(
                feet.x.floor() as i32,
                feet.y.floor() as i32,
                feet.z.floor() as i32,
            );
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let poses = horizon_client::end_crystal::poses(model, age, crystal.show_bottom);
            // poseStack.scale(2) then translate(0, -0.5, 0), not flipped.
            let base = glam::Mat4::from_translation(rel.as_vec3())
                * glam::Mat4::from_scale(glam::Vec3::splat(2.0))
                * glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.5, 0.0));
            horizon_client::mobs::render::draw(
                model,
                base,
                &poses,
                uv,
                0xFFFF_FFFF,
                packed,
                0,
                out,
            );
            if let Some(target) = crystal.beam {
                // The beam from the target's centre up to the bobbing crystal.
                let from = glam::DVec3::new(
                    f64::from(target[0]) + 0.5,
                    f64::from(target[1]) + 0.5,
                    f64::from(target[2]) + 0.5,
                );
                let delta = feet - from
                    + glam::DVec3::new(0.0, f64::from(horizon_client::end_crystal::y(age)), 0.0);
                self.crystal_beam(from, delta, age, camera, out);
            }
        }
    }

    /// ThrownItemRenderer: the ender eye item facing the camera.
    fn eye_geometry(&self, camera: glam::DVec3, out: &mut Vec<Vertex>) {
        let Some(r) = self.front.gui.atlas.sprites.get("item/ender_eye").copied() else {
            return;
        };
        let alpha = self.clock.fraction();
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        let right = glam::Vec3::new(-yaw.cos(), 0.0, -yaw.sin()) * 0.25;
        let up = glam::Vec3::new(
            -yaw.sin() * pitch.sin(),
            pitch.cos(),
            yaw.cos() * pitch.sin(),
        ) * 0.25;
        let full = (15 * 16) | ((15 * 16) << 8);
        for eye in &self.end.eyes {
            let p = eye.previous + (eye.position - eye.previous) * alpha - camera;
            let c = p.as_vec3();
            let corners = [
                (c - right - up, [r.u0, r.v1]),
                (c + right - up, [r.u1, r.v1]),
                (c + right + up, [r.u1, r.v0]),
                (c - right + up, [r.u0, r.v0]),
            ];
            for (pos, uv) in corners {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: pos.to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: full,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
    }
}
impl crate::game::Sim<'_> {
    /// The environment attributes of the current dimension.
    pub(super) fn dim_environment(&self) -> &Environment {
        match self.dim {
            Dim::Overworld => self.res.environment(),
            Dim::Nether => self.res.nether_environment(),
            Dim::End => self.res.end_environment(),
        }
    }
    pub(super) fn block_name(&self, p: [i32; 3]) -> std::sync::Arc<str> {
        self.res
            .facts()
            .get(self.world.block(p[0], p[1], p[2]))
            .name
            .clone()
    }
    /// `LevelEventHandler` 2003: a shattered eye's item crumbs and two rings
    /// of portal particles drawn in towards it.
    pub(super) fn eye_break_particles(&mut self, pos: [i32; 3]) {
        let (x, y, z) = (
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        );
        let crumbs = self.item_particle(&ItemStack::new("minecraft:ender_eye", 1));
        for _ in 0..8 {
            let r = self.particles.random();
            let v = [
                r.next_gaussian() * 0.15,
                r.next_double() * 0.2,
                r.next_gaussian() * 0.15,
            ];
            self.particles.add(&crumbs, [x, y, z], v);
        }
        let portal = ParticleOptions::simple(ParticleType::Portal);
        let mut angle = 0.0f64;
        while angle < std::f64::consts::PI * 2.0 {
            let (c, s) = (angle.cos(), angle.sin());
            let at = [x + c * 5.0, y - 0.4, z + s * 5.0];
            self.particles.add(&portal, at, [c * -5.0, 0.0, s * -5.0]);
            self.particles.add(&portal, at, [c * -7.0, 0.0, s * -7.0]);
            angle += std::f64::consts::PI / 20.0;
        }
    }
    /// `EyeOfEnder.tick` on a client: behind each eye (a quarter of its last
    /// move back), four bubbles in water, else a portal particle.
    pub(super) fn eye_particles(&mut self) {
        for i in 0..self.end.eyes.len() {
            let e = &self.end.eyes[i];
            let movement = (e.position - e.previous).to_array();
            let origin = (e.position - (e.position - e.previous) * 0.25).to_array();
            let block = origin.map(|v| v.floor() as i32);
            let water = self
                .res
                .facts()
                .get(self.world.block(block[0], block[1], block[2]))
                .fluid
                == horizon_client::blocks::Fluid::Water;
            if water {
                let bubble = ParticleOptions::simple(ParticleType::Bubble);
                for _ in 0..4 {
                    self.particles.add(&bubble, origin, movement);
                }
            } else {
                let r = self.particles.random();
                let x = origin[0] + r.next_double() * 0.6 - 0.3;
                let z = origin[2] + r.next_double() * 0.6 - 0.3;
                let portal = ParticleOptions::simple(ParticleType::Portal);
                self.particles
                    .add(&portal, [x, origin[1] - 0.5, z], movement);
            }
        }
    }
}
