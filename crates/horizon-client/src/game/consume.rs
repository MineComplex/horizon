//! Finishing a consumable (`Consumable.onConsume` and the use remainder),
//! its particles and sounds, and `TeleportRandomlyConsumeEffect` (chorus
//! fruit) for the local player. The level random stands in for the player's
//! own, as for every other player draw.
use horizon_client::items::{ConsumeEffect, ItemStack};
use horizon_core::random::LegacyRandom;
use horizon_world::consumable::{Consumable, EmissionWorld};
use std::convert::Infallible;

type ItemConsumable = Consumable<String, ConsumeEffect>;

/// `EntityEvent.USE_ITEM_COMPLETE`: the server finished the player's use.
pub(super) const USE_ITEM_COMPLETE_EVENT: i8 = 9;

impl crate::game::Game<'_> {
    /// `Consumable.emitParticlesAndSounds(user.getRandom(), user, stack, count)`.
    pub(super) fn emit_consume(
        &mut self,
        consumable: &ItemConsumable,
        stack: &ItemStack,
        count: i32,
    ) {
        let Ok(()) = consumable.emit(&mut Emitter(self), (), stack.clone(), count);
    }
    fn feet(&self) -> Option<[f64; 3]> {
        self.player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z])
    }
}

/// `Consumable.emitParticlesAndSounds` against the local player.
struct Emitter<'a, 'g>(&'a mut crate::game::Game<'g>);
impl EmissionWorld for Emitter<'_, '_> {
    type Error = Infallible;
    type Stack = ItemStack;
    type Sound = String;
    type Random = ();
    type Source = LegacyRandom;
    fn random_source(&mut self, (): ()) -> &mut LegacyRandom {
        &mut self.0.world.random
    }
    /// `LivingEntity.spawnItemParticles`.
    fn spawn_item_particles(&mut self, stack: ItemStack, count: i32) -> Result<(), Infallible> {
        self.0.eat_particles(&stack, count.max(0) as u32);
        Ok(())
    }
    fn overrides_consume_sound(&self) -> bool {
        false
    }
    fn consume_sound(&mut self, _: ItemStack) -> Result<String, Infallible> {
        unreachable!("players play the consumable's own sound")
    }
    /// `Entity.playSound`: at the player's position.
    fn play_sound(&mut self, sound: String, volume: f32, pitch: f32) -> Result<(), Infallible> {
        let at = self.0.feet();
        self.0.sounds.play(&sound, at, volume, pitch);
        Ok(())
    }
}
