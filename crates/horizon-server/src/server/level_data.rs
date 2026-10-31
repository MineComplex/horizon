//! What each level keeps for itself, as Leaf 26.3 has it (derived from the sources: `ServerLevel`,
//! `PaperLevelOverrides`, `WorldBorder`, `ServerClockManager`; none of the Paper classes is in the
//! vanilla jar, so there is no jar oracle for the overrides).
//!
//! Every `ServerLevel` has its own `SavedDataStorage` under `dimensions/<ns>/<path>/data/` and
//! reads from it:
//! - `paper:level_overrides` ([`LevelOverrides`]): the level's spawn, **game time**, `initialized`,
//!   game type and **difficulty settings** (difficulty, hardcore, locked). Paper made these per
//!   level: `ServerLevel.serverLevelData` is this object and `getGameTime`/`getDifficulty`/
//!   `isHardcore` read it. Without the file the level starts from a copy of `level.dat`
//!   (`createFromLiveLevelData`, `initialized` false). The overworld's changes are also written to
//!   the root `level.dat` (`syncRootData`); the other levels' are not.
//! - `minecraft:world_border` ([`WorldBorder`]): the level's own border, applied from its settings
//!   the first time it is asked for (`applyInitialSettings`), ticked while the game runs normally.
//! - `minecraft:world_clocks` ([`LevelClocks`]): the level's **day time** (Paper's per-world time:
//!   `time.affects-all-worlds` is false by default and the true case, one server-wide clock
//!   manager, is not ported).
//! - `minecraft:weather` and `minecraft:game_rules` (the latter not ported yet).
//!
//! A saved data object is written only when it is dirty (`SavedData.setDirty`).

use super::level::Level;
use crate::command::wrap_degrees;
use crate::core::position::BlockPos;
use crate::nbt::{Compound, Tag};
use crate::network::play::clientbound::{Border, ClockUpdate};
use crate::world::level_storage::{self as storage, Difficulty, GameType, LevelData};

fn double(tag: Option<&Tag>) -> Option<f64> {
    match *tag? {
        Tag::Double(v) => Some(f64::from_bits(v)),
        Tag::Float(v) => Some(f64::from(f32::from_bits(v))),
        _ => storage::number(tag).map(|v| v as f64),
    }
}

/// `Codec.floatRange`/`doubleRange` fields.
fn float_in(map: &Compound, key: &str, range: std::ops::RangeInclusive<f32>) -> Option<f32> {
    storage::float(storage::get(map, key)).filter(|v| range.contains(v))
}

/// A required field (`fieldOf`) that is a number.
fn required_number(map: &Compound, key: &str) -> Option<i64> {
    storage::number(storage::get(map, key))
}

fn block_pos(tag: Option<&Tag>) -> Option<BlockPos> {
    match tag? {
        Tag::IntArray(v) => match v.as_slice() {
            &[x, y, z] => Some(BlockPos::new(x, y, z)),
            _ => None,
        },
        _ => None,
    }
}

/// `PaperLevelOverrides.RespawnData`: a position in the level and a facing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Respawn {
    pub pos: BlockPos,
    pub yaw: f32,
    pub pitch: f32,
}

impl Respawn {
    /// `normalized`: yaw wrapped, pitch clamped.
    pub fn normalized(self) -> Self {
        Self {
            pos: self.pos,
            yaw: wrap_degrees(self.yaw),
            pitch: self.pitch.clamp(-90.0, 90.0),
        }
    }
}

/// `paper:level_overrides` (see the module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct LevelOverrides {
    pub respawn: Respawn,
    pub game_time: i64,
    pub initialized: bool,
    pub game_type: GameType,
    pub difficulty: Difficulty,
    pub hardcore: bool,
    pub difficulty_locked: bool,
    dirty: bool,
}

impl LevelOverrides {
    /// `createFromLiveLevelData`: a copy of `level.dat`'s values, `initialized` false.
    pub fn from_root(root: &LevelData) -> Self {
        Self {
            respawn: Respawn {
                pos: root.spawn.pos,
                yaw: root.spawn.yaw,
                pitch: root.spawn.pitch,
            }
            .normalized(),
            game_time: root.game_time,
            initialized: false,
            game_type: root.game_type,
            difficulty: root.difficulty,
            hardcore: root.hardcore,
            difficulty_locked: root.difficulty_locked,
            dirty: false,
        }
    }

    /// `CODEC`: every field is required, so a file missing one (or with a bad value) does not
    /// decode ("Failed to parse saved data") and the level starts from [`Self::from_root`].
    pub fn parse(tag: &Compound) -> Option<Self> {
        let spawn = match storage::get(tag, "spawn")? {
            Tag::Compound(s) => s,
            _ => return None,
        };
        let respawn = Respawn {
            pos: block_pos(storage::get(spawn, "pos"))?,
            yaw: float_in(spawn, "yaw", -180.0..=180.0)?,
            pitch: float_in(spawn, "pitch", -90.0..=90.0)?,
        }
        .normalized();
        let difficulty = match storage::get(tag, "difficulty_settings")? {
            Tag::Compound(d) => (
                storage::string(d, "difficulty").and_then(|n| Difficulty::by_name(&n))?,
                required_number(d, "hardcore")? != 0,
                required_number(d, "locked")? != 0,
            ),
            _ => return None,
        };
        Some(Self {
            respawn,
            game_time: required_number(tag, "game_time")?,
            initialized: required_number(tag, "initialized")? != 0,
            game_type: GameType::by_id(required_number(tag, "game_type")? as i32),
            difficulty: difficulty.0,
            hardcore: difficulty.1,
            difficulty_locked: difficulty.2,
            dirty: false,
        })
    }

    pub fn to_tag(&self) -> Compound {
        let r = &self.respawn;
        let Tag::Compound(tag) = Tag::compound([
            (
                "spawn",
                Tag::compound([
                    ("pos", Tag::IntArray(vec![r.pos.x, r.pos.y, r.pos.z])),
                    ("yaw", Tag::Float(r.yaw.to_bits())),
                    ("pitch", Tag::Float(r.pitch.to_bits())),
                ]),
            ),
            ("game_time", Tag::Long(self.game_time)),
            ("initialized", Tag::Byte(self.initialized.into())),
            ("game_type", Tag::Int(self.game_type as i32)),
            (
                "difficulty_settings",
                Tag::compound([
                    ("difficulty", Tag::String(self.difficulty.name().into())),
                    ("hardcore", Tag::Byte(self.hardcore.into())),
                    ("locked", Tag::Byte(self.difficulty_locked.into())),
                ]),
            ),
        ]) else {
            unreachable!()
        };
        tag
    }

    /// `setGameTime`: dirty only when it changes.
    pub fn set_game_time(&mut self, time: i64) {
        if self.game_time != time {
            self.game_time = time;
            self.dirty = true;
        }
    }

    /// `setDifficulty`.
    pub fn set_difficulty(&mut self, difficulty: Difficulty) {
        if self.difficulty != difficulty {
            self.difficulty = difficulty;
            self.dirty = true;
        }
    }

    /// `setSpawn` (`setRespawn`): normalized, dirty only when it changes.
    pub fn set_respawn(&mut self, respawn: Respawn) {
        let respawn = respawn.normalized();
        if self.respawn != respawn {
            self.respawn = respawn;
            self.dirty = true;
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

/// `WorldBorder.Settings`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSettings {
    pub center_x: f64,
    pub center_z: f64,
    pub damage_per_block: f64,
    pub safe_zone: f64,
    pub warning_blocks: i32,
    pub warning_time: i32,
    pub size: f64,
    pub lerp_time: i64,
    pub lerp_target: f64,
}

/// `5.999997E7F`, the border's default size.
const MAX_SIZE: f64 = 5.999997E7_f32 as f64;
/// `WorldBorder.absoluteMaxSize`'s default.
const ABSOLUTE_MAX_SIZE: i32 = 29_999_984;

impl BorderSettings {
    /// `Settings.DEFAULT`.
    pub const DEFAULT: Self = Self {
        center_x: 0.0,
        center_z: 0.0,
        damage_per_block: 0.2,
        safe_zone: 5.0,
        warning_blocks: 5,
        warning_time: 300,
        size: MAX_SIZE,
        lerp_time: 0,
        lerp_target: 0.0,
    };

    /// `CODEC`: all nine fields are required; the centre is within +-2.9999984E7.
    pub fn parse(tag: &Compound) -> Option<Self> {
        let d = |key| double(storage::get(tag, key));
        let center = |key| d(key).filter(|v| (-2.999_998_4E7..=2.999_998_4E7).contains(v));
        Some(Self {
            center_x: center("center_x")?,
            center_z: center("center_z")?,
            damage_per_block: d("damage_per_block")?,
            safe_zone: d("safe_zone")?,
            warning_blocks: required_number(tag, "warning_blocks")? as i32,
            warning_time: required_number(tag, "warning_time")? as i32,
            size: d("size")?,
            lerp_time: required_number(tag, "lerp_time")?,
            lerp_target: d("lerp_target")?,
        })
    }

    pub fn to_tag(&self) -> Compound {
        let double = |v: f64| Tag::Double(v.to_bits());
        let Tag::Compound(tag) = Tag::compound([
            ("center_x", double(self.center_x)),
            ("center_z", double(self.center_z)),
            ("damage_per_block", double(self.damage_per_block)),
            ("safe_zone", double(self.safe_zone)),
            ("warning_blocks", Tag::Int(self.warning_blocks)),
            ("warning_time", Tag::Int(self.warning_time)),
            ("size", double(self.size)),
            ("lerp_time", Tag::Long(self.lerp_time)),
            ("lerp_target", double(self.lerp_target)),
        ]) else {
            unreachable!()
        };
        tag
    }
}

/// `WorldBorder.BorderExtent`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Extent {
    Static {
        size: f64,
    },
    /// `MovingBorderExtent`: `progress` counts down from `duration` ticks.
    Moving {
        from: f64,
        to: f64,
        duration: f64,
        progress: i64,
        size: f64,
        previous_size: f64,
    },
}

impl Extent {
    fn moving(from: f64, to: f64, duration: i64) -> Self {
        let mut e = Self::Moving {
            from,
            to,
            duration: duration as f64,
            progress: duration,
            size: 0.0,
            previous_size: 0.0,
        };
        if let Self::Moving {
            size,
            previous_size,
            ..
        } = &mut e
        {
            *size = from; // calculateSize() at progress 0
            *previous_size = from;
        }
        e
    }

    fn size(&self) -> f64 {
        match *self {
            Self::Static { size } | Self::Moving { size, .. } => size,
        }
    }

    fn lerp_time(&self) -> i64 {
        match *self {
            Self::Static { .. } => 0,
            Self::Moving { progress, .. } => progress,
        }
    }

    fn lerp_target(&self) -> f64 {
        match *self {
            Self::Static { size } => size,
            Self::Moving { to, .. } => to,
        }
    }

    /// `getMinX(0)`-style bounds use the size at partial tick 0: the previous size of a moving
    /// border.
    fn bounds_size(&self) -> f64 {
        match *self {
            Self::Static { size } => size,
            Self::Moving { previous_size, .. } => previous_size,
        }
    }

    /// `update`: one tick of a moving border; true when it just finished (it becomes static).
    fn update(&mut self) -> bool {
        let Self::Moving {
            from,
            to,
            duration,
            progress,
            size,
            previous_size,
        } = self
        else {
            return false;
        };
        *progress -= 1;
        *previous_size = *size;
        let p = (*duration - *progress as f64) / *duration;
        *size = if p < 1.0 {
            *from + p * (*to - *from)
        } else {
            *to
        };
        if *progress <= 0 {
            *self = Self::Static { size: *to };
            return true;
        }
        false
    }
}

/// `WorldBorder` as a level keeps it (see the module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct WorldBorder {
    settings: BorderSettings,
    initialized: bool,
    damage_per_block: f64,
    safe_zone: f64,
    warning_time: i32,
    warning_blocks: i32,
    center_x: f64,
    center_z: f64,
    absolute_max_size: i32,
    extent: Extent,
    dirty: bool,
}

impl WorldBorder {
    /// `new WorldBorder(settings)`: the saved settings, not yet applied.
    pub fn new(settings: BorderSettings) -> Self {
        Self {
            settings,
            initialized: false,
            damage_per_block: 0.2,
            safe_zone: 5.0,
            warning_time: 15,
            warning_blocks: 5,
            center_x: 0.0,
            center_z: 0.0,
            absolute_max_size: ABSOLUTE_MAX_SIZE,
            extent: Extent::Static { size: MAX_SIZE },
            dirty: false,
        }
    }

    /// `ServerLevel.getWorldBorder`'s first call: `applyInitialSettings(gameTime)` (the moving
    /// extent counts its own ticks, so the game time is not needed). Every setter
    /// marks the border dirty, so a new border is written at the first save.
    pub fn apply_initial_settings(&mut self) {
        if self.initialized {
            return;
        }
        let s = self.settings;
        self.center_x = s.center_x;
        self.center_z = s.center_z;
        self.damage_per_block = s.damage_per_block;
        self.safe_zone = s.safe_zone;
        self.warning_blocks = s.warning_blocks;
        self.warning_time = s.warning_time;
        self.extent = if s.lerp_time > 0 && s.size != s.lerp_target {
            Extent::moving(s.size, s.lerp_target, s.lerp_time)
        } else if s.lerp_time > 0 {
            Extent::Static {
                size: s.lerp_target,
            }
        } else {
            Extent::Static { size: s.size }
        };
        self.dirty = true;
        self.initialized = true;
    }

    /// `tick`: one step of a moving border (dirty while it moves).
    pub fn tick(&mut self) {
        if matches!(self.extent, Extent::Moving { .. }) {
            self.extent.update();
            self.dirty = true;
        }
    }

    fn bound(&self, center: f64, sign: f64) -> f64 {
        let max = f64::from(self.absolute_max_size);
        (center + sign * self.extent.bounds_size() / 2.0).clamp(-max, max)
    }

    /// `isWithinBounds(x, z)`.
    pub fn is_within_bounds(&self, x: f64, z: f64) -> bool {
        x >= self.bound(self.center_x, -1.0)
            && x < self.bound(self.center_x, 1.0)
            && z >= self.bound(self.center_z, -1.0)
            && z < self.bound(self.center_z, 1.0)
    }

    pub fn size(&self) -> f64 {
        self.extent.size()
    }

    /// `Settings(WorldBorder)`: what is saved.
    pub fn to_settings(&self) -> BorderSettings {
        BorderSettings {
            center_x: self.center_x,
            center_z: self.center_z,
            damage_per_block: self.damage_per_block,
            safe_zone: self.safe_zone,
            warning_blocks: self.warning_blocks,
            warning_time: self.warning_time,
            size: self.extent.size(),
            lerp_time: self.extent.lerp_time(),
            lerp_target: self.extent.lerp_target(),
        }
    }

    /// `ClientboundInitializeBorderPacket(border)`: the centre is scaled by the level's
    /// dimension type (`border.world.dimensionType().coordinateScale()`).
    pub fn packet(&self, coordinate_scale: f64) -> Border {
        Border {
            center_x: self.center_x * coordinate_scale,
            center_z: self.center_z * coordinate_scale,
            old_size: self.extent.size(),
            new_size: self.extent.lerp_target(),
            lerp_time: self.extent.lerp_time(),
            absolute_max_size: self.absolute_max_size,
            warning_blocks: self.warning_blocks,
            warning_time: self.warning_time,
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

/// `ClockState`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockState {
    pub total_ticks: i64,
    pub partial_tick: f32,
    pub rate: f32,
    pub paused: bool,
}

impl ClockState {
    const NEW: Self = Self {
        total_ticks: 0,
        partial_tick: 0.0,
        rate: 1.0,
        paused: false,
    };

    /// `CODEC`: `total_ticks` is required, the others default; `rate` must be positive.
    fn parse(tag: &Tag) -> Option<Self> {
        let Tag::Compound(c) = tag else { return None };
        let rate = match storage::get(c, "rate") {
            None => 1.0,
            Some(t) => storage::float(Some(t)).filter(|r| *r > 0.0)?,
        };
        Some(Self {
            total_ticks: required_number(c, "total_ticks")?,
            partial_tick: storage::float(storage::get(c, "partial_tick")).unwrap_or(0.0),
            rate,
            paused: storage::boolean(c, "paused", false),
        })
    }

    /// `optionalFieldOf` with a default is left out when equal to it.
    fn to_tag(self) -> Tag {
        let mut c = Compound::new();
        storage::put(&mut c, "total_ticks", Tag::Long(self.total_ticks));
        if self.partial_tick != 0.0 {
            storage::put(
                &mut c,
                "partial_tick",
                Tag::Float(self.partial_tick.to_bits()),
            );
        }
        if self.rate != 1.0 {
            storage::put(&mut c, "rate", Tag::Float(self.rate.to_bits()));
        }
        if self.paused {
            storage::put(&mut c, "paused", Tag::Byte(1));
        }
        Tag::Compound(c)
    }
}

/// `ServerClockManager` of one level: a clock per entry of `minecraft:world_clock`, in registry
/// order (the order of `listElements`). The time markers (sleeping, `/time set <marker>`) come
/// with the commands.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelClocks {
    /// (clock id, state), indexed by the clock's protocol id.
    clocks: Vec<(String, ClockState)>,
    dirty: bool,
}

impl LevelClocks {
    /// `init`: every registered clock, loaded from the saved map when it decodes (an unknown
    /// clock or a bad state fails the whole map, which then reads as empty).
    pub fn load(registered: &[String], saved: Option<&Compound>) -> Self {
        let mut clocks: Vec<_> = registered
            .iter()
            .map(|id| (id.clone(), ClockState::NEW))
            .collect();
        let decoded: Option<Vec<(usize, ClockState)>> = saved.and_then(|map| {
            map.iter()
                .map(|(key, tag)| {
                    let id = key.to_string_lossy();
                    let index = clocks.iter().position(|(c, _)| *c == id)?;
                    Some((index, ClockState::parse(tag)?))
                })
                .collect()
        });
        if saved.is_some() && decoded.is_none() {
            tracing::error!("Failed to parse saved data for 'minecraft:world_clocks'");
        }
        for (index, state) in decoded.unwrap_or_default() {
            clocks[index].1 = state;
        }
        Self {
            clocks,
            dirty: false,
        }
    }

    /// `packState`/`CODEC`: the map of every clock.
    pub fn to_tag(&self) -> Compound {
        self.clocks
            .iter()
            .map(|(id, state)| (id.as_str().into(), state.to_tag()))
            .collect()
    }

    pub fn state(&self, clock: &str) -> Option<ClockState> {
        self.clocks.iter().find(|(c, _)| c == clock).map(|c| c.1)
    }

    /// `ServerClockManager.tick`: every clock advances by its rate unless paused, when the level's
    /// `advance_time` game rule is on. (Purpur's day and night lengths scale the rate by
    /// 12000/ticks; with their defaults that is 1, the only case ported.)
    pub fn tick(&mut self, advance_time: bool) {
        if !advance_time {
            return;
        }
        for (_, clock) in &mut self.clocks {
            if !clock.paused {
                clock.partial_tick += clock.rate;
                let full = clock.partial_tick.floor();
                clock.partial_tick -= full;
                clock.total_ticks += full as i64;
            }
        }
        self.dirty = true;
    }

    /// `createFullSyncPacket`'s clock map (`packNetworkState`: a paused clock, or any when time
    /// does not advance, has rate 0). The order is the registry's: Java's map is a `HashMap` of
    /// holders, whose order the client does not depend on.
    pub fn network_states(&self, advance_time: bool) -> Vec<ClockUpdate> {
        self.clocks
            .iter()
            .enumerate()
            .map(|(id, (_, c))| ClockUpdate {
                clock: id as i32,
                total_ticks: c.total_ticks,
                partial_tick: c.partial_tick,
                rate: if c.paused || !advance_time {
                    0.0
                } else {
                    c.rate
                },
            })
            .collect()
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

impl Level {
    /// `ServerLevel.tick`'s time part, while the game runs normally: the border, then
    /// `tickTime` (the game time + 1).
    pub fn tick_time(&mut self) {
        self.border.tick();
        self.overrides.set_game_time(self.overrides.game_time + 1);
    }

    /// `Level.getDefaultClockTime`: the total ticks of the dimension type's default clock.
    pub fn default_clock_time(&self) -> i64 {
        self.default_clock
            .as_deref()
            .and_then(|c| self.clocks.state(c))
            .map_or(0, |s| s.total_ticks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> LevelData {
        let mut root = LevelData::parse(&Compound::new());
        root.game_time = 77;
        root.difficulty = Difficulty::Hard;
        root.hardcore = true;
        root.spawn.pos = BlockPos::new(3, 64, -9);
        root.spawn.yaw = 90.0;
        root
    }

    #[test]
    fn overrides_start_from_the_root_and_round_trip() {
        let o = LevelOverrides::from_root(&root());
        assert!(!o.initialized);
        assert_eq!(o.game_time, 77);
        assert_eq!(o.difficulty, Difficulty::Hard);
        assert!(o.hardcore);
        assert_eq!(o.respawn.pos, BlockPos::new(3, 64, -9));
        assert_eq!(LevelOverrides::parse(&o.to_tag()), Some(o));
    }

    #[test]
    fn overrides_need_every_field() {
        let tag = LevelOverrides::from_root(&root()).to_tag();
        for key in [
            "spawn",
            "game_time",
            "initialized",
            "game_type",
            "difficulty_settings",
        ] {
            let mut cut = tag.clone();
            cut.remove(&key.into());
            assert_eq!(LevelOverrides::parse(&cut), None, "{key}");
        }
        // A yaw outside the codec's range fails the decode too.
        let mut bad = tag;
        let Some(Tag::Compound(spawn)) = bad.get_mut(&"spawn".into()) else {
            panic!()
        };
        spawn.insert("yaw".into(), Tag::Float(270f32.to_bits()));
        assert_eq!(LevelOverrides::parse(&bad), None);
    }

    #[test]
    fn game_time_is_dirty_only_when_it_changes() {
        let mut o = LevelOverrides::from_root(&root());
        o.set_game_time(77);
        assert!(!o.take_dirty());
        o.set_game_time(78);
        assert!(o.take_dirty());
        assert!(!o.take_dirty());
    }

    #[test]
    fn a_new_border_is_the_default_one_with_its_settings_applied() {
        let mut b = WorldBorder::new(BorderSettings::DEFAULT);
        b.apply_initial_settings();
        let p = b.packet(1.0);
        assert_eq!(p.old_size, 5.999_996_8E7);
        assert_eq!(p.new_size, 5.999_996_8E7);
        // `Settings.DEFAULT`'s warning time (300), not `new WorldBorder()`'s 15.
        assert_eq!((p.warning_blocks, p.warning_time), (5, 300));
        assert_eq!(p.absolute_max_size, 29_999_984);
        assert!(b.take_dirty(), "applying the settings dirties the border");
        assert!(b.is_within_bounds(0.0, 29_999_983.0));
        assert!(!b.is_within_bounds(30_000_000.0, 0.0));
    }

    #[test]
    fn the_nether_border_centre_is_scaled() {
        let mut s = BorderSettings::DEFAULT;
        s.center_x = 100.0;
        s.center_z = -50.0;
        let mut b = WorldBorder::new(s);
        b.apply_initial_settings();
        let p = b.packet(8.0);
        assert_eq!((p.center_x, p.center_z), (800.0, -400.0));
    }

    #[test]
    fn a_shrinking_border_moves_each_tick_and_is_saved_mid_way() {
        let mut s = BorderSettings::DEFAULT;
        s.size = 100.0;
        s.lerp_time = 4;
        s.lerp_target = 60.0;
        let mut b = WorldBorder::new(s);
        b.apply_initial_settings();
        assert_eq!(b.packet(1.0).lerp_time, 4);
        b.tick();
        b.tick();
        assert_eq!(b.size(), 80.0);
        let saved = b.to_settings();
        assert_eq!(
            (saved.size, saved.lerp_time, saved.lerp_target),
            (80.0, 2, 60.0)
        );
        b.tick();
        b.tick();
        assert_eq!(b.size(), 60.0);
        assert_eq!(b.to_settings().lerp_time, 0);
        // The saved settings read back to the same border.
        let mut again = WorldBorder::new(BorderSettings::parse(&saved.to_tag()).unwrap());
        again.apply_initial_settings();
        again.tick();
        again.tick();
        assert_eq!(again.size(), 60.0);
    }

    #[test]
    fn border_settings_need_all_fields_and_a_centre_in_range() {
        let tag = BorderSettings::DEFAULT.to_tag();
        assert_eq!(BorderSettings::parse(&tag), Some(BorderSettings::DEFAULT));
        let mut cut = tag.clone();
        cut.remove(&"safe_zone".into());
        assert_eq!(BorderSettings::parse(&cut), None);
        let mut far = tag;
        far.insert("center_x".into(), Tag::Double(3.0e7f64.to_bits()));
        assert_eq!(BorderSettings::parse(&far), None);
    }

    fn registered() -> Vec<String> {
        vec!["minecraft:overworld".into(), "minecraft:the_end".into()]
    }

    #[test]
    fn clocks_tick_save_and_sync() {
        let mut saved = Compound::new();
        saved.insert(
            "minecraft:the_end".into(),
            Tag::compound([
                ("total_ticks", Tag::Long(6000)),
                ("rate", Tag::Float(0.5f32.to_bits())),
            ]),
        );
        let mut clocks = LevelClocks::load(&registered(), Some(&saved));
        assert_eq!(clocks.state("minecraft:overworld").unwrap().total_ticks, 0);
        clocks.tick(true);
        clocks.tick(true);
        let end = clocks.state("minecraft:the_end").unwrap();
        assert_eq!((end.total_ticks, end.partial_tick), (6001, 0.0));
        assert_eq!(clocks.state("minecraft:overworld").unwrap().total_ticks, 2);
        assert!(clocks.take_dirty());
        let sync = clocks.network_states(true);
        assert_eq!(sync[1].rate, 0.5);
        assert_eq!(clocks.network_states(false)[0].rate, 0.0);
        // Saved with defaults left out, and read back.
        let tag = clocks.to_tag();
        let Some(Tag::Compound(overworld)) = tag.get(&"minecraft:overworld".into()) else {
            panic!()
        };
        assert_eq!(overworld.len(), 1, "only total_ticks");
        assert_eq!(LevelClocks::load(&registered(), Some(&tag)), clocks);
    }

    #[test]
    fn clocks_that_do_not_advance_stay_and_a_bad_map_reads_as_empty() {
        let mut clocks = LevelClocks::load(&registered(), None);
        clocks.tick(false);
        assert_eq!(clocks.state("minecraft:overworld").unwrap().total_ticks, 0);
        assert!(!clocks.take_dirty());
        let mut unknown = Compound::new();
        unknown.insert(
            "minecraft:moon".into(),
            Tag::compound([("total_ticks", Tag::Long(5))]),
        );
        assert_eq!(LevelClocks::load(&registered(), Some(&unknown)), clocks);
    }
}

#[cfg(test)]
mod oracle_tests;
