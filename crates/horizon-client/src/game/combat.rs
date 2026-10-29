//! The player's `CombatTracker`: the damage taken lately, who dealt it and
//! where the player fell from, turned into the vanilla death message.
use horizon_client::{
    particles::{ParticleOptions, ParticleType},
    text::Text,
};
use horizon_core::random::RandomSource;

/// `EntityEvent.DROWN_PARTICLES`: broadcast with the drowning damage.
pub(super) const DROWN_EVENT: i8 = 67;

#[derive(Default)]
pub(super) struct CombatTracker {
    /// `LivingEntity.lastHurtByMob` and its timestamp (the kill credit).
    last_hurt_by: Option<((u32, Text), i64)>,
    /// The mob the player last hurt (`getLastHurtMob`) and when.
    pub(super) last_hurt_mob: Option<(u32, i64)>,
}
impl crate::game::Sim<'_> {
    /// `LivingEntity.makeDrownParticles`: eight bubbles around the entity,
    /// moving with it.
    pub(super) fn drown_particles(&mut self, at: [f64; 3], velocity: [f64; 3]) {
        let bubble = ParticleOptions::simple(ParticleType::Bubble);
        for _ in 0..8 {
            let mut triangle = || {
                let r = self.particles.random();
                r.next_double() - r.next_double()
            };
            let p = [at[0] + triangle(), at[1] + triangle(), at[2] + triangle()];
            self.particles.add(&bubble, p, velocity);
        }
    }
    /// Entity event 67 from a server, for this client's own player or an
    /// entity it draws.
    pub(super) fn remote_drown(&mut self, id: i32) {
        let own = self.remote.as_ref().is_some_and(|r| r.entity_id == id);
        let body = if own {
            self.player.as_ref().map(|p| {
                let (at, v) = (p.position, p.velocity);
                ([at.x, at.y, at.z], [v.x, v.y, v.z])
            })
        } else {
            self.mobs.iter().find(|m| m.id == id as u32).map(|m| {
                let (at, v) = (m.position, m.velocity);
                ([at.x, at.y, at.z], [v.x, v.y, v.z])
            })
        };
        if let Some((at, v)) = body {
            self.drown_particles(at, v);
        }
    }
    /// `getLastHurtByMob`: the last mob to hurt the player, for 100 ticks.
    pub(super) fn player_hurt_by(&self) -> Option<(horizon_client::mobs::Who, i64)> {
        let ((id, _), at) = self.combat.last_hurt_by.as_ref()?;
        (self.world.game_time - at <= 100).then_some((horizon_client::mobs::Who::Mob(*id), *at))
    }
}
