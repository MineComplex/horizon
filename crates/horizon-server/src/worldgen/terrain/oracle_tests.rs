//! The noise fill against the real 26.3 server: `LeafTerrainOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs `doFill` on proto chunks of
//! every noise settings element for two seeds; this compares every block, both generation
//! heightmaps, the fluids marked for post-processing, and `getBaseColumn`/`getBaseHeight`.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafTerrainOracle.java terrain.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_TERRAIN_ORACLE=terrain.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     cargo test -p horizon-server --release terrain_matches_java -- --ignored --nocapture
//! ```

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use super::NoiseTerrain;
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::position::ChunkPos;
use crate::registry::DataPack;
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::ContainerFactory;
use crate::world::chunk::serial::Heightmap;
use crate::world::level_chunk::{HeightmapFlags, HeightmapTable};
use crate::worldgen::data::{LevelDensity, WorldgenData};
use crate::worldgen::density::DensityContext;
use crate::worldgen::noise::DensityVolume;
use crate::worldgen::proto_chunk::ProtoChunk;
use crate::worldgen::settings::RouterField;

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// The real registries, block table and per-state data (`tools/leaf_server/README.md`).
pub(crate) struct Real {
    pub(crate) data: WorldgenData,
    pub(crate) blocks: BlockStates,
    pub(crate) state_data: BlockStateData,
}

pub(crate) fn load_real() -> Option<Real> {
    let jar = env_path("HORIZON_VANILLA_JAR")?;
    let blocks = BlockStates::load(&env_path("HORIZON_SERVER_BLOCKS_JSON")?).expect("blocks.json");
    let state_data = BlockStateData::load(&env_path("HORIZON_SERVER_BLOCK_STATES")?, &blocks)
        .expect("block_states.bin");
    let mut jar = VanillaJar::open(jar.as_ref()).expect("the 26.3 jar");
    let pack = DataPack::vanilla(&mut jar).expect("the vanilla pack");
    let data = WorldgenData::load(&[pack], &blocks).expect("the worldgen registries");
    Some(Real {
        data,
        blocks,
        state_data,
    })
}

impl Real {
    pub(crate) fn factory(&self) -> ContainerFactory {
        ContainerFactory::new(self.blocks.state_count(), 64, 0)
    }
    /// The WG heightmaps' predicates: `!isAir()` and the `blocks_motion_in_heightmap` blocks.
    pub(crate) fn heightmap_table(&self, motion: &HashSet<&str>) -> HeightmapTable {
        HeightmapTable::new(self.blocks.state_count(), |state| {
            let opaque = !self.state_data.is_air(state);
            let motion = motion.contains(self.blocks.block_name(self.blocks.block(state)));
            u8::from(opaque) << Heightmap::WorldSurfaceWg as u8
                | u8::from(motion) << Heightmap::OceanFloorWg as u8
        })
    }
    pub(crate) fn chunk(&self, pos: ChunkPos, min_y: i32, height: i32) -> ProtoChunk {
        let void_air = self.blocks.block_by_name("minecraft:void_air").unwrap();
        ProtoChunk::new(
            pos,
            min_y.div_euclid(16),
            (height / 16) as usize,
            &self.factory(),
            self.blocks.default_state(void_air),
        )
    }
}

/// `id*count` runs, as the oracle writes them.
pub(crate) fn expand(runs: &[&str]) -> Vec<u32> {
    runs.iter()
        .flat_map(|run| {
            let (id, count) = run.split_once('*').expect("a run");
            std::iter::repeat_n(id.parse().unwrap(), count.parse().unwrap())
        })
        .collect()
}

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_TERRAIN_ORACLE (tools/leaf_server/README.md)"]
fn terrain_matches_java() {
    let oracle = env_path("HORIZON_TERRAIN_ORACLE").expect("HORIZON_TERRAIN_ORACLE");
    let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
    let text = std::fs::read_to_string(oracle).expect("the oracle file");
    let motion: HashSet<&str> = text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("motion "))
        .expect("the motion row")
        .split(' ')
        .collect();
    let table = real.heightmap_table(&motion);
    let mut levels: HashMap<(String, i64), LevelDensity> = HashMap::new();
    let mut context = DensityContext::cached();

    let (mut chunks, mut block_count, mut heightmap_values, mut marks) = (0, 0u64, 0, 0);
    let (mut columns, mut column_blocks, mut heights) = (0, 0u64, 0);
    let mut failures = Vec::new();
    let mut current: Option<(ProtoChunk, String)> = None;
    let mut expect_marks: HashMap<usize, Vec<u16>> = HashMap::new();

    let level_of = |levels: &mut HashMap<(String, i64), LevelDensity>, key: &str, seed: i64| {
        levels
            .entry((key.to_owned(), seed))
            .or_insert_with(|| {
                let settings = real.data.noise_settings(key).expect("settings");
                real.data
                    .compile_level(settings, seed, false)
                    .expect("a level")
            })
            .clone()
    };
    // The marks of a finished chunk, compared when the next row is not one of its own.
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
                let seed: i64 = words[2].parse().unwrap();
                let (cx, cz): (i32, i32) = (words[3].parse().unwrap(), words[4].parse().unwrap());
                let (min_y, height): (i32, i32) =
                    (words[5].parse().unwrap(), words[6].parse().unwrap());
                let level = level_of(&mut levels, words[1], seed);
                let settings = real.data.noise_settings(words[1]).unwrap();
                let terrain = NoiseTerrain::new(&level, settings, &real.blocks).unwrap();
                let mut chunk = real.chunk(ChunkPos::new(cx, cz), min_y, height);
                context.reset();
                terrain
                    .fill(&mut chunk, &mut context, &real.state_data, &table)
                    .expect("fill");
                chunks += 1;
                current = Some((chunk, format!("{} {seed} chunk {cx} {cz}", words[1])));
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
                block_count += 4096;
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
            "column" | "height" => {
                finish(&mut current, &mut expect_marks, &mut failures, &mut marks);
                let seed: i64 = words[2].parse().unwrap();
                let (x, z): (i32, i32) = (words[3].parse().unwrap(), words[4].parse().unwrap());
                let level = level_of(&mut levels, words[1], seed);
                let settings = real.data.noise_settings(words[1]).unwrap();
                let terrain = NoiseTerrain::new(&level, settings, &real.blocks).unwrap();
                let world = (settings.noise.min_y, settings.noise.height);
                let label = format!("{} {seed} {} {x} {z}", words[0], words[1]);
                if words[0] == "column" {
                    let want = expand(&words[6..]);
                    let (min_y, got) = terrain
                        .base_column(x, z, world, &mut context)
                        .expect("column")
                        .expect("a column");
                    columns += 1;
                    column_blocks += want.len() as u64;
                    let got: Vec<u32> = got.iter().map(|s| s.0).collect();
                    if min_y != words[5].parse::<i32>().unwrap() || got != want {
                        failures.push(label);
                    }
                } else {
                    let got = terrain
                        .base_height(x, z, world, &mut context, |state| {
                            table.heightmap_mask(state) >> Heightmap::OceanFloorWg as u8 & 1 != 0
                        })
                        .expect("height");
                    heights += 1;
                    if got != words[5].parse::<i32>().unwrap() {
                        failures.push(format!("{label}: {got} not {}", words[5]));
                    }
                }
            }
            other => panic!("unknown row {other}"),
        }
    }
    finish(&mut current, &mut expect_marks, &mut failures, &mut marks);
    println!(
        "{chunks} chunks ({block_count} blocks, {heightmap_values} heightmap values, {marks} fluid marks), \
         {columns} columns ({column_blocks} blocks), {heights} base heights; {} failures",
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{:#?}",
        &failures[..failures.len().min(12)]
    );
    assert!(chunks >= 200 && marks > 0);
}

/// Milliseconds per chunk of the fill (release build): `cargo test --release terrain_fill_speed
/// -- --ignored --nocapture` with the variables above (no oracle file needed).
#[test]
#[ignore = "benchmark; needs the real 26.3 data"]
fn terrain_fill_speed() {
    let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
    // The blocks_motion_in_heightmap tag decides one heightmap only; stone and water will do.
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
    for key in ["minecraft:overworld", "minecraft:nether", "minecraft:end"] {
        let settings = real.data.noise_settings(key).unwrap();
        let level = real.data.compile_level(settings, 12345, false).unwrap();
        let terrain = NoiseTerrain::new(&level, settings, &real.blocks).unwrap();
        let mut run = |count: i32| {
            let start = Instant::now();
            for i in 0..count {
                let mut chunk = real.chunk(
                    ChunkPos::new(i % 8, i / 8),
                    settings.noise.min_y,
                    settings.noise.height,
                );
                context.reset();
                terrain
                    .fill(&mut chunk, &mut context, &real.state_data, &table)
                    .unwrap();
            }
            start.elapsed().as_secs_f64() * 1000.0 / f64::from(count)
        };
        run(8); // warm-up
        let total = run(64);
        // The final density alone (`sampleVolume` of the chunk), to see what the rest costs.
        let volume = DensityVolume::new(
            [16, settings.noise.height, 16],
            [0, settings.noise.min_y, 0],
            [1; 3],
        )
        .unwrap();
        let mut out = vec![0.0f32; volume.size()];
        let start = Instant::now();
        for _ in 0..16 {
            context.reset();
            level.program.fill(
                &mut context,
                level.router(RouterField::FinalDensity),
                &mut out,
                volume,
            );
        }
        let density = start.elapsed().as_secs_f64() * 1000.0 / 16.0;
        println!(
            "fill {key}: {total:.2} ms per chunk (64 chunks), {density:.2} ms of it final density"
        );
    }
}
