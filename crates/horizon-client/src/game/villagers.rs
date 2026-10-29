//! What villagers do with each other and with the player: gossip and
//! reputation, sharing food and wares, picking up dropped items, making
//! babies, calling iron golems and the prices they ask.
use horizon_client::{
    gossip::{self, GossipType},
    mobs::{Species, villager},
};

impl crate::game::Sim<'_> {
    fn villager_index(&self, id: u32) -> Option<usize> {
        self.mobs
            .iter()
            .position(|m| m.id == id && m.species() == Species::Villager)
    }
    /// `Level.broadcastEntityEvent` of an event whose `handleEntityEvent`
    /// draws particles about the mob (villager 12, 13 and 14, taming 6 and 7,
    /// love 18): the guests get the byte, and this game draws it as its own
    /// client would.
    pub(super) fn mob_event_particles(&mut self, id: u32, event: i8) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let at = [m.position.x, m.position.y, m.position.z];
        self.lan_entity_event(id as i32, event, at);
        self.entity_event_particles(id, event);
    }
    /// `handleEntityEvent(event)`'s particles about mob `id`
    /// (`Villager`, `TamableAnimal`, `AbstractHorse`, `Animal`, `Allay`).
    pub(super) fn entity_event_particles(&mut self, id: u32, event: i8) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        if let Some((kind, burst)) =
            horizon_client::particles::entity_event_burst(&m.kind.id, event as u8)
        {
            self.particles_around_mob(id, kind, burst);
        }
    }
    /// `particles::around_self` about mob `id`.
    pub(super) fn particles_around_mob(
        &mut self,
        id: u32,
        kind: &'static str,
        burst: horizon_client::particles::Burst,
    ) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let feet = [m.position.x, m.position.y, m.position.z];
        let size = [f64::from(m.width()), f64::from(m.height())];
        self.particles_around(kind, feet, size, burst);
    }
    /// `particles::around_self` on the client: the particle engine's random
    /// stands in for the client entity's, so no server draw moves.
    pub(super) fn particles_around(
        &mut self,
        kind: &'static str,
        feet: [f64; 3],
        size: [f64; 2],
        burst: horizon_client::particles::Burst,
    ) {
        let Some(ty) = horizon_client::particles::ParticleType::from_name(kind) else {
            return;
        };
        let options = horizon_client::particles::ParticleOptions::simple(ty);
        let mut spawned = Vec::new();
        horizon_client::particles::around_self(
            self.particles.random(),
            feet,
            size[0],
            size[1],
            burst,
            |p, v| spawned.push((p, v)),
        );
        for (p, v) in spawned {
            self.particles.add(&options, p, v);
        }
    }
    /// `ReputationEventType.TRADE`: each trade earns a little trust.
    pub(super) fn villager_traded(&mut self, id: u32) {
        if let Some(uuid) = self.level.player_uuid {
            self.villager_traded_by(id, uuid);
        }
    }
    /// The TRADE reputation event from the player `uuid` (a guest's too).
    pub(super) fn villager_traded_by(&mut self, id: u32, uuid: [i32; 4]) {
        let Some(i) = self.villager_index(id) else {
            return;
        };
        villager::ext_mut(&mut self.mobs[i]).gossips.add(
            uuid,
            GossipType::Trading,
            gossip::REPUTATION_CHANGE_PER_TRADE,
        );
        // Villager.customServerAiStep after a trade: entity event 14.
        self.mob_event_particles(id, 14);
    }
}
