//! Firework rockets (`FireworkRocketItem`, `FireworkRocketEntity`): launching
//! from a block face, boosting a gliding player, the 1.15 horizontal
//! acceleration and 0.04 lift, the flight-duration lifetime, explosion
//! damage with line of sight, and the client explosion (`createFireworks`).
use super::{Game, Hit, item_draw::owner_at};
use horizon_assets::item_model::DisplayContext;
use horizon_client::{
    block_entity_render::vertex_light,
    collisions::SceneCollisions,
    items::ItemStack,
    mesher::Vertex,
    
    particles::FireworkExplosion};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::RandomSource,
};

pub(super) const FIREWORK_EXPLODE_EVENT: i8 = 17;

pub(super) struct Rocket {
    /// The entity id a server tracks it by (0 until it is tracked).
    pub id: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
    pub(super) velocity: Vec3,
    life: i32,
    pub(super) explosions: Vec<FireworkExplosion>,
    /// Boosting the player (attachedToEntity).
    pub(super) attached: bool,
    /// On a server, the player it boosts when not this game's own: a guest's
    /// connection.
    pub(super) guest: Option<u32>,
    /// `DATA_ATTACHED_TO_TARGET`: the server entity the rocket follows;
    /// boosting happens when it is the local player.
    pub(super) attached_to: Option<u32>,
    /// `DATA_SHOT_AT_ANGLE` (dispensers): constant velocity, exploding on
    /// the first block it hits.
    pub(super) angled: bool,
    /// `DATA_ID_FIREWORKS_ITEM`: the stack it was launched from.
    pub(super) item: ItemStack,
}
impl Rocket {
    /// A rocket a server sent (`handleAddEntity`): placed and moved by it.
    pub(super) fn remote(id: u32, position: Vec3, velocity: Vec3) -> Self {
        let item = ItemStack::new("minecraft:firework_rocket", 1);
        let (_, explosions) = fireworks_of(&item);
        Self {
            id,
            position,
            previous: position,
            velocity,
            life: 0,
            explosions,
            attached: false,
            guest: None,
            attached_to: None,
            angled: false,
            item,
        }
    }
    /// One tick of a server's client: the rocket flies on by itself between
    /// the server's updates (a rocket that follows an entity is placed by the
    /// caller).
    pub(super) fn remote_step(&mut self, scene: &mut SceneCollisions) {
        self.previous = self.position;
        if self.attached_to.is_none() {
            self.free_flight(scene);
        }
        self.life += 1;
    }
    /// `FireworkRocketEntity.tick` for a rocket attached to no entity: speed
    /// up (an angled one keeps its motion) and rise, then move. True when a
    /// block stopped it.
    fn free_flight(&mut self, scene: &mut SceneCollisions) -> bool {
        let h = 0.125;
        let bounds = Aabb::new(
            Vec3::new(self.position.x - h, self.position.y, self.position.z - h),
            Vec3::new(
                self.position.x + h,
                self.position.y + 2.0 * h,
                self.position.z + h,
            ),
        );
        let wanted = if self.angled {
            self.velocity
        } else {
            Vec3::new(
                self.velocity.x * 1.15,
                self.velocity.y + 0.04,
                self.velocity.z * 1.15,
            )
        };
        let Ok(moved) = scene.resolve(bounds, wanted, 0.0, false) else {
            return false;
        };
        let blocked = (moved.x - wanted.x).abs() > 1e-7
            || (moved.y - wanted.y).abs() > 1e-7
            || (moved.z - wanted.z).abs() > 1e-7;
        let horizontal = (moved.x - wanted.x).abs() > 1e-7 || (moved.z - wanted.z).abs() > 1e-7;
        self.position = Vec3::new(
            self.position.x + moved.x,
            self.position.y + moved.y,
            self.position.z + moved.z,
        );
        self.velocity = if horizontal {
            Vec3::new(self.velocity.x, wanted.y, self.velocity.z)
        } else {
            wanted
        };
        blocked
    }
}
pub(super) fn fireworks_of(stack: &ItemStack) -> (u8, Vec<FireworkExplosion>) {
    // The item's default component: flight 1, no explosions.
    stack
        .data()
        .and_then(|d| d.fireworks.clone())
        .unwrap_or((1, Vec::new()))
}
impl Game<'_> {
    /// `FireworkEntityRenderer.submit`: the item (GROUND) facing the camera,
    /// lit at the rocket's light probe.
    pub(super) fn rocket_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let orientation = glam::Mat4::from_quat(self.camera_orientation());
        // shouldRender: not while attached to an entity (`isAttachedToEntity`).
        for r in self
            .rockets
            .iter()
            .filter(|r| !r.attached && r.attached_to.is_none())
        {
            // FireworkRocketEntity.shouldRenderAtSqrDistance: within 64 blocks;
            // EntityRenderer.shouldRender: its 0.25 box in the view.
            if !self.standing_entity_in_view(
                camera,
                super::entity_culling::at(r.previous),
                super::entity_culling::at(r.position),
                alpha,
                0.25,
                0.25,
                64.0,
                0.0,
            ) {
                continue;
            }
            let p = glam::DVec3::new(
                r.previous.x + (r.position.x - r.previous.x) * alpha,
                r.previous.y + (r.position.y - r.previous.y) * alpha,
                r.previous.z + (r.position.z - r.previous.z) * alpha,
            );
            // `updateForNonLiving`: GROUND, owned by the rocket.
            let owner = owner_at(p.to_array(), 0.0, "minecraft:firework_rocket");
            let state = self.resolve_item(&r.item, DisplayContext::Ground, Some(&owner), 0);
            if state.is_empty() {
                continue;
            }
            let mut pose = glam::Mat4::from_translation((p - camera).as_vec3()) * orientation;
            if r.angled {
                pose *= glam::Mat4::from_rotation_z(std::f32::consts::PI)
                    * glam::Mat4::from_rotation_y(std::f32::consts::PI)
                    * glam::Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
            }
            // getLightProbePosition: the eye, 0.85 of its 0.25 height.
            let light = self.world.light(
                p.x.floor() as i32,
                (p.y + 0.2125).floor() as i32,
                p.z.floor() as i32,
            );
            self.submit_item_world(&state, pose, vertex_light(light), out);
        }
    }
}
impl crate::game::Sim<'_> {
    pub(super) fn new_rocket(&mut self, at: Vec3, stack: &ItemStack, attached: bool) -> Rocket {
        let (flight, explosions) = fireworks_of(stack);
        // FireworkRocketEntity: motion and lifetime come from the rocket's own random.
        let r = &mut super::dispenser::entity_random();
        let tri = |r: &mut horizon_core::random::LegacyRandom| r.triangle(0.0, 0.002297);
        let velocity = Vec3::new(tri(r), 0.05, tri(r));
        let lifetime = 10 * (1 + i32::from(flight))
            + r.next_int_bound(6).unwrap_or(0)
            + r.next_int_bound(7).unwrap_or(0);
        Rocket {
            id: 0,
            position: at,
            previous: at,
            velocity,
            life: 0,
            explosions,
            attached,
            guest: None,
            attached_to: None,
            angled: false,
            item: stack.clone(),
        }
    }
    /// `FireworkRocketItem.useOn`: launch from the clicked point.
    pub(super) fn launch_firework(&mut self, hit: &Hit, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:firework_rocket" {
            return false;
        }
        let at = Vec3::new(
            hit.point[0] + f64::from(hit.face[0]) * 0.15,
            hit.point[1] + f64::from(hit.face[1]) * 0.15,
            hit.point[2] + f64::from(hit.face[2]) * 0.15,
        );
        let rocket = self.new_rocket(at, stack, false);
        self.rockets.push(rocket);
        if !self.creative {
            self.consume_held();
        }
        self.swing_arm();
        true
    }
    /// A rocket shot at an angle (`DATA_SHOT_AT_ANGLE`): out of a dispenser
    /// (`FireworkRocketItem.asProjectile`) or a crossbow.
    pub(super) fn launch_angled_rocket(&mut self, at: Vec3, velocity: Vec3, stack: &ItemStack) {
        let mut rocket = self.new_rocket(at, stack, false);
        rocket.velocity = velocity;
        rocket.angled = true;
        self.rockets.push(rocket);
    }
    /// `FireworkRocketItem.use` while gliding: a rocket attached to the player.
    pub(super) fn boost_firework(&mut self, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:firework_rocket"
            || !self.player.as_ref().is_some_and(|p| p.fall_flying)
        {
            return false;
        }
        let at = self.player.as_ref().map_or(Vec3::ZERO, |p| p.position);
        let mut rocket = self.new_rocket(at, stack, true);
        rocket.guest = None::<u32>;
        self.rockets.push(rocket);
        if !self.creative {
            self.consume_held();
        }
        self.swing_arm();
        true
    }
    /// FireworkRocketEntity.handleEntityEvent: use the synchronized item data.
    pub(super) fn remote_firework_explosion(&mut self, id: i32) {
        let camera = self.eye_position();
        if let Some(r) = self.rockets.iter().find(|r| r.id == id as u32) {
            let at = [r.position.x, r.position.y, r.position.z];
            let v = [r.velocity.x, r.velocity.y, r.velocity.z];
            self.particles
                .fireworks(at, v, r.explosions.clone(), camera.unwrap_or(at));
        }
    }
}
