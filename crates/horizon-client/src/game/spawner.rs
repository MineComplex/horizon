//! Monster spawners in the game: `SpawnerBlockEntity.serverTick` and
//! `clientTick` for every spawner (`mobs::spawner`), the mobs they add, a
//! spawn egg used on one (`SpawnEggItem.useOn`), their events and the mob
//! spinning inside (`SpawnerRenderer`).
use super::Game;
use horizon_client::{
    containers::BlockEntity,
    mobs::Mob,
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::random::RandomSource;
use std::collections::{HashMap, HashSet};

type Pos = (i32, i32, i32);

/// What the spawner code keeps between ticks.
#[derive(Default)]
pub(super) struct SpawnerState {
    /// Every entity type id (`EntityType.byString`), read once.
    entity_types: Option<HashSet<String>>,
    /// The mob shown in each spawner and the id it was made for
    /// (`getOrCreateDisplayEntity`).
    displays: HashMap<Pos, (String, Mob)>,
}

impl Game<'_> {
    /// `SpawnerRenderer` and `TrialSpawnerRenderer`: the next mob of every
    /// spawner near the camera, small and turning.
    pub(super) fn spawner_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) {
        let atlas = &self.front.gui.atlas;
        let lookup = |name: &str| atlas.sprites.get(name).map(|r| [r.u0, r.v0, r.u1, r.v1]);
        for (key, (_, mob)) in &self.spawners.displays {
            let at = glam::DVec3::new(f64::from(key.0), f64::from(key.1), f64::from(key.2));
            if at.distance_squared(camera) >= 64.0 * 64.0 {
                continue;
            }
            let (spin, scale) = match self.world.block_entities.get(key) {
                Some(BlockEntity::Spawner(s)) => {
                    s.display_pose(alpha as f32, mob.width(), mob.height())
                }
                Some(BlockEntity::TrialSpawner(s)) => horizon_client::mobs::spawner::display_pose(
                    s.o_spin,
                    s.spin,
                    alpha as f32,
                    mob.width(),
                    mob.height(),
                ),
                _ => continue,
            };
            let origin = glam::Mat4::from_translation(
                (at - camera).as_vec3() + glam::Vec3::new(0.5, 0.4, 0.5),
            ) * glam::Mat4::from_rotation_y(spin.to_radians())
                * glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.2, 0.0))
                * glam::Mat4::from_rotation_x((-30.0_f32).to_radians())
                * glam::Mat4::from_scale(glam::Vec3::splat(scale));
            let light = self.world.light(key.0, key.1, key.2);
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            if mob.species() == horizon_client::mobs::Species::SulfurCube {
                self.sulfur_cube_body_geometry(
                    mob,
                    alpha as f32,
                    horizon_client::mobs::render::base_transform_at(mob, alpha as f32, origin),
                    packed,
                    out,
                );
            }
            horizon_client::mobs::render::render_posed(
                mob,
                alpha as f32,
                origin,
                &lookup,
                packed,
                self.res.mob_data(),
                out,
            );
        }
    }
}
impl crate::game::Sim<'_> {
    /// `EntityType.byString`: the type is in the registry.
    pub(super) fn entity_type_exists(&self, id: &str) -> bool {
        self.spawners
            .entity_types
            .as_ref()
            .is_some_and(|t| t.contains(id))
    }
    /// `LivingEntity.makePoofParticles` (entity event 20) for a mob.
    pub(super) fn mob_poof(&mut self, id: u32) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let at = m.position;
        self.lan_entity_event(id as i32, 20, [at.x, at.y, at.z]);
        self.poof_particles(id);
    }
    /// `Mob.handleEntityEvent` 20 (`spawnAnim` on a client): the poof about
    /// mob `id`.
    pub(super) fn poof_particles(&mut self, id: u32) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let (at, w, h) = (m.position, f64::from(m.width()), f64::from(m.height()));
        let options = ParticleOptions::simple(ParticleType::Poof);
        for _ in 0..20 {
            let r = self.particles.level_random();
            let xa = r.next_gaussian() * 0.02;
            let ya = r.next_gaussian() * 0.02;
            let za = r.next_gaussian() * 0.02;
            let x = at.x + w * (2.0 * r.next_double() - 1.0) - xa * 10.0;
            let y = at.y + h * r.next_double() - ya * 10.0;
            let z = at.z + w * (2.0 * r.next_double() - 1.0) - za * 10.0;
            self.particles.add(&options, [x, y, z], [xa, ya, za]);
        }
    }
    /// `LevelEventHandler` 2004: twenty smoke and flame pairs around the spawner.
    pub(super) fn spawner_burst(&mut self, pos: [i32; 3]) {
        let (smoke, flame) = (
            ParticleOptions::simple(ParticleType::Smoke),
            ParticleOptions::simple(ParticleType::Flame),
        );
        for _ in 0..20 {
            let r = self.particles.level_random();
            let x = f64::from(pos[0]) + 0.5 + (r.next_double() - 0.5) * 2.0;
            let y = f64::from(pos[1]) + 0.5 + (r.next_double() - 0.5) * 2.0;
            let z = f64::from(pos[2]) + 0.5 + (r.next_double() - 0.5) * 2.0;
            self.particles.add(&smoke, [x, y, z], [0.0; 3]);
            self.particles.add(&flame, [x, y, z], [0.0; 3]);
        }
    }
}
