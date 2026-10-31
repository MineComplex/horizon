//! The world the tick thread owns: the open world folder (`LevelStorageAccess`, `level.dat`, the
//! player files) and its levels, of which only the overworld is opened for now (the nether and the
//! end come with dimension switching: each would be one more [`Level`] with its own [`ChunkIo`]
//! over `dimensions/minecraft/<name>/region`).
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

#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    #[error(transparent)]
    Storage(#[from] LevelStorageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("the registries have no {0}")]
    Registry(&'static str),
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
    /// `world_gen_settings`: the seed, and whether the overworld is flat.
    pub seed: i64,
    pub flat: bool,
    pub weather: WeatherData,
    pub players: PlayerDataStorage,
    pub overworld: Level,
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
        let dimension = storage.read_dimension_data(OVERWORLD)?;
        let (seed, flat) = dimension.world_gen.as_ref().map_or((0, false), |w| {
            let generator = w
                .dimensions
                .get(&OVERWORLD.into())
                .and_then(|d| d.get("generator"))
                .and_then(|g| g.get("type"));
            let flat = matches!(generator, Some(crate::nbt::Tag::String(t))
                if t.to_string_lossy() == "minecraft:flat");
            (w.seed, flat)
        });
        let players = PlayerDataStorage::new(storage.dir())?;
        let region = storage.region_path(OVERWORLD, "region");
        let overworld = Level::open(OVERWORLD, &region, settings, tables)?;
        Ok(Self {
            storage,
            name: settings.level_name.to_owned(),
            data,
            has_level_data,
            seed,
            flat,
            weather: dimension.weather.unwrap_or(WeatherData {
                clear_weather_time: 0,
                rain_time: 0,
                thunder_time: 0,
                raining: false,
                thundering: false,
            }),
            players,
            overworld,
        })
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
        if !silent {
            info!(
                "Saving chunks for level 'ServerLevel[{}]'/{}",
                self.name, self.overworld.dimension
            );
        }
        let time = self.data.game_time;
        self.overworld.chunks.save_all(flush, false, None, time);
    }

    /// The end of `stopServer`: `saveAllChunks(false, true, false, true)` (`level.dat`, then each
    /// level's `ChunkHolderManager.close`: every chunk saved, the regions flushed, the I/O workers
    /// stopped once their queued requests ran), then the wait for the region I/O.
    pub fn close(self) {
        self.save_level_data();
        info!(
            "Saving chunks for level 'ServerLevel[{}]'/{}",
            self.name, self.overworld.dimension
        );
        self.overworld.close(&self.name, self.data.game_time);
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
    pub dimension: &'static str,
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
        dimension: &'static str,
        region: &Path,
        settings: &WorldSettings<'_>,
        tables: &WorldTables,
    ) -> Result<Self, WorldError> {
        let types = tables
            .registries
            .synced("minecraft:dimension_type")
            .ok_or(WorldError::Registry("minecraft:dimension_type"))?;
        let (dimension_type, entry) = types
            .entries
            .iter()
            .enumerate()
            .find(|(_, e)| e.id == dimension)
            .ok_or(WorldError::Registry("overworld dimension type"))?;
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
            dimension,
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
    fn close(mut self, world: &str, game_time: i64) {
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
