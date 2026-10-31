//! The world the tick thread owns: the open world folder (`LevelStorageAccess`, `level.dat`, the
//! player files) and its levels, one [`Level`] per level stem of `WorldGenSettings` (the
//! overworld, the nether and the end, each with its own [`ChunkIo`] over
//! `dimensions/<namespace>/<path>/region`, its own chunk holders and saved data), in
//! `WorldDimensions.keysInOrder`'s order: the order the server ticks, saves and stops them in
//! (`MinecraftServer.levels` is a linked map; Leaf's parallel world ticking is off by default and
//! not ported, so the levels tick one after the other). Switching a player between levels
//! (portals, `/execute in`) is not ported.
//!
//! **Chunks.** A [`Level`] holds its chunks in Moonrise's chunk holders (`server::chunk_system`),
//! owned by the tick thread; the region I/O workers read and write them and hand the answers back
//! as [`ChunkIoEvent`]s, which [`Level::tick_io`] drains at the start of each tick. Chunks are saved
//! when unloaded and by autosave when changed, and all of them on stop. World generation is not
//! ported: a chunk the region files do not have stays missing and is never sent (logged once per
//! level); the holders keep it as waiting for the generator.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use tracing::{debug, error, info, warn};

use super::chunk_system::{ChunkHolderManager, ChunkSettings, ChunkStorage};
use super::player_data::PlayerDataStorage;
use crate::block::BlockStates;
use crate::block::flags::BlockStateData;
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, Tag};
use crate::network::play::chunk::{LightSection, level_chunk_with_light};
use crate::registry::Registries;
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{BiomeIds, ChunkData, ContainerFactory};
use crate::world::chunk_io::{ChunkIo, ChunkIoEvent, CodecTables};
use crate::world::level_chunk::{HeightmapTable, LevelChunk};
use crate::world::level_storage::{
    Difficulty, GameType, LevelData, LevelStorageAccess, LevelStorageError, WeatherData,
};
use crate::world::storage::compression::Compression;

pub const OVERWORLD: &str = "minecraft:overworld";
pub const NETHER: &str = "minecraft:the_nether";
pub const END: &str = "minecraft:the_end";

/// A level stem of the world generation settings, as far as a level needs it before generation
/// is ported.
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
        }
    }

    /// `ServerLevel.getSeaLevel`, which is the generator's: flat worlds -63, else the noise
    /// settings' (the noise settings are not read yet: the three vanilla ones' values).
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
}

/// The read-only tables a world is opened with (startup's, shared with the I/O workers).
pub struct WorldTables {
    pub blocks: Arc<BlockStates>,
    pub block_data: Arc<BlockStateData>,
    pub registries: Arc<Registries>,
}

/// How the world is opened (`server.properties`).
pub struct WorldSettings<'a> {
    pub level_name: &'a str,
    /// `region-file-compression`, already parsed.
    pub compression: Compression,
    pub sync_chunk_writes: bool,
    /// For a world without `level.dat`: `gamemode`, `difficulty`, `hardcore`.
    pub new_world: (GameType, Difficulty, bool),
    /// Paper's `chunks` world settings.
    pub chunks: ChunkSettings,
}

pub struct World {
    pub storage: LevelStorageAccess,
    /// `level-name` (Bukkit's world name in the chunk system's logs).
    pub name: String,
    pub data: LevelData,
    /// Whether `level.dat` was there: a new world is not written (generation is not ported).
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
        let has_level_data = storage.has_world_data();
        let data = if has_level_data {
            let data = storage.read_level_data()?;
            data.check_compatible()?;
            data
        } else {
            // Leaf creates and generates a new world here; generation is not ported, so the
            // world stays empty and nothing is written: the folder stays a new world.
            warn!(
                "No level.dat in \"{}\": Leaf would create a new world, but world generation is not \
                 ported, so this world has no chunks (none are sent) and no level.dat is written; player files still are",
                settings.level_name
            );
            let (game_type, difficulty, hardcore) = settings.new_world;
            LevelData {
                game_type,
                difficulty,
                hardcore,
                ..LevelData::parse(&Default::default())
            }
        };
        let mut overworld_data = Some(storage.read_dimension_data(OVERWORLD)?);
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
        let players = PlayerDataStorage::new(storage.dir())?;
        let mut levels = Vec::with_capacity(stems.len());
        for stem in &stems {
            // Each level reads its own dimension's saved data (`loadWorldGenSettings`,
            // weather...); one without a folder has the overworld's seed and a clear sky.
            let data = match overworld_data.take_if(|_| stem.key == OVERWORLD) {
                Some(data) => data,
                None => storage.read_dimension_data(&stem.key)?,
            };
            let seed = data.world_gen.as_ref().map_or(overworld_seed, |w| w.seed);
            let region = storage.region_path(&stem.key, "region");
            levels.push(Level::open(
                stem,
                stem.world_name(settings.level_name),
                seed,
                data.weather.unwrap_or(CLEAR_WEATHER),
                &region,
                settings,
                tables,
            )?);
        }
        Ok(Self {
            storage,
            name: settings.level_name.to_owned(),
            data,
            has_level_data,
            players,
            levels,
        })
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

    /// `saveGlobalData`: `level.dat` (not for a world this server did not find one in).
    pub fn save_level_data(&self) {
        if self.has_level_data
            && let Err(e) = self.storage.save_level_data(&self.data)
        {
            error!("Failed to save level {}: {e}", self.storage.dir().display());
        }
    }

    /// `saveAllChunks(silent, flush, force)` without closing: `level.dat`, then every level's
    /// unsaved chunks (`ChunkMap.saveAllChunks`).
    pub fn save_all_chunks(&mut self, silent: bool, flush: bool) {
        self.save_level_data();
        let time = self.data.game_time;
        for level in &mut self.levels {
            if !silent {
                info!(
                    "Saving chunks for level 'ServerLevel[{}]'/{}",
                    self.name, level.dimension
                );
            }
            level.chunks.save_all(flush, false, None, time);
        }
    }

    /// The end of `stopServer`: `saveAllChunks(false, true, false, true)` (`level.dat`, then each
    /// level's `ChunkHolderManager.close`: every chunk saved, the regions flushed, the I/O workers
    /// stopped once their queued requests ran), then the wait for the region I/O.
    pub fn close(self) {
        self.save_level_data();
        let time = self.data.game_time;
        for level in self.levels {
            info!(
                "Saving chunks for level 'ServerLevel[{}]'/{}",
                self.name, level.dimension
            );
            level.close(time);
        }
        info!("Waiting for all RegionFile I/O tasks to complete...");
        info!("All RegionFile I/O tasks to complete");
    }
}

/// The region workers are the chunk holders' storage: requests are messages, answers come back
/// through [`Level::tick_io`].
impl ChunkStorage for ChunkIo {
    fn load(&mut self, pos: ChunkPos) {
        ChunkIo::load(self, pos);
    }
    fn save(&mut self, pos: ChunkPos, chunk: Box<ChunkData>) {
        ChunkIo::save(self, pos, Some(chunk));
    }
    fn flush(&mut self) {
        ChunkIo::flush(self);
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
    /// Its id in `minecraft:dimension_type`.
    pub dimension_type: i32,
    pub min_section_y: i32,
    pub section_count: usize,
    /// The chunk holders, over the region I/O workers.
    pub chunks: ChunkHolderManager<ChunkIo>,
    io_events: Receiver<ChunkIoEvent>,
    tables: Arc<CodecTables<BlockStateData>>,
    heightmaps: HeightmapTable,
    missing_logged: bool,
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

impl Level {
    fn open(
        stem: &Stem,
        world_name: String,
        seed: i64,
        weather: WeatherData,
        region: &Path,
        settings: &WorldSettings<'_>,
        tables: &WorldTables,
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
        let mut biomes: Vec<&str> = tables
            .registries
            .synced("minecraft:worldgen/biome")
            .map(|b| b.entries.iter().map(|e| e.id.as_str()).collect())
            .unwrap_or_default();
        if biomes.is_empty() {
            biomes.push("minecraft:plains");
        }
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
        let heightmaps = HeightmapTable::vanilla(
            &tables.blocks,
            &tables.block_data,
            &block_tag(tables, "minecraft:blocks_motion_in_heightmap"),
            &block_tag(tables, "minecraft:blocks_motion_in_heightmap_no_leaves"),
        );
        // Moonrise sizes its I/O pool from the processors; regions spread over the workers.
        let workers = std::thread::available_parallelism().map_or(1, |n| (n.get() / 4).max(1));
        let (io, io_events) = ChunkIo::spawn(
            region.to_path_buf(),
            settings.compression,
            settings.sync_chunk_writes,
            codec.clone(),
            workers,
        )?;
        Ok(Self {
            dimension: stem.key.clone(),
            world_name,
            seed,
            flat: stem.flat,
            sea_level: stem.sea_level(),
            weather,
            dimension_type: dimension_type as i32,
            min_section_y: min_y >> 4,
            section_count: (height / 16) as usize,
            chunks: ChunkHolderManager::new(io, settings.chunks),
            io_events,
            tables: codec,
            heightmaps,
            missing_logged: false,
        })
    }

    /// The I/O workers' answers: loaded chunks go to their holders; failed writes are logged.
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
                Ok(Some(data)) => Some(LevelChunk::from_data(
                    *data,
                    self.section_count,
                    &self.tables.factory,
                    &self.heightmaps,
                )),
                Ok(None) => {
                    if !std::mem::replace(&mut self.missing_logged, true) {
                        warn!(
                            "Chunk [{}, {}] of {} is not on disk, and world generation is not ported: \
                             it is not sent (further missing chunks are logged at debug level)",
                            pos.x, pos.z, self.dimension
                        );
                    } else {
                        debug!(
                            "Chunk [{}, {}] of {} is not on disk",
                            pos.x, pos.z, self.dimension
                        );
                    }
                    None
                }
                Err(e) => {
                    error!("Couldn't load chunk [{}, {}]: {e}", pos.x, pos.z);
                    None
                }
            };
            self.chunks.chunk_loaded(pos, chunk);
        }
    }

    /// `ServerChunkCache.tick`'s chunk part before the players' loaders: expired tickets
    /// (`purgeStaleTickets`, which processes the level updates), then `runDistanceManagerUpdates`.
    pub fn tick_tickets(&mut self) {
        self.chunks.tick();
        self.chunks.process_ticket_updates();
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
        info!("Halted chunk system for world '{world}'");
        self.chunks.save_all(true, true, Some(world), game_time);
        info!("Waiting 60s for chunk I/O to halt for world '{world}'");
        self.chunks.into_storage().shutdown();
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
}
