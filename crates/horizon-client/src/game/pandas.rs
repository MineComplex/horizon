//! Pandas on the game side: the food they find on the ground and pick up
//! (`Mob.aiStep`, `Panda.pickUpItem`), the bamboo a player gives them
//! (`Panda.mobInteract`), the start a sneeze gives the grown pandas about,
//! and the crumbs and the sneeze a window draws.
use horizon_client::{
    items::ItemStack,
    mobs::{Species, panda},
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::geometry::Vec3;

impl crate::game::Sim<'_> {
    /// `Panda.afterSneeze`: the grown pandas within 10 blocks of a cub jump.
    pub(super) fn panda_sneezed(&mut self, id: u32) {
        let Some(area) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.bounds().inflate(Vec3::new(10.0, 10.0, 10.0)))
        else {
            return;
        };
        for m in &mut self.mobs {
            if m.species() == Species::Panda
                && !m.is_baby()
                && m.on_ground
                && !m.fluids.in_water
                && panda::can_perform_action(m, panda::scared(m))
                && area.intersects(m.bounds())
            {
                // LivingEntity.jumpFromGround.
                m.needs_sync = true;
                let power = f64::from(m.jump_power());
                if power > 1.0e-5 {
                    m.velocity.y = power.max(m.velocity.y);
                }
            }
        }
    }
    /// What a window draws of its pandas: the crumbs of a bite
    /// (`addEatingParticles`) and the sneeze's particle.
    pub(super) fn panda_effects(&mut self) {
        let mut bites = Vec::new();
        let mut sneezes = Vec::new();
        for m in self
            .mobs
            .iter_mut()
            .filter(|m| m.species() == Species::Panda)
        {
            let (crumbs, sneeze) = panda::effects(m);
            if let (Some(crumbs), Some(item)) = (crumbs, m.extra.main_hand.clone()) {
                bites.push((item, crumbs));
            }
            sneezes.extend(sneeze);
        }
        for (item, crumbs) in bites {
            let options = self.item_particle(&ItemStack::new(&item, 1));
            for (at, velocity) in crumbs {
                self.particles.add(&options, at, velocity);
            }
        }
        let options = ParticleOptions::simple(ParticleType::Sneeze);
        for (at, velocity) in sneezes {
            self.particles.add(&options, at, velocity);
        }
    }
}
