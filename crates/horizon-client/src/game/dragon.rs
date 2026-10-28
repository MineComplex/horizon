//! The ender dragon fight (`EnderDragonFight`, `EnderDragon`, its phases,
//! `EndCrystal`, `DragonFireball`): the fight state saved in the End's
//! `ender_dragon_fight.dat`, the dragon spawned over the island, the
//! holding pattern over the 24-node path graph, strafing with fireballs,
//! landing on the podium to breathe and charge, healing from the crystals on
//! the spikes, its parts (head and neck take full damage, the rest a quarter
//! plus one), wing knockback, block breaking, the death flight and rays, the
//! exit portal, the egg and the next gateway; and its rendering and boss bar.
use super::{Dim, Game};
use horizon_client::{
    hud::HudBatch,
    hud_layout, hud_overlays,
    mesher::Vertex,
    particles::{ParticleOptions, ParticleType}};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::{LegacyRandom, RandomSource},
};
use horizon_protocol::entity_data::Value;
use horizon_world::nbt::Tag;

/// Mob ids from here on are dragon parts (`SPECIAL + part`) and crystals
/// (`SPECIAL + 16 + crystal`), so projectiles can target them.
pub(super) const SPECIAL: u32 = u32::MAX - 4096;
const CRYSTAL_IDS: u32 = SPECIAL + 16;
const FIGHT_FILE: &str = "data/minecraft/ender_dragon_fight.dat";
const MAX_HEALTH: f32 = 200.0;
/// `Mob.getAmbientSoundInterval`.
const AMBIENT_INTERVAL: i32 = 80;

#[derive(Default)]
pub(super) struct Fight {
    loaded: bool,
    needs_scan: bool,
    pub killed: bool,
    pub previously_killed: bool,
    gateways: Vec<i32>,
    exit_portal: Option<[i32; 3]>,
    alive_crystals: i32,
    crystal_scan: i32,
    players: bool,
    /// The spikes' crystals have been placed (worldgen does not place entities here).
    crystals_placed: bool,
    /// The respawn sequence (`DragonRespawnStage`) and its tick count.
    respawn: Option<(Respawn, i32)>,
    /// The four crystals on the exit portal driving the respawn.
    respawn_crystals: Vec<Vec3>,
    /// The player behind the crystal blast under way, whose `explosion`
    /// damage source names them (`EndCrystal.hurtServer`).
    blast_by: Option<FightWho>,
    dirty: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Respawn {
    Start,
    Preparing,
    Pillars,
    Summoning,
}
impl Respawn {
    fn name(self) -> &'static str {
        match self {
            Respawn::Start => "start",
            Respawn::Preparing => "preparing_to_summon_pillars",
            Respawn::Pillars => "summoning_pillars",
            Respawn::Summoning => "summoning_dragon",
        }
    }
    fn from_name(name: &str) -> Option<Self> {
        [
            Respawn::Start,
            Respawn::Preparing,
            Respawn::Pillars,
            Respawn::Summoning,
        ]
        .into_iter()
        .find(|r| r.name() == name)
    }
}
#[derive(Clone, Debug)]
pub(super) struct Crystal {
    /// The entity id a server tracks it by (0 until it is tracked).
    pub id: u32,
    pub position: Vec3,
    pub show_bottom: bool,
    /// EndCrystal.time (the bob and spin), starting random.
    pub time: i32,
    pub beam: Option<[i32; 3]>,
    /// `setPermanentlyInvulnerable` (respawned spike crystals).
    pub invulnerable: bool,
}
#[derive(Clone, Debug, Default)]
struct Path {
}
impl Path {
}
#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Holding {
        path: Option<PathBox>,
        target: Option<Vec3>,
        clockwise: bool,
    },
    Strafe {
        charge: i32,
        path: Option<PathBox>,
        target: Option<Vec3>,
        clockwise: bool,
        /// `attackTarget`: the player it strafes.
        player: Option<FightWho>,
    },
    LandingApproach {
        path: Option<PathBox>,
        target: Option<Vec3>,
    },
    Landing {
        target: Option<Vec3>,
    },
    Takeoff {
        first: bool,
        path: Option<PathBox>,
        target: Option<Vec3>,
    },
    Scanning {
        time: i32,
    },
    Attacking {
        ticks: i32,
    },
    Flaming {
        ticks: i32,
    },
    Charging {
        target: Option<Vec3>,
        since: i32,
    },
    Dying {
        target: Option<Vec3>,
        time: i32,
    },
    Hover {
        target: Option<Vec3>,
    },
}
/// A path compared by identity only (phases compare by kind).
#[derive(Clone, Debug)]
struct PathBox(Path);
impl PartialEq for PathBox {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Phase {
    /// `EnderDragonPhase` ids.
    fn id(&self) -> i32 {
        match self {
            Self::Holding { .. } => 0,
            Self::Strafe { .. } => 1,
            Self::LandingApproach { .. } => 2,
            Self::Landing { .. } => 3,
            Self::Takeoff { .. } => 4,
            Self::Flaming { .. } => 5,
            Self::Scanning { .. } => 6,
            Self::Attacking { .. } => 7,
            Self::Charging { .. } => 8,
            Self::Dying { .. } => 9,
            Self::Hover { .. } => 10,
        }
    }
    fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Strafe {
                charge: 0,
                path: None,
                target: None,
                clockwise: false,
                player: None,
            },
            2 => Self::LandingApproach {
                path: None,
                target: None,
            },
            3 => Self::Landing { target: None },
            4 => Self::Takeoff {
                first: true,
                path: None,
                target: None,
            },
            5 => Self::Flaming { ticks: 0 },
            6 => Self::Scanning { time: 0 },
            7 => Self::Attacking { ticks: 0 },
            8 => Self::Charging {
                target: None,
                since: 0,
            },
            9 => Self::Dying {
                target: None,
                time: 0,
            },
            10 => Self::Hover { target: None },
            _ => Self::Holding {
                path: None,
                target: None,
                clockwise: false,
            },
        }
    }
    fn sitting(&self) -> bool {
        matches!(
            self,
            Self::Scanning { .. }
                | Self::Attacking { .. }
                | Self::Flaming { .. }
                | Self::Hover { .. }
        )
    }
}
pub(super) struct Dragon {
    /// `Entity.id` once tracked to guests (0 before); its parts take the
    /// eight ids after it (`EnderDragon`'s constructor reserves them).
    pub id: u32,
    pub position: Vec3,
    pub previous: Vec3,
    velocity: Vec3,
    pub yaw: f32,
    pub previous_yaw: f32,
    yaw_accel: f32,
    pub health: f32,
    pub hurt_time: i32,
    /// `LivingEntity.damageCooldownTime` and `lastHurt`: within the window
    /// only damage above the last hit lands, and only the excess.
    invulnerable: i32,
    last_hurt: f32,
    /// `lastHurtByPlayer` with its 100 tick memory: the player credited with
    /// the kill, and the dragon tick of their last hit.
    hurt_by_player: Option<(FightWho, i32)>,
    pub death_time: i32,
    pub flap: f32,
    pub old_flap: f32,
    phase: Phase,
    flame_count: i32,
    sitting_damage: f32,
    growl: i32,
    /// `Mob.ambientSoundTime`.
    ambient_time: i32,
    ticks: i32,
    /// DragonFlightHistory: (y, yRot) samples, newest at `head`.
    history: [(f64, f32); 64],
    head: usize,
    /// head, neck, body, tail1-3, wing1-2.
    pub parts: [(Vec3, Vec3); 8],
    nearest_crystal: Option<usize>,
    /// A crystal destroyed while it held its pattern (taken on its next tick).
    strafe_request: Option<StrafeRequest>,
    random: LegacyRandom,
}
/// A player in the fight: the window's own player, or a guest by
/// connection (a LAN or server player).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FightWho {
    Host,
    Guest(u32),
}
/// A player in the End as the dragon sees it.
#[derive(Clone, Copy, Debug)]
pub(super) struct FightPlayer {
    pub(super) bounds: Aabb,
    spectator: bool,
}
impl FightPlayer {
}
/// Whom a destroyed crystal sends a holding dragon after
/// (`DragonHoldingPatternPhase.onCrystalDestroyed`).
#[derive(Clone, Copy, Debug)]
enum StrafeRequest {
    /// The player who destroyed it.
    Player(FightWho),
    /// Anything else: the player nearest the crystal.
    Nearest(Vec3),
}
/// Whether `EndCrystal.hurtServer` creates a blast before notifying the fight.
#[derive(Clone, Copy, PartialEq)]
enum CrystalDamage {
    Direct,
    Explosion,
}
/// What `EnderDragon.hurt` made of a hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Hurt {
    /// It returned true; `full` when it was a new hit, not just the excess
    /// over the last one (the hurt sound and animation).
    Landed { full: bool },
    /// It returned false (dying, too little damage).
    Refused,
    /// A perched dragon's `onHurt` took an arrow's damage away (and set the
    /// arrow alight for a second).
    Shrugged,
}
const PART_SIZES: [(f64, f64); 8] = [
    (1.0, 1.0),
    (3.0, 3.0),
    (5.0, 3.0),
    (2.0, 2.0),
    (2.0, 2.0),
    (2.0, 2.0),
    (4.0, 2.0),
    (4.0, 2.0),
];
impl Dragon {
    fn new(position: Vec3, yaw: f32, seed: i64) -> Self {
        Self {
            id: 0,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw,
            previous_yaw: yaw,
            yaw_accel: 0.0,
            health: MAX_HEALTH,
            hurt_time: 0,
            invulnerable: 0,
            last_hurt: 0.0,
            hurt_by_player: None,
            death_time: 0,
            flap: 0.0,
            old_flap: 0.0,
            phase: Phase::Holding {
                path: None,
                target: None,
                clockwise: false,
            },
            flame_count: 0,
            sitting_damage: 0.0,
            growl: 100,
            ambient_time: 0,
            ticks: 0,
            history: [(position.y, yaw); 64],
            head: 0,
            parts: [(position, position); 8],
            nearest_crystal: None,
            strafe_request: None,
            random: LegacyRandom::new(seed),
        }
    }
    /// DragonFlightHistory.record.
    fn record(&mut self) {
        self.head = (self.head + 1) & 63;
        self.history[self.head] = (self.position.y, self.yaw);
    }
    /// DragonFlightHistory.get(delay).
    fn sample(&self, delay: usize) -> (f64, f32) {
        self.history[(self.head + 64 - (delay & 63)) & 63]
    }
    /// DragonFlightHistory.get(delay, partial).
    fn sample_lerp(&self, delay: usize, partial: f32) -> (f64, f32) {
        let a = self.sample(delay);
        let b = self.sample(delay + 1);
        let yaw = b.1 + wrap(a.1 - b.1) * partial;
        (b.0 + (a.0 - b.0) * f64::from(partial), yaw)
    }
    pub fn alive(&self) -> bool {
        self.health > 0.0
    }
    /// `isDeadOrDying` for the killing blow: the dying flight, or no health.
    fn down(&self) -> bool {
        matches!(self.phase, Phase::Dying { .. }) || !self.alive()
    }
    fn part_box(&self, i: usize) -> Aabb {
        let (w, h) = PART_SIZES[i];
        let p = self.parts[i].0;
        Aabb::new(
            Vec3::new(p.x - w / 2.0, p.y, p.z - w / 2.0),
            Vec3::new(p.x + w / 2.0, p.y + h, p.z + w / 2.0),
        )
    }
    /// `EnderDragon.hurt(part, source, damage)`: whether the hit landed
    /// (an arrow is spent, else it turns back). `counts` is
    /// `source.getEntity() instanceof Player` or a damage type that always
    /// hurts dragons (explosions); `arrow` a direct arrow or wind charge,
    /// which a perched dragon shrugs off (`AbstractDragonSittingPhase.onHurt`).
    fn hurt(&mut self, part: usize, damage: f32, counts: bool, arrow: bool) -> Hurt {
        if matches!(self.phase, Phase::Dying { .. }) || !self.alive() {
            return Hurt::Refused;
        }
        if arrow && self.phase.sitting() {
            return Hurt::Shrugged;
        }
        let mut damage = damage;
        if part != 0 && part != 1 {
            damage = damage / 4.0 + damage.min(1.0);
        }
        if damage < 0.01 {
            return Hurt::Refused;
        }
        if !counts {
            return Hurt::Landed { full: false };
        }
        let before = self.health;
        // LivingEntity.hurtServer: within the cooldown only what exceeds the last hit.
        let full = if self.invulnerable > 10 {
            if damage <= self.last_hurt {
                return Hurt::Landed { full: false };
            }
            self.health -= damage - self.last_hurt;
            self.last_hurt = damage;
            false
        } else {
            self.last_hurt = damage;
            self.invulnerable = 20;
            self.health -= damage;
            self.hurt_time = 10;
            true
        };
        if self.health <= 0.0 {
            if self.phase.sitting() {
                self.health = 0.0;
            } else {
                // handleKillingBlow: dying flight first.
                self.health = 1.0;
                self.set_phase(Phase::Dying {
                    target: None,
                    time: 0,
                });
            }
        }
        if self.phase.sitting() {
            self.sitting_damage += before - self.health.max(0.0);
            if self.sitting_damage > 0.25 * MAX_HEALTH {
                self.sitting_damage = 0.0;
                self.set_phase(Phase::Takeoff {
                    first: true,
                    path: None,
                    target: None,
                });
            }
        }
        Hurt::Landed { full }
    }
    /// `ServerExplosion.hurtEntities` on the dragon: the dragon itself (its
    /// body takes it), then each part, in `Level.getEntities` order, each by
    /// its distance and how much of its box the blast sees (`exposure`).
    fn explosion_hits(
        &self,
        center: [f64; 3],
        radius: f32,
        exposure: impl Fn(Aabb) -> f32,
    ) -> Vec<(usize, f32)> {
        let diameter = f64::from(radius * 2.0);
        let dist = |p: Vec3| {
            ((p.x - center[0]).powi(2) + (p.y - center[1]).powi(2) + (p.z - center[2]).powi(2))
                .sqrt()
                / diameter
        };
        let p = self.position;
        let whole = Aabb::new(
            Vec3::new(p.x - 8.0, p.y, p.z - 8.0),
            Vec3::new(p.x + 8.0, p.y + 8.0, p.z + 8.0),
        );
        std::iter::once((2, p, whole))
            .chain((0..8).map(|i| (i, self.parts[i].0, self.part_box(i))))
            .filter_map(|(part, at, b)| {
                let d = dist(at);
                (d <= 1.0).then(|| {
                    let pow = (1.0 - d) * f64::from(exposure(b));
                    (
                        part,
                        ((pow * pow + pow) / 2.0 * 7.0 * diameter + 1.0) as f32,
                    )
                })
            })
            .collect()
    }
    fn set_phase(&mut self, phase: Phase) {
        if self.phase.id() == phase.id() && phase.id() != 1 {
            return;
        }
        if let Phase::Flaming { .. } = phase {
            self.flame_count += 1;
        }
        self.phase = phase;
    }
}
fn wrap(d: f32) -> f32 {
    let r = d.rem_euclid(360.0);
    if r >= 180.0 { r - 360.0 } else { r }
}
impl Game<'_> {
    /// `BossHealthOverlay.extractRenderState`: whatever `publish_boss_bars`
    /// (or a server's packets) put on this client's overlay.
    pub(super) fn draw_boss_bars(&self, batch: &mut HudBatch) {
        let bars = self
            .front
            .boss_overlay
            .bars(horizon_client::boss_overlay::now_millis());
        let draws = hud_overlays::boss_overlay(
            batch.width as i32,
            batch.height as i32,
            &bars,
            &self.front.gui.atlas.font,
        );
        hud_layout::paint(batch, &self.front.gui.atlas, &draws);
    }
    /// `BossHealthOverlay.shouldPlayMusic`.
    pub(super) fn boss_music(&self) -> bool {
        self.front.boss_overlay.should_play_music()
    }
}
impl crate::game::Sim<'_> {
    fn fight_path(&self) -> std::path::PathBuf {
        horizon_client::save::dimension_dir(&self.world_root, "minecraft:the_end").join(FIGHT_FILE)
    }
    /// The saved fight, or `createDefault` with the gateways shuffled by the seed.
    fn load_fight(&mut self) {
        if self.fight.loaded {
            return;
        }
        let tag = horizon_client::save::read_dat(&self.fight_path());
        let data = tag.as_ref().and_then(|t| t.get("data"));
        let flag = |k: &str, default: bool| match data.and_then(|d| d.get(k)) {
            Some(Tag::Byte(v)) => *v != 0,
            _ => default,
        };
        let mut fight = Fight {
            loaded: true,
            needs_scan: flag("needs_state_scanning", true),
            killed: flag("dragon_killed", false),
            previously_killed: flag("previously_killed", false),
            crystals_placed: flag("horizon_crystals_placed", false),
            gateways: match data.and_then(|d| d.get("gateways")) {
                Some(Tag::IntArray(v)) => v.clone(),
                Some(Tag::List(v)) => v
                    .iter()
                    .filter_map(|t| match t {
                        Tag::Int(i) => Some(*i),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            },
            respawn: match data.and_then(|d| d.get("respawn_stage")) {
                Some(Tag::String(name)) => {
                    Respawn::from_name(&name.to_string().unwrap_or_default()).map(|r| {
                        let time = match data.and_then(|d| d.get("respawn_time")) {
                            Some(Tag::Int(t)) => *t,
                            _ => 0,
                        };
                        (r, time)
                    })
                }
                _ => None,
            },
            respawn_crystals: match data.and_then(|d| d.get("horizon_respawn_crystals")) {
                Some(Tag::IntArray(v)) => v
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|b| {
                        Vec3::new(
                            f64::from(b[0]) + 0.5,
                            f64::from(b[1]),
                            f64::from(b[2]) + 0.5,
                        )
                    })
                    .collect(),
                _ => Vec::new(),
            },
            exit_portal: match data.and_then(|d| d.get("exit_portal_location")) {
                Some(Tag::IntArray(v)) if v.len() == 3 => Some([v[0], v[1], v[2]]),
                _ => None,
            },
            // The first tick scans for players.
            ..Default::default()
        };
        if fight.gateways.is_empty() {
            // Util.shuffle(0..20, RandomSource.create(seed)).
            let mut gateways: Vec<i32> = (0..20).collect();
            let mut random = LegacyRandom::new(self.level.seed);
            for i in (2..=gateways.len()).rev() {
                let j = random.next_int_bound(i as i32).unwrap_or(0) as usize;
                gateways.swap(i - 1, j);
            }
            fight.gateways = gateways;
            fight.dirty = true;
        }
        // A world from before this fight existed: one with an active portal
        // already won it.
        if tag.is_none() && self.end_portal_active() {
            fight.killed = true;
            fight.previously_killed = true;
            fight.crystals_placed = true;
            fight.needs_scan = false;
        }
        self.fight = fight;
    }
    fn end_portal_active(&self) -> bool {
        (0..256).any(|y| {
            &*self.res.facts().get(self.world.block(0, y, 0)).name == "minecraft:bedrock"
                && (-3..=3).any(|d| {
                    &*self.res.facts().get(self.world.block(d, y, 0)).name == "minecraft:end_portal"
                })
        })
    }
    pub(super) fn save_fight(&mut self) {
        if !self.fight.loaded || !self.fight.dirty {
            return;
        }

        let f = &self.fight;
        let b = |v: bool| Tag::Byte(i8::from(v));
        let mut data = vec![
            ("needs_state_scanning", b(f.needs_scan)),
            ("dragon_killed", b(f.killed)),
            ("previously_killed", b(f.previously_killed)),
            ("respawn_time", Tag::Int(f.respawn.map_or(0, |r| r.1))),
            ("gateways", Tag::IntArray(f.gateways.clone())),
            ("horizon_crystals_placed", b(f.crystals_placed)),
        ];
        if let Some(p) = f.exit_portal {
            data.push(("exit_portal_location", Tag::IntArray(p.to_vec())));
        }
        if let Some((stage, _)) = f.respawn {
            data.push(("respawn_stage", Tag::String(stage.name().into())));
            data.push((
                "horizon_respawn_crystals",
                Tag::IntArray(
                    f.respawn_crystals
                        .iter()
                        .flat_map(|p| [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32])
                        .collect(),
                ),
            ));
        }
        let tag = Tag::compound([
            ("data", Tag::compound(data)),
            ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
        ]);
        if horizon_client::save::write_dat(&self.fight_path(), tag).is_ok() {
            self.fight.dirty = false;
        }
    }
    /// The player acting now: the guest whose turn it is, else the host.
    fn acting_player(&self) -> FightWho {
        None::<u32>.map_or(FightWho::Host, FightWho::Guest)
    }
    /// The players in the level the dragon sees (`level.players()`): the
    /// window's own, then every guest in the level.
    pub(super) fn fight_players(&self) -> Vec<FightPlayer> {
        let mut out = Vec::new();
        if let Some(p) = self.player.as_ref() {
            out.push(FightPlayer {
                bounds: p.bounds(),
                spectator: self.spectator(),
            });
        }
        out
    }
    /// `updateCrystalCount`: crystals on the spike tops.
    fn count_crystals(&mut self) {
        self.fight.crystal_scan = 0;
        let spikes = horizon_world::worldgen::end_spike::spikes_for_seed(self.level.seed);
        self.fight.alive_crystals = self
            .crystals
            .iter()
            .filter(|c| {
                spikes.iter().any(|s| {
                    (c.position.x - f64::from(s.center_x) - 0.5).abs() <= f64::from(s.radius)
                        && (c.position.z - f64::from(s.center_z) - 0.5).abs() <= f64::from(s.radius)
                        && c.position.y >= f64::from(s.height)
                })
            })
            .count() as i32;
    }
    /// `EndCrystal.hurtServer`: remove it, blast only for non-explosion damage,
    /// then notify the dragon fight even when no blast was created.
    fn destroy_crystal(&mut self, index: usize, by: Option<FightWho>, damage: CrystalDamage) {
        if index >= self.crystals.len() || self.crystals[index].invulnerable {
            return;
        }
        let c = self.crystals.remove(index);
        // Read before the blast, which can take other respawn crystals.
        let respawn_crystal = self.fight.respawn.is_some()
            && self
                .fight
                .respawn_crystals
                .iter()
                .any(|p| p.distance_squared(c.position) < 0.01);
        if damage == CrystalDamage::Direct {
            let at = [c.position.x, c.position.y, c.position.z];
            // The blast's damage source carries the player, so the dragon
            // remembers them before the blast can kill it.
            self.fight.blast_by = by;
            let _ =
                self.server_explode(at, 6.0, super::explosion::BlockInteraction::Destroy, false);
            self.fight.blast_by = None;
        }
        if respawn_crystal {
            self.abort_respawn();
            return;
        }
        // EnderDragonFight.onCrystalDestroyed.
        self.count_crystals();
        if let Some(d) = self.dragon.as_mut() {
            if d.nearest_crystal == Some(index) {
                d.nearest_crystal = None;
                self.hurt_dragon(0, 10.0, true, by, false);
            } else if let Some(n) = d.nearest_crystal.as_mut()
                && *n > index
            {
                *n -= 1;
            }
            // EnderDragon.onCrystalDestroyed: the player who did it, else the
            // nearest; DragonHoldingPatternPhase strafes them.
            if let Some(d) = self.dragon.as_mut()
                && matches!(d.phase, Phase::Holding { .. })
            {
                d.strafe_request = Some(match by {
                    Some(who) => StrafeRequest::Player(who),
                    None => StrafeRequest::Nearest(c.position),
                });
            }
        }
    }
    /// Targets for projectiles: dragon parts and crystals.
    pub(super) fn special_targets(&self) -> Vec<(u32, Aabb)> {
        let mut out = Vec::new();
        if let Some(d) = self.dragon.as_ref().filter(|d| d.alive()) {
            for i in 0..8 {
                out.push((SPECIAL + i as u32, d.part_box(i)));
            }
        }
        for (i, c) in self.crystals.iter().enumerate() {
            let p = c.position;
            out.push((
                CRYSTAL_IDS + i as u32,
                Aabb::new(
                    Vec3::new(p.x - 1.0, p.y, p.z - 1.0),
                    Vec3::new(p.x + 1.0, p.y + 2.0, p.z + 1.0),
                ),
            ));
        }
        out
    }
    /// Damage to a special id (from projectiles and melee): what the
    /// dragon part or crystal made of it. `arrow` for a direct arrow.
    pub(super) fn hit_special(
        &mut self,
        id: u32,
        amount: f32,
        by_player: bool,
        arrow: bool,
    ) -> Hurt {
        if id >= CRYSTAL_IDS {
            // EndCrystal.hurtServer: false while invulnerable, else it blows up.
            let index = (id - CRYSTAL_IDS) as usize;
            if self.crystals.get(index).is_none_or(|c| c.invulnerable) {
                return Hurt::Refused;
            }
            let by = by_player.then(|| self.acting_player());
            self.destroy_crystal(index, by, CrystalDamage::Direct);
            Hurt::Landed { full: true }
        } else if id >= SPECIAL {
            let by = by_player.then(|| self.acting_player());
            self.hurt_dragon((id - SPECIAL) as usize, amount, by_player, by, arrow)
        } else {
            Hurt::Refused
        }
    }
    /// `EnderDragon.hurt(part, source, damage)` on the dragon in the level.
    /// `by` is the player behind the hit, remembered for the kill credit.
    fn hurt_dragon(
        &mut self,
        part: usize,
        damage: f32,
        counts: bool,
        by: Option<FightWho>,
        arrow: bool,
    ) -> Hurt {
        let Some(d) = self.dragon.as_mut() else {
            return Hurt::Refused;
        };
        let was_down = d.down();
        let (cooldown, last_hurt) = (d.invulnerable, d.last_hurt);
        let hurt = d.hurt(part, damage, counts, arrow);
        // LivingEntity.hurtServer: damage that is applied sets lastHurtByPlayer
        // (100 ticks); a hit the damage cooldown absorbs returns before that.
        if counts
            && matches!(hurt, Hurt::Landed { .. })
            && (d.invulnerable != cooldown || d.last_hurt != last_hurt)
            && let Some(who) = by
        {
            d.hurt_by_player = Some((who, d.ticks));
        }
        if !was_down && d.down() {
            self.credit_dragon_kill();
        }
        let Some(d) = self.dragon.as_mut() else {
            return hurt;
        };
        if hurt == (Hurt::Landed { full: true }) {
            // Mob.playHurtSound: the ambient sound clock starts again.
            d.ambient_time = -AMBIENT_INTERVAL;
            let at = [d.position.x, d.position.y, d.position.z];
            let pitch = 0.8 + self.world.random.next_float() * 0.3;
            self.sounds
                .play("minecraft:entity.ender_dragon.hurt", Some(at), 5.0, pitch);
        }
        hurt
    }
    /// `LivingEntity.die`: `getKillCredit().awardKillScore`, for the player
    /// who hurt the dragon within the last 100 ticks. A guest's kill is
    /// theirs: their statistics, scores and advancements.
    fn credit_dragon_kill(&mut self) {
        let Some(d) = self.dragon.as_ref() else {
            return;
        };
        let Some((who, _)) = d.hurt_by_player.filter(|(_, hit)| d.ticks - hit <= 100) else {
            return;
        };
        let view = horizon_client::advancements::EntityView {
            kind: "minecraft:ender_dragon".to_owned(),
            position: [d.position.x, d.position.y, d.position.z],
            ..Default::default()
        };
        let id = d.id;
        let guest = match who {
            FightWho::Guest(conn) if true => Some(conn),
            _ => None,
        };
        if guest.is_none() {
            self.award_dragon_kill(id, &view);
        }
    }
    /// `ServerPlayer.awardKillScore` for the dragon, on the killer's state.
    fn award_dragon_kill(&mut self, id: u32, view: &horizon_client::advancements::EntityView) {
        self.stats.add("killed", "minecraft:ender_dragon", 1);
        self.stats.custom("mob_kills", 1);
        self.score_player_killed(super::commands::Target::Mob(id));
        self.trigger_killed(view, None);
    }
    /// Player melee on the dragon's parts or a crystal under the crosshair.
    /// The dragon part under the crosshair within reach: its id and distance.
    pub(super) fn dragon_hit(&self) -> Option<(u32, f64)> {
        let eye = self.eye_position()?;
        let dir = super::view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        let mut best: Option<(u32, f64)> = None;
        for (id, b) in self.special_targets() {
            let (lo, hi) = (b.min_position(), b.max_position());
            let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
            let (mut t0, mut t1) = (0.0f64, reach);
            let mut hit = true;
            for a in 0..3 {
                if dir[a].abs() < 1e-9 {
                    hit &= eye[a] >= lo[a] && eye[a] <= hi[a];
                    continue;
                }
                let (mut ta, mut tb) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
            }
            if hit && t0 <= t1 && best.is_none_or(|b| t0 < b.1) {
                best = Some((id, t0));
            }
        }
        best
    }
    pub(super) fn attack_dragon(&mut self) -> bool {
        let Some((id, d)) = self.dragon_hit() else {
            return false;
        };
        if !self.before_block(d) || self.mob_hit().is_some_and(|m| m.1 < d) {
            return false;
        }
        let hand_damage = self.attack_item_info().map_or(0.0, |i| i.attack_damage);
        let delay = horizon_client::attack_strength::delay(self.main_hand_attack_speed());
        let scale = ((self.attack_ticks as f32 + 0.5) / delay).clamp(0.0, 1.0);
        let mut amount =
            (1.0 + hand_damage + self.effect_attack_bonus()) * (0.2 + scale * scale * 0.8);
        let falling = self
            .player
            .as_ref()
            .is_some_and(|p| !p.ground.on_ground && p.velocity.y < 0.0 && !p.sprinting);
        if scale > 0.9 && falling {
            amount *= 1.5;
        }
        amount += self.enchant_damage_bonus("minecraft:ender_dragon") * scale;
        self.attack_ticks = 0;
        self.swing_arm();
        self.hit_special(id, amount, true, false);
        true
    }
    /// `getHeightmapPos(MOTION_BLOCKING_NO_LEAVES)`.
    fn motion_blocking_no_leaves(&self, x: i32, z: i32) -> i32 {
        (0..256)
            .rev()
            .find(|&y| {
                let f = self.res.facts().get(self.world.block(x, y, z));
                (f.full_collision
                    || f.legacy_solid
                    || f.fluid != horizon_client::blocks::Fluid::None)
                    && !f.name.ends_with("_leaves")
            })
            .map_or(0, |y| y + 1)
    }
    fn podium_top(&self) -> Vec3 {
        Vec3::new(0.5, f64::from(self.motion_blocking_no_leaves(0, 0)), 0.5)
    }
    /// The part positions from the yaw and the flight history.
    fn place_dragon_parts(&mut self) {
        let Some(d) = self.dragon.as_mut() else {
            return;
        };
        let tilt =
            ((d.sample(5).0 - d.sample(10).0) * 10.0) as f32 * (std::f32::consts::PI / 180.0);
        let (ct, st) = (tilt.cos(), tilt.sin());
        let r1 = d.yaw.to_radians();
        let (s1, c1) = (r1.sin(), r1.cos());
        let p = d.position;
        let set = |d: &mut Dragon, i: usize, x: f32, y: f64, z: f32| {
            d.parts[i].0 = Vec3::new(p.x + f64::from(x), p.y + y, p.z + f64::from(z));
        };
        set(d, 2, s1 * 0.5, 0.0, -c1 * 0.5);
        set(d, 6, c1 * 4.5, 2.0, s1 * 4.5);
        set(d, 7, c1 * -4.5, 2.0, s1 * -4.5);
        let r2 = d.yaw.to_radians() - d.yaw_accel * 0.01;
        let (s2, c2) = (r2.sin(), r2.cos());
        let y_off = if d.phase.sitting() {
            -1.0
        } else {
            d.sample(5).0 - d.sample(0).0
        } as f32;
        set(
            d,
            0,
            s2 * 6.5 * ct,
            f64::from(y_off + st * 6.5),
            -c2 * 6.5 * ct,
        );
        set(
            d,
            1,
            s2 * 5.5 * ct,
            f64::from(y_off + st * 5.5),
            -c2 * 5.5 * ct,
        );
        let p1 = d.sample(5);
        for i in 0..3 {
            let p0 = d.sample(12 + i * 2);
            let rot = d.yaw.to_radians() + wrap(p0.1 - p1.1).to_radians();
            let (ss, cc) = (rot.sin(), rot.cos());
            let dd = (i as f32 + 1.0) * 2.0;
            let x = -(s1 * 1.5 + ss * dd) * ct;
            let y = p0.0 - p1.0 - f64::from((dd + 1.5) * st) + 1.5;
            let z = (c1 * 1.5 + cc * dd) * ct;
            set(d, 3 + i, x, y, z);
        }
    }
    /// The dragon fight's bar for a player at `at`
    /// (`EnderDragonFight.dragonEvent`, visible while the fight is not won
    /// and the player is alive within 192 blocks of (0, 128, 0)).
    pub(super) fn dragon_boss_progress(&self, at: [f64; 3], alive: bool) -> Option<f32> {
        if self.dim != Dim::End || !alive || !self.fight.loaded || self.fight.killed {
            return None;
        }
        let near = at[0].powi(2) + (at[1] - 128.0).powi(2) + at[2].powi(2) <= 192.0 * 192.0;
        if !near {
            return None;
        }
        Some(
            self.dragon
                .as_ref()
                .map_or(1.0, |d| (d.health / MAX_HEALTH).clamp(0.0, 1.0)),
        )
    }
}
pub(super) struct DragonFireball {
    /// Its entity id once a server's players track it (0 until then).
    pub(super) id: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
    pub(super) velocity: Vec3,
    age: i32,
}
impl DragonFireball {
    /// One a server sent (`handleAddEntity`).
    pub(super) fn remote(id: u32, at: Vec3, velocity: Vec3) -> Self {
        Self {
            id,
            position: at,
            previous: at,
            velocity,
            age: 0,
        }
    }
    /// `AbstractHurtingProjectile.tick` on a server's client: `applyInertia`
    /// first (power 0.1, inertia 0.95 or 0.8 in water), then the move; only
    /// the server decides what it hits.
    pub(super) fn remote_step(&mut self, water: bool) {
        self.previous = self.position;
        self.age += 1;
        let v = self.velocity;
        // Vec3.normalize: a vector shorter than 1e-5 stays zero.
        let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
        let n = if l < 1.0e-5 {
            Vec3::new(0.0, 0.0, 0.0)
        } else {
            Vec3::new(v.x / l, v.y / l, v.z / l)
        };
        let inertia = f64::from(if water { 0.8f32 } else { 0.95f32 });
        let v = Vec3::new(
            (v.x + n.x * 0.1) * inertia,
            (v.y + n.y * 0.1) * inertia,
            (v.z + n.z * 0.1) * inertia,
        );
        self.velocity = v;
        self.position = Vec3::new(
            self.position.x + v.x,
            self.position.y + v.y,
            self.position.z + v.z,
        );
    }
}
pub(super) struct Breath {
}
impl Breath {
}
impl Game<'_> {
    /// `EnderDragonRenderer` and `EnderDragonModel.setupAnim`.
    pub(super) fn dragon_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
        additive: &mut Vec<Vertex>,
    ) {
        let (Some(d), Some(model)) = (self.dragon.as_ref(), self.dragon_model.as_ref()) else {
            return;
        };
        let a = alpha as f32;
        let sprite = |n: &str| {
            self.front
                .gui
                .atlas
                .sprites
                .get(n)
                .map(|r| [r.u0, r.v0, r.u1, r.v1])
        };
        let (Some(skin), Some(eyes)) = (
            sprite("entity/enderdragon/dragon"),
            sprite("entity/enderdragon/dragon_eyes"),
        ) else {
            return;
        };
        let pos = glam::DVec3::new(
            d.previous.x + (d.position.x - d.previous.x) * alpha,
            d.previous.y + (d.position.y - d.previous.y) * alpha,
            d.previous.z + (d.position.z - d.previous.z) * alpha,
        );
        let h = |delay: usize| d.sample_lerp(delay, a);
        let yr = h(7).1;
        let rot2 = (h(5).0 - h(10).0) as f32;
        let mut base = glam::Mat4::from_translation((pos - camera).as_vec3())
            * glam::Mat4::from_rotation_y((-yr).to_radians())
            * glam::Mat4::from_rotation_x((rot2 * 10.0).to_radians())
            * glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.0, 1.0))
            * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0))
            * glam::Mat4::from_translation(glam::Vec3::new(0.0, -1.501, 0.0));
        let rays_base = base * glam::Mat4::from_translation(glam::Vec3::new(0.0, -1.0, -2.0));
        // setupAnim.
        let flap = (d.old_flap + (d.flap - d.old_flap) * a) * std::f32::consts::TAU;
        let mut bounce = (flap - 1.0).sin() + 1.0;
        bounce = (bounce * bounce + bounce * 2.0) * 0.05;
        base *=
            glam::Mat4::from_translation(glam::Vec3::new(0.0, (bounce - 2.0) * 16.0, -48.0) / 16.0)
                * glam::Mat4::from_rotation_x((bounce * 2.0).to_radians());
        let mut poses = horizon_client::mobs::anim::Poses::new(model);
        poses.get("jaw")[3] = (flap.sin() + 1.0) * 0.2;
        let sitting = d.phase.sitting();
        let landing = matches!(d.phase, Phase::Landing { .. } | Phase::Takeoff { .. });
        let egg_dist = {
            let p = self.podium_top();
            ((p.x - d.position.x).powi(2)
                + (p.y - d.position.y).powi(2)
                + (p.z - d.position.z).powi(2))
            .sqrt()
        };
        let head_offset = |part: usize, start: (f64, f32), point: (f64, f32)| -> f32 {
            if landing {
                part as f32 / (egg_dist / 4.0).max(1.0) as f32
            } else if sitting {
                part as f32
            } else if part == 6 {
                0.0
            } else {
                (point.0 - start.0) as f32
            }
        };
        let start = h(6);
        let rot2y = wrap(h(5).1 - h(10).1);
        let rot = wrap(h(5).1 + rot2y / 2.0);
        let deg = std::f32::consts::PI / 180.0;
        let (mut xx, mut yy, mut zz) = (0.0f32, 20.0f32, -12.0f32);
        for i in 0..5 {
            let point = h(5 - i);
            let pose = poses.get(&format!("neck{i}"));
            let neck_x = (i as f32 * 0.45 + flap).cos() * 0.15;
            pose[4] = wrap(point.1 - start.1) * deg * 1.5;
            pose[3] = neck_x + head_offset(i, start, point) * deg * 1.5 * 5.0;
            pose[5] = -wrap(point.1 - rot) * deg * 1.5;
            pose[0] = xx;
            pose[1] = yy;
            pose[2] = zz;
            let (xr, yrr) = (pose[3], pose[4]);
            xx -= yrr.sin() * xr.cos() * 10.0;
            yy += xr.sin() * 10.0;
            zz -= yrr.cos() * xr.cos() * 10.0;
        }
        let current = h(0);
        let head = poses.get("head");
        head[0] = xx;
        head[1] = yy;
        head[2] = zz;
        head[4] = wrap(current.1 - start.1) * deg;
        head[3] = wrap(head_offset(6, start, current)) * deg * 1.5 * 5.0;
        head[5] = -wrap(current.1 - rot) * deg;
        poses.get("body")[5] = -rot2y * 1.5 * deg;
        let lw = [0.125 - flap.cos() * 0.2, -0.25, -(flap.sin() + 0.125) * 0.8];
        let lt = ((flap + 2.0).sin() + 0.5) * 0.75;
        {
            let p = poses.get("left_wing");
            (p[3], p[4], p[5]) = (lw[0], lw[1], lw[2]);
        }
        poses.get("left_wing_tip")[5] = lt;
        {
            let p = poses.get("right_wing");
            (p[3], p[4], p[5]) = (lw[0], -lw[1], -lw[2]);
        }
        poses.get("right_wing_tip")[5] = -lt;
        for side in ["left", "right"] {
            poses.get(&format!("{side}_hind_leg"))[3] = 1.0 + bounce * 0.1;
            poses.get(&format!("{side}_hind_leg_tip"))[3] = 0.5 + bounce * 0.1;
            poses.get(&format!("{side}_hind_foot"))[3] = 0.75 + bounce * 0.1;
            poses.get(&format!("{side}_front_leg"))[3] = 1.3 + bounce * 0.1;
            poses.get(&format!("{side}_front_leg_tip"))[3] = -0.5 - bounce * 0.1;
            poses.get(&format!("{side}_front_foot"))[3] = 0.75 + bounce * 0.1;
        }
        let mut tail_x = 0.0f32;
        let (mut xx, mut yy, mut zz) = (0.0f32, 10.0f32, 60.0f32);
        let start = h(11);
        for i in 0..12 {
            let point = h(12 + i);
            tail_x += (i as f32 * 0.45 + flap).sin() * 0.05;
            let p = poses.get(&format!("tail{i}"));
            p[4] = (wrap(point.1 - start.1) * 1.5 + 180.0) * deg;
            p[3] = tail_x + (point.0 - start.0) as f32 * deg * 1.5 * 5.0;
            p[5] = wrap(point.1 - rot) * deg * 1.5;
            p[0] = xx;
            p[1] = yy;
            p[2] = zz;
            let (xr, yrr) = (p[3], p[4]);
            yy += xr.sin() * 10.0;
            zz -= yrr.cos() * xr.cos() * 10.0;
            xx -= yrr.sin() * xr.cos() * 10.0;
        }
        let l = self.world.light(
            d.position.x.floor() as i32,
            d.position.y.floor() as i32,
            d.position.z.floor() as i32,
        );
        let light = (u32::from(l >> 4) * 16) | ((u32::from(l & 15) * 16) << 8);
        let death = if d.death_time > 0 {
            d.death_time as f32 + a
        } else {
            0.0
        };
        // The dissolve is approximated by fading out.
        let color = if death > 0.0 {
            let alpha = ((1.0 - death / 200.0) * 255.0).clamp(0.0, 255.0) as u32;
            alpha << 24 | 0x00FF_FFFF
        } else {
            0xFFFF_FFFF
        };
        let overlay = u32::from(d.hurt_time > 0);
        horizon_client::mobs::render::draw(model, base, &poses, skin, color, light, overlay, out);
        // RenderTypes.eyes: the glowing eyes, added on top.
        let mut eye_quads = Vec::new();
        horizon_client::mobs::render::draw(
            model,
            base,
            &poses,
            eyes,
            0xFFFF_FFFF,
            240 | 240 << 8,
            0,
            &mut eye_quads,
        );
        for v in &mut eye_quads {
            v.layer = 2;
        }
        additive.extend(eye_quads);
        if death > 0.0 {
            self.dragon_rays(rays_base, death / 200.0, additive);
        }
        // The healing beam to the nearest crystal.
        if let Some(c) = d.nearest_crystal.and_then(|i| self.crystals.get(i)) {
            let bob = f64::from(horizon_client::end_crystal::y(c.time as f32 + a));
            let to = glam::DVec3::new(c.position.x, c.position.y + bob, c.position.z);
            self.crystal_beam(pos, to - pos, self.ticks as f32 + a, camera, out);
        }
    }
    /// `submitRays`: the death rays from a fixed seed, fading in white to magenta.
    fn dragon_rays(&self, base: glam::Mat4, t: f32, out: &mut Vec<Vertex>) {
        let white = self.front.gui.atlas.font.white();
        let uv = [white.u0, white.v0];
        let over = if t > 0.8 {
            ((t - 0.8) / 0.2).min(1.0)
        } else {
            0.0
        };
        let inner = (((1.0 - over) * 255.0) as u32) << 24 | 0x00FF_FFFF;
        let outer = 0x00FF_00FF;
        let mut random = LegacyRandom::new(432);
        let mut m = base;
        let count = ((t + t * t) / 2.0 * 60.0).floor() as i32;
        let half_sqrt3 = 3f32.sqrt() / 2.0;
        let tau = std::f32::consts::TAU;
        for _ in 0..count {
            let mut f = || random.next_float() * tau;
            let q1 = glam::Quat::from_rotation_x(f())
                * glam::Quat::from_rotation_y(f())
                * glam::Quat::from_rotation_z(f());
            let (a, b, c) = (f(), f(), f());
            let q2 = glam::Quat::from_rotation_x(a)
                * glam::Quat::from_rotation_y(b)
                * glam::Quat::from_rotation_z(c + t * std::f32::consts::FRAC_PI_2);
            m *= glam::Mat4::from_quat(q1 * q2);
            let length = random.next_float() * 20.0 + 5.0 + over * 10.0;
            let width = random.next_float() * 2.0 + 1.0 + over * 2.0;
            let o = glam::Vec3::ZERO;
            let left = glam::Vec3::new(-half_sqrt3 * width, length, -0.5 * width);
            let right = glam::Vec3::new(half_sqrt3 * width, length, -0.5 * width);
            let bottom = glam::Vec3::new(0.0, length, width);
            for (p1, p2) in [(left, right), (right, bottom), (bottom, left)] {
                // A triangle as a quad with its last corner repeated.
                for (p, c) in [(o, inner), (p1, outer), (p2, outer), (p2, outer)] {
                    out.push(Vertex {
                        normal: [0; 4],
                        pos: m.transform_point3(p).to_array(),
                        uv,
                        layer: 2,
                        color: c,
                        light: 0,

                        glint_uv: [0.0, 0.0],
                    });
                }
            }
        }
    }
    /// `submitCrystalBeams`: an eight-sided tube from `from` (+2 up) along
    /// `delta`, the beam texture scrolling along it.
    pub(super) fn crystal_beam(
        &self,
        from: glam::DVec3,
        delta: glam::DVec3,
        time: f32,
        camera: glam::DVec3,
        out: &mut Vec<Vertex>,
    ) {
        let Some(s) = self
            .front
            .gui
            .atlas
            .sprites
            .get("entity/end_crystal/end_crystal_beam")
            .copied()
        else {
            return;
        };
        let q = |v: f32| (v * 65535.0).round().clamp(0.0, 65535.0) as u32;
        let wrap_light = q(s.v0) | q(s.v1 - s.v0) << 16;
        let (dx, dy, dz) = (delta.x as f32, delta.y as f32, delta.z as f32);
        let horizontal = (dx * dx + dz * dz).sqrt();
        let length = (dx * dx + dy * dy + dz * dz).sqrt();
        let m = glam::Mat4::from_translation(
            (from - camera).as_vec3() + glam::Vec3::new(0.0, 2.0, 0.0),
        ) * glam::Mat4::from_rotation_y(-dz.atan2(dx) - std::f32::consts::FRAC_PI_2)
            * glam::Mat4::from_rotation_x(-horizontal.atan2(dy) - std::f32::consts::FRAC_PI_2);
        let (v0, v1) = (-time * 0.01, length / 32.0 - time * 0.01);
        let (mut last_sin, mut last_cos, mut last_u) = (0.0f32, 0.75f32, 0.0f32);
        let u = |x: f32| s.u0 + (s.u1 - s.u0) * x;
        for i in 1..=8 {
            let a = i as f32 * std::f32::consts::TAU / 8.0;
            let (sin, cos) = (a.sin() * 0.75, a.cos() * 0.75);
            let uu = i as f32 / 8.0;
            for (p, uv, c) in [
                (
                    glam::Vec3::new(last_sin * 0.2, last_cos * 0.2, 0.0),
                    [u(last_u), v0],
                    0xFF00_0000,
                ),
                (
                    glam::Vec3::new(last_sin, last_cos, length),
                    [u(last_u), v1],
                    0xFFFF_FFFF,
                ),
                (glam::Vec3::new(sin, cos, length), [u(uu), v1], 0xFFFF_FFFF),
                (
                    glam::Vec3::new(sin * 0.2, cos * 0.2, 0.0),
                    [u(uu), v0],
                    0xFF00_0000,
                ),
            ] {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: m.transform_point3(p).to_array(),
                    uv,
                    layer: 6,
                    color: c,
                    light: wrap_light,

                    glint_uv: [0.0, 0.0],
                });
            }
            (last_sin, last_cos, last_u) = (sin, cos, uu);
        }
    }
    /// Dragon fireballs as the dragon_fireball sprite facing the camera.
    pub(super) fn dragon_fireball_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        let Some(s) = self
            .front
            .gui
            .atlas
            .sprites
            .get("entity/enderdragon/dragon_fireball")
            .copied()
        else {
            return;
        };
        let forward = glam::DVec3::from_array(super::view_vector(self.yaw, self.pitch));
        let right = forward.cross(glam::DVec3::Y).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        for b in &self.dragon_fireballs {
            let p = glam::DVec3::new(
                b.previous.x + (b.position.x - b.previous.x) * alpha,
                b.previous.y + (b.position.y - b.previous.y) * alpha,
                b.previous.z + (b.position.z - b.previous.z) * alpha,
            ) - camera;
            // DragonFireballRenderer: scale 2, full bright.
            let h = 1.0;
            for (c, uv) in [
                (p - right * h - up * h, [s.u0, s.v1]),
                (p + right * h - up * h, [s.u1, s.v1]),
                (p + right * h + up * h, [s.u1, s.v0]),
                (p - right * h + up * h, [s.u0, s.v0]),
            ] {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: c.as_vec3().to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: 240 | 240 << 8,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
    }
}
impl crate::game::Sim<'_> {
    pub(super) fn exit_portal(&self) -> Option<[i32; 3]> {
        self.fight.exit_portal
    }
}
/// Saving and loading the dragon and the crystals as chunk entities.
impl Dragon {
    pub(super) fn to_nbt(&self, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
        let mut tag = horizon_client::entity_nbt::vehicle_to_nbt(
            "minecraft:ender_dragon",
            self.position,
            self.velocity,
            self.yaw,
            uuid,
            raw,
        );
        if let Tag::Compound(map) = &mut tag {
            map.insert("Health".into(), Tag::Float(self.health.to_bits()));
            map.insert("DragonPhase".into(), Tag::Int(self.phase.id()));
            map.insert("DragonDeathTime".into(), Tag::Int(self.death_time));
            map.insert(
                "sitting_damage_received".into(),
                Tag::Float(self.sitting_damage.to_bits()),
            );
        }
        tag
    }
    pub(super) fn from_nbt(tag: &Tag, seed: i64) -> Option<Self> {
        let (position, velocity, yaw) = horizon_client::entity_nbt::boat_from_nbt(tag)?;
        let mut d = Self::new(position, yaw, seed);
        d.velocity = velocity;
        if let Some(Tag::Float(h)) = tag.get("Health") {
            d.health = f32::from_bits(*h);
        }
        if let Some(Tag::Int(p)) = tag.get("DragonPhase") {
            d.phase = Phase::from_id(*p);
        }
        if let Some(Tag::Int(t)) = tag.get("DragonDeathTime") {
            d.death_time = *t;
        }
        Some(d)
    }
}
impl Crystal {
    pub(super) fn to_nbt(&self, uuid: [i32; 4], raw: Option<&Tag>) -> Tag {
        let mut tag = horizon_client::entity_nbt::vehicle_to_nbt(
            "minecraft:end_crystal",
            self.position,
            Vec3::ZERO,
            0.0,
            uuid,
            raw,
        );
        if let Tag::Compound(map) = &mut tag {
            map.insert("ShowBottom".into(), Tag::Byte(i8::from(self.show_bottom)));
            if let Some(b) = self.beam {
                map.insert("beam_target".into(), Tag::IntArray(b.to_vec()));
            }
            map.insert(
                "Invulnerable".into(),
                Tag::Byte(i8::from(self.invulnerable)),
            );
        }
        tag
    }
    pub(super) fn from_nbt(tag: &Tag, time: i32) -> Option<Self> {
        let (position, _, _) = horizon_client::entity_nbt::boat_from_nbt(tag)?;
        Some(Self {
            id: 0,
            position,
            show_bottom: !matches!(tag.get("ShowBottom"), Some(Tag::Byte(0))),
            time,
            beam: match tag.get("beam_target") {
                Some(Tag::IntArray(v)) if v.len() == 3 => Some([v[0], v[1], v[2]]),
                _ => None,
            },
            invulnerable: matches!(tag.get("Invulnerable"), Some(Tag::Byte(1))),
        })
    }
}
impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// `ServerExplosion.hurtEntities`: crystals in reach are removed without
    /// another blast; explosions always hurt dragons.
    pub(super) fn explosion_hits_special(&mut self, center: [f64; 3], radius: f32) {
        let diameter = f64::from(radius * 2.0);
        let in_reach = |p: Vec3| {
            ((p.x - center[0]).powi(2) + (p.y - center[1]).powi(2) + (p.z - center[2]).powi(2))
                .sqrt()
                / diameter
                <= 1.0
        };
        let crystals: Vec<Vec3> = self
            .crystals
            .iter()
            .filter(|c| !c.invulnerable && in_reach(c.position))
            .map(|c| c.position)
            .collect();
        for at in crystals {
            // Look up the current index after each removal.
            if let Some(i) = self.crystals.iter().position(|c| c.position == at) {
                self.destroy_crystal(i, None, CrystalDamage::Explosion);
            }
        }
        let Some(d) = self.dragon.as_ref() else {
            return;
        };
        let hits = d.explosion_hits(center, radius, |b| {
            let (lo, hi) = (b.min_position(), b.max_position());
            self.seen_percent(center, [lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z])
        });
        let by = self.fight.blast_by;
        for (part, amount) in hits {
            self.hurt_dragon(part, amount, true, by, false);
        }
    }
}
impl Game<'_> {}
impl crate::game::Sim<'_> {
}
impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// The dragon and fight for `horizon-ctl status`.
    pub(super) fn dragon_status(&self) -> serde_json::Value {
        serde_json::json!({
            "dragon": self.dragon.as_ref().map(|d| serde_json::json!({
                "id": d.id,
                "position": [d.position.x, d.position.y, d.position.z],
                "health": d.health,
                "phase": d.phase.id(),
                "death_time": d.death_time,
                // The player it strafes: "host", or the guest's entity id.
                "strafing": match &d.phase {
                    Phase::Strafe { player: Some(FightWho::Host), .. } => serde_json::json!("host"),
                    _ => serde_json::Value::Null,
                },
            })),
            "players": self.fight.players,
            "crystals": self.crystals.len(),
            // The tracked id (0 until tracked) and position of each crystal.
            "crystal_list": self.crystals.iter().map(|c| serde_json::json!({
                "id": c.id,
                "position": [c.position.x, c.position.y, c.position.z],
            })).collect::<Vec<_>>(),
            "alive_crystals": self.fight.alive_crystals,
            "killed": self.fight.killed,
            "exit_portal": self.fight.exit_portal,
            "respawn": self.fight.respawn.map(|(s, t)| format!("{} {t}", s.name())),
        })
    }
}
/// Respawning the dragon with end crystals on the exit portal
/// (`EndCrystalItem.useOn`, `EnderDragonFight.tryRespawn`, `DragonRespawnStage`).
impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// `EndCrystalItem.useOn`: on obsidian or bedrock with room above.
    pub(super) fn place_end_crystal(
        &mut self,
        hit: &super::Hit,
        stack: &horizon_client::items::ItemStack,
    ) -> bool {
        if &*stack.item != "minecraft:end_crystal" {
            return false;
        }
        let name = self
            .res
            .facts()
            .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]))
            .name
            .clone();
        if !matches!(&*name, "minecraft:obsidian" | "minecraft:bedrock") {
            return false;
        }
        let above = [hit.pos[0], hit.pos[1] + 1, hit.pos[2]];
        if !self
            .res
            .facts()
            .is_air(self.world.block(above[0], above[1], above[2]))
        {
            return false;
        }
        let (x, y, z) = (
            f64::from(above[0]),
            f64::from(above[1]),
            f64::from(above[2]),
        );
        let room = Aabb::new(Vec3::new(x, y, z), Vec3::new(x + 1.0, y + 2.0, z + 1.0));
        if self.entity_in(room) {
            return false;
        }
        let time = self.world.random.next_int_bound(100_000).unwrap_or(0);
        // EndCrystal's constructor takes an id from the entity counter, so
        // the crystal is an entity a command can name (`@e[type=end_crystal]`).
        self.next_entity += 1;
        self.crystals.push(Crystal {
            id: self.next_entity,
            position: Vec3::new(x + 0.5, y, z + 0.5),
            show_bottom: false,
            time,
            beam: None,
            invulnerable: false,
        });
        self.world.modified.insert((above[0] >> 4, above[2] >> 4));
        if self.dim == Dim::End {
            self.try_respawn();
        }
        if !self.creative {
            self.consume_held();
        }
        self.swing_arm();
        true
    }
    /// `EndCrystal.kill` (`/kill`): the crystal is removed, without a blast
    /// (`Entity.kill` does not hurt it).
    pub(super) fn kill_crystal(&mut self, id: u32) {
        if let Some(i) = self.crystals.iter().position(|c| c.id == id) {
            self.destroy_crystal(i, None, CrystalDamage::Explosion);
        }
    }
    /// `EnderDragon.kill` (`/kill`): `Entity.kill` removes the entity, it
    /// does not hurt it (the dragon flies its death flight when its health
    /// runs out).
    pub(super) fn kill_dragon(&mut self, id: u32) {
        if self.dragon.as_ref().is_some_and(|d| d.id == id) {
            self.dragon = None;
        }
    }
    /// Any entity touching `room` (`Level.getEntities(null, box)`).
    fn entity_in(&self, room: Aabb) -> bool {
        let touches = |lo: Vec3, hi: Vec3| {
            let (a, b) = (room.min_position(), room.max_position());
            lo.x < b.x && hi.x > a.x && lo.y < b.y && hi.y > a.y && lo.z < b.z && hi.z > a.z
        };
        // Level.getEntities: the host's player and every guest in the level.
        let player = self
            .fight_players()
            .iter()
            .any(|p| !p.spectator && touches(p.bounds.min_position(), p.bounds.max_position()));
        player
            || self.crystals.iter().any(|c| {
                touches(
                    Vec3::new(c.position.x - 1.0, c.position.y, c.position.z - 1.0),
                    Vec3::new(c.position.x + 1.0, c.position.y + 2.0, c.position.z + 1.0),
                )
            })
            || self.mobs.iter().any(|m| {
                let w = f64::from(m.width()) / 2.0;
                let h = f64::from(m.height());
                touches(
                    Vec3::new(m.position.x - w, m.position.y, m.position.z - w),
                    Vec3::new(m.position.x + w, m.position.y + h, m.position.z + w),
                )
            })
    }
    /// `EnderDragonFight.tryRespawn`: a crystal on each side of the portal.
    fn try_respawn(&mut self) {
        self.load_fight();
        if !self.fight.killed || self.fight.respawn.is_some() {
            return;
        }
        let origin = match self.fight.exit_portal {
            Some(o) => o,
            None => {
                let _ = self.activate_exit_portal();
                match self.fight.exit_portal {
                    Some(o) => o,
                    None => return,
                }
            }
        };
        let mut found = Vec::new();
        for d in [[0, -1], [0, 1], [-1, 0], [1, 0]] {
            // new AABB(center.relative(direction, 3)): the block cube.
            let cell = [origin[0] + d[0] * 3, origin[1] + 1, origin[2] + d[1] * 3];
            let lo = Vec3::new(f64::from(cell[0]), f64::from(cell[1]), f64::from(cell[2]));
            let hits: Vec<Vec3> = self
                .crystals
                .iter()
                .filter(|c| {
                    let p = c.position;
                    p.x + 1.0 > lo.x
                        && p.x - 1.0 < lo.x + 1.0
                        && p.y + 2.0 > lo.y
                        && p.y < lo.y + 1.0
                        && p.z + 1.0 > lo.z
                        && p.z - 1.0 < lo.z + 1.0
                })
                .map(|c| c.position)
                .collect();
            if hits.is_empty() {
                return;
            }
            found.extend(hits);
        }
        // respawnDragon: the podium goes back to end stone, then inactive.
        for y in origin[1] - 1..=origin[1] + 3 {
            for z in origin[2] - 3..=origin[2] + 3 {
                for x in origin[0] - 3..=origin[0] + 3 {
                    let name = self.block_name_at([x, y, z]);
                    if matches!(&*name, "minecraft:bedrock" | "minecraft:end_portal") {
                        let _ = self.set_state_named([x, y, z], "minecraft:end_stone", &[]);
                    }
                }
            }
        }
        let _ = self.place_podium(origin, false);
        self.fight.respawn = Some((Respawn::Start, 0));
        self.fight.respawn_crystals = found;
        self.fight.dirty = true;
    }
    fn block_name_at(&self, p: [i32; 3]) -> std::sync::Arc<str> {
        self.res
            .facts()
            .get(self.world.block(p[0], p[1], p[2]))
            .name
            .clone()
    }
    /// `resetSpikeCrystals`: spike crystals lose their beams and protection.
    fn reset_spike_crystals(&mut self) {
        let spikes = horizon_world::worldgen::end_spike::spikes_for_seed(self.level.seed);
        for c in &mut self.crystals {
            let on_spike = spikes.iter().any(|s| {
                (c.position.x - f64::from(s.center_x) - 0.5).abs() <= f64::from(s.radius)
                    && (c.position.z - f64::from(s.center_z) - 0.5).abs() <= f64::from(s.radius)
                    && c.position.y >= f64::from(s.height)
            });
            if on_spike {
                c.invulnerable = false;
                c.beam = None;
            }
        }
    }
    /// `abortRespawnSequence`.
    fn abort_respawn(&mut self) {
        self.fight.respawn = None;
        self.fight.respawn_crystals.clear();
        self.reset_spike_crystals();
        let _ = self.activate_exit_portal();
        self.fight.dirty = true;
    }
    /// `EnderDragon.checkCrystals`' search: the nearest crystal whose box
    /// meets the dragon's box grown by 32.
    fn nearest_crystal_to(&self, p: Vec3) -> Option<usize> {
        // The dragon's 16 by 8 box.
        let reach = Aabb::new(
            Vec3::new(p.x - 8.0, p.y, p.z - 8.0),
            Vec3::new(p.x + 8.0, p.y + 8.0, p.z + 8.0),
        )
        .inflate(Vec3::new(32.0, 32.0, 32.0));
        self.crystals
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                // An end crystal's 2 by 2 box.
                reach.intersects(Aabb::new(
                    Vec3::new(c.position.x - 1.0, c.position.y, c.position.z - 1.0),
                    Vec3::new(c.position.x + 1.0, c.position.y + 2.0, c.position.z + 1.0),
                ))
            })
            .min_by(|a, b| {
                let da = (a.1.position.x - p.x).powi(2)
                    + (a.1.position.y - p.y).powi(2)
                    + (a.1.position.z - p.z).powi(2);
                let db = (b.1.position.x - p.x).powi(2)
                    + (b.1.position.y - p.y).powi(2)
                    + (b.1.position.z - p.z).powi(2);
                da.total_cmp(&db)
            })
            .map(|(i, _)| i)
    }
}

/// The dragon on a server's client (`ClientPacketListener.handleAddEntity`
/// and the client side of `EnderDragon.aiStep`): the server flies it and
/// sends its moves, health and phase; the client flaps its wings, keeps
/// its flight history and places its parts, so it can be drawn and hit.
impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// `handleAddEntity` for `minecraft:ender_dragon`. Its constructor
    /// sets the full health and the hovering phase until the server's
    /// synched data says otherwise.
    pub(super) fn spawn_remote_dragon(&mut self, id: i32, position: [f64; 3], yaw: f32) {
        let at = Vec3::new(position[0], position[1], position[2]);
        let mut d = Dragon::new(at, yaw, i64::from(id));
        d.id = id as u32;
        d.phase = Phase::from_id(10);
        self.dragon = Some(d);
        self.place_dragon_parts();
        if let Some(d) = self.dragon.as_mut() {
            for part in &mut d.parts {
                part.1 = part.0;
            }
        }
    }
    /// The remote dragon, if `id` is it.
    fn remote_dragon_mut(&mut self, id: i32) -> Option<&mut Dragon> {
        self.dragon.as_mut().filter(|d| d.id as i32 == id)
    }
    /// `handleSetEntityData`: LivingEntity.DATA_HEALTH_ID and
    /// EnderDragon.DATA_PHASE (`onSyncedDataUpdated` sets the phase).
    pub(super) fn remote_dragon_data(&mut self, id: i32, values: &[(u8, i32, Value)]) {
        let Some(d) = self.remote_dragon_mut(id) else {
            return;
        };
        for (index, _, value) in values {
            match (index, value) {
                (9, Value::Float(health)) => d.health = *health,
                (16, Value::Int(phase)) => d.set_phase(Phase::from_id(*phase)),
                _ => {}
            }
        }
    }
    /// `handleSetEntityMotion`: the velocity its wings flap by.
    pub(super) fn remote_dragon_motion(&mut self, id: i32, velocity: [f64; 3]) {
        if let Some(d) = self.remote_dragon_mut(id) {
            d.velocity = Vec3::new(velocity[0], velocity[1], velocity[2]);
        }
    }
    /// `handleHurtAnimation` and `handleDamageEvent`: the red flash.
    pub(super) fn remote_dragon_hurt(&mut self, id: i32) {
        if let Some(d) = self.remote_dragon_mut(id) {
            d.hurt_time = 10;
        }
    }
    /// `handleRemoveEntities`, or a crystal removed: the healing beam
    /// keeps pointing at the same crystal.
    pub(super) fn remove_remote_dragon(&mut self, id: i32) {
        if self.remote_dragon_mut(id).is_some() {
            self.dragon = None;
            return;
        }
        let Some(index) = self.crystals.iter().position(|c| c.id as i32 == id) else {
            return;
        };
        if let Some(d) = self.dragon.as_mut() {
            match d.nearest_crystal {
                Some(n) if n == index => d.nearest_crystal = None,
                Some(n) if n > index => d.nearest_crystal = Some(n - 1),
                _ => {}
            }
        }
    }
    /// One client tick of the dragon. `lerp`: the step toward its last
    /// sent position and yaw (LivingEntity's interpolation), and the
    /// steps left including this one.
    pub(super) fn tick_remote_dragon(&mut self, lerp: Option<([f64; 3], Option<f32>, u32)>) {
        let Some(d) = self.dragon.as_mut() else {
            return;
        };
        d.previous = d.position;
        d.previous_yaw = d.yaw;
        d.old_flap = d.flap;
        d.ticks += 1;
        d.hurt_time = (d.hurt_time - 1).max(0);
        for part in &mut d.parts {
            part.1 = part.0;
        }
        if let Some((target, yaw, steps)) = lerp {
            let n = f64::from(steps.max(1));
            d.position = Vec3::new(
                d.position.x + (target[0] - d.position.x) / n,
                d.position.y + (target[1] - d.position.y) / n,
                d.position.z + (target[2] - d.position.z) / n,
            );
            if let Some(yaw) = yaw {
                d.yaw += wrap(yaw - d.yaw) / steps.max(1) as f32;
            }
        }
        if !d.alive() {
            // EnderDragon.tickDeath on a client: it rises, and bursts
            // from tick 180 to 200; the server removes it.
            d.death_time += 1;
            let (t, p) = (d.death_time, d.position);
            if (180..=200).contains(&t) {
                let r = &mut d.random;
                let o = [
                    f64::from((r.next_float() - 0.5) * 8.0),
                    f64::from((r.next_float() - 0.5) * 4.0),
                    f64::from((r.next_float() - 0.5) * 8.0),
                ];
                self.particles.add(
                    &ParticleOptions::simple(ParticleType::ExplosionEmitter),
                    [p.x + o[0], p.y + 2.0 + o[1], p.z + o[2]],
                    [0.0; 3],
                );
            }
            if let Some(d) = self.dragon.as_mut() {
                d.position.y += 0.1;
                for part in &mut d.parts {
                    part.0.y += 0.1;
                }
            }
            return;
        }
        // processFlappingMovement and onFlap: its wing beats, and their sound.
        let flap_was = (d.old_flap * std::f32::consts::TAU).cos();
        let v = d.velocity;
        let mut flap_speed = 0.2 / ((v.x.hypot(v.z) as f32) * 10.0 + 1.0);
        flap_speed *= 2f32.powf(v.y as f32);
        d.flap += if d.phase.sitting() { 0.1 } else { flap_speed };
        let flap_now = (d.flap * std::f32::consts::TAU).cos();
        let at = [d.position.x, d.position.y, d.position.z];
        if flap_was <= -0.3 && flap_now >= -0.3 {
            let pitch = 0.8 + self.world.random.next_float() * 0.3;
            self.sounds
                .play("minecraft:entity.ender_dragon.flap", Some(at), 5.0, pitch);
        }
        let d = self.dragon.as_mut().expect("dragon");
        if !d.phase.sitting() {
            d.growl -= 1;
            if d.growl < 0 {
                d.growl = 200 + d.random.next_int_bound(200).unwrap_or(0);
                let pitch = 0.8 + self.world.random.next_float() * 0.3;
                self.sounds
                    .play("minecraft:entity.ender_dragon.growl", Some(at), 2.5, pitch);
            }
        }
        // checkCrystals' search runs on a client too (the healing beam).
        let d = self.dragon.as_mut().expect("dragon");
        if d.random.next_int_bound(10).unwrap_or(1) == 0 {
            let p = d.position;
            let nearest = self.nearest_crystal_to(p);
            self.dragon.as_mut().expect("dragon").nearest_crystal = nearest;
        }
        let d = self.dragon.as_mut().expect("dragon");
        d.yaw = wrap(d.yaw);
        d.record();
        self.place_dragon_parts();
    }
    /// The dragon's parts as a server's client attacks them: the server's
    /// ids for `EnderDragonPart`s follow the dragon's.
    pub(super) fn remote_dragon_part_hit(
        &self,
        eye: [f64; 3],
        dir: [f64; 3],
        reach: f64,
    ) -> Option<(i32, f64, [f64; 3])> {
        let d = self.dragon.as_ref().filter(|d| d.alive() && d.id != 0)?;
        (0..8)
            .filter_map(|i| {
                let b = d.part_box(i);
                let p = d.parts[i].0;
                super::ray_box(eye, dir, reach, b)
                    .map(|t| ((d.id + 1 + i as u32) as i32, t, [p.x, p.y, p.z]))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dragon(phase: Phase) -> Dragon {
        let mut d = Dragon::new(Vec3::new(0.0, 70.0, 0.0), 0.0, 1);
        d.phase = phase;
        d
    }
    fn flying() -> Phase {
        Phase::Holding {
            path: None,
            target: None,
            clockwise: false,
        }
    }

    #[test]
    fn a_perched_dragon_shrugs_arrows_off() {
        // AbstractDragonSittingPhase.onHurt: an arrow does nothing, melee does.
        let mut d = dragon(Phase::Scanning { time: 0 });
        assert_eq!(d.hurt(0, 9.0, true, true), Hurt::Shrugged);
        assert_eq!(d.health, MAX_HEALTH);
        assert_eq!(d.hurt(0, 9.0, true, false), Hurt::Landed { full: true });
        assert_eq!(d.health, MAX_HEALTH - 9.0);
        // In flight an arrow lands.
        let mut d = dragon(flying());
        assert_eq!(d.hurt(0, 9.0, true, true), Hurt::Landed { full: true });
        assert_eq!(d.health, MAX_HEALTH - 9.0);
    }

    #[test]
    fn the_cooldown_lets_only_the_excess_through() {
        // LivingEntity.hurtServer: damageCooldownTime > 10 keeps lastHurt.
        let mut d = dragon(flying());
        assert_eq!(d.hurt(0, 6.0, true, false), Hurt::Landed { full: true });
        assert_eq!(d.hurt(0, 4.0, true, false), Hurt::Landed { full: false });
        assert_eq!(d.health, MAX_HEALTH - 6.0);
        assert_eq!(d.hurt(0, 10.0, true, false), Hurt::Landed { full: false });
        assert_eq!(d.health, MAX_HEALTH - 10.0);
        // Ten ticks on, the window is over: a full hit again.
        d.invulnerable = 10;
        assert_eq!(d.hurt(0, 3.0, true, false), Hurt::Landed { full: true });
        assert_eq!(d.health, MAX_HEALTH - 13.0);
    }

    #[test]
    fn parts_and_sources() {
        // Neither head nor neck: a quarter plus one.
        let mut d = dragon(flying());
        assert_eq!(d.hurt(2, 20.0, true, false), Hurt::Landed { full: true });
        assert_eq!(d.health, MAX_HEALTH - 6.0);
        // Not a player's and not an explosion: true, but no damage.
        let mut d = dragon(flying());
        assert_eq!(d.hurt(0, 20.0, false, false), Hurt::Landed { full: false });
        assert_eq!(d.health, MAX_HEALTH);
        // Too little after the part's cut, and nothing while dying.
        assert_eq!(d.hurt(3, 0.001, true, false), Hurt::Refused);
        d.phase = Phase::Dying {
            target: None,
            time: 0,
        };
        assert_eq!(d.hurt(0, 20.0, true, false), Hurt::Refused);
    }

    #[test]
    fn a_quarter_of_its_health_while_perched_makes_it_take_off() {
        let mut d = dragon(Phase::Flaming { ticks: 0 });
        d.hurt(0, 30.0, true, false);
        assert!(d.phase.sitting());
        d.invulnerable = 0;
        d.hurt(0, 21.0, true, false);
        assert!(matches!(d.phase, Phase::Takeoff { .. }));
        assert_eq!(d.sitting_damage, 0.0);
    }

    #[test]
    fn explosions_hit_each_part_by_its_exposure() {
        let mut d = dragon(flying());
        for (i, part) in d.parts.iter_mut().enumerate() {
            part.0 = Vec3::new(i as f64 * 4.0, 70.0, 0.0);
        }
        // A power-6 blast at the dragon's feet: diameter 12.
        let center = [0.0, 70.0, 0.0];
        let open = d.explosion_hits(center, 6.0, |_| 1.0);
        // The dragon itself (its body), then the parts within 12 blocks.
        let parts: Vec<usize> = open.iter().map(|h| h.0).collect();
        assert_eq!(parts, vec![2, 0, 1, 2, 3]);
        // At the centre: (1 + 1) / 2 * 7 * 12 + 1.
        assert_eq!(open[0].1, 85.0);
        // Behind cover (no ray reaches the box) only the 1 is left.
        let covered = d.explosion_hits(center, 6.0, |_| 0.0);
        assert!(covered.iter().all(|h| h.1 == 1.0));
        // Through the cooldown the strongest hit after the part's cut wins.
        for (part, amount) in open {
            d.hurt(part, amount, true, false);
        }
        assert_eq!(d.health, MAX_HEALTH - 85.0);
    }


}
