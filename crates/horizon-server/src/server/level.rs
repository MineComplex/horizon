//! The world the tick thread owns: the open world folder (`LevelStorageAccess`, `level.dat`, the
//! player files) and its levels, one [`Level`] per level stem of `WorldGenSettings` (the
//! overworld, the nether and the end, each with its own [`ChunkIo`] over
//! `dimensions/<namespace>/<path>/region`, its own chunk holders and saved data), in
//! `WorldDimensions.keysInOrder`'s order: the order the server ticks, saves and stops them in
//! (`MinecraftServer.levels` is a linked map; Leaf's parallel world ticking is off by default and
//! not ported, so the levels tick one after the other). `allow-nether` (Paper's
//! `misc.enable-nether`) and `allow-end` (`bukkit.yml`'s `settings.allow-end`) decide whether
//! those two stems become levels at all (`PaperWorldLoader.getWorldInfoAndData`). What a level
//! keeps for itself (game time, difficulty, spawn, border, clocks) is in `server::level_data`.
//! A player changes level through `Player::change_level` (portals and `/execute in` are not
//! ported).
//!
//! **Chunks.** A [`Level`] holds its chunks in Moonrise's chunk holders (`server::chunk_system`),
//! owned by the tick thread; the region I/O workers read and write them and the level's generation
//! workers run the status pipeline's steps (`worldgen::generator`), both handing their answers back
//! as messages, which [`Level::tick_io`] drains at the start of each tick. Chunks are saved when
//! unloaded and by autosave when changed, and all of them on stop. A level whose stem is a
//! `minecraft:noise` generator generates the chunks its region files do not have; the flat and
//! debug generators are not ported: there such a chunk never finishes (logged once per level)
//! and is never sent.
//!
//! **Blocks.** A [`Level`] is the `world::block_update::BlockLevel` the block-change path and the
//! fluids run on: `setBlock` writes its loaded chunks (heightmaps, unsaved), its neighbour updater
//! chains the updates (`max-chained-neighbor-updates`), its changed blocks are broadcast by
//! [`Level::broadcast_changes`] and its level events taken by [`Level::take_level_events`];
//! [`Level::tick_scheduled_ticks`] runs the due fluid ticks (block ticks wait: see there).
//!
//! **A new world.** Without `level.dat`, the world is created as `MinecraftServer.loadLevel` does
//! from `server.properties` (`level-seed`, `level-type`'s world preset, `generate-structures`):
//! `level.dat` and the overworld's `world_gen_settings` are written at once, and each level that
//! is not `initialized` gets its spawn (`setInitialSpawn`, Paper's per-level `initWorld`) from its
//! generator, generating the chunks it looks at.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use tracing::{error, info, warn};

use super::chunk_system::{
    ChunkHolderManager, ChunkStorage, FullChunkStatus, GenerationJob, GenerationPool,
    GenerationResult, LoadedChunk, TicketType,
};
use super::config::{self, ConfigError, PaperWorldSettings, SpigotConfig};
use super::level_data::{BorderSettings, LevelClocks, LevelOverrides, Respawn, WorldBorder};
use super::player_data::PlayerDataStorage;
use crate::block::flags::BlockStateData;
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::geometry::{Direction, ShapeOp, VoxelShape};
use crate::core::position::{BlockPos, ChunkPos, SectionPos};
use crate::nbt::{Compound, Tag};
use crate::network::play::chunk::{LightSection, level_chunk_with_light};
use crate::registry::Registries;
use crate::util::random::LegacyRandom;
use crate::world::block_changes::BlockChanges;
use crate::world::block_rules::BlockRules;
use crate::world::block_update::{BlockLevel, MAX_CHAINED_NEIGHBOR_UPDATES, NeighborUpdater};
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{BiomeIds, ChunkData, ContainerFactory};
use crate::world::chunk::{ChunkStatus, SectionFlags, StateFlags};
use crate::world::chunk_io::{ChunkIo, ChunkIoEvent, CodecTables};
use crate::world::fluid::{self, FluidSettings};
use crate::world::level_chunk::{HeightmapTable, LevelChunk};
use crate::world::level_storage::WorldGenSettings;
use crate::world::level_storage::{
    Difficulty, DimensionData, GameType, LevelData, LevelStorageAccess, LevelStorageError,
    RespawnData, WeatherData,
};
use crate::world::storage::compression::Compression;
use crate::world::ticks::TickPriority;
use crate::worldgen::biome::codec::{BiomePresets, decode_noise_generator_nbt};
use crate::worldgen::data::WorldgenData;
use crate::worldgen::generator::convert::{
    ChunkShape, ProtoExtras, proto_from_data, proto_into_level_chunk, proto_to_data,
};
use crate::worldgen::generator::{
    GeneratorError, GeneratorTables, LevelGenerator, LevelGeneratorSettings, respawn_pos_in_column,
};
use crate::worldgen::proto_chunk::ProtoChunk;
use crate::worldgen::structure::StructureRegistry;

/// Paper's `environment.max-block-ticks` and `max-fluid-ticks` defaults (`paper-world-defaults.yml`;
/// the per-world files are not read yet).
const MAX_BLOCK_TICKS: i32 = 65536;
const MAX_FLUID_TICKS: i32 = 65536;

pub const OVERWORLD: &str = "minecraft:overworld";
pub const NETHER: &str = "minecraft:the_nether";
pub const END: &str = "minecraft:the_end";

/// A level stem of the world generation settings.
#[derive(Debug, PartialEq, Eq)]
struct Stem {
    /// The stem's key, which is also its level's dimension.
    key: String,
    /// The stem's `type`, an id in `minecraft:dimension_type`.
    dimension_type: String,
    /// The generator is `minecraft:flat`.
    flat: bool,
    /// The generator's `settings` id when it names noise settings.
    settings: Option<String>,
    /// The `generator` compound as saved.
    generator: Option<Tag>,
}

impl Stem {
    fn read(key: &str, stem: Option<&Compound>) -> Self {
        let string = |t: Option<&Tag>| match t {
            Some(Tag::String(s)) => Some(s.to_string_lossy()),
            _ => None,
        };
        let generator = match stem.and_then(|s| s.get(&"generator".into())) {
            Some(Tag::Compound(g)) => Some(g),
            _ => None,
        };
        Self {
            key: key.to_owned(),
            dimension_type: string(stem.and_then(|s| s.get(&"type".into())))
                .unwrap_or_else(|| key.to_owned()),
            flat: string(generator.and_then(|g| g.get(&"type".into()))).as_deref()
                == Some("minecraft:flat"),
            settings: string(generator.and_then(|g| g.get(&"settings".into()))),
            generator: generator.map(|g| Tag::Compound(g.clone())),
        }
    }

    /// `ServerLevel.getSeaLevel` of a level without a generator here: flat worlds -63, else the
    /// three vanilla noise settings' values (a level with a generator has its settings' own).
    fn sea_level(&self) -> i32 {
        match (self.flat, self.settings.as_deref()) {
            (true, _) => -63,
            (_, Some("minecraft:nether")) => 32,
            (_, Some("minecraft:end")) => 0,
            _ => 63,
        }
    }

    /// `PaperWorldLoader.defaultWorldName`: the level name, then `_nether`, `_the_end` or the
    /// stem's namespace and path.
    fn world_name(&self, level_name: &str) -> String {
        match self.key.as_str() {
            OVERWORLD => level_name.to_owned(),
            NETHER => format!("{level_name}_nether"),
            END => format!("{level_name}_the_end"),
            key => format!("{level_name}_{}", key.replacen(':', "_", 1)),
        }
    }
}

/// `WorldDimensions.keysInOrder` over the stems of the overworld's `world_gen_settings` (a world
/// without them has the three vanilla stems): the builtin ones in order, then the others.
fn level_stems(dimensions: Option<&Compound>) -> Result<Vec<Stem>, WorldError> {
    let Some(dimensions) = dimensions else {
        return Ok([OVERWORLD, NETHER, END]
            .map(|key| Stem::read(key, None))
            .into());
    };
    let get = |key: &str| match dimensions.get(&key.into()) {
        Some(Tag::Compound(c)) => Some(c),
        _ => None,
    };
    if get(OVERWORLD).is_none() {
        return Err(WorldError::NoOverworld);
    }
    let builtin = [OVERWORLD, NETHER, END];
    let others = dimensions
        .keys()
        .map(|k| k.to_string_lossy())
        .filter(|k| !builtin.contains(&k.as_str()));
    Ok(builtin
        .iter()
        .map(|k| (*k).to_owned())
        .filter(|k| get(k).is_some())
        .chain(others)
        .map(|key| Stem::read(&key, get(&key)))
        .collect())
}

/// `DedicatedServerProperties.WorldDimensionData.create` and `WorldOptions` for a new world:
/// `level-type`'s world preset (`default` and `largebiomes` are the legacy names of `normal` and
/// `large_biomes`; an unknown one is `normal`), `level-seed` (random without one) and
/// `generate-structures`. `None` without the worldgen tables.
fn new_world_gen_settings(
    generation: &GenerationSettings<'_>,
    tables: &WorldTables,
) -> Option<WorldGenSettings> {
    let worldgen = tables.worldgen.as_ref()?;
    let presets = worldgen.data.raw("minecraft:worldgen/world_preset")?;
    let name = match generation.level_type {
        "default" => "minecraft:normal".to_owned(),
        "largebiomes" => "minecraft:large_biomes".to_owned(),
        other if other.contains(':') => other.to_owned(),
        other => format!("minecraft:{other}"),
    };
    let find = |id: &str| presets.elements.iter().find(|(e, _)| e == id);
    let (_, raw) = find(&name).or_else(|| {
        warn!("Unknown level type {name}, using minecraft:normal");
        find("minecraft:normal")
    })?;
    let preset: serde_json::Value = serde_json::from_str(raw.get()).ok()?;
    let Tag::Compound(dimensions) = json_to_nbt(preset.get("dimensions")?) else {
        return None;
    };
    Some(WorldGenSettings {
        seed: generation.level_seed.unwrap_or_else(rand::random),
        generate_structures: generation.generate_structures,
        bonus_chest: false,
        legacy_custom_options: None,
        dimensions,
    })
}

/// A world preset's JSON as `WorldGenSettings.CODEC` writes it in NBT (strings, compounds,
/// lists; integers as ints, other numbers as doubles, booleans as bytes).
fn json_to_nbt(value: &serde_json::Value) -> Tag {
    use serde_json::Value;
    match value {
        Value::Null => Tag::End,
        Value::Bool(b) => Tag::Byte((*b).into()),
        Value::Number(n) => match n.as_i64().and_then(|v| i32::try_from(v).ok()) {
            Some(v) => Tag::Int(v),
            None => Tag::Double(n.as_f64().unwrap_or(0.0).to_bits()),
        },
        Value::String(s) => Tag::String(s.as_str().into()),
        Value::Array(items) => Tag::List(items.iter().map(json_to_nbt).collect()),
        Value::Object(map) => Tag::Compound(
            map.iter()
                .map(|(k, v)| (k.as_str().into(), json_to_nbt(v)))
                .collect(),
        ),
    }
}

/// A level with no `weather.dat`.
const CLEAR_WEATHER: WeatherData = WeatherData {
    clear_weather_time: 0,
    rain_time: 0,
    thunder_time: 0,
    raining: false,
    thundering: false,
};

#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    #[error(transparent)]
    Storage(#[from] LevelStorageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("the registries have no {0}")]
    Registry(String),
    /// `PaperWorldLoader.loadInitialWorlds`: `requireNonNull(..., "Overworld stem missing")`.
    #[error("Overworld stem missing")]
    NoOverworld,
    /// A level's generator could not be made (its stem or noise settings do not decode, or its
    /// `spigot.yml` seeds are not numbers).
    #[error("Exception initializing level {level}: {message}")]
    Generator { level: String, message: String },
    /// `PaperConfigurations.createWorldConfig`: a level's `paper-world.yml` can't be read.
    #[error("Could not create world config for {level}: {source}")]
    PaperWorld { level: String, source: ConfigError },
}

/// The worldgen registries and multi-noise presets (startup's, shared by the levels).
pub struct WorldgenTables {
    pub data: Arc<WorldgenData>,
    pub presets: Arc<BiomePresets>,
}

/// How levels generate (`server.properties` for a new world, the Paper/Purpur/Spigot settings).
#[derive(Clone, Debug)]
pub struct GenerationSettings<'a> {
    /// `level-seed` (`None`: a random seed for a new world).
    pub level_seed: Option<i64>,
    /// `level-type`: the new world's preset.
    pub level_type: &'a str,
    /// `generate-structures` for a new world.
    pub generate_structures: bool,
    /// Purpur's `settings.generate-end-void-rings`.
    pub end_void_rings: bool,
    /// `spigot.yml`'s structure seeds (`None`: the jar's salts).
    pub spigot: Option<&'a SpigotConfig>,
}

impl Default for GenerationSettings<'_> {
    fn default() -> Self {
        Self {
            level_seed: None,
            level_type: "minecraft:normal",
            generate_structures: true,
            end_void_rings: false,
            spigot: None,
        }
    }
}

/// Where levels get Paper's world settings (`chunks`, `environment.generate-flat-bedrock`).
#[derive(Clone, Copy, Debug)]
pub enum PaperWorld<'a> {
    /// Paper's files: `paper-world-defaults.yml` in this config folder, under each level's own
    /// `paper-world.yml` ([`config::load_paper_world`]).
    Files(&'a Path),
    /// The same settings for every level, no files (tests).
    Fixed(PaperWorldSettings),
}

impl PaperWorld<'_> {
    /// The settings of the level `key` stored in `dimension_dir`.
    fn for_level(&self, key: &str, dimension_dir: &Path) -> Result<PaperWorldSettings, WorldError> {
        match self {
            PaperWorld::Fixed(settings) => Ok(*settings),
            PaperWorld::Files(config_dir) => {
                config::load_paper_world(config_dir, dimension_dir, key).map_err(|source| {
                    WorldError::PaperWorld {
                        level: key.to_owned(),
                        source,
                    }
                })
            }
        }
    }
}

/// The read-only tables a world is opened with (startup's, shared with the I/O workers).
pub struct WorldTables {
    pub blocks: Arc<BlockStates>,
    pub block_data: Arc<BlockStateData>,
    pub registries: Arc<Registries>,
    /// `None`: no level generates (tests over saved worlds).
    pub worldgen: Option<WorldgenTables>,
}

/// Tag names with the ids of their entries.
type TagIds = Vec<(String, Vec<i32>)>;

impl WorldTables {
    /// The block and fluid tags of the data packs, as ids of the state table's blocks and of
    /// `block_states.bin`'s fluids (`BlockSurvival::new`'s and `BlockRules::new`'s form).
    fn tags(&self) -> (TagIds, TagIds) {
        let tags = |registry: &str, id: &dyn Fn(&str) -> Option<i32>| -> TagIds {
            let Some(registry) = self.registries.builtin(registry) else {
                return Vec::new();
            };
            (registry.tags.0.iter())
                .map(|(tag, ids)| {
                    let ids = (ids.iter())
                        .filter_map(|&i| registry.entries.get(i as usize))
                        .filter_map(|name| id(name))
                        .collect();
                    (tag.clone(), ids)
                })
                .collect()
        };
        let block_tags = tags("minecraft:block", &|name| {
            self.blocks.block_by_name(name).map(|b| b.0 as i32)
        });
        let fluid_tags = tags("minecraft:fluid", &|name| {
            (crate::worldgen::placement::FLUIDS.iter())
                .position(|f| *f == name)
                .map(|i| i as i32)
        });
        (block_tags, fluid_tags)
    }

    /// The block rules of the block-change path and the fluids (`world::block_rules`).
    pub fn block_rules(&self) -> BlockRules {
        let (block_tags, fluid_tags) = self.tags();
        BlockRules::new(
            self.blocks.clone(),
            self.block_data.clone(),
            &block_tags,
            &fluid_tags,
        )
    }
}

/// How the world is opened (`server.properties`).
pub struct WorldSettings<'a> {
    pub level_name: &'a str,
    /// `region-file-compression`, already parsed.
    pub compression: Compression,
    pub sync_chunk_writes: bool,
    /// For a world without `level.dat`: `gamemode`, `difficulty`, `hardcore`.
    pub new_world: (GameType, Difficulty, bool),
    /// Where each level's Paper world settings come from.
    pub paper_world: PaperWorld<'a>,
    /// `misc.enable-nether` of `paper-global.yml` (`CraftServer.getAllowNether`).
    pub allow_nether: bool,
    /// `settings.allow-end` of `bukkit.yml` (`CraftServer.getAllowEnd`).
    pub allow_end: bool,
    pub generation: GenerationSettings<'a>,
}

impl WorldSettings<'_> {
    /// `PaperWorldLoader.getWorldInfo`: a disabled nether or end stem is not loaded.
    fn enabled(&self, stem: &str) -> bool {
        match stem {
            NETHER => self.allow_nether,
            END => self.allow_end,
            _ => true,
        }
    }
}

pub struct World {
    pub storage: LevelStorageAccess,
    /// `level-name` (Bukkit's world name in the chunk system's logs).
    pub name: String,
    pub data: LevelData,
    /// Whether `level.dat` is written: a world found without one and that could not be created
    /// (no worldgen tables) is not.
    pub has_level_data: bool,
    pub players: PlayerDataStorage,
    /// The levels in `MinecraftServer.levels`' order; the overworld is the first.
    pub levels: Vec<Level>,
}

impl World {
    /// `MinecraftServer.loadWorld` as far as it is ported (see the module docs).
    pub fn open(
        base_dir: &Path,
        settings: &WorldSettings<'_>,
        tables: &WorldTables,
    ) -> Result<Self, WorldError> {
        let storage = LevelStorageAccess::open(base_dir, settings.level_name)?;
        info!("Preparing level \"{}\"", settings.level_name);
        let mut has_level_data = storage.has_world_data();
        let mut new_world_gen = None;
        let data = if has_level_data {
            let data = storage.read_level_data()?;
            data.check_compatible()?;
            data
        } else {
            let (game_type, difficulty, hardcore) = settings.new_world;
            let data = LevelData {
                game_type,
                difficulty,
                hardcore,
                level_name: settings.level_name.to_owned(),
                ..LevelData::parse(&Default::default())
            };
            match new_world_gen_settings(&settings.generation, tables) {
                Some(world_gen) => {
                    storage.save_level_data(&data)?;
                    storage.write_saved_data(
                        OVERWORLD,
                        "world_gen_settings",
                        world_gen.to_tag(),
                    )?;
                    has_level_data = true;
                    new_world_gen = Some(world_gen);
                }
                None => warn!(
                    "No level.dat in \"{}\" and no world generation data: the world stays empty \
                     (no chunks are sent) and no level.dat is written; player files still are",
                    settings.level_name
                ),
            }
            data
        };
        let mut overworld_data = Some(storage.read_dimension_data(OVERWORLD)?);
        if let (Some(world_gen), Some(overworld)) = (new_world_gen, overworld_data.as_mut()) {
            overworld.world_gen = Some(world_gen);
        }
        let stems = level_stems(
            overworld_data
                .as_ref()
                .and_then(|d| d.world_gen.as_ref())
                .map(|w| &w.dimensions),
        )?;
        let overworld_seed = overworld_data
            .as_ref()
            .and_then(|d| d.world_gen.as_ref())
            .map_or(0, |w| w.seed);
        let overworld_structures = overworld_data
            .as_ref()
            .and_then(|d| d.world_gen.as_ref())
            .is_none_or(|w| w.generate_structures);
        let players = PlayerDataStorage::new(storage.dir())?;
        let rules = Arc::new(tables.block_rules());
        let mut levels = Vec::with_capacity(stems.len());
        for stem in &stems {
            if !settings.enabled(&stem.key) {
                continue;
            }
            // Each level reads its own dimension's saved data (`loadWorldGenSettings`,
            // weather...); one without a folder has the overworld's seed and a clear sky.
            let saved = match overworld_data.take_if(|_| stem.key == OVERWORLD) {
                Some(saved) => saved,
                None => storage.read_dimension_data(&stem.key)?,
            };
            let seed = saved.world_gen.as_ref().map_or(overworld_seed, |w| w.seed);
            let generate_structures = saved
                .world_gen
                .as_ref()
                .map_or(overworld_structures, |w| w.generate_structures);
            let region = storage.region_path(&stem.key, "region");
            // Paper makes the level's world config as the level is constructed.
            let paper = settings
                .paper_world
                .for_level(&stem.key, &storage.dimension_path(&stem.key))?;
            levels.push(Level::open(
                stem,
                stem.world_name(settings.level_name),
                (seed, generate_structures),
                LevelStart::new(saved, &data),
                &region,
                (settings, &paper),
                (tables, &rules),
            )?);
        }
        let mut world = Self {
            storage,
            name: settings.level_name.to_owned(),
            data,
            has_level_data,
            players,
            levels,
        };
        // `initWorld` per level: a level not initialized gets its spawn from its generator.
        for level in &mut world.levels {
            if !level.overrides.initialized && level.generator.is_some() {
                level.set_initial_spawn();
                if level.dimension == OVERWORLD {
                    world.data.initialized = true;
                }
            }
        }
        world.update_effective_respawn();
        Ok(world)
    }

    /// `MinecraftServer.findRespawnDimension`'s index: the level `paperSpawnDimension` names, else
    /// the overworld.
    fn respawn_level(&self) -> usize {
        self.level_index(&self.data.paper_spawn_dimension)
            .unwrap_or(0)
    }

    /// `MinecraftServer.getRespawnData` (`effectiveRespawnData`): the respawn level's own spawn,
    /// as a position in that level. (`getWorldBorderAdjustedRespawnData` moves a spawn outside the
    /// level's border to the border's centre column; that needs a chunk load and is not ported.)
    pub fn effective_respawn(&self) -> RespawnData {
        let level = &self.levels[self.respawn_level()];
        let r = level.overrides.respawn;
        RespawnData {
            dimension: level.dimension.clone(),
            pos: r.pos,
            yaw: r.yaw,
            pitch: r.pitch,
        }
    }

    /// `updateEffectiveRespawnData`'s write-back: `level.dat` holds the respawn level's spawn.
    pub fn update_effective_respawn(&mut self) {
        self.data.spawn = self.effective_respawn();
    }

    /// `max-chained-neighbor-updates` (`server.properties`) for every level's neighbour updater.
    pub fn set_max_chained_neighbor_updates(&mut self, max: i32) {
        for level in &mut self.levels {
            level.updater.set_max_chained(max);
        }
    }

    /// `ServerLevel.tick`'s time part for one level while the game runs normally; the
    /// overworld's game time is also the root's (`syncRootData`).
    pub fn tick_level_time(&mut self, index: usize) {
        let level = &mut self.levels[index];
        level.tick_time();
        if level.dimension == OVERWORLD {
            self.data.game_time = level.overrides.game_time;
        }
    }

    pub fn overworld(&self) -> &Level {
        &self.levels[0]
    }

    /// `MinecraftServer.getLevel(dimension)`: the index in [`Self::levels`].
    pub fn level_index(&self, dimension: &str) -> Option<usize> {
        self.levels.iter().position(|l| l.dimension == dimension)
    }

    /// The start of each tick's I/O: every level's region answers.
    pub fn tick_io(&mut self) {
        self.levels.iter_mut().for_each(Level::tick_io);
    }

    /// `saveGlobalData`: `level.dat` (not for a world this server did not find one in), and each
    /// level's saved data that changed (`SavedDataStorage.scheduleSave`).
    pub fn save_level_data(&mut self) {
        if self.has_level_data
            && let Err(e) = self.storage.save_level_data(&self.data)
        {
            error!("Failed to save level {}: {e}", self.storage.dir().display());
        }
        for level in &mut self.levels {
            level.save_data(&self.storage);
        }
    }

    /// `saveAllChunks(silent, flush, force)` without closing: `level.dat`, then every level's
    /// unsaved chunks (`ChunkMap.saveAllChunks`).
    pub fn save_all_chunks(&mut self, silent: bool, flush: bool) {
        self.save_level_data();
        for level in &mut self.levels {
            if !silent {
                info!(
                    "Saving chunks for level 'ServerLevel[{}]'/{}",
                    self.name, level.dimension
                );
            }
            let time = level.overrides.game_time;
            level.chunks.save_all(flush, false, None, time);
        }
    }

    /// The end of `stopServer`: `saveAllChunks(false, true, false, true)` (`level.dat`, then each
    /// level's `ChunkHolderManager.close`: every chunk saved, the regions flushed, the I/O workers
    /// stopped once their queued requests ran), then the wait for the region I/O.
    pub fn close(mut self) {
        self.save_level_data();
        for level in self.levels {
            info!(
                "Saving chunks for level 'ServerLevel[{}]'/{}",
                self.name, level.dimension
            );
            let time = level.overrides.game_time;
            level.close(time);
        }
        info!("Waiting for all RegionFile I/O tasks to complete...");
        info!("All RegionFile I/O tasks to complete");
    }
}

/// A level's chunk storage and generation: the region workers, the generation workers (none for
/// a level without a generator) and the level's chunk shape. Requests are messages; answers come
/// back through [`Level::tick_io`].
pub struct LevelChunks {
    io: ChunkIo,
    pool: Option<GenerationPool>,
    tables: Arc<CodecTables<BlockStateData>>,
    heightmaps: Arc<HeightmapTable>,
    /// The structure registry the saved starts are read with (empty without worldgen data).
    structures: Option<Arc<WorldgenData>>,
    empty_structures: StructureRegistry,
    void_air: BlockState,
    /// Steps a level without a generator cannot run: they fail on the next tick.
    refused: Vec<GenerationResult>,
    refusal_logged: bool,
    dimension: String,
}

impl LevelChunks {
    fn shape(&self) -> ChunkShape<'_> {
        ChunkShape {
            min_section_y: self.tables.min_section_y,
            section_count: (self.tables.max_section_y - self.tables.min_section_y + 1) as usize,
            factory: &self.tables.factory,
            void_air: self.void_air,
        }
    }
    fn structures(&self) -> &StructureRegistry {
        self.structures
            .as_ref()
            .map_or(&self.empty_structures, |d| d.structures())
    }
}

impl ChunkStorage for LevelChunks {
    fn load(&mut self, pos: ChunkPos) {
        self.io.load(pos);
    }
    fn save(&mut self, pos: ChunkPos, chunk: Box<ChunkData>) {
        self.io.save(pos, Some(chunk));
    }
    fn flush(&mut self) {
        self.io.flush();
    }
    fn generate(&mut self, job: GenerationJob) {
        match &self.pool {
            Some(pool) => pool.submit(job),
            None => {
                if !std::mem::replace(&mut self.refusal_logged, true) {
                    warn!(
                        "Chunk [{}, {}] of {} is not on disk, and this level's generator is not \
                         ported (flat and debug worlds): it is not sent (further chunks are not logged)",
                        job.pos.x, job.pos.z, self.dimension
                    );
                }
                self.refused.push(GenerationResult {
                    pos: job.pos,
                    status: job.status,
                    result: Err(None),
                    nanos: 0,
                });
            }
        }
    }
    fn empty_chunk(&self, pos: ChunkPos) -> ProtoChunk {
        self.shape().empty(pos)
    }
    fn proto_data(&self, chunk: &ProtoChunk, extras: &ProtoExtras) -> ChunkData {
        proto_to_data(chunk, extras, self.structures())
    }
    fn level_chunk(&self, chunk: &ProtoChunk, extras: &ProtoExtras) -> LevelChunk {
        proto_into_level_chunk(
            chunk,
            extras,
            self.shape(),
            self.structures(),
            &*self.heightmaps,
        )
    }
    fn proto_view(&self, chunk: &LevelChunk) -> ProtoChunk {
        match proto_from_data(chunk.to_data(), self.shape(), self.structures()) {
            Ok((view, _)) => view,
            Err(_) => {
                let mut view = self.shape().empty(chunk.pos());
                view.set_status(ChunkStatus::Full);
                view
            }
        }
    }
}

/// One dimension's chunks (see the module docs).
pub struct Level {
    /// The stem's key (`minecraft:the_nether`).
    pub dimension: String,
    /// Bukkit's name for the world (`world_nether`), which Moonrise's logs use.
    pub world_name: String,
    /// `WorldOptions.seed`.
    pub seed: i64,
    /// The generator is flat (`ServerLevel.isFlat`).
    pub flat: bool,
    pub sea_level: i32,
    /// `weather.dat`: the level's own sky.
    pub weather: WeatherData,
    /// `paper:level_overrides`: the level's game time, spawn, difficulty (`server::level_data`).
    pub overrides: LevelOverrides,
    /// The level's own border (`world_border.dat`).
    pub border: WorldBorder,
    /// The level's own clocks, its day time (`world_clocks.dat`).
    pub clocks: LevelClocks,
    /// The dimension type's `default_clock`.
    pub default_clock: Option<String>,
    /// The dimension type's `coordinate_scale`.
    pub coordinate_scale: f64,
    /// Its id in `minecraft:dimension_type`.
    pub dimension_type: i32,
    pub min_section_y: i32,
    pub section_count: usize,
    /// The chunk holders, over the region I/O and generation workers.
    pub chunks: ChunkHolderManager<LevelChunks>,
    /// The level's generator (`None`: its stem's generator is not ported).
    pub generator: Option<Arc<LevelGenerator>>,
    io_events: Receiver<ChunkIoEvent>,
    gen_events: Receiver<GenerationResult>,
    tables: Arc<CodecTables<BlockStateData>>,
    heightmaps: Arc<HeightmapTable>,
    block_data: Arc<BlockStateData>,
    /// Whether the dimension type has a ceiling (`hasCeiling`: the spawn finder's height).
    has_ceiling: bool,
    rules: Arc<BlockRules>,
    /// `Level.neighborUpdater`.
    updater: NeighborUpdater,
    /// `Level.random` (Moonrise's `ThreadUnsafeRandom`: the legacy LCG, seeded uniquely).
    random: LegacyRandom,
    pub fluid_settings: FluidSettings,
    /// The changed blocks the chunk holders send each tick (`ChunkHolder.blockChanged`).
    changes: BlockChanges,
    /// `levelEvent`s for the players near them, sent after the scheduled ticks.
    events: Vec<LevelEvent>,
}

/// A `ClientboundLevelEventPacket` to the players of the level within 64 blocks of `pos`
/// (`PlayerList.broadcast`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelEvent {
    pub event: i32,
    pub pos: BlockPos,
    pub data: i32,
}

/// `RandomSupport.generateUniqueSeed`: the nano time mixed with a per-process counter.
fn unique_seed() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static SEED: AtomicI64 = AtomicI64::new(8_682_522_807_148_012);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    // `seedUniquifier *= 1181783497276652981` (a racy update only changes which seed is unique).
    let next = SEED
        .load(Ordering::Relaxed)
        .wrapping_mul(1_181_783_497_276_652_981);
    SEED.store(next, Ordering::Relaxed);
    next ^ nanos
}

/// What a level starts with from its saved data (`ServerLevel`'s constructor and `loadWorldData`).
struct LevelStart {
    weather: WeatherData,
    overrides: LevelOverrides,
    border: BorderSettings,
    clocks: Option<Compound>,
}

impl LevelStart {
    /// A saved file that does not decode is logged and read as missing (`SavedDataStorage`):
    /// the overrides then copy the root `level.dat`, the border is the default one.
    fn new(saved: DimensionData, root: &LevelData) -> Self {
        let overrides = match &saved.level_overrides {
            Some(tag) => LevelOverrides::parse(tag).unwrap_or_else(|| {
                error!("Failed to parse saved data for 'paper:level_overrides'");
                LevelOverrides::from_root(root)
            }),
            None => LevelOverrides::from_root(root),
        };
        let border = match &saved.world_border {
            Some(tag) => BorderSettings::parse(tag).unwrap_or_else(|| {
                error!("Failed to parse saved data for 'minecraft:world_border'");
                BorderSettings::DEFAULT
            }),
            None => BorderSettings::DEFAULT,
        };
        Self {
            weather: saved.weather.unwrap_or(CLEAR_WEATHER),
            overrides,
            border,
            clocks: saved.clocks,
        }
    }
}

/// A block tag of the built-in block registry, as block ids of the state table.
fn block_tag(tables: &WorldTables, tag: &str) -> HashSet<crate::block::BlockId> {
    let Some(registry) = tables.registries.builtin("minecraft:block") else {
        return HashSet::new();
    };
    let ids = registry.tags.get(tag).unwrap_or_default();
    ids.iter()
        .filter_map(|&id| registry.entries.get(id as usize))
        .filter_map(|name| tables.blocks.block_by_name(name))
        .collect()
}

/// The blocks' `canSurvive` rules over the data-pack block and fluid tags, as registry ids of
/// the block table and of the fluids `worldgen/placement` names.
fn block_survival(
    tables: &WorldTables,
) -> Result<crate::block::survival::BlockSurvival, crate::block::survival::SurvivalDataError> {
    let (block_tags, fluid_tags) = tables.tags();
    crate::block::survival::BlockSurvival::new(
        &tables.blocks,
        &tables.block_data,
        &block_tags,
        &fluid_tags,
    )
}

impl Level {
    fn open(
        stem: &Stem,
        world_name: String,
        (seed, generate_structures): (i64, bool),
        start: LevelStart,
        region: &Path,
        (settings, paper): (&WorldSettings<'_>, &PaperWorldSettings),
        (tables, rules): (&WorldTables, &Arc<BlockRules>),
    ) -> Result<Self, WorldError> {
        let types = tables
            .registries
            .synced("minecraft:dimension_type")
            .ok_or_else(|| WorldError::Registry("minecraft:dimension_type".into()))?;
        let (dimension_type, entry) = types
            .entries
            .iter()
            .enumerate()
            .find(|(_, e)| e.id == stem.dimension_type)
            .ok_or_else(|| {
                WorldError::Registry(format!("dimension type {}", stem.dimension_type))
            })?;
        let int = |k: &str, default| {
            entry
                .json
                .get(k)
                .and_then(|v| v.as_i64())
                .unwrap_or(default)
        };
        let (min_y, height) = (int("min_y", -64) as i32, int("height", 384) as i32);
        let coordinate_scale = entry
            .json
            .get("coordinate_scale")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        let default_clock = entry
            .json
            .get("default_clock")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let has_ceiling = entry
            .json
            .get("has_ceiling")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // The dimension's `gameplay/fast_lava` attribute (the nether's).
        let fast_lava = entry
            .json
            .get("attributes")
            .and_then(|a| a.get("minecraft:gameplay/fast_lava"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let clock_ids: Vec<String> = tables
            .registries
            .synced("minecraft:world_clock")
            .map(|r| r.entries.iter().map(|e| e.id.clone()).collect())
            .unwrap_or_default();
        let mut border = WorldBorder::new(start.border);
        border.apply_initial_settings();
        let mut biomes: Vec<&str> = tables
            .registries
            .synced("minecraft:worldgen/biome")
            .map(|b| b.entries.iter().map(|e| e.id.as_str()).collect())
            .unwrap_or_default();
        if biomes.is_empty() {
            biomes.push("minecraft:plains");
        }
        let biome_count = biomes.len();
        let biomes = BiomeIds::new(biomes);
        let plains = biomes.id("minecraft:plains").unwrap_or(0);
        let codec = Arc::new(CodecTables {
            factory: ContainerFactory::new(tables.blocks.state_count(), biomes.len(), plains),
            blocks: tables.blocks.clone(),
            flags: tables.block_data.clone(),
            biomes: Arc::new(biomes),
            min_section_y: min_y >> 4,
            max_section_y: ((min_y + height) >> 4) - 1,
        });
        let heightmaps = Arc::new(HeightmapTable::vanilla(
            &tables.blocks,
            &tables.block_data,
            &block_tag(tables, "minecraft:blocks_motion_in_heightmap"),
            &block_tag(tables, "minecraft:blocks_motion_in_heightmap_no_leaves"),
        ));
        let generator = Self::generator(
            stem,
            &world_name,
            (seed, generate_structures),
            (min_y, height, biome_count as u32, plains),
            (settings, paper),
            tables,
            &heightmaps,
        )?;
        let (gen_sender, gen_events) = std::sync::mpsc::channel();
        // Moonrise's `chunk-system.worker-threads` default: half the processors, at least one.
        let threads = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1));
        let pool = generator
            .as_ref()
            .map(|g| GenerationPool::spawn(g.clone(), threads, gen_sender, &world_name))
            .transpose()?;
        let void_air = tables
            .blocks
            .block_by_name("minecraft:void_air")
            .map_or(BlockState::AIR, |b| tables.blocks.default_state(b));
        // Moonrise sizes its I/O pool from the processors; regions spread over the workers.
        let workers = std::thread::available_parallelism().map_or(1, |n| (n.get() / 4).max(1));
        let (io, io_events) = ChunkIo::spawn(
            region.to_path_buf(),
            settings.compression,
            settings.sync_chunk_writes,
            codec.clone(),
            workers,
        )?;
        let sea_level = generator
            .as_ref()
            .map_or_else(|| stem.sea_level(), |g| g.sea_level());
        let backend = LevelChunks {
            io,
            pool,
            tables: codec.clone(),
            heightmaps: heightmaps.clone(),
            structures: tables.worldgen.as_ref().map(|w| w.data.clone()),
            empty_structures: StructureRegistry::default(),
            void_air,
            refused: Vec::new(),
            refusal_logged: false,
            dimension: stem.key.clone(),
        };
        let game_time = start.overrides.game_time;
        Ok(Self {
            dimension: stem.key.clone(),
            world_name,
            seed,
            flat: stem.flat,
            sea_level,
            weather: start.weather,
            overrides: start.overrides,
            border,
            clocks: LevelClocks::load(&clock_ids, start.clocks.as_ref()),
            default_clock,
            coordinate_scale,
            dimension_type: dimension_type as i32,
            min_section_y: min_y >> 4,
            section_count: (height / 16) as usize,
            chunks: {
                let mut chunks = ChunkHolderManager::new(backend, paper.chunks);
                chunks.set_game_time(game_time);
                chunks
            },
            generator,
            io_events,
            gen_events,
            tables: codec,
            heightmaps,
            block_data: tables.block_data.clone(),
            has_ceiling,
            rules: rules.clone(),
            updater: NeighborUpdater::new(MAX_CHAINED_NEIGHBOR_UPDATES),
            random: LegacyRandom::new(unique_seed()),
            fluid_settings: FluidSettings::new(fast_lava),
            changes: BlockChanges::default(),
            events: Vec::new(),
        })
    }

    /// The level's generator from its stem (`LevelStem.generator()`, Paper's world settings), or
    /// `None` for a stem whose generator is not ported or without worldgen tables.
    fn generator(
        stem: &Stem,
        world_name: &str,
        (seed, generate_structures): (i64, bool),
        (min_y, height, biome_count, plains): (i32, i32, u32, u32),
        (settings, paper): (&WorldSettings<'_>, &PaperWorldSettings),
        tables: &WorldTables,
        heightmaps: &Arc<HeightmapTable>,
    ) -> Result<Option<Arc<LevelGenerator>>, WorldError> {
        let (Some(worldgen), Some(generator)) = (&tables.worldgen, &stem.generator) else {
            return Ok(None);
        };
        let error = |message: String| WorldError::Generator {
            level: world_name.to_owned(),
            message,
        };
        let kind = match generator {
            Tag::Compound(g) => match g.get(&"type".into()) {
                Some(Tag::String(t)) => t.to_string_lossy(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if kind != "minecraft:noise" {
            warn!(
                "Level {world_name}: the {kind} generator is not ported; its chunks are not generated"
            );
            return Ok(None);
        }
        let biome_registry = tables
            .registries
            .synced("minecraft:worldgen/biome")
            .ok_or_else(|| WorldError::Registry("minecraft:worldgen/biome".into()))?;
        let biomes = crate::worldgen::biome::codec::BiomeIds::new(biome_registry);
        let noise_stem = decode_noise_generator_nbt(generator, &biomes, &worldgen.presets)
            .map_err(|e| error(e.to_string()))?;
        let spigot_seeds = match settings.generation.spigot {
            Some(spigot) => spigot
                .world_seeds(&stem.key)
                .map_err(|e| error(e.to_string()))?,
            None => Default::default(),
        };
        let generator = LevelGenerator::new(
            GeneratorTables {
                data: worldgen.data.clone(),
                blocks: tables.blocks.clone(),
                flags: tables.block_data.clone(),
                heightmaps: heightmaps.clone(),
                survival: Arc::new(block_survival(tables).map_err(|e| error(e.to_string()))?),
            },
            LevelGeneratorSettings {
                stem: noise_stem,
                seed,
                generate_structures,
                flat_bedrock: paper.generate_flat_bedrock,
                end_void_rings: settings.generation.end_void_rings,
                spigot_seeds,
                min_section_y: min_y >> 4,
                section_count: (height / 16) as usize,
                factory: ContainerFactory::new(tables.blocks.state_count(), biome_count, plains),
            },
        )
        .map_err(|e| match e {
            GeneratorError::FeatureCycle { cycle, sources } => {
                error(cycle.message(|i| biomes.name(sources[i]).unwrap_or("?").to_owned()))
            }
            e => error(e.to_string()),
        })?;
        Ok(Some(Arc::new(generator)))
    }

    /// `SavedDataStorage.saveAndJoin` for what changed: the overrides, the border and the clocks.
    fn save_data(&mut self, storage: &LevelStorageAccess) {
        let write = |id: &str, tag: Compound| {
            if let Err(e) = storage.write_saved_data(&self.dimension, id, tag) {
                error!("Could not save data to {id}.dat: {e}");
            }
        };
        if self.overrides.take_dirty() {
            write("paper:level_overrides", self.overrides.to_tag());
        }
        if self.border.take_dirty() {
            write("world_border", self.border.to_settings().to_tag());
        }
        if self.clocks.take_dirty() {
            write("world_clocks", self.clocks.to_tag());
        }
    }

    /// The workers' answers: loaded chunks go to their holders (a proto chunk for a status before
    /// `full`; an unreadable chunk is logged and generated anew, as Moonrise does), finished
    /// generation steps to theirs; failed writes are logged.
    pub fn tick_io(&mut self) {
        while let Ok(event) = self.io_events.try_recv() {
            let (pos, result) = match event {
                ChunkIoEvent::Loaded { pos, result } => (pos, result),
                ChunkIoEvent::Saved { pos, result } => {
                    if let Err(e) = result {
                        error!(
                            "Failed to save chunk data ({},{}) in {}: {e}",
                            pos.x, pos.z, self.dimension
                        );
                    }
                    continue;
                }
                ChunkIoEvent::Flushed { result, .. } => {
                    if let Err(e) = result {
                        error!("Exception when flushing regions in {}: {e}", self.dimension);
                    }
                    continue;
                }
            };
            let chunk = match result {
                Ok(Some(data)) if data.status == ChunkStatus::Full => {
                    Some(LoadedChunk::Level(Box::new(LevelChunk::from_data(
                        *data,
                        self.section_count,
                        &self.tables.factory,
                        &*self.heightmaps,
                    ))))
                }
                Ok(Some(data)) => {
                    let storage = self.chunks.storage();
                    match proto_from_data(*data, storage.shape(), storage.structures()) {
                        Ok((chunk, extras)) => {
                            Some(LoadedChunk::Proto(Box::new(chunk), Box::new(extras)))
                        }
                        Err(e) => {
                            error!(
                                "Failed to parse chunk data for task: [{}, {}] in {}, chunk data will be lost: {e}",
                                pos.x, pos.z, self.dimension
                            );
                            None
                        }
                    }
                }
                Ok(None) => None,
                Err(e) => {
                    error!(
                        "Failed to load chunk data for task: [{}, {}] in {}, chunk data will be lost: {e}",
                        pos.x, pos.z, self.dimension
                    );
                    None
                }
            };
            self.chunks.chunk_loaded(pos, chunk);
        }
        while let Ok(done) = self.gen_events.try_recv() {
            self.chunks.generation_done(done);
        }
        for done in std::mem::take(&mut self.chunks.storage_mut().refused) {
            self.chunks.generation_done(done);
        }
    }

    /// `ServerChunkCache.tick`'s chunk part before the players' loaders: expired tickets
    /// (`purgeStaleTickets`, which processes the level updates), then `runDistanceManagerUpdates`.
    pub fn tick_tickets(&mut self) {
        self.chunks.tick();
        self.chunks.process_ticket_updates();
    }

    /// `ServerLevel.tick`'s scheduled ticks, while the game runs normally and the level is not a
    /// debug world: first the due block ticks (Paper's `environment.max-block-ticks`), then the
    /// due fluid ticks (`max-fluid-ticks`), each run as `tickBlock` / `tickFluid` do; what they
    /// schedule waits for the next tick.
    ///
    /// Stand-in for the block ticks: no block's `tick` is ported yet, so a due block tick is not
    /// run but put back into its chunk's container unchanged (same trigger time, priority and
    /// sub-tick order) once the block ticks are done. It is collected again every tick (counting
    /// against the limit), is saved with the chunk as vanilla saves a due tick (a negative delay,
    /// due again as the chunk loads) and is dropped only where vanilla drops it (a tick of the
    /// same block at the same position scheduled meanwhile replaces it, the chunk's container
    /// removed at unload after the save).
    pub fn tick_scheduled_ticks(&mut self) {
        let time = self.overrides.game_time;
        self.chunks.collect_block_ticks(time, MAX_BLOCK_TICKS);
        let mut deferred = Vec::new();
        while let Some(tick) = self.chunks.ticks.block.pop_due_tick() {
            if !self.tick_block(tick.pos, tick.ty) {
                deferred.push(tick);
            }
        }
        self.chunks.ticks.block.cleanup_after_tick();
        for tick in deferred {
            self.chunks.ticks.block.schedule(tick);
        }
        self.chunks.collect_fluid_ticks(time, MAX_FLUID_TICKS);
        while let Some((pos, id)) = self.chunks.ticks.fluid.pop_due() {
            fluid::tick_fluid(self, pos, id);
        }
        self.chunks.ticks.fluid.cleanup_after_tick();
    }

    /// `ServerLevel.tickBlock`: false when the block's `tick` is not ported (every block's yet).
    fn tick_block(&mut self, _pos: BlockPos, _block: BlockId) -> bool {
        false
    }

    /// The level events since the last call, for the players near them.
    pub fn take_level_events(&mut self) -> Vec<LevelEvent> {
        std::mem::take(&mut self.events)
    }

    /// `ServerChunkCache.broadcastChangedChunks`: for each chunk holder with changed blocks whose
    /// chunk can be sent (`getChunkToSend`), its `block_update` / `section_blocks_update` packets
    /// (`ChunkHolder.broadcastChanges`), for the players the chunk was sent to. Block entities
    /// and light are not ported (no packets for them).
    pub fn broadcast_changes(&mut self) -> Vec<(ChunkPos, Vec<Vec<u8>>)> {
        let chunks = &self.chunks;
        self.changes.retain(|pos| chunks.chunk(pos).is_some());
        let mut out = Vec::new();
        for pos in self.changes.take_queued() {
            let Some(chunk) = self.chunks.chunk_to_send(pos) else {
                continue;
            };
            let Some(sections) = self.changes.take(pos) else {
                continue;
            };
            let min_section = chunk.min_section_y();
            let packets = sections
                .into_iter()
                .map(|(index, changed)| {
                    let section = SectionPos::new(pos.x, min_section + index as i32, pos.z);
                    let state = |rel: u16| chunk.block_state(section.relative_to_block(rel)).0;
                    if let [only] = changed[..] {
                        let at = section.relative_to_block(only);
                        crate::network::play::clientbound::block_update(at, state(only))
                    } else {
                        let changes: Vec<(u16, u32)> =
                            changed.iter().map(|&rel| (rel, state(rel))).collect();
                        crate::network::play::clientbound::section_blocks_update(section, &changes)
                    }
                })
                .collect();
            out.push((pos, packets));
        }
        out
    }

    /// `ChunkMap.tick`: unloads, then autosave.
    pub fn tick_unloads(&mut self, game_time: i64) {
        self.chunks.process_unloads(game_time);
        self.chunks.auto_save(game_time);
    }

    /// `ChunkHolderManager.close(save, halt)` (see [`World::close`]).
    fn close(mut self, game_time: i64) {
        let world = std::mem::take(&mut self.world_name);
        let world = world.as_str();
        info!("Waiting 60s for chunk system to halt for world '{world}'");
        // The running generation steps finish and their chunks come back; queued ones are
        // dropped (their chunks stay as they were).
        if let Some(pool) = &mut self.chunks.storage_mut().pool {
            pool.halt();
        }
        while let Ok(done) = self.gen_events.try_recv() {
            self.chunks.generation_done(done);
        }
        info!("Halted chunk system for world '{world}'");
        let generated = self.chunks.generated;
        if generated.steps > 0 {
            info!(
                "Generated {} chunks in world '{world}' ({} steps, {:.2} ms of worker time per chunk)",
                generated.chunks,
                generated.steps,
                generated.nanos as f64 / 1e6 / generated.chunks.max(1) as f64
            );
        }
        self.chunks.save_all(true, true, Some(world), game_time);
        info!("Waiting 60s for chunk I/O to halt for world '{world}'");
        self.chunks.into_storage().io.shutdown();
        info!("Halted I/O scheduler for world '{world}'");
        // The answers to the last writes: their failures are logged.
        while let Ok(event) = self.io_events.try_recv() {
            if let ChunkIoEvent::Saved {
                pos,
                result: Err(e),
            } = event
            {
                error!(
                    "Failed to save chunk data ({},{}) in world '{world}': {e}",
                    pos.x, pos.z
                );
            }
        }
    }

    pub fn is_loading(&self, pos: ChunkPos) -> bool {
        self.chunks.is_loading(pos)
    }

    pub fn chunk(&self, pos: ChunkPos) -> Option<&LevelChunk> {
        self.chunks.chunk(pos)
    }

    /// `ClientboundLevelChunkWithLightPacket` for a loaded chunk, the light from the saved
    /// Starlight data. Block entities are not ported: none are sent.
    pub fn chunk_packet(&self, pos: ChunkPos) -> Option<Vec<u8>> {
        let chunk = self.chunk(pos)?;
        let first = chunk.min_section_y() - 1;
        let lit = chunk.light_correct;
        let layer = |sky| -> Vec<LightSection<'_>> {
            (first..first + self.section_count as i32 + 2)
                .map(|y| match lit {
                    true => {
                        let (state, data) = chunk.saved_light(y, sky);
                        LightSection::from_starlight(state, data)
                    }
                    false => LightSection::Absent,
                })
                .collect()
        };
        let heightmaps = Heightmap::ALL.map(|kind| chunk.heightmap_words(kind));
        Some(level_chunk_with_light(
            pos,
            &heightmaps,
            chunk.sections(),
            &[],
            lit.then(|| layer(true)).as_deref(),
            &layer(false),
        ))
    }

    /// The column's top for a spawn (`getLevelRespawnPos` reduced to the heightmap: the block
    /// above `MOTION_BLOCKING`), when its chunk is loaded.
    pub fn spawn_height(&self, x: i32, z: i32) -> Option<i32> {
        let chunk = self.chunk(ChunkPos::new(x >> 4, z >> 4))?;
        let top = chunk.height_at(Heightmap::MotionBlocking, x & 15, z & 15)?;
        (top >= chunk.min_y()).then_some(top + 1)
    }

    /// How many chunk holders there are (for tests and logs).
    pub fn held_count(&self) -> usize {
        self.chunks.holder_count()
    }

    /// `getChunk(x, z)` from the tick thread (`ServerChunkCache.syncLoad`): a `CHUNK_LOAD` ticket
    /// at `FULL` until the chunk is full, the workers' answers taken meanwhile, then the ticket
    /// removed. `false` when the chunk cannot be made full (its generation failed).
    pub fn load_full_blocking(&mut self, pos: ChunkPos) -> bool {
        let id = self.chunks.next_ticket_id();
        let level = super::chunk_system::ticket::FULL_LEVEL;
        self.chunks
            .add_ticket(TicketType::ChunkLoad, pos, level, id);
        self.chunks.process_ticket_updates();
        let loaded = loop {
            self.tick_io();
            self.chunks.process_ticket_updates();
            if self.chunks.full_chunk(pos).is_some() {
                break true;
            }
            if !self.chunks.is_loading(pos) {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        self.chunks
            .remove_ticket(TicketType::ChunkLoad, pos, level, id);
        loaded
    }

    /// `MinecraftServer.setInitialSpawn` for this level (Paper's `initWorld` runs it per level
    /// that is not `initialized`): the generator's origin chunk at the spawn height, then the
    /// first chunk of the 11 x 11 spiral around it where `PlayerSpawnFinder.getSpawnPosInChunk`
    /// finds a column; the level is then `initialized`. No bonus chest (the option is not read).
    pub fn set_initial_spawn(&mut self) {
        let Some(generator) = self.generator.clone() else {
            return;
        };
        let start = std::time::Instant::now();
        let origin = generator.origin();
        let min_y = self.min_section_y << 4;
        let mut height = generator.spawn_height();
        let (block_x, block_z) = (origin.x * 16, origin.z * 16);
        if height < min_y && self.load_full_blocking(origin) {
            height = self
                .chunks
                .chunk(origin)
                .and_then(|c| c.height_at(Heightmap::WorldSurface, 8, 8))
                .map_or(min_y, |h| h + 1);
        }
        let mut spawn = BlockPos::new(block_x + 8, height, block_z + 8);
        let (mut x, mut z, mut dx, mut dz) = (0i32, 0i32, 0i32, -1i32);
        for _ in 0..11 * 11 {
            if (-5..=5).contains(&x) && (-5..=5).contains(&z) {
                let pos = ChunkPos::new(origin.x + x, origin.z + z);
                if let Some(found) = self.spawn_pos_in_chunk(pos) {
                    spawn = found;
                    break;
                }
            }
            if x == z || (x < 0 && x == -z) || (x > 0 && x == 1 - z) {
                (dx, dz) = (-dz, dx);
            }
            x += dx;
            z += dz;
        }
        self.overrides.set_respawn(Respawn {
            pos: spawn,
            yaw: 0.0,
            pitch: 0.0,
        });
        self.overrides.set_initialized(true);
        info!(
            "Set the spawn of {} to {}, {}, {} ({:.0} ms)",
            self.dimension,
            spawn.x,
            spawn.y,
            spawn.z,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }

    /// `PlayerSpawnFinder.getSpawnPosInChunk`: the first column, x then z, with a spawn block.
    fn spawn_pos_in_chunk(&mut self, pos: ChunkPos) -> Option<BlockPos> {
        if !self.load_full_blocking(pos) {
            return None;
        }
        let spawn_height = self.generator.as_ref().map_or(64, |g| g.spawn_height());
        let chunk = self.chunks.chunk(pos)?;
        let min_y = chunk.min_y();
        let data = &*self.block_data;
        let full_top = |state: BlockState| {
            data.collision_shape(state)
                .is_some_and(|shape| is_face_full(shape, Direction::Up))
        };
        let fluid = |state: BlockState| data.section_flags(state).has(StateFlags::HAS_FLUID);
        for bx in 0..16 {
            for bz in 0..16 {
                let height = |kind| chunk.height_at(kind, bx, bz).unwrap_or(min_y - 1);
                let top = if self.has_ceiling {
                    spawn_height
                } else {
                    height(Heightmap::MotionBlocking)
                };
                let (x, z) = (pos.x * 16 + bx, pos.z * 16 + bz);
                let found = respawn_pos_in_column(
                    [
                        top,
                        height(Heightmap::WorldSurface),
                        height(Heightmap::OceanFloor),
                        min_y,
                    ],
                    |y| chunk.block_state(BlockPos::new(x, y, z)),
                    fluid,
                    full_top,
                    (x, z),
                );
                if found.is_some() {
                    return found;
                }
            }
        }
        None
    }
}

impl BlockLevel for Level {
    fn rules(&self) -> &BlockRules {
        &self.rules
    }
    fn min_y(&self) -> i32 {
        self.min_section_y << 4
    }
    fn height(&self) -> i32 {
        self.section_count as i32 * 16
    }
    fn loaded_state(&self, pos: BlockPos) -> Option<BlockState> {
        Some(self.chunks.chunk(pos.chunk())?.block_state(pos))
    }
    fn write_state(&mut self, pos: BlockPos, state: BlockState) -> Option<BlockState> {
        // Only a real change touches the chunk (and drops its proto view).
        if self.chunks.chunk(pos.chunk())?.block_state(pos) == state {
            return None;
        }
        let chunk = self.chunks.chunk_mut(pos.chunk())?;
        chunk.set_block_state(pos, state, &*self.block_data, &*self.heightmaps)
    }
    fn sends_updates(&self, pos: BlockPos) -> bool {
        self.chunks.full_status(pos.chunk()) >= FullChunkStatus::Full
    }
    fn block_changed(&mut self, pos: BlockPos) {
        // `ChunkHolder.blockChanged`: only while the chunk can be sent; each player the chunk
        // was sent to gets the change (none: nobody gets it, as Paper skips it then).
        let chunk = pos.chunk();
        if self.chunks.chunk_to_send(chunk).is_none() {
            return;
        }
        let index = (pos.y >> 4) - self.min_section_y;
        if let Ok(index) = usize::try_from(index)
            && index < self.section_count
        {
            self.changes.changed(chunk, index, pos.section_relative());
        }
    }
    fn level_event(&mut self, event: i32, pos: BlockPos, data: i32) {
        self.events.push(LevelEvent { event, pos, data });
    }
    fn schedule_block_tick(&mut self, pos: BlockPos, block: BlockId, delay: i32) {
        let time = self.overrides.game_time;
        let ticks = &mut self.chunks.ticks;
        let tick = ticks.create_block_tick(time, pos, block, delay, TickPriority::Normal);
        ticks.block.schedule(tick);
    }
    fn schedule_fluid_tick(&mut self, pos: BlockPos, fluid: u8, delay: i32) {
        let time = self.overrides.game_time;
        let ticks = &mut self.chunks.ticks;
        let tick = ticks.create_fluid_tick(time, pos, fluid, delay, TickPriority::Normal);
        ticks.fluid.schedule(tick);
    }
    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.random
    }
    fn updater(&mut self) -> &mut NeighborUpdater {
        &mut self.updater
    }
    fn fluid_settings(&self) -> FluidSettings {
        self.fluid_settings
    }
}

/// `Block.isFaceFull(shape, direction)`: the face's shape is the whole block face
/// (`!joinIsNotEmpty(block(), faceShape, NOT_SAME)`).
fn is_face_full(shape: &VoxelShape, direction: Direction) -> bool {
    shape
        .face_shape(direction)
        .and_then(|face| VoxelShape::block().join_is_not_empty(&face, ShapeOp::XOR))
        .is_ok_and(|differs| !differs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stem(kind: &str, generator: &str, settings: Option<&str>) -> Tag {
        let mut g = Compound::new();
        g.insert("type".into(), Tag::String(generator.into()));
        if let Some(s) = settings {
            g.insert("settings".into(), Tag::String(s.into()));
        }
        let mut s = Compound::new();
        s.insert("type".into(), Tag::String(kind.into()));
        s.insert("generator".into(), Tag::Compound(g));
        Tag::Compound(s)
    }

    #[test]
    fn stems_come_in_keys_in_order_with_their_generator_facts() {
        let mut d = Compound::new();
        // The compound sorts by key: the end before the nether, custom stems after the builtin ones.
        d.insert(
            "minecraft:overworld".into(),
            stem(
                "minecraft:overworld",
                "minecraft:noise",
                Some("minecraft:overworld"),
            ),
        );
        d.insert(
            "minecraft:the_end".into(),
            stem(
                "minecraft:the_end",
                "minecraft:noise",
                Some("minecraft:end"),
            ),
        );
        d.insert(
            "minecraft:the_nether".into(),
            stem(
                "minecraft:the_nether",
                "minecraft:noise",
                Some("minecraft:nether"),
            ),
        );
        d.insert(
            "mod:flat".into(),
            stem("minecraft:overworld", "minecraft:flat", None),
        );
        let stems = level_stems(Some(&d)).unwrap();
        let keys: Vec<_> = stems.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, [OVERWORLD, NETHER, END, "mod:flat"]);
        assert_eq!(
            stems.iter().map(Stem::sea_level).collect::<Vec<_>>(),
            [63, 32, 0, -63]
        );
        assert_eq!(stems[3].dimension_type, "minecraft:overworld");
        let names: Vec<_> = stems.iter().map(|s| s.world_name("world")).collect();
        assert_eq!(
            names,
            ["world", "world_nether", "world_the_end", "world_mod_flat"]
        );
    }

    #[test]
    fn a_world_without_settings_has_the_vanilla_stems_and_one_without_an_overworld_fails() {
        let keys: Vec<_> = level_stems(None)
            .unwrap()
            .into_iter()
            .map(|s| s.key)
            .collect();
        assert_eq!(keys, [OVERWORLD, NETHER, END]);
        let mut d = Compound::new();
        d.insert(
            "minecraft:the_end".into(),
            stem("minecraft:the_end", "minecraft:noise", None),
        );
        assert!(matches!(
            level_stems(Some(&d)),
            Err(WorldError::NoOverworld)
        ));
    }

    fn settings(allow_nether: bool, allow_end: bool) -> WorldSettings<'static> {
        WorldSettings {
            level_name: "world",
            compression: Compression::Gzip,
            sync_chunk_writes: false,
            new_world: (GameType::Survival, Difficulty::Normal, false),
            paper_world: PaperWorld::Fixed(PaperWorldSettings::default()),
            allow_nether,
            allow_end,
            generation: GenerationSettings::default(),
        }
    }

    #[test]
    fn a_disabled_nether_or_end_is_not_a_level() {
        let s = settings(false, true);
        assert!(s.enabled(OVERWORLD) && s.enabled(END) && !s.enabled(NETHER));
        let s = settings(true, false);
        assert!(s.enabled(NETHER) && !s.enabled(END) && s.enabled("mod:flat"));
    }
}
