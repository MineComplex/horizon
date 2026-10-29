//! What villagers do with each other and with the player: gossip and
//! reputation, sharing food and wares, picking up dropped items, making
//! babies, calling iron golems and the prices they ask.
use super::Game;
use horizon_client::{
    gossip::{self, GossipType},
    items::ItemStack,
    mobs::{Mob, Species, spawning, villager, villager_brain},
};
use horizon_core::geometry::Vec3;

fn block_of(p: Vec3) -> [i32; 3] {
    [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
}
impl crate::game::Sim<'_> {
    fn villager_index(&self, id: u32) -> Option<usize> {
        self.mobs
            .iter()
            .position(|m| m.id == id && m.species() == Species::Villager)
    }
    fn item_max_stack(&self, item: &str) -> u32 {
        self.res.items().get(item).map_or(64, |i| i.max_stack)
    }
    /// `WorkAtComposter.useWorkstation`: at a composter the farmer makes
    /// bread, then composts seeds.
    pub(super) fn villager_work_at_composter(&mut self, id: u32, pos: [i32; 3]) {
        let Some(i) = self.villager_index(id) else {
            return;
        };
        if self.composter_level(pos).is_none() {
            return;
        }
        self.villager_make_bread(i);
        self.villager_compost_items(i, pos);
    }
    /// `WorkAtComposter.makeBread`: up to three loaves from nine wheat while
    /// the villager holds at most 36 bread; what does not fit drops.
    fn villager_make_bread(&mut self, i: usize) {
        let max_stack = self.item_max_stack("minecraft:bread");
        let e = villager::ext_mut(&mut self.mobs[i]);
        if e.count_item("minecraft:bread") > 36 {
            return;
        }
        let loaves = (e.count_item("minecraft:wheat") / 3).min(3);
        if loaves == 0 {
            return;
        }
        e.remove_item("minecraft:wheat", loaves * 3);
        let left = e.add_item(ItemStack::new("minecraft:bread", loaves), max_stack);
        if let Some(rest) = left {
            // spawnAtLocation(level, stack, 0.5F)
            let p = self.mobs[i].position;
            self.spawn_item(rest, [p.x, p.y + 0.5, p.z], [0.0, 0.2, 0.0], 10);
        }
    }
    /// `BehaviorUtils.throwItem`: toss a stack from a villager's hand toward
    /// a point.
    fn villager_throw(&mut self, from: Vec3, eye: f64, to: Vec3, stack: ItemStack) {
        let at = [from.x, eye - 0.3, from.z];
        let d = Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z).normalize();
        self.spawn_item(stack, at, [d.x * 0.3, d.y * 0.3, d.z * 0.3], 10);
    }
    /// `TradeWithVillager.tick` for one pair: gossip (at most once in 1200
    /// ticks for both), then surplus food and wanted wares tossed over.
    pub(super) fn villager_meet(&mut self, a: u32, b: u32) {
        let (Some(ia), Some(ib)) = (self.villager_index(a), self.villager_index(b)) else {
            return;
        };
        let now = self.world.game_time;
        let items = self.res.items();
        let meeting = villager::meet(&mut self.mobs, ia, ib, now, |item| {
            items.get(item).map_or(64, |i| i.max_stack)
        });
        for stack in meeting.thrown {
            self.villager_throw(meeting.from, meeting.eye, meeting.to, stack);
        }
        if meeting.gossiped {
            self.villager_golem_check(a, 5);
        }
    }
    /// `Villager.spawnGolemIfNeeded`: when enough villagers nearby that
    /// slept recently and saw no golem agree, an iron golem appears.
    pub(super) fn villager_golem_check(&mut self, id: u32, needed: usize) {
        let Some(i) = self.villager_index(id) else {
            return;
        };
        let now = self.world.game_time;
        let Some(near) = villager::golem_summoners(&self.mobs, i, now, needed) else {
            return;
        };
        if self.try_spawn_iron_golem(block_of(self.mobs[i].position)) {
            for j in near {
                villager_brain::golem_detected(&mut self.mobs[j], now);
            }
        }
    }
    /// `SpawnUtil.trySpawnMob(IRON_GOLEM, ...)`: a golem near `start`, with
    /// the ambient sound `trySpawnMob` plays.
    fn try_spawn_iron_golem(&mut self, start: [i32; 3]) -> bool {
        let Some(kind) = self.res.mob_types().get("minecraft:iron_golem").cloned() else {
            return false;
        };
        let id = self.next_entity + 1;
        let mut random = self.world.random.clone();
        let golem = villager::summon_golem(
            &self.mob_world_base(&[]),
            start,
            &mut random,
            |random, at, yaw| {
                let mut m = Mob::new(id, kind.clone(), at, yaw);
                villager::finalize_summoned_golem(&mut m, random);
                m
            },
        );
        self.world.random = random;
        let Some(golem) = golem else {
            return false;
        };
        self.next_entity = id;
        let sound = golem.kind.sounds[0];
        if !sound.is_empty() {
            let at = golem.center();
            self.sounds
                .play(&format!("minecraft:{sound}"), Some(at), 1.0, 1.0);
        }
        self.mobs.push(golem);
        true
    }
    /// `VillagerMakeLove.tryToGiveBirth`: both eat, a free bed within 48
    /// blocks is taken for the child, who is a baby villager of one of
    /// three villager types.
    pub(super) fn villager_birth(
        &mut self,
        parent: u32,
        partner: u32,
        child: Option<([i32; 3], Option<u8>)>,
    ) {
        let (Some(ia), Some(ib)) = (self.villager_index(parent), self.villager_index(partner))
        else {
            return;
        };
        villager::ext_mut(&mut self.mobs[ib]).eat_and_digest();
        let from = self.mobs[ia].position;
        let Some((bed, variant)) = child else {
            // VillagerMakeLove: entity event 13 for the partner, then the parent.
            for i in [ib, ia] {
                let id = self.mobs[i].id;
                self.mob_event_particles(id, 13);
            }
            return;
        };
        let variant = variant.unwrap_or_else(|| {
            let facts = self.biome_facts(
                from.x.floor() as i32,
                from.y.floor() as i32,
                from.z.floor() as i32,
            );
            spawning::villager_type(&facts)
        });
        self.mobs[ib].extra.age = 6000;
        let Some(id) =
            self.spawn_mob_for("minecraft:villager", [from.x, from.y, from.z], true, true)
        else {
            return;
        };
        if let Some(child) = self.mobs.iter_mut().find(|m| m.id == id) {
            child.extra.variant = variant;
            child.extra.variant2 = villager::NONE;
            child.extra.age = -24000;
            child.extra.bed = Some(bed);
        }
        // VillagerMakeLove.giveBirth: entity event 12 for the child.
        self.mob_event_particles(id, 12);
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
