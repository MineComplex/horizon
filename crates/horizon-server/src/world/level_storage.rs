//! A world directory as Leaf opens it: vanilla's `LevelStorageSource.LevelStorageAccess` with
//! `DirectoryLock` (`session.lock`), `level.dat`/`level.dat_old`, and the 26.3 layout with Paper's
//! dimension-scoped saved data:
//!
//! ```text
//! <level>/session.lock, level.dat, level.dat_old, data/<ns>/<id>.dat (server-wide saved data)
//! <level>/dimensions/<ns>/<path>/{region,entities,poi}/r.X.Z.mca
//! <level>/dimensions/<ns>/<path>/data/minecraft/{world_gen_settings,game_rules,weather,world_clocks}.dat
//! ```
//!
//! Vanilla 26.3 keeps world generation settings, game rules, weather and clocks in the root `data/`;
//! Paper reads them per dimension (`LevelStorageSource.readExistingSavedData` with a dimension,
//! `ServerLevel`'s own `SavedDataStorage`) after `WorldFolderMigration` copied a vanilla world's
//! root files there. That migration (with its 30 s warning) is not ported: [`LevelStorageAccess::read_saved_data`]
//! reads the dimension's file and falls back to the root one, which reads both layouts.
//! Not ported either: the symlink validation of `validateAndCreateAccess` (`allowed_symlinks.txt`),
//! level summaries, backups and data fixing (26.3 worlds only, ARCHITECTURE.md "Scope").
//!
//! NBT compounds are written in key order (a `BTreeMap`), Java writes them in `HashMap` order: the
//! files hold the same tags, not the same bytes.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use thiserror::Error;
use tracing::{info, warn};

use super::chunk::serial::DATA_VERSION;
use crate::core::position::BlockPos;
use crate::nbt::{self, Compound, JavaString, Limits, NamedTag, Tag};
use crate::world::storage::compression::Compression;

/// `LevelStorageSource.getStorageVersion()`: the `version` field (19132 is McRegion).
pub const STORAGE_VERSION: i32 = 19133;
const MCREGION_VERSION: i32 = 19132;
/// `SharedConstants.getCurrentVersion()`: name, series, `stable`.
pub const VERSION_NAME: &str = "26.3";
pub const VERSION_SERIES: &str = "main";

#[derive(Debug, Error)]
pub enum LevelStorageError {
    /// `DirectoryLock.LockException.alreadyLocked`.
    #[error("{}: already locked (possibly by other Minecraft instance?)", .0.display())]
    AlreadyLocked(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
    /// `makeLevelSummary`: neither Anvil nor McRegion.
    #[error("Unknown data version: {0:x}")]
    UnknownStorageVersion(i32),
    /// Leaf's `Main` logs these and stops.
    #[error("This world must be opened in an older version (like 1.6.4) to be safely converted")]
    RequiresManualConversion,
    #[error("This world was created by an incompatible version.")]
    Incompatible,
}

/// The canonical paths of the directories this process has locked. POSIX record locks belong to the
/// process, so a second lock from the same process would succeed; the JVM refuses it
/// (`OverlappingFileLockException`), and this set does the same.
static LOCKED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// `DirectoryLock`: `session.lock` holding a snowman, exclusively locked while the world is open.
/// On Unix this is `fcntl(F_SETLK)` over the whole file, the lock Java's `FileChannel.tryLock` takes,
/// so a Java server and this one exclude each other; elsewhere the platform lock (`File::try_lock`).
#[derive(Debug)]
pub struct DirectoryLock {
    /// Held open: closing it releases the lock.
    _file: File,
    canonical: PathBuf,
}

impl DirectoryLock {
    pub fn create(dir: &Path) -> Result<Self, LevelStorageError> {
        let path = dir.join("session.lock");
        fs::create_dir_all(dir)?;
        // CREATE + WRITE without truncation: the snowman goes over the first bytes.
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        file.write_all("☃".as_bytes())?;
        file.sync_all()?;
        let canonical = fs::canonicalize(&path)?;
        let mut locked = LOCKED.lock().unwrap_or_else(PoisonError::into_inner);
        if locked.contains(&canonical) || !try_lock(&file)? {
            return Err(LevelStorageError::AlreadyLocked(std::path::absolute(
                &path,
            )?));
        }
        locked.push(canonical.clone());
        Ok(Self {
            _file: file,
            canonical,
        })
    }

    /// `DirectoryLock.isLocked`. Closing the probe would drop this process's own POSIX lock on the
    /// file, so a directory this process holds answers from the set without opening it.
    pub fn is_locked(dir: &Path) -> io::Result<bool> {
        let path = dir.join("session.lock");
        let canonical = match fs::canonicalize(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            other => other?,
        };
        if LOCKED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&canonical)
        {
            return Ok(true);
        }
        match OpenOptions::new().write(true).open(&path) {
            Ok(file) => Ok(!try_lock(&file)?),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        // Closing the file (after this) releases the lock.
        let mut locked = LOCKED.lock().unwrap_or_else(PoisonError::into_inner);
        locked.retain(|p| *p != self.canonical);
    }
}

#[cfg(unix)]
fn try_lock(file: &File) -> io::Result<bool> {
    use rustix::fs::{FlockOperation, fcntl_lock};
    match fcntl_lock(file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(true),
        Err(e) if e == rustix::io::Errno::AGAIN || e == rustix::io::Errno::ACCESS => Ok(false),
        Err(e) => Err(e.into()),
    }
}
#[cfg(not(unix))]
fn try_lock(file: &File) -> io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(fs::TryLockError::WouldBlock) => Ok(false),
        Err(fs::TryLockError::Error(e)) => Err(e),
    }
}

/// `Identifier.resolveAgainst`: `<base>/<namespace>/<path>` (`minecraft:` when unqualified).
fn resolve_identifier(base: &Path, id: &str) -> PathBuf {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    base.join(namespace).join(path)
}

/// `LevelStorageSource.LevelStorageAccess`: an open, locked world directory.
#[derive(Debug)]
pub struct LevelStorageAccess {
    level_id: String,
    dir: PathBuf,
    _lock: DirectoryLock,
}

impl LevelStorageAccess {
    /// `createAccess(levelId)` under the server's directory (the "universe"): the directory is
    /// created and locked.
    pub fn open(base_dir: &Path, level_id: &str) -> Result<Self, LevelStorageError> {
        let dir = base_dir.join(level_id);
        let lock = DirectoryLock::create(&dir)?;
        Ok(Self {
            level_id: level_id.to_owned(),
            dir,
            _lock: lock,
        })
    }
    pub fn level_id(&self) -> &str {
        &self.level_id
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    /// `DimensionType.getStorageFolder`: `dimensions/<namespace>/<path>`.
    pub fn dimension_path(&self, dimension: &str) -> PathBuf {
        resolve_identifier(&self.dir.join("dimensions"), dimension)
    }
    /// The region folder of a dimension's chunks (`region`), entities or POI.
    pub fn region_path(&self, dimension: &str, kind: &str) -> PathBuf {
        self.dimension_path(dimension).join(kind)
    }
    fn data_file(&self) -> PathBuf {
        self.dir.join("level.dat")
    }
    fn old_data_file(&self) -> PathBuf {
        self.dir.join("level.dat_old")
    }
    pub fn has_world_data(&self) -> bool {
        self.data_file().exists() || self.old_data_file().exists()
    }

    /// `getUnfixedDataTagWithFallback`: `level.dat`'s `Data`, else `level.dat_old`'s, which then
    /// replaces `level.dat` (kept as `level.dat_corrupted_<date>`).
    pub fn read_level_data(&self) -> Result<LevelData, LevelStorageError> {
        let tag = match read_root(&self.data_file()) {
            Ok(root) => root,
            Err(e) => {
                warn!(
                    "Failed to load world data from {}: {e}",
                    self.data_file().display()
                );
                info!(
                    "Attempting to use fallback {}",
                    self.old_data_file().display()
                );
                let root = read_root(&self.old_data_file())?;
                self.restore_level_data_from_old();
                root
            }
        };
        let data = match tag.get(&JavaString::from("Data")) {
            Some(Tag::Compound(data)) => data.clone(),
            _ => Compound::new(),
        };
        Ok(LevelData::parse(&data))
    }

    /// `restoreLevelDataFromOld` (`Util.safeReplaceOrMoveFile` without rollback).
    fn restore_level_data_from_old(&self) -> bool {
        let date = jiff::Zoned::now().strftime("%Y-%m-%d_%H-%M-%S").to_string();
        let corrupted = self.dir.join(format!("level.dat_corrupted_{date}"));
        safe_replace(&self.data_file(), &self.old_data_file(), &corrupted, true).is_ok()
    }

    /// `saveDataTag`/`saveLevelData`: written to a temporary `level*.dat`, then `level.dat` becomes
    /// `level.dat_old` and the new file `level.dat` (`Util.safeReplaceFile`). Errors are logged by
    /// vanilla ("Failed to save level {}"); here they are returned.
    pub fn save_level_data(&self, data: &LevelData) -> io::Result<()> {
        let mut root = Compound::new();
        root.insert("Data".into(), Tag::Compound(data.to_tag(now_millis())));
        let temp = tempfile_in(&self.dir, "level", ".dat")?;
        let written = write_gzip(&temp, root);
        let result = written
            .and_then(|()| safe_replace(&self.data_file(), &temp, &self.old_data_file(), false));
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    /// A saved data file's `data` compound (`readTagFromDisk`, gzip or plain NBT): the dimension's
    /// own (`dimensions/.../data/<id>.dat`, Paper) or else the world's (`data/<id>.dat`, vanilla).
    pub fn read_saved_data(&self, dimension: &str, id: &str) -> io::Result<Option<Compound>> {
        for dir in [
            self.dimension_path(dimension).join("data"),
            self.dir.join("data"),
        ] {
            let path = resolve_identifier(&dir, &format!("{id}.dat"));
            if path.exists() {
                let root = read_root(&path)?;
                return Ok(match root.get(&JavaString::from("data")) {
                    Some(Tag::Compound(data)) => Some(data.clone()),
                    _ => None,
                });
            }
        }
        Ok(None)
    }

    /// `SavedDataStorage.tryWrite` into the dimension's `data/`: `{data, DataVersion}`, gzip.
    pub fn write_saved_data(&self, dimension: &str, id: &str, data: Compound) -> io::Result<()> {
        let dir = self.dimension_path(dimension).join("data");
        let path = resolve_identifier(&dir, &format!("{id}.dat"));
        fs::create_dir_all(path.parent().unwrap_or(&dir))?;
        let mut root = Compound::new();
        root.insert("data".into(), Tag::Compound(data));
        root.insert("DataVersion".into(), Tag::Int(DATA_VERSION));
        write_gzip(&path, root)
    }

    /// The saved data a level starts from: world generation settings, game rules, weather, clocks.
    pub fn read_dimension_data(&self, dimension: &str) -> io::Result<DimensionData> {
        let world_gen = self
            .read_saved_data(dimension, "world_gen_settings")?
            .map(|t| WorldGenSettings::parse(&t));
        Ok(DimensionData {
            world_border: self.read_saved_data(dimension, "world_border")?,
            level_overrides: self.read_saved_data(dimension, "paper:level_overrides")?,
            world_gen,
            game_rules: self.read_saved_data(dimension, "game_rules")?,
            weather: self
                .read_saved_data(dimension, "weather")?
                .map(|t| WeatherData::parse(&t)),
            clocks: self.read_saved_data(dimension, "world_clocks")?,
        })
    }
}

fn now_millis() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// `Files.createTempFile(dir, prefix, suffix)`.
pub(crate) fn tempfile_in(dir: &Path, prefix: &str, suffix: &str) -> io::Result<PathBuf> {
    let mut n = u64::from(std::process::id()) << 20 ^ now_millis() as u64;
    loop {
        let path = dir.join(format!("{prefix}{n}{suffix}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n = n.wrapping_add(1),
            Err(e) => return Err(e),
        }
    }
}

/// `Util.safeReplaceOrMoveFile` (its retries dropped): back the target up, replace it, and put
/// the backup back when the replacement fails (unless `no_rollback`).
pub(crate) fn safe_replace(
    target: &Path,
    new: &Path,
    backup: &Path,
    no_rollback: bool,
) -> io::Result<()> {
    let remove = |p: &Path| match fs::remove_file(p) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    };
    if target.exists() {
        remove(backup)?;
        fs::rename(target, backup)?;
    }
    remove(target)?;
    let replaced = fs::rename(new, target);
    if replaced.is_err() && !no_rollback {
        let _ = fs::rename(backup, target);
    }
    replaced
}

/// `NbtIo.readCompressed`, or plain NBT when the file is not gzip (`SavedDataStorage.isGzip`).
pub(crate) fn read_root(path: &Path) -> io::Result<Compound> {
    let bytes = fs::read(path)?;
    let limits = Limits::templates();
    let raw = if bytes.starts_with(&[0x1f, 0x8b]) {
        Compression::Gzip.decode(&bytes, limits.max_bytes)?
    } else {
        bytes
    };
    let (root, _) = nbt::read_named(&raw, limits)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    match root.tag {
        Tag::Compound(c) => Ok(c),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Root tag must be a named compound tag",
        )),
    }
}

/// `NbtIo.writeCompressed` (truncate, write, sync).
pub(crate) fn write_gzip(path: &Path, root: Compound) -> io::Result<()> {
    let named = NamedTag {
        name: JavaString::default(),
        tag: Tag::Compound(root),
    };
    let bytes = nbt::write_named(&named, Limits::templates())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let mut file = File::create(path)?;
    file.write_all(&Compression::Gzip.encode(&bytes)?)?;
    file.sync_all()
}

/// `GameType`, by id (`byId`: unknown ids are survival).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameType {
    Survival,
    Creative,
    Adventure,
    Spectator,
}
impl GameType {
    pub fn by_id(id: i32) -> Self {
        [
            Self::Survival,
            Self::Creative,
            Self::Adventure,
            Self::Spectator,
        ]
        .get(id as usize)
        .copied()
        .unwrap_or(Self::Survival)
    }
}

/// `Difficulty` and its codec names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}
impl Difficulty {
    const NAMES: [&'static str; 4] = ["peaceful", "easy", "normal", "hard"];
    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
    pub fn by_name(name: &str) -> Option<Self> {
        let i = Self::NAMES.iter().position(|n| *n == name)?;
        Some([Self::Peaceful, Self::Easy, Self::Normal, Self::Hard][i])
    }
}

/// `LevelData.RespawnData`: a `GlobalPos` with a yaw and pitch.
#[derive(Clone, Debug, PartialEq)]
pub struct RespawnData {
    pub dimension: String,
    pub pos: BlockPos,
    pub yaw: f32,
    pub pitch: f32,
}
impl Default for RespawnData {
    fn default() -> Self {
        Self {
            dimension: "minecraft:overworld".into(),
            pos: BlockPos::new(0, 0, 0),
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// `LevelVersion`: what wrote the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelVersion {
    pub storage_version: i32,
    pub last_played: i64,
    pub name: String,
    pub data_version: i32,
    pub series: String,
    pub snapshot: bool,
}

/// `PrimaryLevelData` as `level.dat`'s `Data` holds it in 26.3 with Paper's additions. Weather, day
/// time (clocks), game rules and world generation settings are saved data ([`DimensionData`]).
#[derive(Clone, Debug, PartialEq)]
pub struct LevelData {
    /// `DataVersion` of the file (-1 when missing); written as the current one.
    pub data_version: i32,
    pub version: LevelVersion,
    pub level_name: String,
    pub game_type: GameType,
    pub difficulty: Difficulty,
    pub hardcore: bool,
    pub difficulty_locked: bool,
    pub allow_commands: bool,
    pub initialized: bool,
    pub spawn: RespawnData,
    /// Paper's `paperSpawnDimension` (the spawn's dimension when absent).
    pub paper_spawn_dimension: String,
    /// `Time`: the game time.
    pub game_time: i64,
    pub server_brands: Vec<String>,
    pub was_modded: bool,
    pub removed_features: Vec<String>,
    pub version_history: Vec<i32>,
    pub singleplayer_uuid: Option<[i32; 4]>,
    /// `DataPacks.Enabled`/`Disabled` (`DataPackConfig.DEFAULT`: vanilla, nothing).
    pub enabled_packs: Vec<String>,
    pub disabled_packs: Vec<String>,
    /// `enabled_features` (`FeatureFlags.DEFAULT_FLAGS`: vanilla).
    pub enabled_features: Vec<String>,
    /// CraftBukkit's `Bukkit.Version` (name/version/API version), written as given.
    pub bukkit_version: String,
}

pub(crate) fn get<'a>(map: &'a Compound, key: &str) -> Option<&'a Tag> {
    map.get(&JavaString::from(key))
}
pub(crate) fn put(map: &mut Compound, key: &str, value: Tag) {
    map.insert(key.into(), value);
}
pub(crate) fn number(tag: Option<&Tag>) -> Option<i64> {
    Some(match *tag? {
        Tag::Byte(v) => v.into(),
        Tag::Short(v) => v.into(),
        Tag::Int(v) => v.into(),
        Tag::Long(v) => v,
        Tag::Float(v) => f32::from_bits(v) as i64,
        Tag::Double(v) => f64::from_bits(v) as i64,
        _ => return None,
    })
}
pub(crate) fn int(map: &Compound, key: &str, default: i32) -> i32 {
    number(get(map, key)).map_or(default, |v| v as i32)
}
pub(crate) fn boolean(map: &Compound, key: &str, default: bool) -> bool {
    number(get(map, key)).map_or(default, |v| v as i32 != 0)
}
pub(crate) fn string(map: &Compound, key: &str) -> Option<String> {
    match get(map, key) {
        Some(Tag::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}
fn strings(tag: Option<&Tag>) -> Vec<String> {
    match tag {
        Some(Tag::List(list)) => list
            .iter()
            .filter_map(|t| match t {
                Tag::String(s) => Some(s.to_string_lossy()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}
fn string_list(values: &[String]) -> Tag {
    Tag::List(
        values
            .iter()
            .map(|s| Tag::String(s.as_str().into()))
            .collect(),
    )
}
pub(crate) fn float(tag: Option<&Tag>) -> Option<f32> {
    match *tag? {
        Tag::Float(v) => Some(f32::from_bits(v)),
        Tag::Double(v) => Some(f64::from_bits(v) as f32),
        _ => number(tag).map(|v| v as f32),
    }
}

impl LevelData {
    /// `LevelSettings.parse`, `PrimaryLevelData.parse`, `LevelVersion.parse` and
    /// `WorldDataConfiguration.MAP_CODEC` with their defaults.
    pub fn parse(tag: &Compound) -> Self {
        let game_type = GameType::by_id(int(tag, "GameType", 0));
        let version = match get(tag, "Version") {
            Some(Tag::Compound(v)) => LevelVersion {
                storage_version: int(tag, "version", 0),
                last_played: number(get(tag, "LastPlayed")).unwrap_or(0),
                name: string(v, "Name").unwrap_or_else(|| VERSION_NAME.into()),
                data_version: int(v, "Id", DATA_VERSION),
                series: string(v, "Series").unwrap_or_else(|| VERSION_SERIES.into()),
                snapshot: boolean(v, "Snapshot", false),
            },
            _ => LevelVersion {
                storage_version: int(tag, "version", 0),
                last_played: number(get(tag, "LastPlayed")).unwrap_or(0),
                name: String::new(),
                data_version: 0,
                series: VERSION_SERIES.into(),
                snapshot: false,
            },
        };
        // `difficulty_settings` reads whole or not at all (`DifficultySettings.DEFAULT`).
        let difficulty = match get(tag, "difficulty_settings") {
            Some(Tag::Compound(d)) => string(d, "difficulty")
                .and_then(|n| Difficulty::by_name(&n))
                .zip(number(get(d, "hardcore")).zip(number(get(d, "locked")))),
            _ => None,
        };
        let (difficulty, (hardcore, locked)) = difficulty.unwrap_or((Difficulty::Normal, (0, 0)));
        let spawn = match get(tag, "spawn") {
            Some(Tag::Compound(s)) => (|| {
                let dimension = string(s, "dimension")?;
                let Some(Tag::IntArray(pos)) = get(s, "pos") else {
                    return None;
                };
                let &[x, y, z] = pos.as_slice() else {
                    return None;
                };
                let yaw = float(get(s, "yaw")).filter(|v| (-180.0..=180.0).contains(v))?;
                let pitch = float(get(s, "pitch")).filter(|v| (-90.0..=90.0).contains(v))?;
                Some(RespawnData {
                    dimension,
                    pos: BlockPos::new(x, y, z),
                    yaw,
                    pitch,
                })
            })(),
            _ => None,
        }
        .unwrap_or_default();
        let (enabled_packs, disabled_packs) = match get(tag, "DataPacks") {
            Some(Tag::Compound(p)) => (strings(get(p, "Enabled")), strings(get(p, "Disabled"))),
            _ => (vec!["vanilla".into()], Vec::new()),
        };
        let enabled_features = match get(tag, "enabled_features") {
            Some(t @ Tag::List(_)) => strings(Some(t)),
            _ => vec!["minecraft:vanilla".into()],
        };
        let singleplayer_uuid = match get(tag, "singleplayer_uuid") {
            Some(Tag::IntArray(v)) => <[i32; 4]>::try_from(v.as_slice()).ok(),
            _ => None,
        };
        Self {
            data_version: int(tag, "DataVersion", -1),
            version,
            level_name: string(tag, "LevelName").unwrap_or_default(),
            game_type,
            difficulty,
            hardcore: hardcore != 0,
            difficulty_locked: locked != 0,
            allow_commands: boolean(tag, "allowCommands", game_type == GameType::Creative),
            initialized: boolean(tag, "initialized", true),
            paper_spawn_dimension: string(tag, "paperSpawnDimension")
                .unwrap_or_else(|| spawn.dimension.clone()),
            spawn,
            game_time: number(get(tag, "Time")).unwrap_or(0),
            server_brands: strings(get(tag, "ServerBrands")),
            was_modded: boolean(tag, "WasModded", false),
            removed_features: strings(get(tag, "removed_features")),
            version_history: match get(tag, "version_history") {
                Some(Tag::List(l)) => l
                    .iter()
                    .map(|t| number(Some(t)).map_or(-1, |v| v as i32))
                    .collect(),
                _ => Vec::new(),
            },
            singleplayer_uuid,
            enabled_packs,
            disabled_packs,
            enabled_features,
            bukkit_version: string(tag, "Bukkit.Version").unwrap_or_default(),
        }
    }

    /// `LevelSummary`'s checks as Leaf's `Main` runs them before loading: the storage version and
    /// the version series. An older `DataVersion` is read as it is (no data fixing).
    pub fn check_compatible(&self) -> Result<(), LevelStorageError> {
        match self.version.storage_version {
            STORAGE_VERSION => {}
            MCREGION_VERSION => return Err(LevelStorageError::RequiresManualConversion),
            other => return Err(LevelStorageError::UnknownStorageVersion(other)),
        }
        if self.version.series != VERSION_SERIES {
            return Err(LevelStorageError::Incompatible);
        }
        Ok(())
    }

    /// `PrimaryLevelData.setTagData` with Paper's `paperSpawnDimension` and `Bukkit.Version`.
    pub fn to_tag(&self, last_played: i64) -> Compound {
        let mut tag = Compound::new();
        put(&mut tag, "ServerBrands", string_list(&self.server_brands));
        put(&mut tag, "WasModded", Tag::Byte(self.was_modded.into()));
        if !self.removed_features.is_empty() {
            put(
                &mut tag,
                "removed_features",
                string_list(&self.removed_features),
            );
        }
        let version = Tag::compound([
            ("Name", Tag::String(VERSION_NAME.into())),
            ("Id", Tag::Int(DATA_VERSION)),
            ("Snapshot", Tag::Byte(0)),
            ("Series", Tag::String(VERSION_SERIES.into())),
        ]);
        put(&mut tag, "Version", version);
        let mut history = self.version_history.clone();
        if history.last() != Some(&DATA_VERSION) {
            history.push(DATA_VERSION);
        }
        put(
            &mut tag,
            "version_history",
            Tag::List(history.into_iter().map(Tag::Int).collect()),
        );
        put(&mut tag, "DataVersion", Tag::Int(DATA_VERSION));
        put(&mut tag, "GameType", Tag::Int(self.game_type as i32));
        let s = &self.spawn;
        let spawn = Tag::compound([
            ("dimension", Tag::String(s.dimension.as_str().into())),
            ("pos", Tag::IntArray(vec![s.pos.x, s.pos.y, s.pos.z])),
            ("yaw", Tag::Float(s.yaw.to_bits())),
            ("pitch", Tag::Float(s.pitch.to_bits())),
        ]);
        put(&mut tag, "spawn", spawn);
        let dimension = Tag::String(self.paper_spawn_dimension.as_str().into());
        put(&mut tag, "paperSpawnDimension", dimension);
        put(&mut tag, "Time", Tag::Long(self.game_time));
        put(&mut tag, "LastPlayed", Tag::Long(last_played));
        put(
            &mut tag,
            "LevelName",
            Tag::String(self.level_name.as_str().into()),
        );
        put(&mut tag, "version", Tag::Int(STORAGE_VERSION));
        put(
            &mut tag,
            "allowCommands",
            Tag::Byte(self.allow_commands.into()),
        );
        put(&mut tag, "initialized", Tag::Byte(self.initialized.into()));
        let difficulty = Tag::compound([
            ("difficulty", Tag::String(self.difficulty.name().into())),
            ("hardcore", Tag::Byte(self.hardcore.into())),
            ("locked", Tag::Byte(self.difficulty_locked.into())),
        ]);
        put(&mut tag, "difficulty_settings", difficulty);
        if let Some(uuid) = self.singleplayer_uuid {
            put(&mut tag, "singleplayer_uuid", Tag::IntArray(uuid.to_vec()));
        }
        // `lenientOptionalFieldOf` with a default: omitted when equal to it.
        if self.enabled_packs != ["vanilla"] || !self.disabled_packs.is_empty() {
            let packs = Tag::compound([
                ("Enabled", string_list(&self.enabled_packs)),
                ("Disabled", string_list(&self.disabled_packs)),
            ]);
            put(&mut tag, "DataPacks", packs);
        }
        let features: HashSet<&str> = self.enabled_features.iter().map(String::as_str).collect();
        if features != HashSet::from(["minecraft:vanilla"]) {
            put(
                &mut tag,
                "enabled_features",
                string_list(&self.enabled_features),
            );
        }
        put(
            &mut tag,
            "Bukkit.Version",
            Tag::String(self.bukkit_version.as_str().into()),
        );
        tag
    }
}

/// `WeatherData`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeatherData {
    pub clear_weather_time: i32,
    pub rain_time: i32,
    pub thunder_time: i32,
    pub raining: bool,
    pub thundering: bool,
}
impl WeatherData {
    pub fn parse(tag: &Compound) -> Self {
        Self {
            clear_weather_time: int(tag, "clear_weather_time", 0),
            rain_time: int(tag, "rain_time", 0),
            thunder_time: int(tag, "thunder_time", 0),
            raining: boolean(tag, "raining", false),
            thundering: boolean(tag, "thundering", false),
        }
    }
    pub fn to_tag(&self) -> Compound {
        let Tag::Compound(c) = Tag::compound([
            ("clear_weather_time", Tag::Int(self.clear_weather_time)),
            ("rain_time", Tag::Int(self.rain_time)),
            ("thunder_time", Tag::Int(self.thunder_time)),
            ("raining", Tag::Byte(self.raining.into())),
            ("thundering", Tag::Byte(self.thundering.into())),
        ]) else {
            unreachable!()
        };
        c
    }
}

/// `WorldGenSettings`: `WorldOptions` typed, the level stems raw until generation is ported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldGenSettings {
    pub seed: i64,
    pub generate_structures: bool,
    pub bonus_chest: bool,
    pub legacy_custom_options: Option<String>,
    /// `dimensions`: stem id -> `{type, generator}`.
    pub dimensions: Compound,
}
impl WorldGenSettings {
    pub fn parse(tag: &Compound) -> Self {
        Self {
            seed: number(get(tag, "seed")).unwrap_or(0),
            generate_structures: boolean(tag, "generate_structures", true),
            bonus_chest: boolean(tag, "bonus_chest", false),
            legacy_custom_options: string(tag, "legacy_custom_options"),
            dimensions: match get(tag, "dimensions") {
                Some(Tag::Compound(d)) => d.clone(),
                _ => Compound::new(),
            },
        }
    }
    pub fn to_tag(&self) -> Compound {
        let mut tag = Compound::new();
        put(&mut tag, "seed", Tag::Long(self.seed));
        put(
            &mut tag,
            "generate_structures",
            Tag::Byte(self.generate_structures.into()),
        );
        put(&mut tag, "bonus_chest", Tag::Byte(self.bonus_chest.into()));
        if let Some(options) = &self.legacy_custom_options {
            put(
                &mut tag,
                "legacy_custom_options",
                Tag::String(options.as_str().into()),
            );
        }
        put(
            &mut tag,
            "dimensions",
            Tag::Compound(self.dimensions.clone()),
        );
        tag
    }
}

/// A level's saved data read at load (`None`: no file, the caller's defaults apply).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimensionData {
    /// `minecraft:world_border` (`WorldBorder.Settings`), raw: `server::level_data` decodes it.
    pub world_border: Option<Compound>,
    /// Paper's `paper:level_overrides` (`PaperLevelOverrides`), raw likewise.
    pub level_overrides: Option<Compound>,
    pub world_gen: Option<WorldGenSettings>,
    /// `GameRuleMap`: rule id -> value, raw until game rules are ported.
    pub game_rules: Option<Compound>,
    pub weather: Option<WeatherData>,
    /// `ServerClockManager`'s packed clocks (`minecraft:overworld: {total_ticks}`...): day time.
    pub clocks: Option<Compound>,
}
