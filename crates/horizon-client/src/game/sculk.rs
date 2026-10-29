//! Game events and sculk on the game side: dispatching events to sculk
//! sensors, shriekers and wardens (`GameEventDispatcher`), the vibration
//! ticker, redstone output, resonance, shrieks and warden summoning
//! (`SculkShriekerBlockEntity`, `SpawnUtil`), catalysts feeding the sculk
//! spreader from deaths, and the darkness wardens and shriekers cast.
use super::{Game, Result};
use horizon_client::{
    containers::BlockEntity,
    fast_hash::FxHashMap,
    mobs::{Mob, Species, Who, warden},
    particles::{ParticleOptions, ParticleType},
    sculk::{self, Context, Info, SculkEntity, SculkKind, WardenSpawnTracker},
};
use horizon_core::{geometry::Direction, position::BlockPos, random::RandomSource};
use horizon_world::registry::BlockStateId;
use std::collections::HashSet;
use winit::keyboard::KeyCode;

pub const HELP: &[&str] = &[
    "sculk                          sculk listeners, the warning level and the darkness blend",
    "sculk charge X Y Z AMOUNT      a mob dying there with that much experience (the nearest catalyst takes it)",
    "sculk warn LEVEL               set the player's warden warning level",
    "sculk dig                      end the minute of calm of every warden",
];

/// The sculk block kinds by state id, built once.
#[derive(Default)]
struct Tables {
    built: bool,
    occludes: Vec<bool>,
    dampens: Vec<bool>,
}
/// `SculkSensorBlock.getAnalogOutputSignal`: the last vibration's frequency
/// while the sensor is active.
pub(super) fn sensor_signal(
    world: &horizon_client::world::World,
    behaviours: &horizon_world::level::Behaviours,
    state: BlockStateId,
    pos: BlockPos,
) -> Option<i32> {
    match world.block_entities.get(&(pos.x, pos.y, pos.z)) {
        Some(BlockEntity::Sculk(e))
            if matches!(e.kind, SculkKind::Sensor | SculkKind::Calibrated) =>
        {
            Some(if behaviours.sculk_sensor_active(state) {
                e.last_frequency
            } else {
                0
            })
        }
        _ => None,
    }
}

#[derive(Default)]
pub(super) struct SculkState {
    /// The player's warning level toward a warden.
    pub tracker: WardenSpawnTracker,
    tables: Tables,
    /// The sculk block entities in the order they were added.
    index: Index,
    pub known: HashSet<(i32, i32)>,
    // Keyed by mob ids the server gives out: the fast hasher is safe.
    damaged: FxHashMap<u32, i64>,
    /// The darkness effect's blend factor, previous and current tick.
    darkness: (f32, f32),
    /// Who causes the block changes being processed (the player's actions).
    pub actor: Option<Who>,
}

/// An insertion-ordered set of block positions (`Level.blockEntityTickers`
/// keeps the order block entities were added in).
#[derive(Default)]
struct Index {
    order: Vec<(i32, i32, i32)>,
    set: HashSet<(i32, i32, i32)>,
}
impl Index {
    fn remove(&mut self, key: &(i32, i32, i32)) {
        if self.set.remove(key) {
            self.order.retain(|k| k != key);
        }
    }
    fn iter(&self) -> impl Iterator<Item = &(i32, i32, i32)> {
        self.order.iter()
    }
}

fn center(p: (i32, i32, i32)) -> [f64; 3] {
    [
        f64::from(p.0) + 0.5,
        f64::from(p.1) + 0.5,
        f64::from(p.2) + 0.5,
    ]
}
fn block_of(p: [f64; 3]) -> [i32; 3] {
    [
        p[0].floor() as i32,
        p[1].floor() as i32,
        p[2].floor() as i32,
    ]
}
fn dist_sqr(a: [i32; 3], b: (i32, i32, i32)) -> i64 {
    let d = |x: i32, y: i32| i64::from(x) - i64::from(y);
    d(a[0], b.0).pow(2) + d(a[1], b.1).pow(2) + d(a[2], b.2).pow(2)
}

impl Game<'_> {
    // ---- setup and the block entity index ----

    // ---- contexts ----

    // ---- posting ----

    // ---- ticking ----

    // ---- sensors ----

    // ---- shriekers ----

    // ---- darkness ----

    /// `LightmapRenderStateExtractor`: the night vision effect's intensity
    /// (`GameRenderer.nightVisionScale`), or conduit power's water vision.
    pub(super) fn night_vision_intensity(&self, partial: f32) -> f32 {
        use horizon_client::fog::FogType;
        if let Some(e) = self.potions.effects.get("night_vision") {
            // MobEffectInstance.endsWithin(200); -1 is infinite.
            return if e.duration < 0 || e.duration > 200 {
                1.0
            } else {
                0.7 + ((e.duration as f32 - partial) * std::f32::consts::PI * 0.2).sin() * 0.3
            };
        }
        if self.potions.effects.get("conduit_power").is_some()
            && let Some(eye) = self.eye_position()
        {
            let in_water =
                horizon_client::fog::fog_type(&self.world, self.res.facts(), eye, self.forward())
                    == FogType::Water;
            return self.front.fog.water_vision(in_water);
        }
        0.0
    }

    // ---- catalysts ----

    // ---- particles ----

    // ---- entity events ----
}
impl crate::game::Sim<'_> {
    /// Run `f` with the player as the cause of the block changes it makes.
    pub(super) fn with_player_actor<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let prior = self.sculk.actor.replace(Who::Player);
        let out = f(self);
        self.sculk.actor = prior;
        out
    }
    /// The `sculk` control command: listeners and their vibrations, or a
    /// test charge for the nearest catalyst.
    pub(super) fn sculk_command(&mut self, words: &[&str]) -> Result<serde_json::Value> {
        match words.get(1).copied() {
            Some("charge") => {
                let num = |i: usize| -> Result<f64> {
                    Ok(words.get(i).ok_or("missing argument")?.parse::<f64>()?)
                };
                let at = [num(2)?, num(3)?, num(4)?];
                let amount = num(5)? as u32;
                let taken = self.catalyst_takes_experience(at, |_| amount);
                Ok(serde_json::json!({ "ok": true, "taken": taken }))
            }
            Some("dig") => {
                // Let every warden's minute of calm run out.
                for m in self
                    .mobs
                    .iter_mut()
                    .filter(|m| m.species() == Species::Warden)
                {
                    warden::expire_dig_cooldown(m);
                }
                Ok(serde_json::json!({ "ok": true }))
            }
            Some("warn") => {
                let level = words.get(2).ok_or("missing level")?.parse::<i32>()?;
                self.sculk.tracker.set_warning_level(level);
                Ok(serde_json::json!({ "ok": true }))
            }
            _ => Ok(self.sculk_status()),
        }
    }
    /// The `sculk` control command's listing.
    pub(super) fn sculk_status(&self) -> serde_json::Value {
        let listeners: Vec<serde_json::Value> = self
            .sculk
            .index
            .iter()
            .filter_map(|key| match self.world.block_entities.get(key) {
                Some(BlockEntity::Sculk(e)) => Some(serde_json::json!({
                    "pos": [key.0, key.1, key.2],
                    "kind": e.id(),
                    "current": e.vibration.current.map(|i| i.event),
                    "travel": e.vibration.travel,
                    "last_frequency": e.last_frequency,
                    "warning_level": e.warning_level,
                    "cursors": e.spreader.cursors.len(),
                })),
                _ => None,
            })
            .collect();
        serde_json::json!({
            "listeners": listeners,
            "warning_level": self.sculk.tracker.warning_level,
            "cooldown": self.sculk.tracker.cooldown_ticks,
            "darkness": self.sculk.darkness.1,
            "chunks_known": self.sculk.known.len(),
        })
    }
    /// Who is causing the block changes being processed.
    pub(super) fn sculk_actor(&self) -> Option<Who> {
        self.sculk.actor
    }
    fn state_is(&self, table: &[bool], state: BlockStateId) -> bool {
        table.get(state.0 as usize).copied().unwrap_or(false)
    }
    fn sculk_entity(&mut self, key: (i32, i32, i32)) -> Option<&mut SculkEntity> {
        match self.world.block_entities.get_mut(&key) {
            Some(BlockEntity::Sculk(e)) => Some(e),
            _ => None,
        }
    }
    fn state_dampens(&self, state: Option<BlockStateId>) -> bool {
        state.is_some_and(|s| self.state_is(&self.sculk.tables.dampens, s))
    }
    /// `GameEvent.Context.of(player, state)`.
    pub(super) fn player_context(&self, state: Option<BlockStateId>) -> Context {
        let (dead, creative) = self
            .player
            .as_ref()
            .map_or((true, true), |p| (p.health.dead_or_dying(), self.creative));
        Context {
            source: Some(Who::Player),
            owner: None,
            dampening_state: self.state_dampens(state),
            spectator: self.spectator(),
            stepping_carefully: self.keys.contains(&KeyCode::ShiftLeft),
            dampens: false,
            targetable: !dead && !creative && !self.spectator(),
            living: true,
            player: true,
        }
    }
    /// `GameEvent.Context.of(mob, state)`.
    pub(super) fn mob_context(&self, id: u32, state: Option<BlockStateId>) -> Context {
        self.context_of_mob(self.mobs.iter().find(|m| m.id == id), id, state)
    }
    fn context_of_mob(&self, mob: Option<&Mob>, id: u32, state: Option<BlockStateId>) -> Context {
        Context {
            source: Some(Who::Mob(id)),
            owner: None,
            dampening_state: self.state_dampens(state),
            spectator: false,
            stepping_carefully: false,
            dampens: mob.is_some_and(|m| m.species() == Species::Warden),
            targetable: mob.is_some_and(|m| m.alive() && m.species() != Species::Warden),
            living: true,
            player: mob.is_some_and(|m| m.rider.is_some()),
        }
    }
    pub(super) fn who_context(&self, who: Option<Who>, state: Option<BlockStateId>) -> Context {
        match who {
            Some(Who::Player) => self.player_context(state),
            Some(Who::Mob(id)) => self.mob_context(id, state),
            None => Context {
                dampening_state: self.state_dampens(state),
                ..Context::default()
            },
        }
    }
    /// `Level.gameEvent`: tell the listeners within reach.
    pub(super) fn post_game_event(&mut self, event: &'static str, pos: [f64; 3], ctx: Context) {
        super::dispense_fixture::record(|| {
            format!(
                "G {},{},{} {event}",
                pos[0].floor() as i32,
                pos[1].floor() as i32,
                pos[2].floor() as i32
            )
        });
        if true || !self.sculk.tables.built {
            return;
        }
        let at = block_of(pos);
        let radius = sculk::notification_radius(event).min(16);
        let near: Vec<(i32, i32, i32)> = self
            .sculk
            .index
            .iter()
            .copied()
            .filter(|p| dist_sqr(at, *p) <= i64::from(radius * radius))
            .collect();
        for key in near {
            self.sculk_listen(key, event, pos, &ctx);
        }
        for i in 0..self.mobs.len() {
            if self.mobs[i].species() == Species::Warden && self.mobs[i].alive() {
                self.warden_listen(i, event, pos, &ctx);
            }
        }
    }
    /// A game event at a block (`Level.gameEvent(source, event, pos)`).
    pub(super) fn post_block_game_event(
        &mut self,
        event: &'static str,
        pos: [i32; 3],
        who: Option<Who>,
        state: Option<BlockStateId>,
    ) {
        let ctx = self.who_context(who, state);
        self.post_game_event(event, center((pos[0], pos[1], pos[2])), ctx);
    }
    /// `LivingEntity.actuallyHurt`'s damage vibration, heard as the hit lands;
    /// the end-of-tick scan then knows this damage.
    pub(super) fn mob_damage_event(&mut self, id: u32) {
        if let Some(t) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.last_damage.map(|(_, t)| t))
        {
            self.sculk.damaged.insert(id, t);
        }
        self.post_entity_event("minecraft:entity_damage", Who::Mob(id));
    }
    /// A game event of an entity at its position.
    pub(super) fn post_entity_event(&mut self, event: &'static str, who: Who) {
        let at = match who {
            Who::Player => self
                .player
                .as_ref()
                .map(|p| [p.position.x, p.position.y, p.position.z]),
            Who::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map(|m| [m.position.x, m.position.y, m.position.z]),
        };
        if let Some(at) = at {
            let ctx = self.who_context(Some(who), None);
            self.post_game_event(event, at, ctx);
        }
    }
    fn occluded(&self, origin: [f64; 3], dest: [f64; 3]) -> bool {
        sculk::is_occluded(origin, dest, |p| {
            self.state_is(&self.sculk.tables.occludes, self.world.block(p.x, p.y, p.z))
        })
    }
    /// `VibrationSystem.Listener.handleGameEvent` for a sculk block.
    fn sculk_listen(
        &mut self,
        key: (i32, i32, i32),
        event: &'static str,
        pos: [f64; 3],
        ctx: &Context,
    ) {
        let Some(entity) = self.sculk_entity(key) else {
            self.sculk.index.remove(&key);
            return;
        };
        let (kind, radius, busy) = (
            entity.kind,
            entity.listener_radius(),
            entity.vibration.current.is_some(),
        );
        if kind == SculkKind::Catalyst || busy {
            return;
        }
        let source_block = block_of(pos);
        if dist_sqr(source_block, key) > i64::from(radius * radius) {
            return;
        }
        let listens = match kind {
            SculkKind::Shrieker => sculk::shrieker_can_listen(event),
            _ => sculk::is_vibration(event),
        };
        // VibrationSystem.User.isValidVibration: a player sneaking past a
        // sensor, whose step it ignores, is AVOID_VIBRATION (sensors only;
        // a shrieker cannot trigger it).
        if listens
            && matches!(kind, SculkKind::Sensor | SculkKind::Calibrated)
            && ctx.source == Some(Who::Player)
            && !ctx.spectator
            && ctx.stepping_carefully
            && sculk::ignored_when_sneaking(event)
        {
            self.trigger_player("minecraft:avoid_vibration");
        }
        if !sculk::valid_vibration(listens, event, ctx) {
            return;
        }
        let state = self.world.block(key.0, key.1, key.2);
        let can_receive = match kind {
            SculkKind::Sensor | SculkKind::Calibrated => {
                // The sensor does not hear its own placement or removal.
                let own = source_block == [key.0, key.1, key.2]
                    && matches!(event, "minecraft:block_destroy" | "minecraft:block_place");
                let frequency = sculk::frequency(event);
                let calibrated_out = kind == SculkKind::Calibrated && {
                    let back = self.calibrated_signal(key);
                    back != 0 && frequency != back
                };
                !own && !calibrated_out
                    && frequency != 0
                    && self.res.behaviours().sculk_sensor_can_activate(state)
            }
            SculkKind::Shrieker => !self.shrieker_shrieking(state) && ctx.player,
            SculkKind::Catalyst => false,
        };
        if !can_receive {
            return;
        }
        let dest = center(key);
        if self.occluded(pos, dest) {
            return;
        }
        let distance = {
            let d = [pos[0] - dest[0], pos[1] - dest[1], pos[2] - dest[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32
        };
        let time = self.world.game_time;
        if let Some(entity) = self.sculk_entity(key) {
            entity.vibration.schedule(
                Info {
                    event,
                    distance,
                    pos,
                    source: ctx.source,
                    owner: ctx.owner,
                },
                time,
            );
        }
    }
    fn calibrated_signal(&mut self, key: (i32, i32, i32)) -> i32 {
        let pos = BlockPos::new(key.0, key.1, key.2);
        let mut value = 0;
        self.with_level(|b, level, ticks| {
            value = b.calibrated_back_signal(level, ticks, pos);
        });
        value
    }
    fn shrieker_shrieking(&self, state: BlockStateId) -> bool {
        self.state_property(state, "shrieking") == Some("true")
    }
    fn state_property(&self, state: BlockStateId, key: &str) -> Option<&str> {
        self.pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get(key))
            .map(String::as_str)
    }
    /// `Warden.VibrationUser`: accept a vibration for the warden at `index`.
    fn warden_listen(&mut self, index: usize, event: &'static str, pos: [f64; 3], ctx: &Context) {
        let now = self.world.game_time;
        let (occludes, world) = (&self.sculk.tables.occludes, &self.world);
        let avoided = warden::listen(&mut self.mobs[index], now, event, pos, ctx, |a, b| {
            sculk::is_occluded(a, b, |p| {
                let state = world.block(p.x, p.y, p.z);
                occludes.get(state.0 as usize).copied().unwrap_or(false)
            })
        });
        if avoided {
            self.trigger_player("minecraft:avoid_vibration");
        }
    }
    fn rides_player(&self, who: Option<Who>) -> bool {
        matches!(who, Some(Who::Mob(id)) if self.mobs.iter().any(|m| m.id == id && m.rider.is_some()))
    }
    /// The darkness effect's blend factor at the partial tick.
    fn darkness_blend(&self, partial: f32) -> f32 {
        let (previous, current) = self.sculk.darkness;
        previous + (current - previous) * partial
    }
    /// The fog's view of the darkness effect, while it is on.
    pub(super) fn darkness_fog(&self) -> Option<f32> {
        self.potions
            .effects
            .get("darkness")
            .map(|_| self.darkness_blend(self.clock.fraction() as f32))
    }
    /// `LightmapRenderStateExtractor`: (brightness taken off the gamma
    /// option, the pulsing light scale).
    pub(super) fn darkness_lighting(&self, partial: f32) -> (f32, f32) {
        let scale = self.settings.file.double("darknessEffectScale") as f32;
        let modifier = self.darkness_blend(partial) * scale;
        let pulse =
            ((self.ticks as f32 - partial) * std::f32::consts::PI * 0.025).cos() * 0.45 * modifier;
        (modifier, pulse.max(0.0) * scale)
    }
    /// `Warden.applyDarknessAround` / `MobEffectUtil.addEffectToPlayersAround`.
    pub(super) fn apply_darkness(&mut self, at: [f64; 3], radius: f64) {
        self.for_each_player(|g| g.darkness_on_player(at, radius));
    }
    fn darkness_on_player(&mut self, at: [f64; 3], radius: f64) {
        if self.creative || self.spectator() {
            return;
        }
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let d = [
            p.position.x - at[0],
            p.position.y - at[1],
            p.position.z - at[2],
        ];
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= radius * radius {
            return;
        }
        if let Some(active) = self.potions.effects.get("darkness")
            && active.duration > warden::DARKNESS_REAPPLY - 1
        {
            return;
        }
        let mut effect =
            horizon_client::effects::EffectInstance::new("darkness", warden::DARKNESS_DURATION, 0);
        effect.visible = false;
        self.potions.effects.add(effect);
    }
    /// `SculkCatalystBlockEntity.CatalystListener.handleGameEvent` for the
    /// death of a mob: the nearest catalyst within 8 blocks takes its
    /// experience. Returns whether it was consumed.
    pub(super) fn catalyst_takes_experience(
        &mut self,
        at: [f64; 3],
        experience: impl FnOnce(&mut Self) -> u32,
    ) -> bool {
        // GameEventDispatcher: catalysts are BY_DISTANCE listeners, handled
        // nearest first (exact distance to the block centre); a tie keeps the
        // order they were visited in: chunk x, chunk z, section y, then the
        // order they registered in. The first one takes the experience.
        let source = block_of(at);
        let mut candidates: Vec<(f64, (i32, i32, i32), usize, (i32, i32, i32))> = Vec::new();
        for (order, key) in self.sculk.index.iter().copied().enumerate() {
            if dist_sqr(source, key) > 64 {
                continue;
            }
            let is_catalyst = matches!(
                self.world.block_entities.get(&key),
                Some(BlockEntity::Sculk(e)) if e.kind == SculkKind::Catalyst
            );
            if is_catalyst {
                let c = center(key);
                let d = (at[0] - c[0]).powi(2) + (at[1] - c[1]).powi(2) + (at[2] - c[2]).powi(2);
                candidates.push((d, (key.0 >> 4, key.2 >> 4, key.1 >> 4), order, key));
            }
        }
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let nearest = candidates.first().map(|c| (c.3, ()));
        let Some((key, _)) = nearest else {
            return false;
        };
        let xp = experience(self);
        if xp > 0 {
            // sourcePosition.relative(UP, 0.5)
            let start = BlockPos::new(source[0], (at[1] + 0.5).floor() as i32, source[2]);
            if let Some(e) = self.sculk_entity(key) {
                e.spreader.add_cursors(start, xp as i32);
            }
        }
        self.catalyst_bloom(key);
        true
    }
    fn catalyst_bloom(&mut self, key: (i32, i32, i32)) {
        let pos = BlockPos::new(key.0, key.1, key.2);
        self.with_level(|b, level, ticks| b.sculk_catalyst_bloom(level, ticks, pos));
        let _ = self.process_level_events();
        let c = center(key);
        let options = ParticleOptions::simple(ParticleType::SculkSoul);
        for _ in 0..2 {
            // Client-side (LevelEventHandler / particles): the client random.
            let r = self.particles.random();
            let p = [
                c[0] + r.next_gaussian() * 0.2,
                c[1] + 0.65,
                c[2] + r.next_gaussian() * 0.2,
            ];
            self.particles.add(&options, p, [0.0; 3]);
        }
        let pitch = 0.6 + self.world.random.next_float() * 0.4;
        self.sounds
            .play("minecraft:block.sculk_catalyst.bloom", Some(c), 2.0, pitch);
    }
    /// `LevelEvent` 3006: charge cursors moving over sculk, or popping.
    pub(super) fn sculk_charge_event(&mut self, pos: BlockPos, data: i32) {
        let c = center((pos.x, pos.y, pos.z));
        let count = data >> 6;
        if count > 0 {
            // Client-side (LevelEventHandler / particles): the client random.
            let r = self.particles.random();
            if r.next_float() < 0.3 + count as f32 * 0.1 {
                let volume = 0.15 + 0.02 * (count * count) as f32 * r.next_float();
                let pitch = 0.4 + 0.3 * count as f32 * r.next_float();
                self.sounds
                    .play("minecraft:block.sculk.charge", Some(c), volume, pitch);
            }
            let face_data = data & 63;
            let faces: Vec<(Direction, f32, f64)> = if face_data == 0 {
                Direction::ALL
                    .into_iter()
                    .map(|d| {
                        let roll = if d == Direction::Down {
                            std::f32::consts::PI
                        } else {
                            0.0
                        };
                        let factor = if matches!(d, Direction::Up | Direction::Down) {
                            0.65
                        } else {
                            0.57
                        };
                        (d, roll, factor)
                    })
                    .collect()
            } else {
                horizon_world::level::sculk::unpack_faces(face_data)
                    .into_iter()
                    .map(|d| {
                        let roll = if d == Direction::Up {
                            std::f32::consts::PI
                        } else {
                            0.0
                        };
                        (d, roll, 0.35)
                    })
                    .collect()
            };
            for (direction, roll, factor) in faces {
                let repetitions = self
                    .particles
                    .random()
                    .next_int_bound(count + 1)
                    .unwrap_or(0);
                for _ in 0..repetitions {
                    self.spawn_particle_on_face(pos, direction, roll, factor);
                }
            }
        } else {
            self.sounds
                .play("minecraft:block.sculk.charge", Some(c), 1.0, 1.0);
            let solid = self
                .res
                .facts()
                .get(self.world.block(pos.x, pos.y, pos.z))
                .full_collision;
            let (particles, spread) = if solid { (40, 0.45f32) } else { (20, 0.25f32) };
            let options = ParticleOptions::simple(ParticleType::SculkChargePop);
            for _ in 0..particles {
                // Client-side (LevelEventHandler / particles): the client random.
                let r = self.particles.random();
                let v = [
                    2.0 * r.next_float() - 1.0,
                    2.0 * r.next_float() - 1.0,
                    2.0 * r.next_float() - 1.0,
                ];
                let at = [
                    c[0] + f64::from(v[0] * spread),
                    c[1] + f64::from(v[1] * spread),
                    c[2] + f64::from(v[2] * spread),
                ];
                let speed = [
                    f64::from(v[0] * 0.07),
                    f64::from(v[1] * 0.07),
                    f64::from(v[2] * 0.07),
                ];
                self.particles.add(&options, at, speed);
            }
        }
    }
    /// `ParticleUtils.spawnParticleOnFace` for a sculk charge.
    fn spawn_particle_on_face(&mut self, pos: BlockPos, face: Direction, roll: f32, factor: f64) {
        let c = center((pos.x, pos.y, pos.z));
        let step = face.normal();
        // Client-side (LevelEventHandler / particles): the client random.
        let r = self.particles.random();
        let mut jitter = || r.next_double() - 0.5;
        let mut at = [0.0; 3];
        for i in 0..3 {
            at[i] = c[i]
                + if step[i] == 0 {
                    jitter()
                } else {
                    f64::from(step[i]) * factor
                };
        }
        let mut speed = [0.0; 3];
        for i in 0..3 {
            let s = (r.next_double() * 2.0 - 1.0) * f64::from(0.005f32);
            speed[i] = if step[i] == 0 { s } else { 0.0 };
        }
        self.particles
            .add(&ParticleOptions::sculk_charge(roll), at, speed);
    }
    /// `LevelEvent` 3007: ten shrieks rising from the shrieker, and the shriek.
    pub(super) fn shriek_event(&mut self, pos: BlockPos) {
        let top = f64::from(pos.y) + 0.5;
        for i in 0..10 {
            self.particles.add(
                &ParticleOptions::shriek(i * 5),
                [f64::from(pos.x) + 0.5, top, f64::from(pos.z) + 0.5],
                [0.0; 3],
            );
        }
        let state = self.world.block(pos.x, pos.y, pos.z);
        if self.state_property(state, "waterlogged") != Some("true") {
            let pitch = 0.6 + self.particles.random().next_float() * 0.4;
            self.sounds.play(
                "minecraft:block.sculk_shrieker.shriek",
                Some([f64::from(pos.x) + 0.5, top, f64::from(pos.z) + 0.5]),
                2.0,
                pitch,
            );
        }
    }
    fn vibration_particle(&mut self, from: [f64; 3], to: [f64; 3], ticks: i32) {
        self.particles.vibration(from, to, ticks);
    }
    /// A vibration travelling to a warden (`MobEvent::Vibration`).
    pub(super) fn mob_vibration_particle(&mut self, from: [f64; 3], to: [f64; 3], ticks: i32) {
        self.vibration_particle(from, to, ticks);
    }
    /// `Warden.clientDiggingParticles`: thirty crumbs of the block underfoot.
    pub(super) fn warden_dig_particles(&mut self, at: [f64; 3]) {
        let below = (
            at[0].floor() as i32,
            (at[1] - 0.2).floor() as i32,
            at[2].floor() as i32,
        );
        let state = self.world.block(below.0, below.1, below.2);
        let Some((sprite, tint)) = self.particle_sprite(state, [below.0, below.1, below.2]) else {
            return;
        };
        for _ in 0..30 {
            // Client-side (LevelEventHandler / particles): the client random.
            let r = self.particles.random();
            let off = |v: f32| f64::from(v * 1.4 - 0.7);
            let p = [
                at[0] + off(r.next_float()),
                at[1],
                at[2] + off(r.next_float()),
            ];
            let particle =
                horizon_client::particles::Particle::terrain(r, p, [0.0; 3], sprite.clone(), tint);
            self.particles.push(particle);
        }
    }
    /// `Projectile.shoot` / `onHit`: a projectile of `owner` at a position.
    pub(super) fn projectile_event(
        &mut self,
        event: &'static str,
        owner: Option<Who>,
        at: [f64; 3],
    ) {
        let mut ctx = Context::default();
        if event == "minecraft:projectile_shoot" {
            // The shooter is the source of the shot.
            ctx = self.who_context(owner, None);
        } else {
            ctx.owner = owner;
            ctx.player = owner == Some(Who::Player) || self.rides_player(owner);
        }
        self.post_game_event(event, at, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centres_and_blocks_agree() {
        assert_eq!(center((1, 2, 3)), [1.5, 2.5, 3.5]);
        assert_eq!(block_of([1.5, 2.5, -0.5]), [1, 2, -1]);
        assert_eq!(dist_sqr([0, 0, 0], (3, 4, 0)), 25);
    }
}
