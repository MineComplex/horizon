//! Spears (`KineticWeapon`): holding the use button charges the spear, and
//! from `delay_ticks` on every living entity along the view that the
//! player's speed allows is stabbed: the damage grows with the speed
//! relative to the target, the knockback and the dismount need the
//! attacker's own speed, and each condition runs out after its time
//! (`Item.use`, `ItemStack.onUseTick`, `KineticWeapon.damageEntities`,
//! `Player.stabAttack`). The hit's feedback (`onKineticHit`) reaches the
//! players around as entity event 2. A left click jabs instead
//! (`PiercingWeapon.attack`), which Lunge turns into a dash.
use horizon_client::{
    attack_strength::delay,
    spear::{HIT_FEEDBACK_TICKS, Kinetic},
};

/// The key of this window's own player in `Game::kinetic_feedback`.
pub(super) const OWN: i32 = i32::MIN;

impl crate::game::Sim<'_> {
    /// `Player.cannotAttackWithItem` for the held item: a spear needs its
    /// `minimum_attack_charge` of 1, counting `tolerance` ticks more than the
    /// `ticker` ticks charged.
    pub(super) fn cannot_attack_with_item(&self, ticker: i32, tolerance: i32) -> bool {
        self.held().is_some_and(|s| Kinetic::of(&s.item).is_some())
            && (ticker + tolerance) as f32 / delay(self.main_hand_attack_speed()) < 1.0
    }
    /// A sound at the player's feet.
    pub(super) fn play_at_feet(&mut self, sound: &str) {
        if let Some(p) = &self.player {
            let at = [p.position.x, p.position.y, p.position.z];
            self.sounds.play(sound, Some(at), 1.0, 1.0);
        }
    }
    /// `LivingEntity.onKineticHit` for `entity`: the hit feels (the spear
    /// moves back, `kinetic_hit_ticks`) and sounds, at most once every ten
    /// ticks.
    pub(super) fn kinetic_hit_feedback(&mut self, entity: i32, kinetic: &Kinetic, at: [f64; 3]) {
        let now = self.world.game_time;
        if self
            .kinetic_feedback
            .get(&entity)
            .is_none_or(|&t| now - t > HIT_FEEDBACK_TICKS)
        {
            self.kinetic_feedback.insert(entity, now);
            self.sounds
                .play_unshared(kinetic.hit_sound, Some(at), 1.0, 1.0);
        }
    }
    /// `getTicksSinceLastKineticHitFeedback` of this window's player.
    pub(super) fn kinetic_hit_ticks(&self, partial: f32) -> f32 {
        self.kinetic_feedback
            .get(&OWN)
            .map_or(0.0, |&t| (self.world.game_time - t) as f32 + partial)
    }
    /// `LivingEntity.handleEntityEvent(2)` on a server's client: the
    /// player `id` (this one, or another) was hit by the spear it uses.
    pub(super) fn remote_kinetic_hit(&mut self, id: i32) {
        let own = self.remote.as_ref().is_some_and(|r| r.entity_id == id);
        let (item, at) = if own {
            let item = self
                .using
                .and_then(|(slot, _)| self.stack_in(slot))
                .map(|s| s.item.clone());
            let at = self.player.as_ref().map(|p| p.position);
            (item, at)
        } else {
            let mob = self
                .mobs
                .iter()
                .find(|m| m.id == id as u32 && m.extra.using_item > 0);
            let item = mob.and_then(|m| {
                if m.extra.using_off_hand {
                    m.extra.off_hand.clone()
                } else {
                    m.extra.main_hand.clone()
                }
            });
            (item, mob.map(|m| m.position))
        };
        if let (Some(kinetic), Some(at)) = (item.as_deref().and_then(Kinetic::of), at) {
            self.kinetic_hit_feedback(if own { OWN } else { id }, &kinetic, [at.x, at.y, at.z]);
        }
    }
}
