//! The flat and debug generators against the real 26.3 server.
//!
//! - `flat_generator_matches_java` and `debug_generator_matches_java` read
//!   `LeafFlatDebugOracle.java`'s output (`HORIZON_FLAT_DEBUG_ORACLE`): `generator-settings`
//!   strings decoded and what the pipeline asks of the generators, the decorated chunks and the
//!   debug grid (tools/leaf_server/README.md).
//! - `flat_matches_vanilla_world` generates chunks 0..15 of a flat world a vanilla 26.3 server
//!   made (`HORIZON_SERVER_VANILLA_WORLD`, level type flat, seed 12345, those chunks forceloaded;
//!   see `world/storage_tests.rs`) and compares blocks, biomes, heightmaps and structures with
//!   its region files.
//!
//! ```sh
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_FLAT_DEBUG_ORACLE=flat_debug.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     cargo test -p horizon-server --release fixed_tests -- --ignored --nocapture
//! ```

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use super::GeneratorTables;
use super::fixed::{FixedGenerator, FixedGeneratorSettings, FixedKind, FixedWorker};
use super::flat_settings::{FlatLookup, FlatSettings};
use super::pyramid::Pyramids;
use crate::block::flags::BlockStateData;
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::position::ChunkPos;
use crate::registry::{DataPack, Registries, load_tags, registry_dir};
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{ChunkStatus, ContainerFactory};
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::biome::codec::BiomeIds;
use crate::worldgen::codec::{object, required};
use crate::worldgen::data::WorldgenData;
use crate::worldgen::proto_chunk::{GenerationRegion, ProtoChunk, RegionChunk};
use crate::worldgen::structure::template::StructureTemplates;
use crate::worldgen::terrain::oracle_tests::{env_path, load_real};

/// The real data the generators are made from.
struct Setup {
    data: Arc<WorldgenData>,
    blocks: Arc<BlockStates>,
    flags: Arc<BlockStateData>,
    heightmaps: Arc<HeightmapTable>,
    survival: Arc<crate::block::survival::BlockSurvival>,
    registries: Registries,
}

impl Setup {
    fn load() -> Option<Self> {
        let real = load_real()?;
        let mut jar = VanillaJar::open(env_path("HORIZON_VANILLA_JAR")?.as_ref()).unwrap();
        let pack = DataPack::vanilla(&mut jar).unwrap();
        let packs = std::slice::from_ref(&pack);
        let block_tags = load_tags(packs, &registry_dir("minecraft:block"), |id| {
            real.blocks.block_by_name(id).map(|b| b.0 as i32)
        });
        let tag = |name: &str| -> std::collections::HashSet<BlockId> {
            (block_tags.iter())
                .find(|(t, _)| t == name)
                .map(|(_, ids)| ids.iter().map(|&i| BlockId(i as u32)).collect())
                .unwrap_or_default()
        };
        let heightmaps = HeightmapTable::vanilla(
            &real.blocks,
            &real.state_data,
            &tag("minecraft:blocks_motion_in_heightmap"),
            &tag("minecraft:blocks_motion_in_heightmap_no_leaves"),
        );
        let survival = crate::block::survival::BlockSurvival::new(
            &real.blocks,
            &real.state_data,
            &block_tags,
            &load_tags(packs, &registry_dir("minecraft:fluid"), |id| {
                crate::worldgen::placement::FLUIDS
                    .iter()
                    .position(|f| *f == id)
                    .map(|i| i as i32)
            }),
        )
        .unwrap();
        let registries = Registries::build(
            Vec::new(),
            &[DataPack::vanilla(&mut jar).unwrap(), DataPack::paper()],
        )
        .unwrap();
        let crate::worldgen::terrain::oracle_tests::Real {
            mut data,
            blocks,
            state_data,
        } = real;
        let templates = StructureTemplates::load(&mut jar, None, &blocks, &state_data).unwrap();
        data.set_templates(templates);
        Some(Self {
            data: Arc::new(data),
            blocks: Arc::new(blocks),
            flags: Arc::new(state_data),
            heightmaps: Arc::new(heightmaps),
            survival: Arc::new(survival),
            registries,
        })
    }

    fn biomes(&self) -> BiomeIds<'_> {
        BiomeIds::new(self.registries.synced("minecraft:worldgen/biome").unwrap())
    }

    fn biome_names(&self) -> Vec<String> {
        let registry = self.registries.synced("minecraft:worldgen/biome").unwrap();
        registry.entries.iter().map(|e| e.id.clone()).collect()
    }

    fn decode_flat(&self, json: &str) -> Result<FlatSettings, String> {
        let biomes = self.biomes();
        let lookup = FlatLookup {
            biomes: &biomes,
            blocks: &self.blocks,
            structures: self.data.structures(),
        };
        let value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        FlatSettings::decode(&value, &lookup).map_err(|e| e.to_string())
    }

    fn generator(&self, kind: FixedKind, structures: bool) -> FixedGenerator {
        let biomes = self.biomes();
        let biome = match &kind {
            FixedKind::Flat(settings) => settings.biome,
            FixedKind::Debug => biomes.id("minecraft:plains").unwrap(),
        };
        let biome_count = self.biome_names().len() as u32;
        FixedGenerator::new(
            GeneratorTables {
                data: self.data.clone(),
                blocks: self.blocks.clone(),
                flags: self.flags.clone(),
                heightmaps: self.heightmaps.clone(),
                survival: self.survival.clone(),
            },
            FixedGeneratorSettings {
                biome_is_void: biomes.name(biome) == Some("minecraft:the_void"),
                kind,
                biome,
                seed: 12345,
                generate_structures: structures,
                spigot_seeds: Default::default(),
                min_section_y: -4,
                section_count: 24,
                factory: ContainerFactory::new(
                    self.blocks.state_count(),
                    biome_count,
                    biomes.id("minecraft:plains").unwrap(),
                ),
            },
        )
        .expect("the generator")
    }
}

/// One generation step on the chunk at `pos` over its region.
fn run_step(
    chunks: &mut HashMap<ChunkPos, ProtoChunk>,
    worker: &mut FixedWorker<'_>,
    status: ChunkStatus,
    pos: ChunkPos,
) {
    let step = Pyramids::get().generation.step_to(status);
    let (read, write) = (step.read_radius(), step.block_state_write_radius.max(0));
    let spots: Vec<(i32, i32)> = (-read..=read)
        .flat_map(|dz| (-read..=read).map(move |dx| (dx, dz)))
        .collect();
    let mut owned: Vec<ProtoChunk> = Vec::new();
    for &(dx, dz) in &spots {
        if dx.abs() <= write && dz.abs() <= write {
            let at = ChunkPos::new(pos.x + dx, pos.z + dz);
            owned.push(chunks.remove(&at).expect("a chunk of the region"));
        }
    }
    {
        let mut owned_iter = owned.iter_mut();
        let slots: Vec<RegionChunk<'_>> = spots
            .iter()
            .map(|&(dx, dz)| {
                if dx.abs() <= write && dz.abs() <= write {
                    RegionChunk::Writable(owned_iter.next().unwrap())
                } else {
                    let at = ChunkPos::new(pos.x + dx, pos.z + dz);
                    RegionChunk::ReadOnly(chunks.get(&at).expect("a chunk of the region"))
                }
            })
            .collect();
        let mut region = GenerationRegion::new(pos, read, slots);
        worker.run(status, &mut region).expect("the step");
    }
    for chunk in owned {
        chunks.insert(chunk.pos(), chunk);
    }
}

/// The targets taken through FEATURES with the chunks around them as far as the pyramid reaches
/// (what the scheduler does, one status after the other).
fn generate(generator: &FixedGenerator, targets: &[ChunkPos]) -> HashMap<ChunkPos, ProtoChunk> {
    let features = Pyramids::get().generation.step_to(ChunkStatus::Features);
    let reach = |status| features.accumulated.radius_of(status).unwrap_or(0);
    let widest = reach(ChunkStatus::Empty);
    let mut chunks: HashMap<ChunkPos, ProtoChunk> = HashMap::new();
    for t in targets {
        for dz in -widest..=widest {
            for dx in -widest..=widest {
                let pos = ChunkPos::new(t.x + dx, t.z + dz);
                chunks
                    .entry(pos)
                    .or_insert_with(|| generator.empty_chunk(pos));
            }
        }
    }
    let mut worker = generator.worker();
    for status in [
        ChunkStatus::StructureStarts,
        ChunkStatus::StructureReferences,
        ChunkStatus::Biomes,
        ChunkStatus::Terrain,
        ChunkStatus::Features,
    ] {
        let r = reach(status);
        let mut spots = BTreeSet::new();
        for t in targets {
            for dz in -r..=r {
                for dx in -r..=r {
                    spots.insert((t.x + dx, t.z + dz));
                }
            }
        }
        for (x, z) in spots {
            run_step(&mut chunks, &mut worker, status, ChunkPos::new(x, z));
        }
    }
    chunks
}

/// `id*count` runs, with `-1` for a null layer.
fn runs(words: &[&str]) -> Vec<i64> {
    let mut out = Vec::new();
    for word in words {
        let (id, count) = word.split_once('*').expect("a run");
        let (id, count): (i64, usize) = (id.parse().unwrap(), count.parse().unwrap());
        out.extend(std::iter::repeat_n(id, count));
    }
    out
}

fn run_text(ids: &[i64]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < ids.len() {
        let mut j = i;
        while j < ids.len() && ids[j] == ids[i] {
            j += 1;
        }
        out.push_str(&format!(" {}*{}", ids[i], j - i));
        i = j;
    }
    out
}

#[test]
#[ignore = "needs HORIZON_FLAT_DEBUG_ORACLE and the real 26.3 data (tools/leaf_server/README.md)"]
fn flat_generator_matches_java() {
    let oracle = env_path("HORIZON_FLAT_DEBUG_ORACLE").expect("HORIZON_FLAT_DEBUG_ORACLE");
    let setup = Setup::load().expect("HORIZON_VANILLA_JAR and the block data");
    let text = std::fs::read_to_string(oracle).expect("the oracle file");
    let mut failures: Vec<String> = Vec::new();
    let (mut cases, mut rows, mut chunk_count, mut blocks_seen, mut heights) =
        (0, 0, 0, 0u64, 0u64);
    let mut label = String::new();
    let mut settings: Option<FlatSettings> = None;
    let mut generator: Option<FixedGenerator> = None;
    let mut generated: HashMap<ChunkPos, ProtoChunk> = HashMap::new();
    let mut centre = ChunkPos::new(0, 0);
    let biomes = setup.biomes();
    for line in text.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        match words[0] {
            "flat" => {
                cases += 1;
                label = format!("flat case {}", words[1]);
                settings = None;
                generator = None;
                if words[2] == "error" {
                    let json = line.splitn(4, ' ').nth(3).unwrap();
                    if setup.decode_flat(json).is_ok() {
                        failures.push(format!("{label}: decoded, Java's codec fails"));
                    }
                    continue;
                }
                let json = line.splitn(3, ' ').nth(2).unwrap();
                match setup.decode_flat(json) {
                    Ok(decoded) => {
                        generator = Some(setup.generator(FixedKind::Flat(decoded.clone()), false));
                        settings = Some(decoded);
                    }
                    Err(e) => failures.push(format!("{label}: does not decode: {e}")),
                }
            }
            _ if settings.is_none() => {}
            "layers" => {
                let s = settings.as_ref().unwrap();
                let ours: Vec<String> = s
                    .layers
                    .iter()
                    .map(|(state, height)| format!("{}*{height}", state.0))
                    .collect();
                if ours != words[1..] {
                    failures.push(format!("{label}: layers {ours:?} not {:?}", &words[1..]));
                }
                rows += 1;
            }
            "info" => {
                let s = settings.as_ref().unwrap();
                let overrides = match &s.structure_overrides {
                    None => "none".to_owned(),
                    Some(sets) => sets
                        .iter()
                        .map(|&set| setup.data.structures().set(set).id.clone())
                        .collect::<Vec<_>>()
                        .join(","),
                };
                let ours = format!(
                    "{} {} {} {overrides}",
                    biomes.name(s.biome).unwrap(),
                    u8::from(s.features),
                    u8::from(s.lakes)
                );
                if ours != words[1..].join(" ") {
                    failures.push(format!("{label}: info {ours} not {}", words[1..].join(" ")));
                }
                rows += 1;
            }
            "adjusted" => {
                let g = generator.as_ref().unwrap();
                let ids: Vec<i64> = g
                    .layers()
                    .iter()
                    .map(|l| l.map_or(-1, |s| i64::from(s.0)))
                    .collect();
                if run_text(&ids).trim() != words[1..].join(" ") {
                    failures.push(format!(
                        "{label}: adjusted layers{} not {}",
                        run_text(&ids),
                        words[1..].join(" ")
                    ));
                }
                rows += 1;
            }
            "facts" => {
                let g = generator.as_ref().unwrap();
                let level = (-64, 384);
                let ours = [
                    g.spawn_height(),
                    g.base_height_at(Heightmap::WorldSurfaceWg, level),
                    g.base_height_at(Heightmap::OceanFloorWg, level),
                    g.base_height_at(Heightmap::MotionBlocking, level),
                ];
                let want: Vec<i32> = words[1..5].iter().map(|w| w.parse().unwrap()).collect();
                if ours.as_slice() != want {
                    failures.push(format!("{label}: facts {ours:?} not {want:?}"));
                }
                let mut column: Vec<i64> = g
                    .base_column_of(level)
                    .map(|(_, states)| states.iter().map(|s| i64::from(s.0)).collect())
                    .unwrap_or_default();
                column.resize(384, 0);
                if column != runs(&words[5..]) {
                    failures.push(format!("{label}: base column differs"));
                }
                rows += 1;
            }
            "plan" => {
                let g = generator.as_ref().unwrap();
                let step: usize = words[1].parse().unwrap();
                let ours = g.plan_names().get(step).cloned().unwrap_or_default();
                if ours != words[2..] {
                    failures.push(format!(
                        "{label}: plan of step {step}: {ours:?} not {:?}",
                        &words[2..]
                    ));
                }
                rows += 1;
            }
            "chunk" => {
                let g = generator.as_ref().unwrap();
                centre = ChunkPos::new(words[2].parse().unwrap(), words[3].parse().unwrap());
                generated = generate(g, &[centre]);
                chunk_count += 1;
            }
            "sec" => {
                let (dx, dz): (i32, i32) = (words[1].parse().unwrap(), words[2].parse().unwrap());
                let section: i32 = words[3].parse().unwrap();
                let chunk = &generated[&ChunkPos::new(centre.x + dx, centre.z + dz)];
                let ids = runs(&words[4..]);
                let base = chunk.min_y() + section * 16;
                let mut bad = 0;
                for (i, &id) in ids.iter().enumerate() {
                    let (x, y, z) = (
                        (i & 15) as i32,
                        base + (i >> 8) as i32,
                        ((i >> 4) & 15) as i32,
                    );
                    if chunk.block_state(x, y, z) != BlockState(id as u32) {
                        if bad < 8 {
                            failures.push(format!(
                                "{label} chunk {} {}: block {x} {y} {z} is {} not {id}",
                                centre.x + dx,
                                centre.z + dz,
                                chunk.block_state(x, y, z).0
                            ));
                        }
                        bad += 1;
                    }
                }
                blocks_seen += 4096;
            }
            "hm" => {
                let (dx, dz): (i32, i32) = (words[1].parse().unwrap(), words[2].parse().unwrap());
                let chunk = &generated[&ChunkPos::new(centre.x + dx, centre.z + dz)];
                let kind = *Heightmap::ALL.iter().find(|k| k.key() == words[3]).unwrap();
                for (i, want) in words[4..].iter().enumerate() {
                    let got = chunk
                        .height_at(kind, (i % 16) as i32, (i / 16) as i32)
                        .map(|h| h + 1);
                    heights += 1;
                    if got != Some(want.parse().unwrap()) {
                        failures.push(format!(
                            "{label} chunk {} {}: {} column {i}: {got:?} not {want}",
                            centre.x + dx,
                            centre.z + dz,
                            words[3]
                        ));
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    println!(
        "{cases} flat cases, {rows} fact rows, {chunk_count} decorated chunks ({blocks_seen} blocks, \
         {heights} heightmap values); {} failures",
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{:#?}",
        &failures[..failures.len().min(12)]
    );
    assert!(cases >= 13 && chunk_count >= 18);
}

#[test]
#[ignore = "needs HORIZON_FLAT_DEBUG_ORACLE and the real 26.3 data (tools/leaf_server/README.md)"]
fn debug_generator_matches_java() {
    let oracle = env_path("HORIZON_FLAT_DEBUG_ORACLE").expect("HORIZON_FLAT_DEBUG_ORACLE");
    let setup = Setup::load().expect("HORIZON_VANILLA_JAR and the block data");
    let text = std::fs::read_to_string(oracle).expect("the oracle file");
    let generator = setup.generator(FixedKind::Debug, false);
    let mut failures: Vec<String> = Vec::new();
    let (mut grid_rows, mut probes, mut layers) = (0, 0, 0);
    let mut generated: HashMap<ChunkPos, ProtoChunk> = HashMap::new();
    let mut centre = ChunkPos::new(0, 0);
    let mut seen_header = false;
    for line in text.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        match words[0] {
            "debug" => {
                let want = (
                    words[1].parse::<usize>().unwrap(),
                    words[2].parse::<i32>().unwrap(),
                    words[3].parse::<i32>().unwrap(),
                );
                if generator.debug_grid_size() != want {
                    failures.push(format!(
                        "grid {:?} not {want:?}",
                        generator.debug_grid_size()
                    ));
                }
                seen_header = true;
            }
            "dbgrow" => {
                let z: i32 = words[1].parse().unwrap();
                for (i, want) in words[2..].iter().enumerate() {
                    let x = 1 + 2 * i as i32;
                    let got = generator.debug_block_state(x, z).0;
                    grid_rows += 1;
                    if got != want.parse::<u32>().unwrap() {
                        failures.push(format!("state at {x} {z} is {got} not {want}"));
                        break;
                    }
                }
            }
            "dbgprobe" => {
                let (x, z): (i32, i32) = (words[1].parse().unwrap(), words[2].parse().unwrap());
                let got = generator.debug_block_state(x, z).0;
                probes += 1;
                if got != words[3].parse::<u32>().unwrap() {
                    failures.push(format!("probe {x} {z}: {got} not {}", words[3]));
                }
            }
            "dbgchunk" => {
                centre = ChunkPos::new(words[1].parse().unwrap(), words[2].parse().unwrap());
                generated = generate(&generator, &[centre]);
            }
            "dbglayer" => {
                let y: i32 = words[1].parse().unwrap();
                let chunk = &generated[&centre];
                for (i, want) in words[2..].iter().enumerate() {
                    let (x, z) = ((i % 16) as i32, (i / 16) as i32);
                    let got = chunk.block_state(x, y, z).0;
                    if got != want.parse::<u32>().unwrap() {
                        failures.push(format!(
                            "chunk {} {}: block {x} {y} {z} is {got} not {want}",
                            centre.x, centre.z
                        ));
                        break;
                    }
                }
                layers += 1;
            }
            _ => {}
        }
    }
    println!(
        "debug grid: {grid_rows} states, {probes} probes, {layers} chunk layers; {} failures",
        failures.len()
    );
    assert!(
        seen_header && failures.is_empty(),
        "{:#?}",
        &failures[..failures.len().min(12)]
    );
    assert!(grid_rows > 20000 && probes >= 18 && layers == 6);
}

/// The vanilla-made flat world (`HORIZON_SERVER_VANILLA_WORLD`): chunks 0..15 generated here
/// against its region files.
#[test]
#[ignore = "needs HORIZON_SERVER_VANILLA_WORLD (a flat world a vanilla 26.3 server made) and the real data"]
fn flat_matches_vanilla_world() {
    use crate::world::chunk_io::{ChunkIo, ChunkIoEvent, CodecTables};
    use crate::world::level_chunk::LevelChunk;
    use crate::world::level_storage::LevelStorageAccess;
    use crate::world::storage::compression::Compression;

    let Some(world) = env_path("HORIZON_SERVER_VANILLA_WORLD") else {
        panic!("HORIZON_SERVER_VANILLA_WORLD");
    };
    let setup = Setup::load().expect("HORIZON_VANILLA_JAR and the block data");
    // The generator the preset gives (`generator-settings={}` does not parse: the flat preset's).
    let presets = setup.data.raw("minecraft:worldgen/world_preset").unwrap();
    let preset = presets
        .elements
        .iter()
        .find(|(id, _)| id == "minecraft:flat")
        .map(|(_, raw)| raw)
        .unwrap();
    let dimensions = object(required(&object(preset).unwrap(), "dimensions").unwrap()).unwrap();
    let generator_json = required(
        &object(&dimensions["minecraft:overworld"]).unwrap(),
        "generator",
    )
    .unwrap()
    .to_owned();
    let value: serde_json::Value = serde_json::from_str(generator_json.get()).unwrap();
    let settings = setup.decode_flat(&value["settings"].to_string()).unwrap();
    let generator = setup.generator(FixedKind::Flat(settings), true);

    // The chunks the vanilla server saved as `full` (the forceloaded area and what it kept loaded
    // around it): looked for in a square around the spawn.
    let (base, level_id) = (
        world.parent().unwrap(),
        world.file_name().unwrap().to_str().unwrap(),
    );
    let access = LevelStorageAccess::open(base, level_id).unwrap();
    let region = access.region_path("minecraft:overworld", "region");
    let names = setup.biome_names();
    let plains = names.iter().position(|n| n == "minecraft:plains").unwrap() as u32;
    let tables = Arc::new(CodecTables {
        factory: ContainerFactory::new(setup.blocks.state_count(), names.len() as u32, plains),
        blocks: setup.blocks.clone(),
        flags: setup.flags.clone(),
        biomes: Arc::new(crate::world::chunk::BiomeIds::new(names.clone())),
        min_section_y: -4,
        max_section_y: 19,
    });
    let (io, events) = ChunkIo::spawn(region, Compression::Lz4, false, tables.clone(), 2).unwrap();
    let candidates: Vec<ChunkPos> = (-20..20)
        .flat_map(|z| (-20..20).map(move |x| ChunkPos::new(x, z)))
        .collect();
    for &pos in &candidates {
        io.load(pos);
    }
    let mut vanilla: HashMap<ChunkPos, LevelChunk> = HashMap::new();
    let mut structures: HashMap<ChunkPos, crate::nbt::Compound> = HashMap::new();
    for _ in &candidates {
        let ChunkIoEvent::Loaded { pos, result } = events.recv().unwrap() else {
            continue;
        };
        let Some(data) = result.unwrap() else {
            continue;
        };
        let data = *data;
        if data.status != ChunkStatus::Full {
            continue;
        }
        if let Some(tag) = data.raw.structures.clone() {
            structures.insert(pos, tag);
        }
        vanilla.insert(
            pos,
            LevelChunk::from_data(data, 24, &tables.factory, &*setup.heightmaps),
        );
    }
    io.shutdown();
    let mut targets: Vec<ChunkPos> = vanilla.keys().copied().collect();
    targets.sort_by_key(|p| (p.z, p.x));
    assert!(!targets.is_empty(), "no full chunk saved in the world");

    let start = std::time::Instant::now();
    let generated = generate(&generator, &targets);
    let elapsed = start.elapsed();

    // The two blocks the world's maker set by hand (`setblock`), and their heightmap columns.
    let by_hand = [(0, 3, -60, 3), (0, 4, -55, 4)];
    let (mut blocks, mut biome_cells, mut heights, mut starts, mut references) =
        (0u64, 0u64, 0u64, 0usize, 0usize);
    let mut failures: Vec<String> = Vec::new();
    for &pos in &targets {
        let (ours, theirs) = (&generated[&pos], &vanilla[&pos]);
        for y in -64..320 {
            for z in 0..16 {
                for x in 0..16 {
                    let (bx, bz) = (pos.x * 16 + x, pos.z * 16 + z);
                    let hand = pos == ChunkPos::new(0, 0)
                        && by_hand
                            .iter()
                            .any(|&(_, hx, hy, hz)| (hx, hy, hz) == (bx, y, bz));
                    let want = theirs.block_state(crate::core::position::BlockPos::new(bx, y, bz));
                    blocks += 1;
                    if !hand && ours.block_state(x, y, z) != want {
                        failures.push(format!(
                            "chunk {} {}: block {x} {y} {z} is {} not {}",
                            pos.x,
                            pos.z,
                            ours.block_state(x, y, z).0,
                            want.0
                        ));
                    }
                }
            }
        }
        for (i, section) in theirs.sections().iter().enumerate() {
            for qy in 0..4 {
                for qz in 0..4 {
                    for qx in 0..4 {
                        biome_cells += 1;
                        let got = ours.sections()[i].noise_biome(qx, qy, qz);
                        if got != section.noise_biome(qx, qy, qz) {
                            failures
                                .push(format!("chunk {} {}: biome of section {i}", pos.x, pos.z));
                        }
                    }
                }
            }
        }
        for kind in [
            Heightmap::MotionBlocking,
            Heightmap::MotionBlockingNoLeaves,
            Heightmap::OceanFloor,
            Heightmap::WorldSurface,
        ] {
            for z in 0..16 {
                for x in 0..16 {
                    let hand = pos == ChunkPos::new(0, 0) && ((x, z) == (3, 3) || (x, z) == (4, 4));
                    heights += 1;
                    if !hand && ours.height_at(kind, x, z) != theirs.height_at(kind, x, z) {
                        failures.push(format!(
                            "chunk {} {}: {} at {x} {z} is {:?} not {:?}",
                            pos.x,
                            pos.z,
                            kind.key(),
                            ours.height_at(kind, x, z),
                            theirs.height_at(kind, x, z)
                        ));
                    }
                }
            }
        }
        // The structures: the same starts and references.
        let saved = ours.structures.save(setup.data.structures(), pos);
        let names_of = |tag: &crate::nbt::Compound, key: &str| -> Vec<String> {
            let mut names: Vec<String> = match tag.get(&key.into()) {
                Some(crate::nbt::Tag::Compound(c)) => {
                    c.keys().map(|k| k.to_string_lossy()).collect()
                }
                _ => Vec::new(),
            };
            names.sort();
            names
        };
        let want = structures.get(&pos).cloned().unwrap_or_default();
        starts += names_of(&saved, "starts").len();
        references += names_of(&saved, "References").len();
        for key in ["starts", "References"] {
            if names_of(&saved, key) != names_of(&want, key) {
                failures.push(format!(
                    "chunk {} {}: {key} {:?} not {:?}",
                    pos.x,
                    pos.z,
                    names_of(&saved, key),
                    names_of(&want, key)
                ));
            }
        }
    }
    println!(
        "{} chunks generated in {:.2?} ({blocks} blocks, {biome_cells} biome cells, {heights} \
         heightmap values, {starts} starts, {references} reference lists); {} failures",
        targets.len(),
        elapsed,
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{:#?}",
        &failures[..failures.len().min(12)]
    );
}

#[test]
fn the_debug_grid_is_sized_as_vanilla_floats_do() {
    use super::fixed::DebugGrid;
    // 26.3's 36672 states: ceil(sqrt) 192 wide, ceil(36672f / 192) = 191 high (the oracle's row).
    let grid = DebugGrid::new(36672);
    assert_eq!((grid.width, grid.height), (192, 191));
    assert_eq!(grid.state_for(1, 1), BlockState(0));
    assert_eq!(grid.state_for(0, 1), BlockState::AIR);
    assert_eq!(grid.state_for(-1, -1), BlockState::AIR);
    assert_eq!(grid.state_for(3, 5), BlockState(192 + 2));
    assert_eq!(DebugGrid::new(1).width, 1);
}

/// A flat world with the classic preset has villages and strongholds that start and are placed:
/// a square of chunks generated with structures on has a village start, and blocks above the
/// layers in the chunks around it.
#[test]
#[ignore = "needs the real 26.3 data"]
fn flat_generates_villages() {
    let setup = Setup::load().expect("HORIZON_VANILLA_JAR and the block data");
    let presets = setup.data.raw("minecraft:worldgen/world_preset").unwrap();
    let preset = presets
        .elements
        .iter()
        .find(|(id, _)| id == "minecraft:flat")
        .map(|(_, raw)| raw)
        .unwrap();
    let dimensions = object(required(&object(preset).unwrap(), "dimensions").unwrap()).unwrap();
    let generator_json = required(
        &object(&dimensions["minecraft:overworld"]).unwrap(),
        "generator",
    )
    .unwrap()
    .to_owned();
    let value: serde_json::Value = serde_json::from_str(generator_json.get()).unwrap();
    let generator = setup.generator(
        FixedKind::Flat(setup.decode_flat(&value["settings"].to_string()).unwrap()),
        true,
    );
    let targets: Vec<ChunkPos> = (-24..24)
        .flat_map(|z| (-24..24).map(move |x| ChunkPos::new(x, z)))
        .collect();
    let start = std::time::Instant::now();
    let generated = generate(&generator, &targets);
    let mut villages = Vec::new();
    for &pos in &targets {
        let saved = generated[&pos]
            .structures
            .save(setup.data.structures(), pos);
        if let Some(crate::nbt::Tag::Compound(starts)) = saved.get(&"starts".into()) {
            for key in starts.keys() {
                if key.to_string_lossy().starts_with("minecraft:village") {
                    villages.push(pos);
                }
            }
        }
    }
    println!(
        "{} chunks with structures in {:.2?}: village starts in {villages:?}",
        targets.len(),
        start.elapsed()
    );
    assert!(!villages.is_empty(), "no village start in the square");
    let village = villages[0];
    let built = (-1..=1)
        .flat_map(|dz| (-1..=1).map(move |dx| (dx, dz)))
        .map(|(dx, dz)| &generated[&ChunkPos::new(village.x + dx, village.z + dz)])
        .any(|chunk| {
            (-59..-40).any(|y| {
                (0..16).any(|x| (0..16).any(|z| chunk.block_state(x, y, z) != BlockState::AIR))
            })
        });
    assert!(built, "the village's pieces are placed above the layers");
}
