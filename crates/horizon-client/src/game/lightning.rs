//! Lightning (`ServerLevel.tickThunder`, `LightningBolt`,
//! `LightningBoltRenderer`): strikes on the highest motion-blocking block or
//! a living entity under open sky nearby, skeleton trap horses, fire,
//! `thunderHit` (damage and burning, charged creepers, zombified piglins,
//! witches, mooshroom colours, turtles, burnt items), lightning rods, the sky
//! flash, the sounds and the branching additive bolt.
use horizon_core::random::RandomSource;

pub(super) struct Bolt {
    /// The entity id a server's players know it by (0 until it has one).
    pub id: u32,
    pub at: [f64; 3],
    life: i32,
    flashes: i32,
    seed: i64,
}
impl crate::game::Sim<'_> {
    /// `ClientLevel.addEntity` for a server's lightning bolt: on a client it
    /// only flashes, sounds and draws (`LightningBolt.tick` off the server).
    pub(super) fn spawn_remote_bolt(&mut self, id: u32, at: [f64; 3]) {
        self.bolts.retain(|b| b.id != id);
        let r = &mut self.world.random;
        let seed = r.next_long();
        let flashes = r.next_int_bound(3).unwrap_or(0) + 1;
        self.lightning_seen[0] += 1;
        self.bolts.push(Bolt {
            id,
            at,
            life: 2,
            flashes,
            seed,
        });
    }
    /// `LightningBolt.tick` on a client for every bolt: the sounds and the flashes.
    pub(super) fn tick_bolts(&mut self) {
        let mut bolts = std::mem::take(&mut self.bolts);
        bolts.retain_mut(|b| {
            if b.life == 2 {
                let pitch = 0.8 + self.world.random.next_float() * 0.2;
                self.sounds.play(
                    "minecraft:entity.lightning_bolt.thunder",
                    Some(b.at),
                    10000.0,
                    pitch,
                );
                let pitch = 0.5 + self.world.random.next_float() * 0.2;
                self.sounds.play(
                    "minecraft:entity.lightning_bolt.impact",
                    Some(b.at),
                    2.0,
                    pitch,
                );
                // The fires, the rod and the copper are the server's.
            }
            b.life -= 1;
            if b.life < 0 {
                if b.flashes == 0 {
                    return false;
                }
                if b.life < -self.world.random.next_int_bound(10).unwrap_or(0) {
                    b.flashes -= 1;
                    b.life = 1;
                    b.seed = self.world.random.next_long();
                }
            }
            if b.life >= 0 {
                if self.flash == 0 {
                    self.lightning_seen[1] += 1;
                }
                // A client only flashes the sky; the server hits what is near.
                self.flash = 2;
            }
            true
        });
        bolts.append(&mut self.bolts);
        self.bolts = bolts;
    }
    /// Simulation-order render inputs; seeded geometry lives in the renderer.
    pub(super) fn bolt_instances(&self) -> Vec<horizon_client::gfx::lightning::Bolt> {
        self.bolts
            .iter()
            .map(|b| horizon_client::gfx::lightning::Bolt {
                at: b.at,
                seed: b.seed,
            })
            .collect()
    }
}
