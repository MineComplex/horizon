//! Evoker fangs (`EvokerFangs`): rising out of the floor after a delay,
//! biting what stands on them for 6 damage, sinking back after 22 ticks.
use crate::hc::mobs::{Part, Species, anim::Poses};
use horizon_core::geometry::Vec3;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(super) struct Fangs {
    pub id: u32,
    pub pos: [f64; 3],
    pub yaw: f32,
    warmup: i32,
    pub life: i32,
    owner: u32,
    pub started: bool,
}
#[derive(Default)]
pub(super) struct FangState {
    pub list: Vec<Fangs>,
    model: Option<Arc<Part>>,
}
impl crate::game::Sim<'_> {
    pub(super) fn add_fangs(&mut self, at: [f64; 3], yaw: f32, delay: i32, owner: u32) {
        self.next_entity += 1;
        self.insert_fangs(self.next_entity, at, yaw, delay, owner);
    }
    pub(super) fn insert_fangs(&mut self, id: u32, at: [f64; 3], yaw: f32, delay: i32, owner: u32) {
        if self.fangs.model.is_none() {
            self.fangs.model = crate::hc::mobs::load_model(
                &self.options.resources,
                "minecraft:evoker_fangs#main",
            )
            .map(Arc::new);
        }
        self.fangs.list.push(Fangs {
            id,
            pos: at,
            yaw,
            warmup: delay,
            life: 22,
            owner,
            started: false,
        });
    }
    /// `EvokerFangs.handleEntityEvent`: only event 4 starts a client attack.
    pub(super) fn remote_fangs_event(&mut self, id: u32, event: i8) {
        if event == 4
            && let Some(f) = self.fangs.list.iter_mut().find(|f| f.id == id)
        {
            f.started = true;
            self.sounds.play_unshared(
                "minecraft:entity.evoker_fangs.attack",
                Some(f.pos),
                1.0,
                1.0,
            );
        }
    }
    /// `EvokerFangs.tick`: clients only advance animation, never damage or discard.
    pub(super) fn tick_remote_fangs(&mut self) {
        for f in &mut self.fangs.list {
            if f.started {
                f.life -= 1;
            }
        }
    }
    /// `EvokerFangs.tick`.
    pub(super) fn tick_fangs(&mut self) {
        let mut list = std::mem::take(&mut self.fangs.list);
        list.retain_mut(|f| {
            f.warmup -= 1;
            if f.warmup >= 0 {
                return true;
            }
            if f.warmup == -8 {
                self.fangs_bite(f);
            }
            if !f.started {
                f.started = true;
                self.lan_entity_event(f.id as i32, 4, f.pos);
                self.sounds.play_unshared(
                    "minecraft:entity.evoker_fangs.attack",
                    Some(f.pos),
                    1.0,
                    1.0,
                );
            }
            f.life -= 1;
            f.life >= 0
        });
        self.fangs.list = list;
    }
    /// `dealDamageTo` everything alive in the fangs' box (0.5 x 0.8,
    /// inflated 0.2 sideways) but the evoker's allies.
    fn fangs_bite(&mut self, f: &Fangs) {
        let half = 0.25 + 0.2;
        let lo = Vec3::new(f.pos[0] - half, f.pos[1], f.pos[2] - half);
        let hi = Vec3::new(f.pos[0] + half, f.pos[1] + 0.8, f.pos[2] + half);
        let b = horizon_core::geometry::Aabb::new(lo, hi);
        // Every player in the box, the server's players too.
        let pos = f.pos;
        // `EvokerFangs.dealDamageTo`: indirectMagic(fangs, owner) while the
        // owner lives, plain magic (nobody to name) once it is gone.
        let owner = self
            .mobs
            .iter()
            .any(|m| m.id == f.owner && m.alive())
            .then_some(f.owner);
        let bite = move |g: &mut Self| {
            if let Some(p) = g.player_view()
                && p.alive
                && p.bounds.intersects(b)
            {
                let kind = match owner {
                    Some(_) => "minecraft:indirect_magic",
                    None => "minecraft:magic",
                };
                g.mob_hits_player_caused(6.0, pos, kind, owner);
            }
        };
        bite(self);
        self.each_guest_hit(bite);
        // `AbstractIllager.considersEntityAsAlly`: with no teams, the illager
        // tag is allied; `Evoker.considersEntityAsAlly` adds a vex whose
        // owner is allied (itself or another illager). Teams are not modelled.
        let illager = |s: Species| {
            matches!(
                s,
                Species::Evoker | Species::Illusioner | Species::Pillager | Species::Vindicator
            )
        };
        let illagers: Vec<u32> = self
            .mobs
            .iter()
            .filter(|m| illager(m.species()))
            .map(|m| m.id)
            .collect();
        // A missing owner hurts everyone (`currentOwner == null`).
        let owner_known = illagers.contains(&f.owner);
        for m in &mut self.mobs {
            let ally = owner_known
                && (illager(m.species())
                    || m.species() == Species::Vex
                        && illagers.contains(&(m.extra.counter2 as u32)));
            if m.id != f.owner && !ally && m.alive() && m.bounds().intersects(b) {
                m.damage(
                    6.0,
                    crate::hc::mobs::Damage {
                        kind: crate::hc::mobs::DamageKind::Magic,
                        attacker: Some(crate::hc::mobs::Who::Mob(f.owner)),
                        from: Some(f.pos),
                    },
                );
            }
        }
    }
}
fn p_zrot(p: &mut Poses, part: &str, v: f32) {
    p.zrot(part, v);
}
