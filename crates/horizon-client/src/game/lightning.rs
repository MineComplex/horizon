//! Lightning (`ServerLevel.tickThunder`, `LightningBolt`,
//! `LightningBoltRenderer`): strikes on the highest motion-blocking block or
//! a living entity under open sky nearby, skeleton trap horses, fire,
//! `thunderHit` (damage and burning, charged creepers, zombified piglins,
//! witches, mooshroom colours, turtles, burnt items), lightning rods, the sky
//! flash, the sounds and the branching additive bolt.
use super::{Game, commands::Target};
use horizon_client::advancements::EntityView;
use horizon_client::mobs::{Damage, DamageKind, MobEvent, SpawnSetup, Species};
use horizon_core::{position::BlockPos, random::RandomSource};

pub(super) struct Bolt {
    /// The entity id a server's players know it by (0 until it has one).
    pub id: u32,
    pub at: [f64; 3],
    life: i32,
    flashes: i32,
    seed: i64,
    visual_only: bool,
    /// Mooshrooms and copper golems change once per bolt.
    converted: Vec<u32>,
    /// The mobs it has struck (`hitEntities`).
    hit: Vec<u32>,
    /// Blocks it set on fire (`blocksSetOnFire`).
    fires: i32,
    /// The player who channelled it (`LightningBolt.cause`): `Some(None)` for
    /// the host, `Some(Some(conn))` for a guest.
    pub cause: Option<Option<u32>>,
}
impl Game<'_> {}
impl crate::game::Sim<'_> {
    pub(super) fn spawn_bolt(&mut self, at: [f64; 3], visual_only: bool) {
        if !visual_only {
            let ctx = horizon_client::sculk::Context::default();
            self.post_game_event("minecraft:lightning_strike", at, ctx);
        }
        let r = &mut self.world.random;
        let seed = r.next_long();
        let flashes = r.next_int_bound(3).unwrap_or(0) + 1;
        self.lightning_seen[0] += 1;
        self.bolts.push(Bolt {
            id: 0,
            at,
            life: 2,
            flashes,
            seed,
            visual_only,
            converted: Vec::new(),
            hit: Vec::new(),
            fires: 0,
            cause: None,
        });
    }
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
            visual_only: true,
            converted: Vec::new(),
            hit: Vec::new(),
            fires: 0,
            cause: None,
        });
    }
    /// `spawnFire`: fire at the strike point and `extra` random neighbours,
    /// wherever `BaseFireBlock.getState` fire can survive. Returns how many
    /// blocks it set on fire.
    fn bolt_fire(&mut self, at: [f64; 3], extra: i32) -> i32 {
        let mut set = 0;
        let base = [
            at[0].floor() as i32,
            at[1].floor() as i32,
            at[2].floor() as i32,
        ];
        let mut spots = vec![base];
        for _ in 0..extra {
            let r = &mut self.world.random;
            let mut d = || r.next_int_bound(3).unwrap_or(1) - 1;
            spots.push([base[0] + d(), base[1] + d(), base[2] + d()]);
        }
        for p in spots {
            let state = self.world.block(p[0], p[1], p[2]);
            if !self.res.facts().is_air(state) {
                continue;
            }
            // BaseFireBlock.getState and FireBlock/SoulFireBlock.canSurvive.
            let at = BlockPos::new(p[0], p[1], p[2]);
            let fire = self.with_level(|b, level, ticks| {
                let fire = b.fire_state_for(level, ticks, at);
                b.can_survive(level, ticks, at, fire).then_some(fire)
            });
            if let Some(fire) = fire {
                let _ = self.level_set(p, fire);
                set += 1;
            }
        }
        set
    }
    /// `LightningBolt.tick` for every bolt.
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
                if !b.visual_only {
                    // Only Normal and Hard start fires with the strike itself.
                    if self.world_data.difficulty >= 2 {
                        b.fires += self.bolt_fire(b.at, 4);
                    }
                    self.strike_rod(b.at);
                }
                // LightningBolt.tick runs clearCopperOnLightningStrike for
                // every server bolt, a skeleton trap's visual one included;
                // a client's copy of a server bolt (spawn_remote_bolt) only
                // flashes.
            }
            b.life -= 1;
            if b.life < 0 {
                if b.flashes == 0 {
                    self.lightning_strike_triggers(b);
                    return false;
                }
                if b.life < -self.world.random.next_int_bound(10).unwrap_or(0) {
                    b.flashes -= 1;
                    b.life = 1;
                    b.seed = self.world.random.next_long();
                    if !b.visual_only {
                        b.fires += self.bolt_fire(b.at, 0);
                    }
                }
            }
            if b.life >= 0 {
                if self.flash == 0 {
                    self.lightning_seen[1] += 1;
                }
                self.flash = 2;
                if !b.visual_only {
                    let struck = self.thunder_hit(b);
                    if let Some(cause) = b.cause {
                        let who = cause.map_or(Target::Player, Target::Guest);
                        self.for_player(who, |g| g.trigger_channeled_lightning(&struck));
                    }
                }
            }
            true
        });
        bolts.append(&mut self.bolts);
        self.bolts = bolts;
    }
    /// `LightningRodBlock.onLightningStrike`: powered for 8 ticks.
    fn strike_rod(&mut self, at: [f64; 3]) {
        let p = [
            at[0].floor() as i32,
            (at[1] - 1.0e-6).floor() as i32,
            at[2].floor() as i32,
        ];
        let state = self.world.block(p[0], p[1], p[2]);
        if !self.res.facts().get(state).name.ends_with("lightning_rod") {
            return;
        }
        let registry = self.pack.assets.registry;
        if let Some(mut s) = registry.state(state).cloned() {
            s.properties.insert("powered".into(), "true".into());
            if let Some(powered) = registry.id(&s) {
                // The rod's onPlace schedules the 8-tick unpowering.
                let _ = self.level_set(p, powered);
            }
            // Level event 3002 with the rod's axis: sparks along it.
            let axis = match s.properties.get("facing").map(String::as_str) {
                Some("east" | "west") => 0,
                Some("north" | "south") => 2,
                _ => 1,
            };
            self.level_event_at(3002, p, axis);
        }
    }
    /// `LightningStrikeTrigger` at the bolt's discard: every player within 256
    /// blocks, with the mobs in its 30-block box that it did not hit.
    fn lightning_strike_triggers(&mut self, b: &Bolt) {
        let at = b.at;
        let bolt = EntityView {
            kind: "minecraft:lightning_bolt".into(),
            position: at,
            blocks_set_on_fire: b.fires,
            ..Default::default()
        };
        let viewers: Vec<EntityView> = self
            .mobs
            .iter()
            .filter(|m| m.alive() && !b.hit.contains(&m.id))
            .filter(|m| {
                (m.position.x - at[0]).abs() <= 15.0
                    && (m.position.z - at[2]).abs() <= 15.0
                    && m.position.y >= at[1] - 15.0
                    && m.position.y <= at[1] + 21.0
            })
            .map(|m| self.mob_entity(m))
            .collect();
        self.for_each_player(|g| {
            let Some(p) = g.player.as_ref().map(|p| p.position) else {
                return;
            };
            let d2 = (p.x - at[0]).powi(2) + (p.y - at[1]).powi(2) + (p.z - at[2]).powi(2);
            if d2 < 256.0 * 256.0 {
                g.trigger_lightning_strike(&bolt, &viewers);
            }
        });
    }
    /// `thunderHit` for entities within 3 blocks (up to 9 above). Returns the
    /// mobs in that box before the hit, as `CHANNELED_LIGHTNING` counts them.
    fn thunder_hit(&mut self, b: &mut Bolt) -> Vec<EntityView> {
        let at = b.at;
        let inside = move |p: [f64; 3]| {
            (p[0] - at[0]).abs() <= 3.0
                && (p[2] - at[2]).abs() <= 3.0
                && p[1] >= at[1] - 3.0
                && p[1] <= at[1] + 9.0
        };
        let struck: Vec<(u32, EntityView)> = self
            .mobs
            .iter()
            .filter(|m| m.alive() && inside([m.position.x, m.position.y, m.position.z]))
            .map(|m| (m.id, self.mob_entity(m)))
            .collect();
        b.hit.extend(struck.iter().map(|(id, _)| *id));
        let damage = Damage {
            kind: DamageKind::Generic,
            attacker: None,
            from: Some(b.at),
        };
        let mut events = Vec::new();
        let peaceful = self.world_data.difficulty == 0;
        for m in self.mobs.iter_mut().filter(|m| m.alive()) {
            if !inside([m.position.x, m.position.y, m.position.z]) {
                continue;
            }
            let setup = |m: &horizon_client::mobs::Mob| SpawnSetup {
                yaw: m.yaw,
                persistent: true,
                ..Default::default()
            };
            let at = [m.position.x, m.position.y, m.position.z];
            match m.species() {
                Species::Pig if !peaceful => {
                    m.removed = true;
                    events.push(MobEvent::SpawnKind {
                        kind: "minecraft:zombified_piglin".into(),
                        at,
                        setup: setup(m),
                    });
                    continue;
                }
                Species::Villager if !peaceful => {
                    m.removed = true;
                    events.push(MobEvent::SpawnKind {
                        kind: "minecraft:witch".into(),
                        at,
                        setup: setup(m),
                    });
                    continue;
                }
                Species::Mooshroom => {
                    if !b.converted.contains(&m.id) {
                        b.converted.push(m.id);
                        m.extra.variant = 1 - m.extra.variant.min(1);
                        self.sounds
                            .play("minecraft:entity.mooshroom.convert", Some(at), 2.0, 1.0);
                    }
                    continue;
                }
                Species::Turtle => {
                    m.damage(f32::MAX, damage);
                    continue;
                }
                Species::Creeper => m.extra.charged = true,
                // CopperGolem.thunderHit (after Entity.thunderHit's fire and
                // damage): once per bolt, an oxidized golem goes back one step
                // and its weathering timer (or wax, -2) is reset to -1.
                Species::CopperGolem if !b.converted.contains(&m.id) => {
                    b.converted.push(m.id);
                    if m.extra.variant > 0 {
                        m.extra.vector[0] = -1.0;
                        m.extra.variant =
                            horizon_client::mobs::copper_golem::weather_previous(m.extra.variant);
                    }
                }
                _ => {}
            }
            // Entity.thunderHit: burning (8 seconds) and 5 lightning damage.
            m.fire_ticks += 1;
            if m.fire_ticks == 0 {
                m.ignite_for_ticks(160);
            }

            m.damage(5.0, damage);
        }
        if !events.is_empty() {
            let _ = self.handle_mob_events(events);
        }
        // Every player in the box, the server's players too.
        let strike = move |g: &mut Self| {
            if let Some(p) = g.player.as_ref().map(|p| p.position)
                && inside([p.x, p.y, p.z])
            {
                if let Some(p) = g.player.as_mut() {
                    p.fire_ticks += 1;
                    if p.fire_ticks == 0
                        && let Err(error) = p.ignite_for_ticks(160)
                    {
                        g.error = Some(error);
                    }
                }
                g.hurt_player_by("minecraft:lightning_bolt", 5.0);
            }
        };
        strike(self);
        self.each_guest_hit(strike);
        // Dropped items burn up (ItemEntity health 5), save fire-resistant ones.
        self.dropped.retain(|e| {
            !inside([e.position.x, e.position.y, e.position.z])
                || e.stack.item.contains("netherite")
        });
        struck.into_iter().map(|(_, view)| view).collect()
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
