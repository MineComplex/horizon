//! The status pipeline against the real 26.3 server: `LeafChunkGenOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) prints the generation pyramid,
//! `ChunkLevel.generationStatus`, and whole chunks of the normal preset's overworld, Nether and
//! End for two seeds taken through STRUCTURE_STARTS (structures off), STRUCTURE_REFERENCES,
//! BIOMES (the chunk and its 8 neighbours) and TERRAIN (noise, surface, carvers, final heightmaps)
//! as `ChunkStatusTasks` runs them; this runs the same chunks through [`LevelGenerator`]'s
//! worker steps over generation regions and compares the pyramid, every block, biome, heightmap,
//! post-processing mark, the status and the structure starts and references.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafChunkGenOracle.java chunkgen.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_CHUNKGEN_ORACLE=chunkgen.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     HORIZON_BIOME_REPORTS=<reports dir> \
//!     cargo test -p horizon-server --release chunk_generation_matches_java -- --ignored --nocapture
//! ```
//!
//! Structures: the vanilla jar cannot place structure pieces without a level, and the pieces are
//! not ported, so both sides run with `generate-structures` off (no starts, no references; the
//! placement itself is `LeafStructurePlacementOracle`'s).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::pyramid::{Pyramids, STATUSES};
use super::{GeneratorTables, LevelGenerator, LevelGeneratorSettings};
use crate::block::BlockState;
use crate::core::position::ChunkPos;
use crate::registry::{DataPack, Registries};
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{ChunkStatus, ContainerFactory};
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::biome::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use crate::worldgen::codec::{object, required};
use crate::worldgen::proto_chunk::{GenerationRegion, ProtoChunk, RegionChunk};
use crate::worldgen::terrain::oracle_tests::{env_path, expand, load_real};

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_CHUNKGEN_ORACLE (tools/leaf_server/README.md)"]
fn chunk_generation_matches_java() {
    let oracle = env_path("HORIZON_CHUNKGEN_ORACLE").expect("HORIZON_CHUNKGEN_ORACLE");
    let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
    let reports = env_path("HORIZON_BIOME_REPORTS").expect("HORIZON_BIOME_REPORTS");
    let mut jar = VanillaJar::open(env_path("HORIZON_VANILLA_JAR").unwrap().as_ref()).unwrap();
    let pack = DataPack::vanilla(&mut jar).unwrap();
    let registries = Registries::build(Vec::new(), &[pack, DataPack::paper()]).unwrap();
    let biome_registry = registries.synced("minecraft:worldgen/biome").unwrap();
    let biomes = BiomeIds::new(biome_registry);
    let biome_count = biome_registry.entries.len() as u32;
    let plains = biomes.id("minecraft:plains").unwrap();
    let presets = BiomePresets::load(reports.as_ref(), &real.data, &biomes).unwrap();
    let world_presets = real.data.raw("minecraft:worldgen/world_preset").unwrap();
    let normal = world_presets
        .elements
        .iter()
        .find(|(id, _)| id == "minecraft:normal")
        .map(|(_, raw)| raw)
        .unwrap();
    let dimensions = object(required(&object(normal).unwrap(), "dimensions").unwrap()).unwrap();
    let text = std::fs::read_to_string(oracle).expect("the oracle file");
    let state_count = real.blocks.state_count();
    let crate::worldgen::terrain::oracle_tests::Real {
        data,
        blocks,
        state_data,
    } = real;
    let (data, blocks, flags) = (Arc::new(data), Arc::new(blocks), Arc::new(state_data));
    let mut failures = Vec::new();
    let mut heightmaps: Option<Arc<HeightmapTable>> = None;
    let mut java_biomes: Vec<String> = Vec::new();
    let (mut pyramid_rows, mut chunks, mut blocks_seen, mut biomes_seen, mut heights, mut marks) =
        (0, 0, 0u64, 0u64, 0, 0);
    let mut time = Duration::ZERO;
    // The current level's generator and chunk.
    let mut generator: Option<(String, i64, Arc<LevelGenerator>)> = None;
    let mut current: Option<(ProtoChunk, String, Vec<Vec<u16>>)> = None;
    let finish = |current: &mut Option<(ProtoChunk, String, Vec<Vec<u16>>)>,
                  failures: &mut Vec<String>,
                  marks: &mut usize| {
        if let Some((chunk, label, expected)) = current.take() {
            for (section, got) in chunk.postprocessing().iter().enumerate() {
                let want = expected.get(section).cloned().unwrap_or_default();
                *marks += want.len();
                if *got != want {
                    failures.push(format!("{label}: post-processing of section {section}"));
                }
            }
        }
    };
    for line in text.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        match words[0] {
            "step" => {
                let status = ChunkStatus::by_id(words[1]).unwrap();
                let step = Pyramids::get().generation.step_to(status);
                let want: Vec<ChunkStatus> = words[3..]
                    .iter()
                    .map(|w| ChunkStatus::by_id(w).unwrap())
                    .collect();
                if step.accumulated.as_slice() != want
                    || step.block_state_write_radius != words[2].parse::<i32>().unwrap()
                {
                    failures.push(format!("pyramid step {}", words[1]));
                }
                pyramid_rows += 1;
            }
            "level" => {
                let level: u8 = words[1].parse().unwrap();
                let want = ChunkStatus::by_id(words[2]);
                if Pyramids::get().generation_status(level) != want {
                    failures.push(format!("generationStatus({level})"));
                }
                pyramid_rows += 1;
            }
            "masks" => {
                let masks = expand(&words[1..]);
                assert_eq!(masks.len(), state_count as usize);
                heightmaps = Some(Arc::new(HeightmapTable::new(state_count, |s| {
                    masks[s.0 as usize] as u8
                })));
            }
            "biomes" => java_biomes = words[1..].iter().map(|w| (*w).to_owned()).collect(),
            "chunk" => {
                finish(&mut current, &mut failures, &mut marks);
                let (settings, stem_key) = (words[1], words[2]);
                let seed: i64 = words[3].parse().unwrap();
                let pos = ChunkPos::new(words[4].parse().unwrap(), words[5].parse().unwrap());
                let (min_y, height): (i32, i32) =
                    (words[6].parse().unwrap(), words[7].parse().unwrap());
                let fresh = generator
                    .as_ref()
                    .is_none_or(|(key, s, _)| key != stem_key || *s != seed);
                if fresh {
                    let raw = required(&object(&dimensions[stem_key]).unwrap(), "generator")
                        .unwrap()
                        .to_owned();
                    let stem = decode_noise_generator(&raw, &biomes, &presets).unwrap();
                    assert_eq!(stem.settings, settings);
                    let made = LevelGenerator::new(
                        GeneratorTables {
                            data: data.clone(),
                            blocks: blocks.clone(),
                            flags: flags.clone(),
                            heightmaps: heightmaps.clone().expect("the masks row"),
                        },
                        LevelGeneratorSettings {
                            stem,
                            seed,
                            generate_structures: false,
                            flat_bedrock: false,
                            end_void_rings: false,
                            spigot_seeds: Default::default(),
                            min_section_y: min_y >> 4,
                            section_count: (height / 16) as usize,
                            factory: ContainerFactory::new(state_count, biome_count, plains),
                        },
                    )
                    .expect("the generator");
                    generator = Some((stem_key.to_owned(), seed, Arc::new(made)));
                }
                let level = generator.as_ref().unwrap().2.clone();
                let mut worker = level.worker().unwrap();
                let start = Instant::now();
                let mut around: Vec<ProtoChunk> = Vec::with_capacity(9);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let mut chunk = level.empty_chunk(ChunkPos::new(pos.x + dx, pos.z + dz));
                        for status in [
                            ChunkStatus::StructureStarts,
                            ChunkStatus::StructureReferences,
                            ChunkStatus::Biomes,
                        ] {
                            let mut region = GenerationRegion::new(
                                chunk.pos(),
                                0,
                                vec![RegionChunk::Writable(&mut chunk)],
                            );
                            worker.run(status, &mut region).unwrap();
                        }
                        around.push(chunk);
                    }
                }
                let mut center = around.remove(4);
                {
                    let mut slots: Vec<RegionChunk<'_>> =
                        around.iter().map(RegionChunk::ReadOnly).collect();
                    slots.insert(4, RegionChunk::Writable(&mut center));
                    let mut region = GenerationRegion::new(pos, 1, slots);
                    worker.run(ChunkStatus::Terrain, &mut region).unwrap();
                }
                time += start.elapsed();
                let label = format!("{settings} {seed} chunk {} {}", pos.x, pos.z);
                if center.status() != ChunkStatus::by_id(words[8]).unwrap() {
                    failures.push(format!("{label}: status {:?}", center.status()));
                }
                let starts = center.structures.starts().count();
                let references = center.structures.all_references().count();
                if (starts.to_string(), references.to_string())
                    != (words[9].to_owned(), words[10].to_owned())
                {
                    failures.push(format!("{label}: {starts} starts, {references} references"));
                }
                chunks += 1;
                current = Some((center, label, Vec::new()));
            }
            "sec" => {
                let (chunk, label, _) = current.as_ref().unwrap();
                let section: usize = words[1].parse().unwrap();
                let ids = expand(&words[2..]);
                let base = chunk.min_y() + section as i32 * 16;
                let mut bad = 0;
                for (i, &id) in ids.iter().enumerate() {
                    let (x, y, z) = (
                        (i & 15) as i32,
                        base + (i >> 8) as i32,
                        ((i >> 4) & 15) as i32,
                    );
                    if chunk.block_state(x, y, z) != BlockState(id) {
                        if bad == 0 {
                            failures.push(format!(
                                "{label}: block {x} {y} {z} is {} not {id}",
                                chunk.block_state(x, y, z).0
                            ));
                        }
                        bad += 1;
                    }
                }
                blocks_seen += 4096;
            }
            "bio" => {
                let (chunk, label, _) = current.as_ref().unwrap();
                let section: i32 = words[1].parse().unwrap();
                let quart_y = (chunk.min_y() >> 2) + section * 4;
                let (qx, qz) = (chunk.pos().x * 4, chunk.pos().z * 4);
                for (i, name) in words[2..].iter().enumerate() {
                    let i = i as i32;
                    let (x, y, z) = (i & 3, i >> 4, (i >> 2) & 3);
                    let got = chunk.noise_biome(qx + x, quart_y + y, qz + z);
                    if biomes.name(got) != Some(*name) {
                        failures.push(format!(
                            "{label}: biome at quart {x} {} {z} is {:?} not {name}",
                            quart_y + y,
                            biomes.name(got)
                        ));
                        break;
                    }
                    biomes_seen += 1;
                }
            }
            "hm" => {
                let (chunk, label, _) = current.as_ref().unwrap();
                let kind = *Heightmap::ALL.iter().find(|k| k.key() == words[1]).unwrap();
                for (i, want) in words[2..].iter().enumerate() {
                    let got = chunk
                        .height_at(kind, (i % 16) as i32, (i / 16) as i32)
                        .map(|h| h + 1);
                    heights += 1;
                    if got != Some(want.parse().unwrap()) {
                        failures.push(format!(
                            "{label}: {} column {i}: {got:?} not {want}",
                            words[1]
                        ));
                        break;
                    }
                }
            }
            "post" => {
                let (_, _, expected) = current.as_mut().unwrap();
                let section: usize = words[1].parse().unwrap();
                if expected.len() <= section {
                    expected.resize(section + 1, Vec::new());
                }
                expected[section] = words[2..].iter().map(|w| w.parse().unwrap()).collect();
            }
            other => panic!("unknown row {other}"),
        }
    }
    finish(&mut current, &mut failures, &mut marks);
    assert!(!java_biomes.is_empty() && java_biomes.iter().all(|b| biomes.id(b).is_some()));
    println!(
        "{pyramid_rows} pyramid rows, {chunks} chunks ({blocks_seen} blocks, {biomes_seen} \
         biomes, {heights} heightmap values, {marks} fluid marks); {} failures; {:.2} ms per chunk \
         (BIOMES for 3 x 3 chunks and TERRAIN for one, this build)",
        failures.len(),
        time.as_secs_f64() * 1000.0 / f64::from(chunks.max(1))
    );
    assert!(
        failures.is_empty(),
        "{:#?}",
        &failures[..failures.len().min(12)]
    );
    assert!(pyramid_rows == STATUSES.len() + 19 && chunks >= 90);
}
