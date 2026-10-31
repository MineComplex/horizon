//! The CARVERS step against the real 26.3 server: `LeafCarverOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) fills, builds the surface of and
//! carves whole chunks of the overworld, large biomes, amplified and Nether noise settings for two
//! seeds, with the stem's real biome source, running `NoiseBasedChunkGenerator`'s own
//! `getBiomeGenerationSettingsForCarver`, the carvers and `applyCarvingMask`; this does the same
//! and compares the carving mask, every block, both generation heightmaps and the fluids marked
//! for post-processing.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafCarverOracle.java carver.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_CARVER_ORACLE=carver.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     HORIZON_BIOME_REPORTS=<reports dir> \
//!     cargo test -p horizon-server --release carver_matches_java -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use super::CarverStep;
use crate::block::BlockState;
use crate::core::position::ChunkPos;
use crate::registry::{DataPack, Registries};
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::biome::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use crate::worldgen::codec::{object, required};
use crate::worldgen::density::DensityContext;
use crate::worldgen::proto_chunk::ProtoChunk;
use crate::worldgen::surface::SurfaceSystem;
use crate::worldgen::surface::oracle_tests::{BiomeChunks, Stem, snapshot};
use crate::worldgen::terrain::NoiseTerrain;
use crate::worldgen::terrain::oracle_tests::{env_path, expand, load_real};

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_CARVER_ORACLE (tools/leaf_server/README.md)"]
fn carver_matches_java() {
    let oracle = env_path("HORIZON_CARVER_ORACLE").expect("HORIZON_CARVER_ORACLE");
    let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
    let reports = env_path("HORIZON_BIOME_REPORTS").expect("HORIZON_BIOME_REPORTS");
    let mut jar = VanillaJar::open(env_path("HORIZON_VANILLA_JAR").unwrap().as_ref()).unwrap();
    let pack = DataPack::vanilla(&mut jar).unwrap();
    let registries = Registries::build(Vec::new(), &[pack, DataPack::paper()]).unwrap();
    let biome_registry = registries.synced("minecraft:worldgen/biome").unwrap();
    let biomes = BiomeIds::new(biome_registry);
    let biome_count = biome_registry.entries.len() as u32;
    let presets = BiomePresets::load(reports.as_ref(), &real.data, &biomes).unwrap();
    let world_presets = real.data.raw("minecraft:worldgen/world_preset").unwrap();
    let text = std::fs::read_to_string(oracle).expect("the oracle file");
    let motion: HashSet<&str> = text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("motion "))
        .expect("the motion row")
        .split(' ')
        .collect();
    let table = real.heightmap_table(&motion);
    let mut context = DensityContext::cached();
    let mut stems: HashMap<(String, String, String, i64), Stem> = HashMap::new();

    let (mut chunks, mut blocks, mut heightmap_values, mut marks, mut runs) = (0, 0u64, 0, 0, 0);
    let (mut carved_chunks, mut changed) = (0, 0u64);
    let mut carve_time = Duration::ZERO;
    let mut failures = Vec::new();
    let mut current: Option<(ProtoChunk, String)> = None;
    let mut expect_marks: HashMap<usize, Vec<u16>> = HashMap::new();
    let mut notes: BTreeMap<String, usize> = BTreeMap::new();
    let finish = |current: &mut Option<(ProtoChunk, String)>,
                  expect_marks: &mut HashMap<usize, Vec<u16>>,
                  failures: &mut Vec<String>,
                  marks: &mut usize| {
        if let Some((chunk, label)) = current.take() {
            for (section, got) in chunk.postprocessing().iter().enumerate() {
                let want = expect_marks.get(&section).cloned().unwrap_or_default();
                *marks += want.len();
                if *got != want {
                    failures.push(format!("{label}: post-processing of section {section}"));
                }
            }
        }
        expect_marks.clear();
    };

    for line in text.lines().skip(1) {
        let words: Vec<&str> = line.split(' ').collect();
        match words[0] {
            "chunk" => {
                finish(&mut current, &mut expect_marks, &mut failures, &mut marks);
                let seed: i64 = words[4].parse().unwrap();
                let (cx, cz): (i32, i32) = (words[5].parse().unwrap(), words[6].parse().unwrap());
                let key = (
                    words[1].to_owned(),
                    words[2].to_owned(),
                    words[3].to_owned(),
                    seed,
                );
                let stem = stems.entry(key).or_insert_with(|| {
                    let preset = world_presets
                        .elements
                        .iter()
                        .find(|(id, _)| id == words[2])
                        .map(|(_, raw)| raw)
                        .expect("world preset");
                    let dimensions =
                        object(required(&object(preset).unwrap(), "dimensions").unwrap()).unwrap();
                    let generator = required(&object(&dimensions[words[3]]).unwrap(), "generator")
                        .unwrap()
                        .to_owned();
                    let stem = decode_noise_generator(&generator, &biomes, &presets).unwrap();
                    let settings = real.data.noise_settings(words[1]).unwrap();
                    Stem {
                        source: stem.biome_source,
                        density: real.data.compile_level(settings, seed, false).unwrap(),
                        settings: words[1].to_owned(),
                    }
                });
                let settings = real.data.noise_settings(words[1]).unwrap();
                let (min_y, height): (i32, i32) =
                    (words[7].parse().unwrap(), words[8].parse().unwrap());
                let pos = ChunkPos::new(cx, cz);
                // The BIOMES steps of the chunk and its neighbours leave the climate tree's last
                // result where the carvers' own queries pick it up.
                let mut biome_chunks = BiomeChunks::build(&real, stem, biome_count, pos, seed);
                let mut chunk = real.chunk(pos, min_y, height);
                context.reset();
                let mut aquifer = NoiseTerrain::new(&stem.density, settings, &real.blocks)
                    .unwrap()
                    .fill_with_aquifer(&mut chunk, &mut context, &real.state_data, &table)
                    .expect("fill")
                    .expect("a world with height");
                let system = SurfaceSystem::new(
                    &real.data,
                    &stem.density,
                    settings,
                    &real.blocks,
                    seed,
                    false,
                )
                .expect("the surface system");
                system.build_surface(
                    &mut chunk,
                    &mut context,
                    &real.state_data,
                    &table,
                    |at| biome_chunks.biome_at(at),
                    None,
                );
                let surfaced = snapshot(&chunk);
                let step = CarverStep::new(
                    real.data.carvers(),
                    &stem.density,
                    settings,
                    &real.blocks,
                    seed,
                )
                .expect("the carver step");
                let mut uncached = DensityContext::uncached();
                let mut search = std::mem::take(&mut biome_chunks.search);
                let start = Instant::now();
                let mask = step
                    .carve(
                        &mut chunk,
                        &mut aquifer,
                        &system,
                        &mut context,
                        &real.state_data,
                        &table,
                        biome_chunks.zoom,
                        |quart| {
                            stem.source.noise_biome(
                                &stem.density,
                                &mut uncached,
                                &mut search,
                                quart,
                            )
                        },
                    )
                    .expect("carve");
                carve_time += start.elapsed();
                let got_runs = mask.runs();
                runs += got_runs.len();
                carved_chunks += usize::from(!mask.is_empty());
                changed += surfaced
                    .iter()
                    .zip(snapshot(&chunk))
                    .filter(|(before, after)| *before != after)
                    .count() as u64;
                let label = format!("{} {seed} chunk {cx} {cz} ({})", words[1], words[9]);
                let want_runs: Vec<[i32; 4]> = parse_runs(words.get(10..).unwrap_or(&[]));
                compare_masks(&mut failures, &label, &got_runs, &want_runs);
                chunks += 1;
                *notes.entry(words[9].to_owned()).or_default() += 1;
                current = Some((chunk, label));
            }
            "sec" => {
                let (chunk, label) = current.as_ref().unwrap();
                let section: usize = words[1].parse().unwrap();
                let ids = expand(&words[2..]);
                assert_eq!(ids.len(), 4096);
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
                blocks += 4096;
            }
            "hm" => {
                let (chunk, label) = current.as_ref().unwrap();
                let kind = match words[1] {
                    "OCEAN_FLOOR_WG" => Heightmap::OceanFloorWg,
                    "WORLD_SURFACE_WG" => Heightmap::WorldSurfaceWg,
                    other => panic!("heightmap {other}"),
                };
                for (i, want) in words[2..].iter().enumerate() {
                    let got = chunk
                        .height_at(kind, (i % 16) as i32, (i / 16) as i32)
                        .map(|h| h + 1);
                    heightmap_values += 1;
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
                let section: usize = words[1].parse().unwrap();
                expect_marks.insert(
                    section,
                    words[2..].iter().map(|w| w.parse().unwrap()).collect(),
                );
            }
            other => panic!("unknown row {other}"),
        }
    }
    finish(&mut current, &mut expect_marks, &mut failures, &mut marks);
    println!(
        "{chunks} chunks ({carved_chunks} carved, {runs} mask runs, {blocks} blocks, {changed} of \
         them changed by the carvers, {heightmap_values} heightmap values, {marks} fluid marks); \
         {} failures; CARVERS {:.2} ms per chunk (this build, with the 3x3 biome chunks apart)",
        failures.len(),
        carve_time.as_secs_f64() * 1000.0 / f64::from(chunks.max(1))
    );
    println!("chunks by note: {notes:?}");
    assert!(
        failures.is_empty(),
        "{:#?}",
        &failures[..failures.len().min(12)]
    );
    assert!(chunks >= 100 && carved_chunks >= 80 && changed > 100_000 && marks > 0);
}

/// The oracle's mask runs, four integers each, after the chunk row's note.
fn parse_runs(words: &[&str]) -> Vec<[i32; 4]> {
    words
        .chunks(4)
        .map(|run| std::array::from_fn(|i| run[i].parse().unwrap()))
        .collect()
}

fn compare_masks(failures: &mut Vec<String>, label: &str, got: &[[i32; 4]], want: &[[i32; 4]]) {
    if got != want {
        let first = got
            .iter()
            .zip(want)
            .position(|(a, b)| a != b)
            .unwrap_or(got.len().min(want.len()));
        failures.push(format!(
            "{label}: carving mask differs at run {first} ({} runs, want {}): {:?} vs {:?}",
            got.len(),
            want.len(),
            got.get(first),
            want.get(first)
        ));
    }
}
