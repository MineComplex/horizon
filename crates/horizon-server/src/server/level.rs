//! The world the tick thread owns: the open world folder (`LevelStorageAccess`, `level.dat`, the
//! player files) and its levels, of which only the overworld is opened for now (the nether and the
//! end come with dimension switching: each would be one more [`Level`] with its own [`ChunkIo`]
//! over `dimensions/minecraft/<name>/region`).
//!
//! **Chunks.** A [`Level`] keeps its loaded chunks in a map owned by the tick thread; the region I/O
//! workers read them and hand them back as [`ChunkIoEvent`]s, which [`Level::tick_io`] drains at the
//! start of each tick. Moonrise's ticket levels and chunk holders are not ported: each chunk counts
//! who holds it (players' chunk loaders, joins preparing a spawn) and is dropped when nobody does,
//! a stand-in for the player and spawn tickets. Nothing changes chunks yet, so an unloaded chunk has
//! nothing to save. World generation is not ported either: a chunk the region files do not have
//! stays missing and is never sent (logged once per level).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use tracing::{debug, error, info, warn};

use super::player_data::PlayerDataStorage;
use crate::block::BlockStates;
use crate::block::flags::BlockStateData;
use crate::core::position::ChunkPos;
use crate::network::play::chunk::{LightSection, level_chunk_with_light};
use crate::registry::Registries;
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{BiomeIds, ChunkStatus, ContainerFactory};
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
}

pub struct World {
    pub storage: LevelStorageAccess,
    pub data: LevelData,
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
        let data = if storage.has_world_data() {
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
            data,
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

    /// Stops the I/O workers once their queued requests ran (`ChunkMap.close`).
    pub fn close(self) {
        self.overworld.io.shutdown();
    }
}

enum Slot {
    Loading,
    Loaded(Box<LevelChunk>),
    /// Not on disk (or unreadable): never sent.
    Missing,
}

struct Holder {
    holders: u32,
    slot: Slot,
}

/// One dimension's chunks (see the module docs).
pub struct Level {
    pub dimension: &'static str,
    /// Its id in `minecraft:dimension_type`.
    pub dimension_type: i32,
    pub min_section_y: i32,
    pub section_count: usize,
    io: ChunkIo,
    io_events: Receiver<ChunkIoEvent>,
    chunks: HashMap<ChunkPos, Holder>,
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
            io,
            io_events,
            chunks: HashMap::new(),
            tables: codec,
            heightmaps,
            missing_logged: false,
        })
    }

    /// Takes a hold on the chunk, loading it when it is new.
    pub fn hold(&mut self, pos: ChunkPos) {
        let holder = self.chunks.entry(pos).or_insert_with(|| {
            self.io.load(pos);
            Holder {
                holders: 0,
                slot: Slot::Loading,
            }
        });
        holder.holders += 1;
    }

    /// Gives a hold back; the chunk is dropped when it was the last (nothing to save yet).
    pub fn release(&mut self, pos: ChunkPos) {
        if let Some(holder) = self.chunks.get_mut(&pos) {
            holder.holders -= 1;
            if holder.holders == 0 {
                self.chunks.remove(&pos);
            }
        }
    }

    /// The I/O workers' answers: loaded chunks join the map (if still held).
    pub fn tick_io(&mut self) {
        while let Ok(event) = self.io_events.try_recv() {
            let ChunkIoEvent::Loaded { pos, result } = event else {
                continue;
            };
            let Some(holder) = self.chunks.get_mut(&pos) else {
                continue; // released while loading
            };
            holder.slot = match result {
                Ok(Some(data)) => Slot::Loaded(Box::new(LevelChunk::from_data(
                    *data,
                    self.section_count,
                    &self.tables.factory,
                    &self.heightmaps,
                ))),
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
                    Slot::Missing
                }
                Err(e) => {
                    error!("Couldn't load chunk [{}, {}]: {e}", pos.x, pos.z);
                    Slot::Missing
                }
            };
        }
    }

    pub fn is_loading(&self, pos: ChunkPos) -> bool {
        matches!(
            self.chunks.get(&pos),
            Some(Holder {
                slot: Slot::Loading,
                ..
            })
        )
    }

    pub fn chunk(&self, pos: ChunkPos) -> Option<&LevelChunk> {
        match &self.chunks.get(&pos)?.slot {
            Slot::Loaded(chunk) => Some(chunk),
            _ => None,
        }
    }

    /// Whether the chunk may be sent: a full chunk with its 8 neighbours loaded (Moonrise sends
    /// a chunk once it is FULL, which needs its neighbours generated; without generation, a
    /// neighbour must be on disk).
    pub fn ready_to_send(&self, pos: ChunkPos) -> bool {
        self.chunk(pos)
            .is_some_and(|c| c.status == ChunkStatus::Full)
            && (-1..=1).all(|dx| {
                (-1..=1).all(|dz| self.chunk(ChunkPos::new(pos.x + dx, pos.z + dz)).is_some())
            })
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

    /// How many chunks are held (for tests and logs).
    pub fn held_count(&self) -> usize {
        self.chunks.len()
    }
}
