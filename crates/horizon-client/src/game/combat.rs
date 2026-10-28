//! The player's `CombatTracker`: the damage taken lately, who dealt it and
//! where the player fell from, turned into the vanilla death message.
use super::Game;
use horizon_client::{
    particles::{ParticleOptions, ParticleType},
    text::Text};
use horizon_core::random::RandomSource;

/// `EntityEvent.DROWN_PARTICLES`: broadcast with the drowning damage.
pub(super) const DROWN_EVENT: i8 = 67;

#[derive(Default)]
pub(super) struct CombatTracker {
    /// Attackers of damage dealt this tick, claimed by the matching
    /// `RecordedDamage` event.
    pending: Vec<(String, (u32, Text))>,
    /// `pending` for a direct entity with no causing entity: only its name.
    pending_direct: Vec<(String, Text)>,
    /// `LivingEntity.lastHurtByMob` and its timestamp (the kill credit).
    last_hurt_by: Option<((u32, Text), i64)>,
    /// The mob the player last hurt (`getLastHurtMob`) and when.
    pub(super) last_hurt_mob: Option<(u32, i64)>,
    /// The mob whose projectile is hitting the player now.
    pub(super) projectile_owner: Option<u32>,
    /// The display name of the projectile hitting the player now: the
    /// `DamageSource.directEntity` the death message names when no owner is
    /// known (`AbstractArrow.onHitEntity` makes an ownerless arrow its own
    /// cause).
    pub(super) projectile_direct: Option<Text>,
    /// The projectile hitting the player now is another player's that
    /// `ServerPlayer.canHarmPlayer` refuses (the `pvp` rule, an ally).
    pub(super) refuse_player_hit: bool,
    /// The player whose melee hit is landing on the player now: where they
    /// stand and the seconds their weapon disables a shield for
    /// (`getSecondsToDisableBlocking`).
    pub(super) player_attacker: Option<([f64; 3], f32)>,
    /// The mob exploding now (`ServerExplosion.source`): its team's friendly
    /// fire decides whom the blast may hurt.
    pub(super) explosion_source: Option<u32>,
    /// The blast now going off comes from a primed TNT, creeper, wither
    /// skull, wither or TNT minecart, the sources that empty a hive they
    /// destroy (`BeehiveBlock.getDrops`).
    pub(super) hive_emptying_blast: bool,
    /// The mob that owns the projectile exploding now (`getIndirectSourceEntity`):
    /// the mobs the blast hurts record it as their attacker.
    pub(super) explosion_owner: Option<u32>,
    /// Charged creepers whose blast just went off and that have not dropped
    /// a head yet (`Creeper.droppedSkulls`): they are gone from the mob list,
    /// but the mobs they killed still ask for their loot.
    pub(super) charged_exploders: Vec<u32>,
}
impl Game<'_> {}
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
    /// The display name of a mob: its custom name or its type's name.
    pub(super) fn mob_display_name(&self, id: u32) -> Option<Text> {
        let m = self.mobs.iter().find(|m| m.id == id)?;
        Some(match &m.custom_name {
            Some(name) => Text::literal(name.clone()),
            None => Text::translate(&format!("entity.{}", m.kind.id.replace(':', ".")), &[]),
        })
    }
    /// `getLastHurtByMob`: the last mob to hurt the player, for 100 ticks.
    pub(super) fn player_hurt_by(&self) -> Option<(horizon_client::mobs::Who, i64)> {
        let ((id, _), at) = self.combat.last_hurt_by.as_ref()?;
        (self.world.game_time - at <= 100).then_some((horizon_client::mobs::Who::Mob(*id), *at))
    }
    /// Remember who deals the next `damage_type` damage to the player.
    pub(super) fn note_attacker(&mut self, damage_type: &str, mob: u32) {
        if let Some(name) = self.mob_display_name(mob) {
            self.note_named_attacker(damage_type, mob, name);
        }
    }
    /// `note_attacker` for an entity that is not one of `self.mobs` (the
    /// ender dragon).
    pub(super) fn note_named_attacker(&mut self, damage_type: &str, id: u32, name: Text) {
        self.combat
            .pending
            .push((damage_type.to_owned(), (id, name)));
    }
    /// The `DamageSource.directEntity` of the next `damage_type` damage when
    /// nothing caused it (a splash potion nobody threw): the death message
    /// names it instead (`getLocalizedDeathMessage`). It is no kill credit.
    pub(super) fn note_direct_entity(&mut self, damage_type: &str, name: Text) {
        self.combat
            .pending_direct
            .push((damage_type.to_owned(), name));
    }
}
