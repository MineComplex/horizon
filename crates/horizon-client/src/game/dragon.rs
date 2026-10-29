//! The ender dragon as the server sends it (`EnderDragon` on a client): its
//! phase, flight history and parts, the crystals' beams, fireballs, the
//! death rays, the boss bar and its rendering. The fight is the server's.
use super::Game;
use horizon_client::{
    hud::HudBatch,
    hud_layout, hud_overlays,
    mesher::Vertex,
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::{LegacyRandom, RandomSource},
};
use horizon_protocol::entity_data::Value;

/// Mob ids from here on are dragon parts (`SPECIAL + part`) and crystals
/// (`SPECIAL + 16 + crystal`), so projectiles can target them.
pub(super) const SPECIAL: u32 = u32::MAX - 4096;
const CRYSTAL_IDS: u32 = SPECIAL + 16;
const MAX_HEALTH: f32 = 200.0;

#[derive(Clone, Debug)]
pub(super) struct Crystal {
    /// The entity id a server tracks it by (0 until it is tracked).
    pub id: u32,
    pub position: Vec3,
    pub show_bottom: bool,
    /// EndCrystal.time (the bob and spin), starting random.
    pub time: i32,
    pub beam: Option<[i32; 3]>,
}
/// `EnderDragonPhase`, as the server's `DATA_PHASE` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Holding,
    Strafe,
    LandingApproach,
    Landing,
    Takeoff,
    Flaming,
    Scanning,
    Attacking,
    Charging,
    Dying,
    Hover,
}
impl Phase {
    /// `EnderDragonPhase` ids, in order.
    const ALL: [Self; 11] = [
        Self::Holding,
        Self::Strafe,
        Self::LandingApproach,
        Self::Landing,
        Self::Takeoff,
        Self::Flaming,
        Self::Scanning,
        Self::Attacking,
        Self::Charging,
        Self::Dying,
        Self::Hover,
    ];
    fn id(self) -> i32 {
        self as i32
    }
    fn from_id(id: i32) -> Self {
        usize::try_from(id)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or(Self::Holding)
    }
    fn sitting(self) -> bool {
        matches!(
            self,
            Self::Scanning | Self::Attacking | Self::Flaming | Self::Hover
        )
    }
}
pub(super) struct Dragon {
    /// `Entity.id` once tracked to guests (0 before); its parts take the
    /// eight ids after it (`EnderDragon`'s constructor reserves them).
    pub id: u32,
    pub position: Vec3,
    pub previous: Vec3,
    velocity: Vec3,
    pub yaw: f32,
    pub previous_yaw: f32,
    yaw_accel: f32,
    pub health: f32,
    pub hurt_time: i32,
    pub death_time: i32,
    pub flap: f32,
    pub old_flap: f32,
    phase: Phase,
    flame_count: i32,
    growl: i32,
    ticks: i32,
    /// DragonFlightHistory: (y, yRot) samples, newest at `head`.
    history: [(f64, f32); 64],
    head: usize,
    /// head, neck, body, tail1-3, wing1-2.
    pub parts: [(Vec3, Vec3); 8],
    nearest_crystal: Option<usize>,
    random: LegacyRandom,
}
const PART_SIZES: [(f64, f64); 8] = [
    (1.0, 1.0),
    (3.0, 3.0),
    (5.0, 3.0),
    (2.0, 2.0),
    (2.0, 2.0),
    (2.0, 2.0),
    (4.0, 2.0),
    (4.0, 2.0),
];
impl Dragon {
    fn new(position: Vec3, yaw: f32, seed: i64) -> Self {
        Self {
            id: 0,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw,
            previous_yaw: yaw,
            yaw_accel: 0.0,
            health: MAX_HEALTH,
            hurt_time: 0,
            death_time: 0,
            flap: 0.0,
            old_flap: 0.0,
            phase: Phase::Holding,
            flame_count: 0,
            growl: 100,
            ticks: 0,
            history: [(position.y, yaw); 64],
            head: 0,
            parts: [(position, position); 8],
            nearest_crystal: None,
            random: LegacyRandom::new(seed),
        }
    }
    /// DragonFlightHistory.record.
    fn record(&mut self) {
        self.head = (self.head + 1) & 63;
        self.history[self.head] = (self.position.y, self.yaw);
    }
    /// DragonFlightHistory.get(delay).
    fn sample(&self, delay: usize) -> (f64, f32) {
        self.history[(self.head + 64 - (delay & 63)) & 63]
    }
    /// DragonFlightHistory.get(delay, partial).
    fn sample_lerp(&self, delay: usize, partial: f32) -> (f64, f32) {
        let a = self.sample(delay);
        let b = self.sample(delay + 1);
        let yaw = b.1 + wrap(a.1 - b.1) * partial;
        (b.0 + (a.0 - b.0) * f64::from(partial), yaw)
    }
    pub fn alive(&self) -> bool {
        self.health > 0.0
    }
    fn part_box(&self, i: usize) -> Aabb {
        let (w, h) = PART_SIZES[i];
        let p = self.parts[i].0;
        Aabb::new(
            Vec3::new(p.x - w / 2.0, p.y, p.z - w / 2.0),
            Vec3::new(p.x + w / 2.0, p.y + h, p.z + w / 2.0),
        )
    }
    fn set_phase(&mut self, phase: Phase) {
        if self.phase.id() == phase.id() && phase.id() != 1 {
            return;
        }
        if phase == Phase::Flaming {
            self.flame_count += 1;
        }
        self.phase = phase;
    }
}
fn wrap(d: f32) -> f32 {
    let r = d.rem_euclid(360.0);
    if r >= 180.0 { r - 360.0 } else { r }
}
impl Game<'_> {
    /// `BossHealthOverlay.extractRenderState`: whatever `publish_boss_bars`
    /// (or a server's packets) put on this client's overlay.
    pub(super) fn draw_boss_bars(&self, batch: &mut HudBatch) {
        let bars = self
            .front
            .boss_overlay
            .bars(horizon_client::boss_overlay::now_millis());
        let draws = hud_overlays::boss_overlay(
            batch.width as i32,
            batch.height as i32,
            &bars,
            &self.front.gui.atlas.font,
        );
        hud_layout::paint(batch, &self.front.gui.atlas, &draws);
    }
    /// `BossHealthOverlay.shouldPlayMusic`.
    pub(super) fn boss_music(&self) -> bool {
        self.front.boss_overlay.should_play_music()
    }
}
impl crate::game::Sim<'_> {
    /// Targets for projectiles: dragon parts and crystals.
    pub(super) fn special_targets(&self) -> Vec<(u32, Aabb)> {
        let mut out = Vec::new();
        if let Some(d) = self.dragon.as_ref().filter(|d| d.alive()) {
            for i in 0..8 {
                out.push((SPECIAL + i as u32, d.part_box(i)));
            }
        }
        for (i, c) in self.crystals.iter().enumerate() {
            let p = c.position;
            out.push((
                CRYSTAL_IDS + i as u32,
                Aabb::new(
                    Vec3::new(p.x - 1.0, p.y, p.z - 1.0),
                    Vec3::new(p.x + 1.0, p.y + 2.0, p.z + 1.0),
                ),
            ));
        }
        out
    }
    /// Player melee on the dragon's parts or a crystal under the crosshair.
    /// The dragon part under the crosshair within reach: its id and distance.
    pub(super) fn dragon_hit(&self) -> Option<(u32, f64)> {
        let eye = self.eye_position()?;
        let dir = super::view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        let mut best: Option<(u32, f64)> = None;
        for (id, b) in self.special_targets() {
            let (lo, hi) = (b.min_position(), b.max_position());
            let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
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
                best = Some((id, t0));
            }
        }
        best
    }
    /// `getHeightmapPos(MOTION_BLOCKING_NO_LEAVES)`.
    fn motion_blocking_no_leaves(&self, x: i32, z: i32) -> i32 {
        (0..256)
            .rev()
            .find(|&y| {
                let f = self.res.facts().get(self.world.block(x, y, z));
                (f.full_collision
                    || f.legacy_solid
                    || f.fluid != horizon_client::blocks::Fluid::None)
                    && !f.name.ends_with("_leaves")
            })
            .map_or(0, |y| y + 1)
    }
    fn podium_top(&self) -> Vec3 {
        Vec3::new(0.5, f64::from(self.motion_blocking_no_leaves(0, 0)), 0.5)
    }
    /// The part positions from the yaw and the flight history.
    fn place_dragon_parts(&mut self) {
        let Some(d) = self.dragon.as_mut() else {
            return;
        };
        let tilt =
            ((d.sample(5).0 - d.sample(10).0) * 10.0) as f32 * (std::f32::consts::PI / 180.0);
        let (ct, st) = (tilt.cos(), tilt.sin());
        let r1 = d.yaw.to_radians();
        let (s1, c1) = (r1.sin(), r1.cos());
        let p = d.position;
        let set = |d: &mut Dragon, i: usize, x: f32, y: f64, z: f32| {
            d.parts[i].0 = Vec3::new(p.x + f64::from(x), p.y + y, p.z + f64::from(z));
        };
        set(d, 2, s1 * 0.5, 0.0, -c1 * 0.5);
        set(d, 6, c1 * 4.5, 2.0, s1 * 4.5);
        set(d, 7, c1 * -4.5, 2.0, s1 * -4.5);
        let r2 = d.yaw.to_radians() - d.yaw_accel * 0.01;
        let (s2, c2) = (r2.sin(), r2.cos());
        let y_off = if d.phase.sitting() {
            -1.0
        } else {
            d.sample(5).0 - d.sample(0).0
        } as f32;
        set(
            d,
            0,
            s2 * 6.5 * ct,
            f64::from(y_off + st * 6.5),
            -c2 * 6.5 * ct,
        );
        set(
            d,
            1,
            s2 * 5.5 * ct,
            f64::from(y_off + st * 5.5),
            -c2 * 5.5 * ct,
        );
        let p1 = d.sample(5);
        for i in 0..3 {
            let p0 = d.sample(12 + i * 2);
            let rot = d.yaw.to_radians() + wrap(p0.1 - p1.1).to_radians();
            let (ss, cc) = (rot.sin(), rot.cos());
            let dd = (i as f32 + 1.0) * 2.0;
            let x = -(s1 * 1.5 + ss * dd) * ct;
            let y = p0.0 - p1.0 - f64::from((dd + 1.5) * st) + 1.5;
            let z = (c1 * 1.5 + cc * dd) * ct;
            set(d, 3 + i, x, y, z);
        }
    }
}
pub(super) struct DragonFireball {
    /// Its entity id once a server's players track it (0 until then).
    pub(super) id: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
    pub(super) velocity: Vec3,
    age: i32,
}
impl DragonFireball {
    /// One a server sent (`handleAddEntity`).
    pub(super) fn remote(id: u32, at: Vec3, velocity: Vec3) -> Self {
        Self {
            id,
            position: at,
            previous: at,
            velocity,
            age: 0,
        }
    }
    /// `AbstractHurtingProjectile.tick` on a server's client: `applyInertia`
    /// first (power 0.1, inertia 0.95 or 0.8 in water), then the move; only
    /// the server decides what it hits.
    pub(super) fn remote_step(&mut self, water: bool) {
        self.previous = self.position;
        self.age += 1;
        let v = self.velocity;
        // Vec3.normalize: a vector shorter than 1e-5 stays zero.
        let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
        let n = if l < 1.0e-5 {
            Vec3::new(0.0, 0.0, 0.0)
        } else {
            Vec3::new(v.x / l, v.y / l, v.z / l)
        };
        let inertia = f64::from(if water { 0.8f32 } else { 0.95f32 });
        let v = Vec3::new(
            (v.x + n.x * 0.1) * inertia,
            (v.y + n.y * 0.1) * inertia,
            (v.z + n.z * 0.1) * inertia,
        );
        self.velocity = v;
        self.position = Vec3::new(
            self.position.x + v.x,
            self.position.y + v.y,
            self.position.z + v.z,
        );
    }
}
impl Game<'_> {
    /// `EnderDragonRenderer` and `EnderDragonModel.setupAnim`.
    pub(super) fn dragon_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
        additive: &mut Vec<Vertex>,
    ) {
        let (Some(d), Some(model)) = (self.dragon.as_ref(), self.dragon_model.as_ref()) else {
            return;
        };
        let a = alpha as f32;
        let sprite = |n: &str| {
            self.front
                .gui
                .atlas
                .sprites
                .get(n)
                .map(|r| [r.u0, r.v0, r.u1, r.v1])
        };
        let (Some(skin), Some(eyes)) = (
            sprite("entity/enderdragon/dragon"),
            sprite("entity/enderdragon/dragon_eyes"),
        ) else {
            return;
        };
        let pos = glam::DVec3::new(
            d.previous.x + (d.position.x - d.previous.x) * alpha,
            d.previous.y + (d.position.y - d.previous.y) * alpha,
            d.previous.z + (d.position.z - d.previous.z) * alpha,
        );
        let h = |delay: usize| d.sample_lerp(delay, a);
        let yr = h(7).1;
        let rot2 = (h(5).0 - h(10).0) as f32;
        let mut base = glam::Mat4::from_translation((pos - camera).as_vec3())
            * glam::Mat4::from_rotation_y((-yr).to_radians())
            * glam::Mat4::from_rotation_x((rot2 * 10.0).to_radians())
            * glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.0, 1.0))
            * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0))
            * glam::Mat4::from_translation(glam::Vec3::new(0.0, -1.501, 0.0));
        let rays_base = base * glam::Mat4::from_translation(glam::Vec3::new(0.0, -1.0, -2.0));
        // setupAnim.
        let flap = (d.old_flap + (d.flap - d.old_flap) * a) * std::f32::consts::TAU;
        let mut bounce = (flap - 1.0).sin() + 1.0;
        bounce = (bounce * bounce + bounce * 2.0) * 0.05;
        base *=
            glam::Mat4::from_translation(glam::Vec3::new(0.0, (bounce - 2.0) * 16.0, -48.0) / 16.0)
                * glam::Mat4::from_rotation_x((bounce * 2.0).to_radians());
        let mut poses = horizon_client::mobs::anim::Poses::new(model);
        poses.get("jaw")[3] = (flap.sin() + 1.0) * 0.2;
        let sitting = d.phase.sitting();
        let landing = matches!(d.phase, Phase::Landing | Phase::Takeoff);
        let egg_dist = {
            let p = self.podium_top();
            ((p.x - d.position.x).powi(2)
                + (p.y - d.position.y).powi(2)
                + (p.z - d.position.z).powi(2))
            .sqrt()
        };
        let head_offset = |part: usize, start: (f64, f32), point: (f64, f32)| -> f32 {
            if landing {
                part as f32 / (egg_dist / 4.0).max(1.0) as f32
            } else if sitting {
                part as f32
            } else if part == 6 {
                0.0
            } else {
                (point.0 - start.0) as f32
            }
        };
        let start = h(6);
        let rot2y = wrap(h(5).1 - h(10).1);
        let rot = wrap(h(5).1 + rot2y / 2.0);
        let deg = std::f32::consts::PI / 180.0;
        let (mut xx, mut yy, mut zz) = (0.0f32, 20.0f32, -12.0f32);
        for i in 0..5 {
            let point = h(5 - i);
            let pose = poses.get(&format!("neck{i}"));
            let neck_x = (i as f32 * 0.45 + flap).cos() * 0.15;
            pose[4] = wrap(point.1 - start.1) * deg * 1.5;
            pose[3] = neck_x + head_offset(i, start, point) * deg * 1.5 * 5.0;
            pose[5] = -wrap(point.1 - rot) * deg * 1.5;
            pose[0] = xx;
            pose[1] = yy;
            pose[2] = zz;
            let (xr, yrr) = (pose[3], pose[4]);
            xx -= yrr.sin() * xr.cos() * 10.0;
            yy += xr.sin() * 10.0;
            zz -= yrr.cos() * xr.cos() * 10.0;
        }
        let current = h(0);
        let head = poses.get("head");
        head[0] = xx;
        head[1] = yy;
        head[2] = zz;
        head[4] = wrap(current.1 - start.1) * deg;
        head[3] = wrap(head_offset(6, start, current)) * deg * 1.5 * 5.0;
        head[5] = -wrap(current.1 - rot) * deg;
        poses.get("body")[5] = -rot2y * 1.5 * deg;
        let lw = [0.125 - flap.cos() * 0.2, -0.25, -(flap.sin() + 0.125) * 0.8];
        let lt = ((flap + 2.0).sin() + 0.5) * 0.75;
        {
            let p = poses.get("left_wing");
            (p[3], p[4], p[5]) = (lw[0], lw[1], lw[2]);
        }
        poses.get("left_wing_tip")[5] = lt;
        {
            let p = poses.get("right_wing");
            (p[3], p[4], p[5]) = (lw[0], -lw[1], -lw[2]);
        }
        poses.get("right_wing_tip")[5] = -lt;
        for side in ["left", "right"] {
            poses.get(&format!("{side}_hind_leg"))[3] = 1.0 + bounce * 0.1;
            poses.get(&format!("{side}_hind_leg_tip"))[3] = 0.5 + bounce * 0.1;
            poses.get(&format!("{side}_hind_foot"))[3] = 0.75 + bounce * 0.1;
            poses.get(&format!("{side}_front_leg"))[3] = 1.3 + bounce * 0.1;
            poses.get(&format!("{side}_front_leg_tip"))[3] = -0.5 - bounce * 0.1;
            poses.get(&format!("{side}_front_foot"))[3] = 0.75 + bounce * 0.1;
        }
        let mut tail_x = 0.0f32;
        let (mut xx, mut yy, mut zz) = (0.0f32, 10.0f32, 60.0f32);
        let start = h(11);
        for i in 0..12 {
            let point = h(12 + i);
            tail_x += (i as f32 * 0.45 + flap).sin() * 0.05;
            let p = poses.get(&format!("tail{i}"));
            p[4] = (wrap(point.1 - start.1) * 1.5 + 180.0) * deg;
            p[3] = tail_x + (point.0 - start.0) as f32 * deg * 1.5 * 5.0;
            p[5] = wrap(point.1 - rot) * deg * 1.5;
            p[0] = xx;
            p[1] = yy;
            p[2] = zz;
            let (xr, yrr) = (p[3], p[4]);
            yy += xr.sin() * 10.0;
            zz -= yrr.cos() * xr.cos() * 10.0;
            xx -= yrr.sin() * xr.cos() * 10.0;
        }
        let l = self.world.light(
            d.position.x.floor() as i32,
            d.position.y.floor() as i32,
            d.position.z.floor() as i32,
        );
        let light = (u32::from(l >> 4) * 16) | ((u32::from(l & 15) * 16) << 8);
        let death = if d.death_time > 0 {
            d.death_time as f32 + a
        } else {
            0.0
        };
        // The dissolve is approximated by fading out.
        let color = if death > 0.0 {
            let alpha = ((1.0 - death / 200.0) * 255.0).clamp(0.0, 255.0) as u32;
            alpha << 24 | 0x00FF_FFFF
        } else {
            0xFFFF_FFFF
        };
        let overlay = u32::from(d.hurt_time > 0);
        horizon_client::mobs::render::draw(model, base, &poses, skin, color, light, overlay, out);
        // RenderTypes.eyes: the glowing eyes, added on top.
        let mut eye_quads = Vec::new();
        horizon_client::mobs::render::draw(
            model,
            base,
            &poses,
            eyes,
            0xFFFF_FFFF,
            240 | 240 << 8,
            0,
            &mut eye_quads,
        );
        for v in &mut eye_quads {
            v.layer = 2;
        }
        additive.extend(eye_quads);
        if death > 0.0 {
            self.dragon_rays(rays_base, death / 200.0, additive);
        }
        // The healing beam to the nearest crystal.
        if let Some(c) = d.nearest_crystal.and_then(|i| self.crystals.get(i)) {
            let bob = f64::from(horizon_client::end_crystal::y(c.time as f32 + a));
            let to = glam::DVec3::new(c.position.x, c.position.y + bob, c.position.z);
            self.crystal_beam(pos, to - pos, self.ticks as f32 + a, camera, out);
        }
    }
    /// `submitRays`: the death rays from a fixed seed, fading in white to magenta.
    fn dragon_rays(&self, base: glam::Mat4, t: f32, out: &mut Vec<Vertex>) {
        let white = self.front.gui.atlas.font.white();
        let uv = [white.u0, white.v0];
        let over = if t > 0.8 {
            ((t - 0.8) / 0.2).min(1.0)
        } else {
            0.0
        };
        let inner = (((1.0 - over) * 255.0) as u32) << 24 | 0x00FF_FFFF;
        let outer = 0x00FF_00FF;
        let mut random = LegacyRandom::new(432);
        let mut m = base;
        let count = ((t + t * t) / 2.0 * 60.0).floor() as i32;
        let half_sqrt3 = 3f32.sqrt() / 2.0;
        let tau = std::f32::consts::TAU;
        for _ in 0..count {
            let mut f = || random.next_float() * tau;
            let q1 = glam::Quat::from_rotation_x(f())
                * glam::Quat::from_rotation_y(f())
                * glam::Quat::from_rotation_z(f());
            let (a, b, c) = (f(), f(), f());
            let q2 = glam::Quat::from_rotation_x(a)
                * glam::Quat::from_rotation_y(b)
                * glam::Quat::from_rotation_z(c + t * std::f32::consts::FRAC_PI_2);
            m *= glam::Mat4::from_quat(q1 * q2);
            let length = random.next_float() * 20.0 + 5.0 + over * 10.0;
            let width = random.next_float() * 2.0 + 1.0 + over * 2.0;
            let o = glam::Vec3::ZERO;
            let left = glam::Vec3::new(-half_sqrt3 * width, length, -0.5 * width);
            let right = glam::Vec3::new(half_sqrt3 * width, length, -0.5 * width);
            let bottom = glam::Vec3::new(0.0, length, width);
            for (p1, p2) in [(left, right), (right, bottom), (bottom, left)] {
                // A triangle as a quad with its last corner repeated.
                for (p, c) in [(o, inner), (p1, outer), (p2, outer), (p2, outer)] {
                    out.push(Vertex {
                        normal: [0; 4],
                        pos: m.transform_point3(p).to_array(),
                        uv,
                        layer: 2,
                        color: c,
                        light: 0,

                        glint_uv: [0.0, 0.0],
                    });
                }
            }
        }
    }
    /// `submitCrystalBeams`: an eight-sided tube from `from` (+2 up) along
    /// `delta`, the beam texture scrolling along it.
    pub(super) fn crystal_beam(
        &self,
        from: glam::DVec3,
        delta: glam::DVec3,
        time: f32,
        camera: glam::DVec3,
        out: &mut Vec<Vertex>,
    ) {
        let Some(s) = self
            .front
            .gui
            .atlas
            .sprites
            .get("entity/end_crystal/end_crystal_beam")
            .copied()
        else {
            return;
        };
        let q = |v: f32| (v * 65535.0).round().clamp(0.0, 65535.0) as u32;
        let wrap_light = q(s.v0) | q(s.v1 - s.v0) << 16;
        let (dx, dy, dz) = (delta.x as f32, delta.y as f32, delta.z as f32);
        let horizontal = (dx * dx + dz * dz).sqrt();
        let length = (dx * dx + dy * dy + dz * dz).sqrt();
        let m = glam::Mat4::from_translation(
            (from - camera).as_vec3() + glam::Vec3::new(0.0, 2.0, 0.0),
        ) * glam::Mat4::from_rotation_y(-dz.atan2(dx) - std::f32::consts::FRAC_PI_2)
            * glam::Mat4::from_rotation_x(-horizontal.atan2(dy) - std::f32::consts::FRAC_PI_2);
        let (v0, v1) = (-time * 0.01, length / 32.0 - time * 0.01);
        let (mut last_sin, mut last_cos, mut last_u) = (0.0f32, 0.75f32, 0.0f32);
        let u = |x: f32| s.u0 + (s.u1 - s.u0) * x;
        for i in 1..=8 {
            let a = i as f32 * std::f32::consts::TAU / 8.0;
            let (sin, cos) = (a.sin() * 0.75, a.cos() * 0.75);
            let uu = i as f32 / 8.0;
            for (p, uv, c) in [
                (
                    glam::Vec3::new(last_sin * 0.2, last_cos * 0.2, 0.0),
                    [u(last_u), v0],
                    0xFF00_0000,
                ),
                (
                    glam::Vec3::new(last_sin, last_cos, length),
                    [u(last_u), v1],
                    0xFFFF_FFFF,
                ),
                (glam::Vec3::new(sin, cos, length), [u(uu), v1], 0xFFFF_FFFF),
                (
                    glam::Vec3::new(sin * 0.2, cos * 0.2, 0.0),
                    [u(uu), v0],
                    0xFF00_0000,
                ),
            ] {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: m.transform_point3(p).to_array(),
                    uv,
                    layer: 6,
                    color: c,
                    light: wrap_light,

                    glint_uv: [0.0, 0.0],
                });
            }
            (last_sin, last_cos, last_u) = (sin, cos, uu);
        }
    }
    /// Dragon fireballs as the dragon_fireball sprite facing the camera.
    pub(super) fn dragon_fireball_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        let Some(s) = self
            .front
            .gui
            .atlas
            .sprites
            .get("entity/enderdragon/dragon_fireball")
            .copied()
        else {
            return;
        };
        let forward = glam::DVec3::from_array(super::view_vector(self.yaw, self.pitch));
        let right = forward.cross(glam::DVec3::Y).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        for b in &self.dragon_fireballs {
            let p = glam::DVec3::new(
                b.previous.x + (b.position.x - b.previous.x) * alpha,
                b.previous.y + (b.position.y - b.previous.y) * alpha,
                b.previous.z + (b.position.z - b.previous.z) * alpha,
            ) - camera;
            // DragonFireballRenderer: scale 2, full bright.
            let h = 1.0;
            for (c, uv) in [
                (p - right * h - up * h, [s.u0, s.v1]),
                (p + right * h - up * h, [s.u1, s.v1]),
                (p + right * h + up * h, [s.u1, s.v0]),
                (p - right * h + up * h, [s.u0, s.v0]),
            ] {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: c.as_vec3().to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: 240 | 240 << 8,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
    }
}
/// Saving and loading the dragon and the crystals as chunk entities.
/// Respawning the dragon with end crystals on the exit portal
/// (`EndCrystalItem.useOn`, `EnderDragonFight.tryRespawn`, `DragonRespawnStage`).
impl crate::game::Sim<'_> {
    /// `EnderDragon.checkCrystals`' search: the nearest crystal whose box
    /// meets the dragon's box grown by 32.
    fn nearest_crystal_to(&self, p: Vec3) -> Option<usize> {
        // The dragon's 16 by 8 box.
        let reach = Aabb::new(
            Vec3::new(p.x - 8.0, p.y, p.z - 8.0),
            Vec3::new(p.x + 8.0, p.y + 8.0, p.z + 8.0),
        )
        .inflate(Vec3::new(32.0, 32.0, 32.0));
        self.crystals
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                // An end crystal's 2 by 2 box.
                reach.intersects(Aabb::new(
                    Vec3::new(c.position.x - 1.0, c.position.y, c.position.z - 1.0),
                    Vec3::new(c.position.x + 1.0, c.position.y + 2.0, c.position.z + 1.0),
                ))
            })
            .min_by(|a, b| {
                let da = (a.1.position.x - p.x).powi(2)
                    + (a.1.position.y - p.y).powi(2)
                    + (a.1.position.z - p.z).powi(2);
                let db = (b.1.position.x - p.x).powi(2)
                    + (b.1.position.y - p.y).powi(2)
                    + (b.1.position.z - p.z).powi(2);
                da.total_cmp(&db)
            })
            .map(|(i, _)| i)
    }
}

/// The dragon on a server's client (`ClientPacketListener.handleAddEntity`
/// and the client side of `EnderDragon.aiStep`): the server flies it and
/// sends its moves, health and phase; the client flaps its wings, keeps
/// its flight history and places its parts, so it can be drawn and hit.
impl crate::game::Sim<'_> {
    /// `handleAddEntity` for `minecraft:ender_dragon`. Its constructor
    /// sets the full health and the hovering phase until the server's
    /// synched data says otherwise.
    pub(super) fn spawn_remote_dragon(&mut self, id: i32, position: [f64; 3], yaw: f32) {
        let at = Vec3::new(position[0], position[1], position[2]);
        let mut d = Dragon::new(at, yaw, i64::from(id));
        d.id = id as u32;
        d.phase = Phase::from_id(10);
        self.dragon = Some(d);
        self.place_dragon_parts();
        if let Some(d) = self.dragon.as_mut() {
            for part in &mut d.parts {
                part.1 = part.0;
            }
        }
    }
    /// The remote dragon, if `id` is it.
    fn remote_dragon_mut(&mut self, id: i32) -> Option<&mut Dragon> {
        self.dragon.as_mut().filter(|d| d.id as i32 == id)
    }
    /// `handleSetEntityData`: LivingEntity.DATA_HEALTH_ID and
    /// EnderDragon.DATA_PHASE (`onSyncedDataUpdated` sets the phase).
    pub(super) fn remote_dragon_data(&mut self, id: i32, values: &[(u8, i32, Value)]) {
        let Some(d) = self.remote_dragon_mut(id) else {
            return;
        };
        for (index, _, value) in values {
            match (index, value) {
                (9, Value::Float(health)) => d.health = *health,
                (16, Value::Int(phase)) => d.set_phase(Phase::from_id(*phase)),
                _ => {}
            }
        }
    }
    /// `handleSetEntityMotion`: the velocity its wings flap by.
    pub(super) fn remote_dragon_motion(&mut self, id: i32, velocity: [f64; 3]) {
        if let Some(d) = self.remote_dragon_mut(id) {
            d.velocity = Vec3::new(velocity[0], velocity[1], velocity[2]);
        }
    }
    /// `handleHurtAnimation` and `handleDamageEvent`: the red flash.
    pub(super) fn remote_dragon_hurt(&mut self, id: i32) {
        if let Some(d) = self.remote_dragon_mut(id) {
            d.hurt_time = 10;
        }
    }
    /// `handleRemoveEntities`, or a crystal removed: the healing beam
    /// keeps pointing at the same crystal.
    pub(super) fn remove_remote_dragon(&mut self, id: i32) {
        if self.remote_dragon_mut(id).is_some() {
            self.dragon = None;
            return;
        }
        let Some(index) = self.crystals.iter().position(|c| c.id as i32 == id) else {
            return;
        };
        if let Some(d) = self.dragon.as_mut() {
            match d.nearest_crystal {
                Some(n) if n == index => d.nearest_crystal = None,
                Some(n) if n > index => d.nearest_crystal = Some(n - 1),
                _ => {}
            }
        }
    }
    /// One client tick of the dragon. `lerp`: the step toward its last
    /// sent position and yaw (LivingEntity's interpolation), and the
    /// steps left including this one.
    pub(super) fn tick_remote_dragon(&mut self, lerp: Option<([f64; 3], Option<f32>, u32)>) {
        let Some(d) = self.dragon.as_mut() else {
            return;
        };
        d.previous = d.position;
        d.previous_yaw = d.yaw;
        d.old_flap = d.flap;
        d.ticks += 1;
        d.hurt_time = (d.hurt_time - 1).max(0);
        for part in &mut d.parts {
            part.1 = part.0;
        }
        if let Some((target, yaw, steps)) = lerp {
            let n = f64::from(steps.max(1));
            d.position = Vec3::new(
                d.position.x + (target[0] - d.position.x) / n,
                d.position.y + (target[1] - d.position.y) / n,
                d.position.z + (target[2] - d.position.z) / n,
            );
            if let Some(yaw) = yaw {
                d.yaw += wrap(yaw - d.yaw) / steps.max(1) as f32;
            }
        }
        if !d.alive() {
            // EnderDragon.tickDeath on a client: it rises, and bursts
            // from tick 180 to 200; the server removes it.
            d.death_time += 1;
            let (t, p) = (d.death_time, d.position);
            if (180..=200).contains(&t) {
                let r = &mut d.random;
                let o = [
                    f64::from((r.next_float() - 0.5) * 8.0),
                    f64::from((r.next_float() - 0.5) * 4.0),
                    f64::from((r.next_float() - 0.5) * 8.0),
                ];
                self.particles.add(
                    &ParticleOptions::simple(ParticleType::ExplosionEmitter),
                    [p.x + o[0], p.y + 2.0 + o[1], p.z + o[2]],
                    [0.0; 3],
                );
            }
            if let Some(d) = self.dragon.as_mut() {
                d.position.y += 0.1;
                for part in &mut d.parts {
                    part.0.y += 0.1;
                }
            }
            return;
        }
        // processFlappingMovement and onFlap: its wing beats, and their sound.
        let flap_was = (d.old_flap * std::f32::consts::TAU).cos();
        let v = d.velocity;
        let mut flap_speed = 0.2 / ((v.x.hypot(v.z) as f32) * 10.0 + 1.0);
        flap_speed *= 2f32.powf(v.y as f32);
        d.flap += if d.phase.sitting() { 0.1 } else { flap_speed };
        let flap_now = (d.flap * std::f32::consts::TAU).cos();
        let at = [d.position.x, d.position.y, d.position.z];
        if flap_was <= -0.3 && flap_now >= -0.3 {
            let pitch = 0.8 + self.world.random.next_float() * 0.3;
            self.sounds
                .play("minecraft:entity.ender_dragon.flap", Some(at), 5.0, pitch);
        }
        let d = self.dragon.as_mut().expect("dragon");
        if !d.phase.sitting() {
            d.growl -= 1;
            if d.growl < 0 {
                d.growl = 200 + d.random.next_int_bound(200).unwrap_or(0);
                let pitch = 0.8 + self.world.random.next_float() * 0.3;
                self.sounds
                    .play("minecraft:entity.ender_dragon.growl", Some(at), 2.5, pitch);
            }
        }
        // checkCrystals' search runs on a client too (the healing beam).
        let d = self.dragon.as_mut().expect("dragon");
        if d.random.next_int_bound(10).unwrap_or(1) == 0 {
            let p = d.position;
            let nearest = self.nearest_crystal_to(p);
            self.dragon.as_mut().expect("dragon").nearest_crystal = nearest;
        }
        let d = self.dragon.as_mut().expect("dragon");
        d.yaw = wrap(d.yaw);
        d.record();
        self.place_dragon_parts();
    }
    /// The dragon's parts as a server's client attacks them: the server's
    /// ids for `EnderDragonPart`s follow the dragon's.
    pub(super) fn remote_dragon_part_hit(
        &self,
        eye: [f64; 3],
        dir: [f64; 3],
        reach: f64,
    ) -> Option<(i32, f64, [f64; 3])> {
        let d = self.dragon.as_ref().filter(|d| d.alive() && d.id != 0)?;
        (0..8)
            .filter_map(|i| {
                let b = d.part_box(i);
                let p = d.parts[i].0;
                super::ray_box(eye, dir, reach, b)
                    .map(|t| ((d.id + 1 + i as u32) as i32, t, [p.x, p.y, p.z]))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}
