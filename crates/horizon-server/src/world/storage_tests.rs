//! Level storage, chunk I/O and `LevelChunk`: synthetic round trips, plus a world made by the real
//! vanilla 26.3 server when `HORIZON_SERVER_VANILLA_WORLD` names its level folder (with
//! `HORIZON_SERVER_BLOCKS_JSON`, `HORIZON_SERVER_BLOCK_STATES` and `HORIZON_SERVER_JAR` for the block
//! table, its flags and the heightmap tags). To make one: in an empty scratch folder write
//! `eula=true` to `eula.txt` and a `server.properties` with `level-type=minecraft\:flat`,
//! `level-seed=12345`, `difficulty=hard`, `gamemode=creative`, then run the 26.3 server
//! (`java -jar server.jar --nogui`) and type `forceload add 0 0 15 15`,
//! `setblock 3 -60 3 minecraft:oak_leaves`, `setblock 4 -55 4 minecraft:stone`, `stop`; the level is
//! `<folder>/world`. Never commit it.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use super::chunk::serial::Heightmap;
use super::chunk::{
    BiomeIds, ChunkData, ChunkStatus, ContainerFactory, LevelChunkSection, SectionFlags, StateFlags,
};
use super::chunk_io::{ChunkIo, ChunkIoEvent, CodecTables, RegionStorage};
use super::level_chunk::{HeightmapTable, LevelChunk, heightmaps_after};
use super::level_storage::{
    Difficulty, DirectoryLock, GameType, LevelData, LevelStorageAccess, LevelStorageError,
    WeatherData,
};
use super::storage::compression::Compression;
use crate::block::{BlockState, BlockStates, BlockStatesBuilder};
use crate::core::position::{BlockPos, ChunkPos};
use crate::nbt::{Compound, Tag};

fn table() -> BlockStates {
    let mut t = BlockStatesBuilder::new();
    for name in ["air", "stone", "oak_leaves", "water", "cave_air"] {
        t.add_block(&format!("minecraft:{name}"), &[]).unwrap();
    }
    t.finish()
}
const STONE: BlockState = BlockState(1);
const LEAVES: BlockState = BlockState(2);
const WATER: BlockState = BlockState(3);
const CAVE_AIR: BlockState = BlockState(4);

struct Flags;
impl SectionFlags for Flags {
    fn section_flags(&self, state: BlockState) -> StateFlags {
        StateFlags(match state.0 {
            0 | 4 => StateFlags::AIR,
            3 => StateFlags::HAS_FLUID,
            _ => 0,
        })
    }
}
/// The vanilla predicates over the table: stone and leaves block motion, leaves only with leaves.
fn heightmap_table(t: &BlockStates) -> HeightmapTable {
    HeightmapTable::new(t.state_count(), |s| {
        let (air, fluid) = (matches!(s.0, 0 | 4), s == WATER);
        let (motion, no_leaves) = (s == STONE || s == LEAVES, s == STONE);
        let bits = [
            !air,
            !air,
            motion,
            motion,
            motion || fluid,
            no_leaves || fluid,
        ];
        bits.iter()
            .enumerate()
            .fold(0, |m, (i, &b)| m | u8::from(b) << i)
    })
}
fn factory(t: &BlockStates) -> ContainerFactory {
    ContainerFactory::new(t.state_count(), 1, 0)
}

#[test]
fn session_lock_excludes_a_second_access() {
    let dir = tempfile::tempdir().unwrap();
    let access = LevelStorageAccess::open(dir.path(), "world").unwrap();
    let lock = dir.path().join("world/session.lock");
    assert_eq!(std::fs::read(&lock).unwrap(), "☃".as_bytes());
    assert!(DirectoryLock::is_locked(access.dir()).unwrap());
    let err = LevelStorageAccess::open(dir.path(), "world").unwrap_err();
    assert!(matches!(err, LevelStorageError::AlreadyLocked(_)));
    let message = format!(
        "{}: already locked (possibly by other Minecraft instance?)",
        lock.display()
    );
    assert_eq!(err.to_string(), message);
    drop(access);
    assert!(!DirectoryLock::is_locked(&dir.path().join("world")).unwrap());
    assert!(!DirectoryLock::is_locked(&dir.path().join("other")).unwrap());
    LevelStorageAccess::open(dir.path(), "world").unwrap();
}

#[test]
fn level_dat_defaults_round_trip_and_backup() {
    let defaults = LevelData::parse(&Compound::new());
    assert_eq!(defaults.game_type, GameType::Survival);
    assert_eq!(defaults.difficulty, Difficulty::Normal);
    assert!(defaults.initialized && !defaults.allow_commands);
    assert_eq!(defaults.spawn.dimension, "minecraft:overworld");
    assert_eq!(defaults.enabled_packs, ["vanilla"]);
    assert_eq!(defaults.enabled_features, ["minecraft:vanilla"]);
    assert!(defaults.check_compatible().is_err());
    let tag = defaults.to_tag(7);
    for absent in [
        "DataPacks",
        "enabled_features",
        "removed_features",
        "singleplayer_uuid",
    ] {
        assert!(!tag.contains_key(&absent.into()), "{absent}");
    }

    let dir = tempfile::tempdir().unwrap();
    let access = LevelStorageAccess::open(dir.path(), "w").unwrap();
    assert!(!access.has_world_data());
    let mut data = defaults.clone();
    data.level_name = "w".into();
    data.game_type = GameType::Creative;
    data.difficulty = Difficulty::Hard;
    data.hardcore = true;
    data.game_time = 1234;
    data.spawn.pos = BlockPos::new(5, 70, -3);
    data.spawn.yaw = 90.0;
    data.disabled_packs = vec!["minecart_improvements".into()];
    data.singleplayer_uuid = Some([1, 2, 3, 4]);
    data.bukkit_version = "Leaf/test".into();
    access.save_level_data(&data).unwrap();
    let read = access.read_level_data().unwrap();
    read.check_compatible().unwrap();
    assert_eq!(read.version_history, [5023]);
    assert_eq!(read.data_version, 5023);
    let expected = LevelData {
        data_version: 5023,
        version: read.version.clone(),
        version_history: vec![5023],
        ..data.clone()
    };
    assert_eq!(read, expected);

    // A second save keeps the first as level.dat_old; a broken level.dat falls back to it.
    data.game_time = 99;
    access.save_level_data(&data).unwrap();
    std::fs::write(access.dir().join("level.dat"), b"not nbt").unwrap();
    assert_eq!(access.read_level_data().unwrap().game_time, 1234);
    let names: Vec<String> = std::fs::read_dir(access.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        names.iter().any(|n| n.starts_with("level.dat_corrupted_")),
        "{names:?}"
    );
    assert!(!names.contains(&"level.dat_old".to_owned()));
    assert_eq!(access.read_level_data().unwrap().game_time, 1234);
}

#[test]
fn saved_data_is_per_dimension_with_the_root_as_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let access = LevelStorageAccess::open(dir.path(), "w").unwrap();
    let nether = access.dimension_path("minecraft:the_nether");
    assert_eq!(nether, dir.path().join("w/dimensions/minecraft/the_nether"));
    let weather = WeatherData {
        rain_time: 12,
        raining: true,
        ..Default::default()
    };
    access
        .write_saved_data("minecraft:overworld", "minecraft:weather", weather.to_tag())
        .unwrap();
    assert!(
        access
            .dir()
            .join("dimensions/minecraft/overworld/data/minecraft/weather.dat")
            .is_file()
    );
    let read = access.read_dimension_data("minecraft:overworld").unwrap();
    assert_eq!(read.weather, Some(weather));
    assert_eq!(read.world_gen, None);
    // A vanilla world's root `data/` answers for every dimension without its own file.
    let root = access.dir().join("data/minecraft");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(
        access
            .dir()
            .join("dimensions/minecraft/overworld/data/minecraft/weather.dat"),
        root.join("weather.dat"),
    )
    .unwrap();
    assert_eq!(
        access
            .read_dimension_data("minecraft:the_end")
            .unwrap()
            .weather,
        Some(weather)
    );
}

#[test]
fn region_storage_writes_lz4_and_creates_nothing_on_reads() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("region");
    let mut storage = RegionStorage::new(folder.clone(), Compression::Lz4, false);
    let pos = ChunkPos::new(-1, 33);
    assert_eq!(storage.read(pos).unwrap(), None);
    storage.write(pos, None).unwrap();
    assert!(!folder.exists());
    let Tag::Compound(chunk) = Tag::compound([("Status", Tag::String("minecraft:full".into()))])
    else {
        unreachable!()
    };
    storage.write(pos, Some(chunk.clone())).unwrap();
    let file = std::fs::read(folder.join("r.-1.1.mca")).unwrap();
    let slot = 31 + 32;
    let sector = u32::from_be_bytes(file[slot * 4..slot * 4 + 4].try_into().unwrap()) >> 8;
    assert_eq!(file[sector as usize * 4096 + 4], 4, "LZ4 compression id");
    assert_eq!(storage.read(pos).unwrap(), Some(chunk));
    storage.write(pos, None).unwrap();
    assert_eq!(storage.read(pos).unwrap(), None);
    storage.flush().unwrap();
}

fn chunk_data(t: &BlockStates, pos: ChunkPos, fill: BlockState) -> ChunkData {
    let mut chunk = LevelChunk::empty(pos, -4, 8, &factory(t));
    chunk.prime_heightmaps(heightmaps_after(ChunkStatus::Full), &heightmap_table(t));
    for x in 0..16 {
        chunk.set_block_state(BlockPos::new(x, 3, 0), fill, &Flags, &heightmap_table(t));
    }
    chunk.to_data()
}

#[test]
fn chunk_io_keeps_the_order_of_a_region() {
    let t = Arc::new(table());
    let tables = Arc::new(CodecTables {
        factory: factory(&t),
        blocks: t.clone(),
        flags: Arc::new(Flags),
        biomes: Arc::new(BiomeIds::new(["minecraft:plains"])),
        min_section_y: -4,
        max_section_y: 3,
    });
    let dir = tempfile::tempdir().unwrap();
    let (io, events) = ChunkIo::spawn(
        dir.path().into(),
        Compression::Lz4,
        false,
        tables.clone(),
        3,
    )
    .unwrap();
    let positions: Vec<ChunkPos> = (0..40)
        .map(|i| ChunkPos::new(i * 7 - 100, i * 13 - 200))
        .collect();
    for &pos in &positions {
        io.save(pos, Some(Box::new(chunk_data(&t, pos, LEAVES))));
        io.save(pos, Some(Box::new(chunk_data(&t, pos, STONE))));
        io.load(pos);
        io.save(pos, None);
        io.load(pos);
    }
    io.flush();
    let mut loads: Vec<(ChunkPos, Option<ChunkData>)> = Vec::new();
    let mut flushed = 0;
    while flushed < 3 {
        match events.recv_timeout(Duration::from_secs(30)).unwrap() {
            ChunkIoEvent::Loaded { pos, result } => loads.push((pos, result.unwrap().map(|c| *c))),
            ChunkIoEvent::Saved { result, .. } => result.unwrap(),
            ChunkIoEvent::Flushed { result, .. } => {
                result.unwrap();
                flushed += 1;
            }
        }
    }
    io.shutdown();
    for &pos in &positions {
        let mine: Vec<_> = loads
            .iter()
            .filter(|(p, _)| *p == pos)
            .map(|(_, c)| c)
            .collect();
        assert_eq!(mine.len(), 2);
        let written = mine[0].as_ref().map(|c| tables.codec().write(c));
        assert_eq!(
            written,
            Some(tables.codec().write(&chunk_data(&t, pos, STONE)))
        );
        assert_eq!(mine[1], &None);
    }
}

#[test]
fn heightmaps_prime_and_update_like_vanilla() {
    let t = table();
    let hm = heightmap_table(&t);
    let mut chunk = LevelChunk::empty(ChunkPos::new(2, 3), -4, 8, &factory(&t));
    let all = Heightmap::ALL;
    chunk.prime_heightmaps(&all, &hm);
    assert_eq!(chunk.height_at(Heightmap::WorldSurface, 0, 0), Some(-65));
    let set =
        |c: &mut LevelChunk, x, y, z, s| c.set_block_state(BlockPos::new(x, y, z), s, &Flags, &hm);
    assert_eq!(set(&mut chunk, 1, -60, 1, BlockState::AIR), None);
    assert_eq!(set(&mut chunk, 1, -60, 1, STONE), Some(BlockState::AIR));
    assert_eq!(set(&mut chunk, 1, -60, 1, STONE), None);
    set(&mut chunk, 1, 20, 1, LEAVES);
    set(&mut chunk, 1, 30, 1, WATER);
    set(&mut chunk, 1, 40, 1, CAVE_AIR);
    let heights = |c: &LevelChunk| all.map(|k| c.height_at(k, 1, 1).unwrap());
    // WG maps are not updated by setBlockState.
    assert_eq!(heights(&chunk), [-65, 30, -65, 20, 30, 30]);
    let mut primed = chunk.clone();
    primed.prime_heightmaps(&all, &hm);
    assert_eq!(heights(&primed), [30, 30, 20, 20, 30, 30]);
    // Removing the top block scans down for the next opaque one.
    set(&mut chunk, 1, 30, 1, BlockState::AIR);
    set(&mut chunk, 1, 40, 1, BlockState::AIR);
    assert_eq!(&heights(&chunk)[1..], [20, -65, 20, 20, -60]);
    set(&mut chunk, 1, 20, 1, BlockState::AIR);
    assert_eq!(&heights(&chunk)[1..], [-60, -65, -60, -60, -60]);
    assert_eq!(chunk.block_state(BlockPos::new(17, -60, 33)), STONE);
    assert_eq!(chunk.block_state(BlockPos::new(1, 500, 1)), BlockState::AIR);
}

#[test]
fn level_chunk_round_trips_through_chunk_data() {
    let t = table();
    let hm = heightmap_table(&t);
    let mut data = chunk_data(&t, ChunkPos::new(0, 0), STONE);
    // The stored maps are as vanilla left them; a missing one is primed, a short one re-primed.
    let chunk = LevelChunk::from_data(data.clone(), 8, &factory(&t), &hm);
    assert_eq!(chunk.to_data(), data);
    assert_eq!(chunk.height_at(Heightmap::OceanFloor, 0, 0), Some(3));
    data.heightmaps[Heightmap::OceanFloor as usize] = Some(vec![0; 3]);
    data.heightmaps[Heightmap::WorldSurface as usize] = None;
    data.heightmaps[Heightmap::WorldSurfaceWg as usize] = Some(vec![0; 37]);
    let chunk = LevelChunk::from_data(data, 8, &factory(&t), &hm);
    assert_eq!(chunk.height_at(Heightmap::OceanFloor, 0, 0), Some(3));
    assert_eq!(chunk.height_at(Heightmap::WorldSurface, 5, 0), Some(3));
    assert_eq!(chunk.height_at(Heightmap::WorldSurfaceWg, 0, 0), None);
    assert!(
        chunk
            .sections()
            .iter()
            .all(|s: &LevelChunkSection| s.biomes().get(0) == 0)
    );
}

/// A block tag of the jar's data pack, with its nested tags.
fn block_tag(
    jar: &mut crate::server::vanilla_jar::VanillaJar,
    t: &BlockStates,
    id: &str,
) -> HashSet<crate::block::BlockId> {
    let path = id.trim_start_matches("minecraft:");
    let json = jar
        .read(&format!("data/minecraft/tags/block/{path}.json"))
        .unwrap()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let mut out = HashSet::new();
    for value in json["values"].as_array().unwrap() {
        let entry = value.as_str().or_else(|| value["id"].as_str()).unwrap();
        match entry.strip_prefix('#') {
            Some(tag) => out.extend(block_tag(jar, t, tag)),
            None => out.extend(t.block_by_name(entry)),
        }
    }
    out
}

fn vanilla_world_env() -> Option<[std::path::PathBuf; 4]> {
    let names = [
        "HORIZON_SERVER_VANILLA_WORLD",
        "HORIZON_SERVER_BLOCKS_JSON",
        "HORIZON_SERVER_BLOCK_STATES",
        "HORIZON_SERVER_JAR",
    ];
    let values: Vec<_> = names.iter().map(std::env::var_os).collect();
    if values.iter().any(Option::is_none) {
        eprintln!("{names:?} are not all set: the vanilla world is not read");
        return None;
    }
    Some(std::array::from_fn(|i| values[i].clone().unwrap().into()))
}

#[test]
fn reads_a_world_made_by_vanilla() {
    let Some([world, blocks_json, block_states, jar]) = vanilla_world_env() else {
        return;
    };
    let t = Arc::new(BlockStates::load(&blocks_json).unwrap());
    let data = Arc::new(crate::block::BlockStateData::load(&block_states, &t).unwrap());
    let mut jar = crate::server::vanilla_jar::VanillaJar::open(&jar).unwrap();
    let motion = block_tag(&mut jar, &t, "blocks_motion_in_heightmap");
    let no_leaves = block_tag(&mut jar, &t, "blocks_motion_in_heightmap_no_leaves");
    let hm = HeightmapTable::vanilla(&t, &data, &motion, &no_leaves);

    let (base, level_id) = (
        world.parent().unwrap(),
        world.file_name().unwrap().to_str().unwrap(),
    );
    let access = LevelStorageAccess::open(base, level_id).unwrap();
    let level = access.read_level_data().unwrap();
    level.check_compatible().unwrap();
    assert_eq!(
        (level.data_version, level.version.name.as_str()),
        (5023, "26.3")
    );
    assert_eq!(
        (level.game_type, level.difficulty, level.hardcore),
        (GameType::Creative, Difficulty::Hard, false)
    );
    assert_eq!(level.spawn.dimension, "minecraft:overworld");
    assert_eq!(
        LevelData::parse(&level.to_tag(level.version.last_played)),
        LevelData {
            bukkit_version: String::new(),
            ..level.clone()
        }
    );
    let overworld = access.read_dimension_data("minecraft:overworld").unwrap();
    let settings = overworld.world_gen.unwrap();
    assert_eq!(settings.seed, 12345);
    assert!(
        settings
            .dimensions
            .contains_key(&"minecraft:the_nether".into())
    );
    assert!(
        overworld.weather.is_some() && overworld.game_rules.is_some() && overworld.clocks.is_some()
    );

    // The biomes the chunk uses, as a registry (ids are only used in memory here).
    let region = access.region_path("minecraft:overworld", "region");
    let pos = ChunkPos::new(0, 0);
    let raw = RegionStorage::new(region.clone(), Compression::Lz4, false)
        .read(pos)
        .unwrap()
        .unwrap();
    let mut biomes = vec!["minecraft:plains".to_owned()];
    if let Some(Tag::List(sections)) = Tag::Compound(raw).get("sections") {
        for s in sections {
            if let Some(Tag::List(palette)) = s.get("biomes").and_then(|b| b.get("palette")) {
                biomes.extend(palette.iter().filter_map(|p| match p {
                    Tag::String(s) => Some(s.to_string_lossy()),
                    _ => None,
                }));
            }
        }
    }
    biomes.dedup();
    let biomes = Arc::new(BiomeIds::new(biomes));
    let tables = Arc::new(CodecTables {
        factory: ContainerFactory::new(t.state_count(), biomes.len(), 0),
        blocks: t.clone(),
        flags: data.clone(),
        biomes,
        min_section_y: -4,
        max_section_y: 19,
    });
    let (io, events) = ChunkIo::spawn(region, Compression::Lz4, false, tables.clone(), 2).unwrap();
    io.load(pos);
    let ChunkIoEvent::Loaded { result, .. } = events.recv().unwrap() else {
        panic!()
    };
    io.shutdown();
    let read = *result.unwrap().unwrap();
    assert_eq!(read.status, ChunkStatus::Full);
    let chunk = LevelChunk::from_data(read.clone(), 24, &tables.factory, &hm);
    let name = |p| t.block_name(t.block(chunk.block_state(p))).to_owned();
    assert_eq!(name(BlockPos::new(3, -60, 3)), "minecraft:oak_leaves");
    assert_eq!(name(BlockPos::new(4, -55, 4)), "minecraft:stone");
    assert_eq!(name(BlockPos::new(0, -61, 0)), "minecraft:grass_block");
    // Vanilla's stored heightmaps (updated by the two setblocks) are what priming computes.
    let mut primed = chunk.clone();
    primed.prime_heightmaps(heightmaps_after(ChunkStatus::Full), &hm);
    assert_eq!(primed, chunk);
    assert_eq!(chunk.height_at(Heightmap::MotionBlocking, 3, 3), Some(-60));
    assert_eq!(
        chunk.height_at(Heightmap::MotionBlockingNoLeaves, 3, 3),
        Some(-61)
    );
    assert_eq!(chunk.height_at(Heightmap::WorldSurface, 4, 4), Some(-55));
    let codec = tables.codec();
    assert_eq!(codec.write(&chunk.to_data()), codec.write(&read));

    // Written back through a worker and read again: the same chunk.
    let copy = tempfile::tempdir().unwrap();
    let (io, events) = ChunkIo::spawn(
        copy.path().into(),
        Compression::Lz4,
        false,
        tables.clone(),
        1,
    )
    .unwrap();
    io.save(pos, Some(Box::new(chunk.to_data())));
    io.load(pos);
    io.shutdown();
    let events: Vec<_> = events.iter().collect();
    let Some(ChunkIoEvent::Loaded { result, .. }) = events.last() else {
        panic!()
    };
    let again = result.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(tables.codec().write(again), tables.codec().write(&read));
}
