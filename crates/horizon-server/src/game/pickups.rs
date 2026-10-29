//! `ItemPickupParticle`: a taken item entity, experience orb or arrow flies
//! into whoever took it over three ticks (`ClientPacketListener.handleTakeItemEntity`
//! adds one for every `ClientboundTakeItemEntityPacket`), drawn with the render
//! state it had when it was taken (`extractEntity(from, 1.0F)`).
use crate::hc::{entities::{ExperienceOrb, ItemEntity}, projectiles::Projectile};


/// The taken entity, as it was when taken.
pub(super) enum Taken {
    Item(Box<ItemEntity>),
    Orb(Box<ExperienceOrb>),
    Arrow(Box<Projectile>),
}
impl Taken {
}

/// Whoever took it: the particle's `target`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Collector {
    Player,
    /// A mob, or another player on a server (both kept as mobs here).
    Mob(u32),
    /// A LAN guest, by entity id (the host draws it as a model).
    Guest(u32),
}

pub(super) struct Pickup {
    pub taken: Taken,
    collector: Collector,
    life: u32,
    target: [f64; 3],
    target_old: [f64; 3],
}

/// `ParticleEngine`'s ITEM_PICKUP group and the particles waiting to join it.
#[derive(Default)]
pub(super) struct Pickups {
    pub live: Vec<Pickup>,
    added: Vec<Pickup>,
}
impl Pickups {
    pub fn clear(&mut self) {
        self.live.clear();
        self.added.clear();
    }
}


/// A LAN guest's target, from the model the host draws for it.
pub(super) fn guest_target(host: &super::lan::LanHost, id: u32) -> Option<[f64; 3]> {
    let m = host.models.values().find(|m| m.id == id)?;
    let (p, eye) = (m.position, m.eye_height());
    Some([p.x, (p.y + (p.y + eye)) / 2.0, p.z])
}

impl crate::game::Sim<'_> {
    /// `ItemPickupParticle.updatePosition`: the collector's x and z, halfway up to its eyes.
    fn collector_target(&self, collector: Collector) -> Option<[f64; 3]> {
        let (p, eye) = match collector {
            Collector::Player => {
                let p = self.player.as_ref()?;
                (p.position, f64::from(p.dimensions.eye_height))
            }
            Collector::Mob(id) => {
                let m = self.mobs.iter().find(|m| m.id == id)?;
                (m.position, m.eye_height())
            }
            Collector::Guest(id) => {
                return guest_target(self.lan.as_ref()?, id);
            }
        };
        Some([p.x, (p.y + (p.y + eye)) / 2.0, p.z])
    }
    /// `handleTakeItemEntity`: `particleEngine.add(new ItemPickupParticle(...))` when the
    /// collector is known; it joins the group at the end of the next particle tick.
    pub(super) fn add_pickup(&mut self, taken: Taken, collector: Collector) {
        if let Some(target) = self.collector_target(collector) {
            self.add_pickup_toward(taken, collector, target);
        }
    }
    /// [`Self::add_pickup`] with the collector's first target given (a LAN
    /// guest's model while the host is taken out for its tick).
    pub(super) fn add_pickup_toward(
        &mut self,
        taken: Taken,
        collector: Collector,
        target: [f64; 3],
    ) {
        self.pickups.added.push(Pickup {
            taken,
            collector,
            life: 0,
            target,
            target_old: target,
        });
    }
}
