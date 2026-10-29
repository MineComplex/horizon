//! The totem of undying (`LivingEntity.checkTotemDeathProtection` with the
//! totem's `death_protection` component, entity event 35 on the client).
use horizon_client::particles::{ParticleOptions, ParticleType};

/// `EntityEvent.TOTEM_OF_UNDYING`.
pub(super) const TOTEM_EVENT: i8 = 35;

impl crate::game::Game<'_> {
    /// Entity event 35 from a server, for this client's own player or an
    /// entity it draws.
    pub(super) fn remote_totem(&mut self, id: i32) {
        let own = self.remote.as_ref().is_some_and(|r| r.entity_id == id);
        let body = if own {
            self.player.as_ref().map(|p| Body {
                at: [p.position.x, p.position.y, p.position.z],
                width: f64::from(p.dimensions.width),
                height: f64::from(p.dimensions.height),
            })
        } else {
            self.mobs.iter().find(|m| m.id == id as u32).map(|m| Body {
                at: [m.position.x, m.position.y, m.position.z],
                width: f64::from(m.width()),
                height: f64::from(m.height()),
            })
        };
        if let Some(body) = body {
            self.totem_event(body, own);
        }
    }
    /// `ClientPacketListener.handleEntityEvent` 35: the totem particles (a
    /// 30-tick emitter) and its sound, heard here only (`playLocalSound`),
    /// and the player's own rumble.
    pub(super) fn totem_event(&mut self, body: Body, own: bool) {
        self.particles.tracking_emitter_for(
            ParticleOptions::simple(ParticleType::TotemOfUndying),
            body.at,
            body.width,
            body.height,
            30,
        );
        self.sounds
            .play_unshared("minecraft:item.totem.use", Some(body.at), 1.0, 1.0);
        if own {
            self.rumbles.push(horizon_client::gamepad::Rumble::Totem);
        }
    }
}

/// Where an entity event's entity stands and its size.
#[derive(Clone, Copy)]
pub(super) struct Body {
    pub at: [f64; 3],
    width: f64,
    height: f64,
}
