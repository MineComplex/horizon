//! Game events and sculk on the game side: dispatching events to sculk
//! sensors, shriekers and wardens (`GameEventDispatcher`), the vibration
//! ticker, redstone output, resonance, shrieks and warden summoning
//! (`SculkShriekerBlockEntity`, `SpawnUtil`), catalysts feeding the sculk
//! spreader from deaths, and the darkness wardens and shriekers cast.
use super::Game;
use horizon_client::{
    containers::BlockEntity,
    mobs::{Who, warden},
    particles::{ParticleOptions, ParticleType},
    sculk::SculkKind,
};
use horizon_core::{geometry::Direction, position::BlockPos, random::RandomSource};
use horizon_world::registry::BlockStateId;

/// `SculkSensorBlock.getAnalogOutputSignal`: the last vibration's frequency
/// while the sensor is active.
pub(super) fn sensor_signal(
    world: &horizon_client::world::World,
    behaviours: &horizon_world::level::Behaviours,
    state: BlockStateId,
    pos: BlockPos,
) -> Option<i32> {
    match world.block_entities.get(&(pos.x, pos.y, pos.z)) {
        Some(BlockEntity::Sculk(e))
            if matches!(e.kind, SculkKind::Sensor | SculkKind::Calibrated) =>
        {
            Some(if behaviours.sculk_sensor_active(state) {
                e.last_frequency
            } else {
                0
            })
        }
        _ => None,
    }
}

#[derive(Default)]
pub(super) struct SculkState {
    /// The darkness effect's blend factor, previous and current tick.
    darkness: (f32, f32),
    /// Who causes the block changes being processed (the player's actions).
    pub actor: Option<Who>,
}

fn center(p: (i32, i32, i32)) -> [f64; 3] {
    [
        f64::from(p.0) + 0.5,
        f64::from(p.1) + 0.5,
        f64::from(p.2) + 0.5,
    ]
}

impl Game<'_> {
    // ---- setup and the block entity index ----

    // ---- contexts ----

    // ---- posting ----

    // ---- ticking ----

    // ---- sensors ----

    // ---- shriekers ----

    // ---- darkness ----

    /// `LightmapRenderStateExtractor`: the night vision effect's intensity
    /// (`GameRenderer.nightVisionScale`), or conduit power's water vision.
    pub(super) fn night_vision_intensity(&self, partial: f32) -> f32 {
        use horizon_client::fog::FogType;
        if let Some(e) = self.potions.effects.get("night_vision") {
            // MobEffectInstance.endsWithin(200); -1 is infinite.
            return if e.duration < 0 || e.duration > 200 {
                1.0
            } else {
                0.7 + ((e.duration as f32 - partial) * std::f32::consts::PI * 0.2).sin() * 0.3
            };
        }
        if self.potions.effects.get("conduit_power").is_some()
            && let Some(eye) = self.eye_position()
        {
            let in_water =
                horizon_client::fog::fog_type(&self.world, self.res.facts(), eye, self.forward())
                    == FogType::Water;
            return self.front.fog.water_vision(in_water);
        }
        0.0
    }

    // ---- catalysts ----

    // ---- particles ----

    // ---- entity events ----
}
impl crate::game::Sim<'_> {
    /// Run `f` with the player as the cause of the block changes it makes.
    pub(super) fn with_player_actor<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let prior = self.sculk.actor.replace(Who::Player);
        let out = f(self);
        self.sculk.actor = prior;
        out
    }
    fn state_property(&self, state: BlockStateId, key: &str) -> Option<&str> {
        self.pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get(key))
            .map(String::as_str)
    }
    /// `MobEffectInstance.BlendState.tick` (from `tickClient`) for the
    /// darkness effect: it fades in and out over 22 ticks.
    pub(super) fn tick_darkness_blend(&mut self) {
        const BLEND: f32 = 22.0;
        let (_, current) = self.sculk.darkness;
        let next = match self.potions.effects.get("darkness") {
            Some(e) => {
                let target = if e.duration == -1 || e.duration > BLEND as i32 {
                    1.0
                } else {
                    0.0
                };
                current + (target - current).clamp(-1.0 / BLEND, 1.0 / BLEND)
            }
            None => 0.0,
        };
        self.sculk.darkness = (current, next);
    }
    /// The darkness effect's blend factor at the partial tick.
    fn darkness_blend(&self, partial: f32) -> f32 {
        let (previous, current) = self.sculk.darkness;
        previous + (current - previous) * partial
    }
    /// The fog's view of the darkness effect, while it is on.
    pub(super) fn darkness_fog(&self) -> Option<f32> {
        self.potions
            .effects
            .get("darkness")
            .map(|_| self.darkness_blend(self.clock.fraction() as f32))
    }
    /// `LightmapRenderStateExtractor`: (brightness taken off the gamma
    /// option, the pulsing light scale).
    pub(super) fn darkness_lighting(&self, partial: f32) -> (f32, f32) {
        let scale = self.settings.file.double("darknessEffectScale") as f32;
        let modifier = self.darkness_blend(partial) * scale;
        let pulse =
            ((self.ticks as f32 - partial) * std::f32::consts::PI * 0.025).cos() * 0.45 * modifier;
        (modifier, pulse.max(0.0) * scale)
    }
    /// `Warden.applyDarknessAround` / `MobEffectUtil.addEffectToPlayersAround`.
    pub(super) fn apply_darkness(&mut self, at: [f64; 3], radius: f64) {
        self.for_each_player(|g| g.darkness_on_player(at, radius));
    }
    fn darkness_on_player(&mut self, at: [f64; 3], radius: f64) {
        if self.creative || self.spectator() {
            return;
        }
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let d = [
            p.position.x - at[0],
            p.position.y - at[1],
            p.position.z - at[2],
        ];
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= radius * radius {
            return;
        }
        if let Some(active) = self.potions.effects.get("darkness")
            && active.duration > warden::DARKNESS_REAPPLY - 1
        {
            return;
        }
        let mut effect =
            horizon_client::effects::EffectInstance::new("darkness", warden::DARKNESS_DURATION, 0);
        effect.visible = false;
        self.potions.effects.add(effect);
    }
    /// `LevelEvent` 3006: charge cursors moving over sculk, or popping.
    pub(super) fn sculk_charge_event(&mut self, pos: BlockPos, data: i32) {
        let c = center((pos.x, pos.y, pos.z));
        let count = data >> 6;
        if count > 0 {
            // Client-side (LevelEventHandler / particles): the client random.
            let r = self.particles.random();
            if r.next_float() < 0.3 + count as f32 * 0.1 {
                let volume = 0.15 + 0.02 * (count * count) as f32 * r.next_float();
                let pitch = 0.4 + 0.3 * count as f32 * r.next_float();
                self.sounds
                    .play("minecraft:block.sculk.charge", Some(c), volume, pitch);
            }
            let face_data = data & 63;
            let faces: Vec<(Direction, f32, f64)> = if face_data == 0 {
                Direction::ALL
                    .into_iter()
                    .map(|d| {
                        let roll = if d == Direction::Down {
                            std::f32::consts::PI
                        } else {
                            0.0
                        };
                        let factor = if matches!(d, Direction::Up | Direction::Down) {
                            0.65
                        } else {
                            0.57
                        };
                        (d, roll, factor)
                    })
                    .collect()
            } else {
                horizon_world::level::sculk::unpack_faces(face_data)
                    .into_iter()
                    .map(|d| {
                        let roll = if d == Direction::Up {
                            std::f32::consts::PI
                        } else {
                            0.0
                        };
                        (d, roll, 0.35)
                    })
                    .collect()
            };
            for (direction, roll, factor) in faces {
                let repetitions = self
                    .particles
                    .random()
                    .next_int_bound(count + 1)
                    .unwrap_or(0);
                for _ in 0..repetitions {
                    self.spawn_particle_on_face(pos, direction, roll, factor);
                }
            }
        } else {
            self.sounds
                .play("minecraft:block.sculk.charge", Some(c), 1.0, 1.0);
            let solid = self
                .res
                .facts()
                .get(self.world.block(pos.x, pos.y, pos.z))
                .full_collision;
            let (particles, spread) = if solid { (40, 0.45f32) } else { (20, 0.25f32) };
            let options = ParticleOptions::simple(ParticleType::SculkChargePop);
            for _ in 0..particles {
                // Client-side (LevelEventHandler / particles): the client random.
                let r = self.particles.random();
                let v = [
                    2.0 * r.next_float() - 1.0,
                    2.0 * r.next_float() - 1.0,
                    2.0 * r.next_float() - 1.0,
                ];
                let at = [
                    c[0] + f64::from(v[0] * spread),
                    c[1] + f64::from(v[1] * spread),
                    c[2] + f64::from(v[2] * spread),
                ];
                let speed = [
                    f64::from(v[0] * 0.07),
                    f64::from(v[1] * 0.07),
                    f64::from(v[2] * 0.07),
                ];
                self.particles.add(&options, at, speed);
            }
        }
    }
    /// `ParticleUtils.spawnParticleOnFace` for a sculk charge.
    fn spawn_particle_on_face(&mut self, pos: BlockPos, face: Direction, roll: f32, factor: f64) {
        let c = center((pos.x, pos.y, pos.z));
        let step = face.normal();
        // Client-side (LevelEventHandler / particles): the client random.
        let r = self.particles.random();
        let mut jitter = || r.next_double() - 0.5;
        let mut at = [0.0; 3];
        for i in 0..3 {
            at[i] = c[i]
                + if step[i] == 0 {
                    jitter()
                } else {
                    f64::from(step[i]) * factor
                };
        }
        let mut speed = [0.0; 3];
        for i in 0..3 {
            let s = (r.next_double() * 2.0 - 1.0) * f64::from(0.005f32);
            speed[i] = if step[i] == 0 { s } else { 0.0 };
        }
        self.particles
            .add(&ParticleOptions::sculk_charge(roll), at, speed);
    }
    /// `LevelEvent` 3007: ten shrieks rising from the shrieker, and the shriek.
    pub(super) fn shriek_event(&mut self, pos: BlockPos) {
        let top = f64::from(pos.y) + 0.5;
        for i in 0..10 {
            self.particles.add(
                &ParticleOptions::shriek(i * 5),
                [f64::from(pos.x) + 0.5, top, f64::from(pos.z) + 0.5],
                [0.0; 3],
            );
        }
        let state = self.world.block(pos.x, pos.y, pos.z);
        if self.state_property(state, "waterlogged") != Some("true") {
            let pitch = 0.6 + self.particles.random().next_float() * 0.4;
            self.sounds.play(
                "minecraft:block.sculk_shrieker.shriek",
                Some([f64::from(pos.x) + 0.5, top, f64::from(pos.z) + 0.5]),
                2.0,
                pitch,
            );
        }
    }
    fn vibration_particle(&mut self, from: [f64; 3], to: [f64; 3], ticks: i32) {
        self.particles.vibration(from, to, ticks);
    }
    /// A vibration travelling to a warden (`MobEvent::Vibration`).
    pub(super) fn mob_vibration_particle(&mut self, from: [f64; 3], to: [f64; 3], ticks: i32) {
        self.vibration_particle(from, to, ticks);
    }
    /// `Warden.clientDiggingParticles`: thirty crumbs of the block underfoot.
    pub(super) fn warden_dig_particles(&mut self, at: [f64; 3]) {
        let below = (
            at[0].floor() as i32,
            (at[1] - 0.2).floor() as i32,
            at[2].floor() as i32,
        );
        let state = self.world.block(below.0, below.1, below.2);
        let Some((sprite, tint)) = self.particle_sprite(state, [below.0, below.1, below.2]) else {
            return;
        };
        for _ in 0..30 {
            // Client-side (LevelEventHandler / particles): the client random.
            let r = self.particles.random();
            let off = |v: f32| f64::from(v * 1.4 - 0.7);
            let p = [
                at[0] + off(r.next_float()),
                at[1],
                at[2] + off(r.next_float()),
            ];
            let particle =
                horizon_client::particles::Particle::terrain(r, p, [0.0; 3], sprite.clone(), tint);
            self.particles.push(particle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centres_agree() {
        assert_eq!(center((1, 2, 3)), [1.5, 2.5, 3.5]);
    }
}
