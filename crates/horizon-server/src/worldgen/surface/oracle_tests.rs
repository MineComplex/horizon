//! The surface against the real 26.3 server: `LeafSurfaceOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) fills whole chunks of every noise
//! settings element for two seeds and builds their surface with the stem's real biome source;
//! this does the same and compares every block, both generation heightmaps and the fluids marked
//! for post-processing, then every biome's temperature at spread positions. The jar is vanilla's,
//! so flat bedrock (Paper's, off here) is checked against Paper's arithmetic in `tests.rs`.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafSurfaceOracle.java surface.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_SURFACE_ORACLE=surface.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     HORIZON_BIOME_REPORTS=<reports dir> \
//!     cargo test -p horizon-server --release surface_matches_java -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use super::SurfaceSystem;
use crate::block::BlockState;
use crate::core::position::{BlockPos, ChunkPos};
use crate::registry::{DataPack, Registries};
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::biome::BiomeSource;
use crate::worldgen::biome::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use crate::worldgen::biome::zoom::BiomeZoom;
use crate::worldgen::climate::ClimateSearch;
use crate::worldgen::codec::{object, required};
use crate::worldgen::data::LevelDensity;
use crate::worldgen::density::DensityContext;
use crate::worldgen::proto_chunk::ProtoChunk;
use crate::worldgen::terrain::NoiseTerrain;
use crate::worldgen::terrain::oracle_tests::{Real, env_path, expand, load_real};

/// `BiomeManager.obfuscateSeed`: the first 8 bytes of SHA-256 over the seed, little-endian.
pub(crate) fn obfuscate_seed(seed: i64) -> i64 {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(seed.to_le_bytes());
    i64::from_le_bytes(hash[..8].try_into().expect("8 bytes"))
}

/// Every block of a chunk, by section, y, z, x.
pub(crate) fn snapshot(chunk: &ProtoChunk) -> Vec<u32> {
    let mut blocks = Vec::with_capacity(chunk.height() as usize * 256);
    for y in chunk.min_y()..chunk.min_y() + chunk.height() {
        for z in 0..16 {
            for x in 0..16 {
                blocks.push(chunk.block_state(x, y, z).0);
            }
        }
    }
    blocks
}

/// What a stem of a world preset gives a row.
pub(crate) struct Stem {
    pub(crate) source: BiomeSource,
    pub(crate) density: LevelDensity,
    pub(crate) settings: String,
}

/// A chunk's biomes and its neighbours', as the BIOMES steps store them, read through the zoom.
pub(crate) struct BiomeChunks {
    chunks: HashMap<ChunkPos, ProtoChunk>,
    pub(crate) zoom: BiomeZoom,
    /// The climate tree's last result after building them (vanilla keeps it per thread).
    pub(crate) search: ClimateSearch,
}

impl BiomeChunks {
    /// The 3 x 3 chunks around `center`, built in the oracle's order with a fresh search state.
    pub(crate) fn build(
        real: &Real,
        stem: &Stem,
        biome_count: u32,
        center: ChunkPos,
        seed: i64,
    ) -> Self {
        let settings = real.data.noise_settings(&stem.settings).unwrap();
        let factory = crate::world::chunk::ContainerFactory::new(1, biome_count, 0);
        let mut search = ClimateSearch::default();
        let mut chunks = HashMap::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                let pos = ChunkPos::new(center.x + dx, center.z + dz);
                let mut chunk = ProtoChunk::new(
                    pos,
                    settings.noise.min_y >> 4,
                    (settings.noise.height >> 4) as usize,
                    &factory,
                    BlockState(0),
                );
                stem.source
                    .create_biomes(&mut chunk, &stem.density, &mut search);
                chunks.insert(pos, chunk);
            }
        }
        Self {
            chunks,
            zoom: BiomeZoom::new(obfuscate_seed(seed)),
            search,
        }
    }

    /// `BiomeManager.getBiome(pos)`.
    pub(crate) fn biome_at(&self, pos: BlockPos) -> u32 {
        let [x, y, z] = self.zoom.quart_at_block(pos);
        self.chunks[&ChunkPos::new(x >> 2, z >> 2)].noise_biome(x, y, z)
    }
}

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_SURFACE_ORACLE (tools/leaf_server/README.md)"]
fn surface_matches_java() {
    let oracle = env_path("HORIZON_SURFACE_ORACLE").expect("HORIZON_SURFACE_ORACLE");
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

    let (mut chunks, mut blocks, mut heightmap_values, mut marks, mut temps) = (0, 0u64, 0, 0, 0);
    let mut surface_time = Duration::ZERO;
    let mut changed = 0u64;
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
            "temp" => {
                let biome = biomes.id(words[1]).expect("a biome");
                let sea: i32 = words[2].parse().unwrap();
                let pos = BlockPos::new(
                    words[3].parse().unwrap(),
                    words[4].parse().unwrap(),
                    words[5].parse().unwrap(),
                );
                let climates = real.data.climates();
                let bits = climates.temperature(biome, pos, sea).to_bits() as i32;
                let ok = bits.to_string() == words[6]
                    && climates.cold_enough_to_snow(biome, pos, sea) == (words[7] == "true")
                    && climates.melts_iceberg_slightly(biome, pos, sea) == (words[8] == "true");
                temps += 1;
                if !ok {
                    failures.push(format!("temperature: {line}"));
                }
            }
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
                let biome_chunks =
                    BiomeChunks::build(&real, stem, biome_count, ChunkPos::new(cx, cz), seed);
                let mut chunk = real.chunk(ChunkPos::new(cx, cz), min_y, height);
                context.reset();
                NoiseTerrain::new(&stem.density, settings, &real.blocks)
                    .unwrap()
                    .fill(&mut chunk, &mut context, &real.state_data, &table)
                    .expect("fill");
                let filled = snapshot(&chunk);
                let system = SurfaceSystem::new(
                    &real.data,
                    &stem.density,
                    settings,
                    &real.blocks,
                    seed,
                    false,
                )
                .expect("the surface system");
                let start = Instant::now();
                system.build_surface(
                    &mut chunk,
                    &mut context,
                    &real.state_data,
                    &table,
                    |pos| biome_chunks.biome_at(pos),
                    None,
                );
                surface_time += start.elapsed();
                changed += filled
                    .iter()
                    .zip(snapshot(&chunk))
                    .filter(|(before, after)| *before != after)
                    .count() as u64;
                chunks += 1;
                *notes.entry(words[9].to_owned()).or_default() += 1;
                current = Some((
                    chunk,
                    format!("{} {seed} chunk {cx} {cz} ({})", words[1], words[9]),
                ));
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
        "{chunks} chunks ({blocks} blocks, {changed} of them changed by the surface, \
         {heightmap_values} heightmap values, {marks} fluid marks), {temps} temperatures; \
         {} failures; buildSurface {:.2} ms per chunk (this build, with the 3x3 biome chunks apart)",
        failures.len(),
        surface_time.as_secs_f64() * 1000.0 / f64::from(chunks.max(1))
    );
    println!("chunks by note: {notes:?}");
    let (temperature, other): (Vec<_>, Vec<_>) = failures
        .iter()
        .partition(|failure| failure.starts_with("temperature"));
    println!(
        "{} temperature and {} block failures",
        temperature.len(),
        other.len()
    );
    failures = other.into_iter().chain(temperature).cloned().collect();
    assert!(
        failures.is_empty(),
        "{:#?}",
        &failures[..failures.len().min(12)]
    );
    assert!(chunks >= 250 && changed > 0 && temps > 4_000);
}

/// Milliseconds per chunk of `buildSurface` alone (release build): `cargo test --release
/// surface_build_speed -- --ignored --nocapture` with the variables above (no oracle file).
#[test]
#[ignore = "benchmark; needs the real 26.3 data"]
fn surface_build_speed() {
    let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
    let reports = env_path("HORIZON_BIOME_REPORTS").expect("HORIZON_BIOME_REPORTS");
    let mut jar = VanillaJar::open(env_path("HORIZON_VANILLA_JAR").unwrap().as_ref()).unwrap();
    let pack = DataPack::vanilla(&mut jar).unwrap();
    let registries = Registries::build(Vec::new(), &[pack, DataPack::paper()]).unwrap();
    let biome_registry = registries.synced("minecraft:worldgen/biome").unwrap();
    let biomes = BiomeIds::new(biome_registry);
    let presets = BiomePresets::load(reports.as_ref(), &real.data, &biomes).unwrap();
    let world_presets = real.data.raw("minecraft:worldgen/world_preset").unwrap();
    let motion: HashSet<&str> = [
        "minecraft:stone",
        "minecraft:deepslate",
        "minecraft:dirt",
        "minecraft:gravel",
    ]
    .into_iter()
    .collect();
    let table = real.heightmap_table(&motion);
    let mut context = DensityContext::cached();
    for (name, stem_name) in [
        ("minecraft:overworld", "minecraft:overworld"),
        ("minecraft:nether", "minecraft:the_nether"),
        ("minecraft:end", "minecraft:the_end"),
    ] {
        let preset = world_presets
            .elements
            .iter()
            .find(|(id, _)| id == "minecraft:normal")
            .map(|(_, raw)| raw)
            .unwrap();
        let dimensions = object(required(&object(preset).unwrap(), "dimensions").unwrap()).unwrap();
        let generator = required(&object(&dimensions[stem_name]).unwrap(), "generator")
            .unwrap()
            .to_owned();
        let decoded = decode_noise_generator(&generator, &biomes, &presets).unwrap();
        let settings = real.data.noise_settings(name).unwrap();
        let stem = Stem {
            source: decoded.biome_source,
            density: real.data.compile_level(settings, 12345, false).unwrap(),
            settings: name.to_owned(),
        };
        let system = SurfaceSystem::new(
            &real.data,
            &stem.density,
            settings,
            &real.blocks,
            12345,
            false,
        )
        .unwrap();
        let terrain = NoiseTerrain::new(&stem.density, settings, &real.blocks).unwrap();
        let mut total = Duration::ZERO;
        let count = 32;
        for i in 0..count {
            let pos = ChunkPos::new(i % 8, i / 8);
            let biome_chunks = BiomeChunks::build(
                &real,
                &stem,
                biome_registry.entries.len() as u32,
                pos,
                12345,
            );
            let mut chunk = real.chunk(pos, settings.noise.min_y, settings.noise.height);
            context.reset();
            terrain
                .fill(&mut chunk, &mut context, &real.state_data, &table)
                .unwrap();
            let start = Instant::now();
            system.build_surface(
                &mut chunk,
                &mut context,
                &real.state_data,
                &table,
                |pos| biome_chunks.biome_at(pos),
                None,
            );
            if i >= 4 {
                total += start.elapsed();
            }
        }
        println!(
            "buildSurface {name}: {:.2} ms per chunk ({} chunks after a warm-up)",
            total.as_secs_f64() * 1000.0 / f64::from(count - 4),
            count - 4
        );
    }
}

/// Paper's `environment.generate-flat-bedrock`, which the vanilla jar cannot run: derived from
/// `OptionallyFlatBedrockCondition` (`io.papermc.paper.world.worldgen`), which with the flag on
/// makes both bounds of the gradient `min(trueAt, falseAt)` for the floor and
/// `max(trueAt, falseAt) - 1` for the roof, so the floor is the world's lowest layer, the roof
/// its highest, and nothing in between. With the flag off it equals vanilla's gradient (the
/// oracle above). Checked through the real rules on the overworld's floor and the Nether's floor
/// and roof.
#[test]
#[ignore = "needs the real 26.3 data"]
fn flat_bedrock_is_one_layer() {
    let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
    let motion: HashSet<&str> = [
        "minecraft:stone",
        "minecraft:netherrack",
        "minecraft:deepslate",
    ]
    .into_iter()
    .collect();
    let table = real.heightmap_table(&motion);
    let bedrock = {
        let block = real.blocks.block_by_name("minecraft:bedrock").unwrap();
        real.blocks.default_state(block).0
    };
    let mut context = DensityContext::cached();
    for (name, layers) in [
        ("minecraft:overworld", vec![-64, -63, -62, -61, -60, -59]),
        (
            "minecraft:nether",
            vec![0, 1, 2, 3, 4, 5, 122, 123, 124, 125, 126, 127],
        ),
    ] {
        let settings = real.data.noise_settings(name).unwrap();
        let level = real.data.compile_level(settings, 12345, false).unwrap();
        let mut counts = |flat: bool| {
            let mut chunk = real.chunk(
                ChunkPos::new(3, -2),
                settings.noise.min_y,
                settings.noise.height,
            );
            context.reset();
            NoiseTerrain::new(&level, settings, &real.blocks)
                .unwrap()
                .fill(&mut chunk, &mut context, &real.state_data, &table)
                .unwrap();
            SurfaceSystem::new(&real.data, &level, settings, &real.blocks, 12345, flat)
                .unwrap()
                .build_surface(
                    &mut chunk,
                    &mut context,
                    &real.state_data,
                    &table,
                    |_| 0,
                    None,
                );
            layers
                .iter()
                .map(|&y| {
                    let count = (0..256)
                        .filter(|i| chunk.block_state(i % 16, y, i / 16).0 == bedrock)
                        .count();
                    (y, count)
                })
                .collect::<Vec<_>>()
        };
        let (gradient, flat) = (counts(false), counts(true));
        println!("{name}: gradient {gradient:?}, flat {flat:?}");
        let (bottom, top) = (layers[0], *layers.last().unwrap());
        for (y, count) in &flat {
            let layer = *y == bottom || (*y == top && name == "minecraft:nether");
            assert_eq!(
                *count,
                if layer { 256 } else { 0 },
                "{name} flat bedrock at y={y}"
            );
        }
        // The gradient has bedrock above the lowest layer, in some columns only.
        assert!(
            gradient[1..5]
                .iter()
                .any(|(_, count)| *count > 0 && *count < 256)
        );
    }
}
