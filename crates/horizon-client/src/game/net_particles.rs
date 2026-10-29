//! Particles a server sends its players and its clients draw:
//! `ClientboundLevelParticlesPacket` (`ServerLevel.sendParticles`, which
//! `/particle` uses) and `ClientboundExplodePacket` (`ServerLevel.explode`).
use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_core::{geometry::Vec3, random::RandomSource};
use horizon_protocol::{
    entity_data::{Particle, ParticleData},
    packets::play,
};
use horizon_world::registry::BlockStateId;

/// An ARGB colour as the options' RGBA.
fn rgba(argb: i32) -> [f32; 4] {
    let c = |shift: i32| ((argb >> shift) & 255) as f32 / 255.0;
    [c(16), c(8), c(0), c(24)]
}
fn rgb(c: [f32; 4]) -> [f32; 3] {
    [c[0], c[1], c[2]]
}

impl crate::game::Game<'_> {
    /// The options a server's particle stands for (None for a type or data
    /// this client doesn't draw).
    pub(super) fn received_particle(
        &mut self,
        p: &Particle,
        at: [f64; 3],
    ) -> Option<ParticleOptions> {
        let name = self
            .remote
            .as_ref()?
            .builtin
            .get("minecraft:particle_type")?
            .name(p.id)?
            .to_owned();
        let ty = ParticleType::from_name(&name)?;
        Some(match &p.data {
            None => ParticleOptions::simple(ty),
            Some(ParticleData::State(s)) => {
                let pos = at.map(|v| v.floor() as i32);
                let (sprite, tint) = self
                    .particle_sprite(BlockStateId(*s as u32), pos)
                    .unwrap_or_else(|| ("block/stone".into(), 0xFFFFFF));
                ParticleOptions::block(ty, sprite, tint)
            }
            Some(ParticleData::ColorScale(c, scale)) => {
                ParticleOptions::dust(rgb(rgba(*c)), *scale)
            }
            Some(ParticleData::ColorScale2(from, to, scale)) => {
                ParticleOptions::dust_transition(rgb(rgba(*from)), rgb(rgba(*to)), *scale)
            }
            Some(ParticleData::ColorPower(c, power)) => ParticleOptions {
                scale: *power,
                ..ParticleOptions::colored(ty, [rgba(*c)[0], rgba(*c)[1], rgba(*c)[2], 1.0])
            },
            Some(ParticleData::Color(c)) => ParticleOptions::colored(ty, rgba(*c)),
            Some(ParticleData::Scalar(v)) if ty == ParticleType::SculkCharge => {
                ParticleOptions::sculk_charge(*v)
            }
            Some(ParticleData::Scalar(v)) => ParticleOptions {
                scale: *v,
                ..ParticleOptions::simple(ty)
            },
            Some(ParticleData::Delay(d)) => ParticleOptions::shriek(*d),
            Some(ParticleData::Trail(target, color, duration)) => {
                ParticleOptions::trail(*target, *color as u32 & 0xFF_FFFF, *duration)
            }
            Some(_) => return None,
        })
    }
    /// `ClientPacketListener.handleParticleEvent`: no count is one particle
    /// moving by speed times spread; otherwise each is spread and sped up by
    /// gaussians on each axis.
    pub(super) fn remote_level_particles(&mut self, p: &play::LevelParticles) {
        let Some(options) = self.received_particle(&p.particle, p.pos) else {
            return;
        };
        let (d, s) = (p.dist.map(f64::from), p.max_speed.map(f64::from));
        // ClientLevel.addParticle: the camera's 32 blocks and the Particles
        // option, unless the packet or the type overrides the limiter.
        let add = |particles: &mut horizon_client::particles::ParticleEngine, at, v| {
            particles.add_with(&options, at, v, p.override_limiter, p.always_show);
        };
        if p.count == 0 {
            add(
                &mut self.particles,
                p.pos,
                [s[0] * d[0], s[1] * d[1], s[2] * d[2]],
            );
            return;
        }
        for _ in 0..p.count {
            let r = self.particles.random();
            let at = [
                p.pos[0] + r.next_gaussian() * d[0],
                p.pos[1] + r.next_gaussian() * d[1],
                p.pos[2] + r.next_gaussian() * d[2],
            ];
            let v = [
                r.next_gaussian() * s[0],
                r.next_gaussian() * s[1],
                r.next_gaussian() * s[2],
            ];
            add(&mut self.particles, at, v);
        }
    }
    /// `ClientPacketListener.handleExplosion`: the blast's particles (and its
    /// sound, when the packet carries it), then the player's push.
    pub(super) fn remote_explode(&mut self, p: &play::Explode) {
        if p.play_sound {
            let pitch = (1.0
                + (self.particles.random().next_float() - self.particles.random().next_float())
                    * 0.2)
                * 0.7;
            // The packet's explosionSound: a wind charge's burst, a bed's
            // or anchor's blast, a modded sound.
            let sound = self
                .remote_sound_name(&p.sound)
                .unwrap_or_else(|| "minecraft:entity.generic.explode".to_owned());
            self.sounds.play(&sound, Some(p.center), 4.0, pitch);
        }
        if let Some(options) = self.received_particle(&p.particle, p.center) {
            self.particles.add(&options, p.center, [1.0, 0.0, 0.0]);
        }
        // A wind charge's burst throws none.
        if !p.block_particles.is_empty() {
            let (world, facts) = (&self.world, self.res.facts());
            self.particles.explosion_blocks(
                p.center,
                p.radius,
                p.block_count.max(0) as usize,
                &|b| facts.is_air(world.block(b[0], b[1], b[2])),
            );
        }
        // Entity.pushFromExplosion.
        if let (Some(push), Some(player)) = (p.knockback, self.player.as_mut()) {
            player.velocity = Vec3::new(
                player.velocity.x + push[0],
                player.velocity.y + push[1],
                player.velocity.z + push[2],
            );
        }
    }
}
