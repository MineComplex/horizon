//! Biomes against the real 26.3 server: `LeafBiomeOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) records the multi-noise presets, the
//! climate and biome at quart positions, whole chunks' biome containers, the spawn finder and the
//! `BiomeManager` zoom for the normal world's three stems and the large biomes and amplified
//! overworlds, two seeds each; this replays every row in order and compares.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafBiomeOracle.java biomes.txt
//! python3 tools/leaf_server_reports.py --jar <26.3 jar> ... --server-dir <dir>
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_BIOME_ORACLE=biomes.txt \
//!     HORIZON_BIOME_REPORTS=<dir>/cache/vanilla-26.3/reports \
//!     cargo test -p horizon-server --release biomes_match_java -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap};

use super::BiomeSource;
use super::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use super::spawn::find_spawn_position;
use super::zoom::BiomeZoom;
use crate::block::BlockState;
use crate::core::position::{BlockPos, ChunkPos};
use crate::registry::{DataPack, Registries};
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::ContainerFactory;
use crate::worldgen::climate::{ClimateSampler, ClimateSearch};
use crate::worldgen::codec::{object, required};
use crate::worldgen::data::{LevelDensity, WorldgenData};
use crate::worldgen::density::DensityContext;
use crate::worldgen::oracle_tests::settings_blocks;
use crate::worldgen::proto_chunk::ProtoChunk;

struct Level {
    density: LevelDensity,
    source: BiomeSource,
    /// The search state's key: the stem's `biome_source` JSON (one tree per preset in Java).
    tree: String,
    min_y: i32,
    height: i32,
}

#[test]
#[ignore = "needs the 26.3 jar, its data generator reports and LeafBiomeOracle's output"]
fn biomes_match_java() {
    let jar_path = std::env::var_os("HORIZON_VANILLA_JAR").expect("HORIZON_VANILLA_JAR");
    let oracle = std::fs::read_to_string(std::env::var("HORIZON_BIOME_ORACLE").unwrap()).unwrap();
    let reports = std::env::var_os("HORIZON_BIOME_REPORTS").expect("HORIZON_BIOME_REPORTS");
    let mut jar = VanillaJar::open(jar_path.as_ref()).unwrap();
    let pack = DataPack::vanilla(&mut jar).unwrap();
    let registries = Registries::build(Vec::new(), &[pack.clone(), DataPack::paper()]).unwrap();
    let data = WorldgenData::load(&[pack], &settings_blocks()).unwrap();
    let biome_registry = registries.synced("minecraft:worldgen/biome").unwrap();
    let biomes = BiomeIds::new(biome_registry);
    let presets = BiomePresets::load(reports.as_ref(), &data, &biomes).unwrap();
    let world_presets = data.raw("minecraft:worldgen/world_preset").unwrap();
    let factory = ContainerFactory::new(1, biome_registry.entries.len() as u32, 0);

    let mut checked: BTreeMap<&str, usize> = BTreeMap::new();
    let mut mismatches = Vec::new();
    let mut searches: HashMap<String, ClimateSearch> = HashMap::new();
    let mut level: Option<Level> = None;
    let mut chunk: Option<ProtoChunk> = None;
    let mut zoom_vanilla_differs = 0;
    let name = |id: u32| biomes.name(id).unwrap_or("?").to_owned();
    for line in oracle.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        let int = |i: usize| f[i].parse::<i32>().unwrap();
        let long = |i: usize| f[i].parse::<i64>().unwrap();
        let mut check = |kind: &'static str, ok: bool, detail: String| {
            *checked.entry(kind).or_default() += 1;
            if !ok {
                mismatches.push(format!("{line:.200}: {detail}"));
            }
        };
        match f[0] {
            "preset" => {
                let (point, biome) =
                    &presets.preset(f[1]).expect("preset").entries()[int(2) as usize];
                let mut got = vec![name(*biome)];
                for p in point.coordinates {
                    got.push(p.min.to_string());
                    got.push(p.max.to_string());
                }
                got.push(point.offset.to_string());
                check("preset entries", got == f[3..], format!("got {got:?}"));
            }
            "level" => {
                let preset = world_presets
                    .elements
                    .iter()
                    .find(|(id, _)| id == f[1])
                    .map(|(_, raw)| raw)
                    .expect("world preset");
                let dimensions =
                    object(required(&object(preset).unwrap(), "dimensions").unwrap()).unwrap();
                let generator = required(&object(&dimensions[f[2]]).unwrap(), "generator")
                    .unwrap()
                    .to_owned();
                let tree = required(&object(&generator).unwrap(), "biome_source")
                    .unwrap()
                    .get()
                    .to_owned();
                let stem = decode_noise_generator(&generator, &biomes, &presets).unwrap();
                let settings = data.noise_settings(&stem.settings).unwrap();
                level = Some(Level {
                    density: data.compile_level(settings, long(3)).unwrap(),
                    source: stem.biome_source,
                    tree,
                    min_y: settings.noise.min_y,
                    height: settings.noise.height,
                });
            }
            "target" => {
                let level = level.as_ref().unwrap();
                let target = ClimateSampler::new(&level.density)
                    .sample(&mut DensityContext::uncached(), [int(1), int(2), int(3)]);
                let expected: Vec<i64> = (4..10).map(long).collect();
                check(
                    "climate targets",
                    target.values[..] == expected[..],
                    format!("got {:?}", target.values),
                );
            }
            "biome" => {
                let level = level.as_ref().unwrap();
                let search = searches.entry(level.tree.clone()).or_default();
                let biome = level.source.noise_biome(
                    &level.density,
                    &mut DensityContext::uncached(),
                    search,
                    [int(1), int(2), int(3)],
                );
                check(
                    "quart biomes",
                    name(biome) == f[4],
                    format!("got {}", name(biome)),
                );
            }
            "chunk" => {
                let level = level.as_ref().unwrap();
                let pos = ChunkPos::new(int(1), int(2));
                let section = int(3) as usize;
                if section == 0 {
                    let mut c = ProtoChunk::new(
                        pos,
                        level.min_y >> 4,
                        (level.height >> 4) as usize,
                        &factory,
                        BlockState(0),
                    );
                    let search = searches.entry(level.tree.clone()).or_default();
                    level.source.create_biomes(&mut c, &level.density, search);
                    chunk = Some(c);
                }
                let c = chunk.as_ref().unwrap();
                let s = &c.sections()[section];
                let got: Vec<String> = (0..64u32)
                    .map(|i| name(s.noise_biome(i & 3, i >> 4, (i >> 2) & 3)))
                    .collect();
                check(
                    "chunk section biomes",
                    got == f[4..68],
                    format!("got {got:?}"),
                );
                let mut bytes = Vec::new();
                s.biomes().write(&mut bytes);
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                check(
                    "chunk section containers (bytes)",
                    hex == f[68],
                    format!("got {hex}"),
                );
            }
            "spawn" => {
                let level = level.as_ref().unwrap();
                let got = find_spawn_position(&level.density);
                check(
                    "spawn positions",
                    got == Some([int(1), int(2)]),
                    format!("got {got:?}"),
                );
            }
            "zoom" => {
                let quart =
                    BiomeZoom::new(long(2)).quart_at_block(BlockPos::new(int(3), int(4), int(5)));
                if f[6..9] != f[9..12] {
                    zoom_vanilla_differs += 1;
                }
                check(
                    "zoom quarts (Leaf)",
                    quart == [int(9), int(10), int(11)],
                    format!("got {quart:?}"),
                );
            }
            other => panic!("unknown row {other}"),
        }
    }
    eprintln!("checked: {checked:?}");
    eprintln!("zoom rows where vanilla's quart differs from Leaf's: {zoom_vanilla_differs}");
    for mismatch in mismatches.iter().take(30) {
        eprintln!("{mismatch}");
    }
    assert!(
        mismatches.is_empty(),
        "{} rows differ from Java",
        mismatches.len()
    );
}
