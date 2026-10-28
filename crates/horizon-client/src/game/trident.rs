//! Tridents: `TridentItem.releaseUsing` (throw after 10 ticks, or Riptide in
//! water or rain) and `ThrownTrident` (arrow flight with 0.99 water inertia,
//! 8 damage plus Impaling, the reversed bounce off a hit, sticking, Loyalty's
//! return to the owner, pickup) with `ThrownTridentRenderer`, and Channeling's
//! lightning on a hit (`channeling.json`).
use super::{Game, view_vector};
use horizon_client::{
    collisions::SceneCollisions,
    items::ItemStack,
    mesher::Vertex,
    mobs::{ Who, anim::Poses},
    projectiles::ProjectileWorld};
use horizon_core::geometry::Vec3;

pub(super) struct ThrownTrident {
    /// The entity id a server tracks it by (0 until it is tracked).
    pub id: u32,
    pub(super) stack: ItemStack,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
    pub(super) velocity: Vec3,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) in_ground: bool,
    in_ground_time: u32,
    dealt_damage: bool,
    pub(super) loyalty: u32,
    pub(super) no_physics: bool,
    returning: u32,
    creative_only: bool,
    /// The entity id a server sent as its owner (the one Loyalty returns it to).
    pub(super) owner: u32,
    /// The guest that threw it (`Projectile.getOwner`); `None` for the host.
    guest: Option<u32>,
}
impl ThrownTrident {
    /// A trident a server sent (`handleAddEntity`): placed and moved by it.
    pub(super) fn remote(id: u32, position: Vec3, yaw: f32, pitch: f32) -> Self {
        Self {
            id,
            stack: ItemStack::new("minecraft:trident", 1),
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw,
            pitch,
            in_ground: false,
            in_ground_time: 0,
            dealt_damage: false,
            loyalty: 0,
            no_physics: false,
            returning: 0,
            creative_only: false,
            owner: 0,
            guest: None,
        }
    }
    /// `ThrownTrident.tick`'s Loyalty return: pulled up and towards the
    /// owner's eye.
    fn loyalty_pull(&mut self, owner_eye: Vec3) {
        let v = Vec3::new(
            owner_eye.x - self.position.x,
            owner_eye.y - self.position.y,
            owner_eye.z - self.position.z,
        );
        self.position.y += v.y * 0.015 * f64::from(self.loyalty);
        let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt().max(1e-9);
        let accel = 0.05 * f64::from(self.loyalty);
        self.velocity = Vec3::new(
            self.velocity.x * 0.95 + v.x / len * accel,
            self.velocity.y * 0.95 + v.y / len * accel,
            self.velocity.z * 0.95 + v.z / len * accel,
        );
    }
    /// `ThrownTrident.tick` on a server's client: it flies on by itself
    /// between the server's updates (`AbstractArrow.tick`: gravity and drag,
    /// a block holds it), and Loyalty pulls it back to its owner; only the
    /// server decides what it hits.
    pub(super) fn remote_tick(&mut self, w: &impl ProjectileWorld, owner_eye: Option<Vec3>) {
        self.previous = self.position;
        if self.in_ground_time > 4 {
            self.dealt_damage = true;
        }
        if self.loyalty > 0
            && (self.dealt_damage || self.no_physics)
            && let Some(eye) = owner_eye
        {
            self.no_physics = true;
            self.in_ground = false;
            self.loyalty_pull(eye);
        }
        if self.in_ground && !self.no_physics {
            self.in_ground_time += 1;
            return;
        }
        self.in_ground_time = 0;
        let m = self.velocity;
        let (x, z) = if self.no_physics {
            (-m.x, -m.z)
        } else {
            (m.x, m.z)
        };
        self.yaw = x.atan2(z).to_degrees() as f32;
        self.pitch = m.y.atan2(m.x.hypot(m.z)).to_degrees() as f32;
        let to = self.position.add_vector(m);
        if !self.no_physics
            && let Some((at, _, _)) = w.clip(self.position, to)
        {
            self.position = at;
            self.velocity = Vec3::ZERO;
            self.in_ground = true;
            return;
        }
        self.position = to;
        // A trident's water inertia (0.99) equals the air drag.
        self.velocity = self.velocity.scale(0.99);
        if !self.no_physics {
            self.velocity.y -= 0.05;
        }
    }
}
const HALF: f64 = 0.25;
impl Game<'_> {
    /// ThrownTridentRenderer.
    pub(super) fn trident_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let Some(model) = &self.trident_model else {
            return;
        };
        let Some(uv) = self
            .front
            .gui
            .atlas
            .sprites
            .get("entity/trident/trident")
            .map(|r| [r.u0, r.v0, r.u1, r.v1])
        else {
            return;
        };
        // AbstractArrow.shouldRenderAtSqrDistance: ten times the default.
        let range = 10.0
            * super::entity_culling::default_range(
                glam::DVec3::splat(2.0 * HALF),
                self.view_scale(),
            );
        for t in &self.tridents {
            // EntityRenderer.shouldRender; ThrownTridentRenderer grows the box by 1.5.
            if !self.standing_entity_in_view(
                camera,
                super::entity_culling::at(t.previous),
                super::entity_culling::at(t.position),
                alpha,
                2.0 * HALF,
                2.0 * HALF,
                range,
                1.5,
            ) {
                continue;
            }
            let p = glam::DVec3::new(
                t.previous.x + (t.position.x - t.previous.x) * alpha,
                t.previous.y + (t.position.y - t.previous.y) * alpha,
                t.previous.z + (t.position.z - t.previous.z) * alpha,
            ) - camera;
            let base = glam::Mat4::from_translation(p.as_vec3())
                * glam::Mat4::from_rotation_y((t.yaw - 90.0).to_radians())
                * glam::Mat4::from_rotation_z((t.pitch + 90.0).to_radians());
            let l = self.world.light(
                t.position.x.floor() as i32,
                t.position.y.floor() as i32,
                t.position.z.floor() as i32,
            );
            let packed = (u32::from(l >> 4) * 16) | ((u32::from(l & 15) * 16) << 8);
            let poses = Poses::new(model);
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
        }
    }
}
impl crate::game::Sim<'_> {
    /// `TridentItem.use`: start the throw unless one more use would break it.
    pub(super) fn use_trident(&mut self, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:trident" {
            return false;
        }
        let max = self
            .res
            .items()
            .get(&stack.item)
            .map_or(250, |i| i.max_damage);
        let riptide = stack.enchantment_level("minecraft:riptide") > 0;
        if stack.damage + 1 >= max || (riptide && !self.in_water_or_rain()) {
            return true;
        }
        if self.using.is_none() {
            self.using = Some((self.held_slot(), 0));
        }
        true
    }
    fn in_water_or_rain(&mut self) -> bool {
        let Some(p) = &self.player else { return false };
        let (x, y, z) = (
            p.position.x.floor() as i32,
            p.position.y.floor() as i32,
            p.position.z.floor() as i32,
        );
        let water = self.res.facts().get(self.world.block(x, y, z)).fluid
            == horizon_client::blocks::Fluid::Water;
        let (at, top) = (p.position, p.bounds().max_position().y);
        water || self.in_rain(at, top)
    }
    /// `TridentItem.releaseUsing`.
    pub(super) fn release_trident(&mut self, ticks: u32) {
        if ticks < 10 {
            return;
        }
        let Some(stack) = self.held().cloned() else {
            return;
        };
        let Some(eye) = self.eye_position() else {
            return;
        };
        let riptide = stack.enchantment_level("minecraft:riptide");
        let at = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]);
        if riptide > 0 {
            // `isInWaterOrRain() && !isPassenger()`.
            if !self.in_water_or_rain() || self.riding.is_some() {
                return;
            }
            // `hurtWithoutBreaking` and the sound belong to the server; a server's
            // client only launches.
            let server_client = self.remote.is_some();
            if !server_client {
                self.hurt_held_without_breaking(1);
            }
            // trident_spin_attack_strength: 1.5 + 0.75 per level above the first.
            let strength = 1.5 + 0.75 * (riptide as f32 - 1.0);
            let d = view_vector(self.yaw, self.pitch);
            if let Some(p) = self.player.as_mut() {
                p.velocity = Vec3::new(
                    p.velocity.x + d[0] * f64::from(strength),
                    p.velocity.y + d[1] * f64::from(strength),
                    p.velocity.z + d[2] * f64::from(strength),
                );
                // Player.startAutoSpinAttack(20, 8, stack).
                p.auto_spin_attack_ticks = 20;
                p.auto_spin_attack_dmg = 8.0;
            }
            // `player.move(SELF, (0, 1.1999999, 0))` when on the ground: the lift
            // stops at a ceiling.
            let lift = self
                .player
                .as_ref()
                .filter(|p| p.ground.on_ground)
                .map(|p| p.bounds());
            if let Some(bounds) = lift {
                let mut scene = SceneCollisions {
                    world: &self.world,
                    binding: self.collision,
                    subject: None,
                };
                let up = Vec3::new(0.0, f64::from(1.1999999f32), 0.0);
                let moved = scene.resolve(bounds, up, 0.0, true).unwrap_or(Vec3::ZERO);
                if let Some(p) = self.player.as_mut() {
                    p.position.y += moved.y;
                }
            }
            if !server_client {
                let sound = format!("minecraft:item.trident.riptide_{}", riptide.min(3));
                self.sounds.play(&sound, at, 1.0, 1.0);
            }
            return;
        }
        // The server throws the trident.
        if self.remote.is_some() {
            return;
        }
        // hurtWithoutBreaking (Unbreaking applies), then the thrown copy.
        self.hurt_held_without_breaking(1);
        let thrown = self.held().cloned().unwrap_or(stack);
        if !self.creative {
            *self.held_mut() = None;
        }
        let start = Vec3::new(eye[0], eye[1] - 0.1, eye[2]);
        let mut projectile = horizon_client::projectiles::Projectile::new(
            horizon_client::projectiles::ProjectileKind::Snowball,
            start,
            Some(Who::Player),
        );
        let (v, on_ground) = self
            .player
            .as_ref()
            .map_or((Vec3::ZERO, true), |p| (p.velocity, p.ground.on_ground));
        projectile.shoot_from_rotation(
            &mut self.world.random,
            self.pitch,
            self.yaw,
            0.0,
            2.5,
            1.0,
            v,
            on_ground,
        );
        self.tridents.push(ThrownTrident {
            id: 0,
            loyalty: thrown.enchantment_level("minecraft:loyalty"),
            stack: thrown,
            position: start,
            previous: start,
            velocity: projectile.velocity,
            yaw: projectile.yaw,
            pitch: projectile.pitch,
            in_ground: false,
            in_ground_time: 0,
            dealt_damage: false,
            no_physics: false,
            returning: 0,
            creative_only: self.creative,
            owner: 0,
            guest: self.lan_acting,
        });
        self.player_projectile_shot();
        self.sounds
            .play("minecraft:item.trident.throw", at, 1.0, 1.0);
    }
}
