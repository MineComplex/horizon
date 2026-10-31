//! The FEATURES step against the real 26.3 server: `LeafDecorationOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) fills and surfaces the 3 x 3 chunks
//! around a centre chunk with the stem's real biome source, then decorates the centre as
//! `ChunkGenerator.addVanillaDecorations` does (without structures; the feature types horizon-server
//! has not ported yet are skipped on both sides). This does the same through
//! [`apply_biome_decoration`] and compares every block of the nine chunks.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafDecorationOracle.java decoration.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_DECORATION_ORACLE=decoration.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     HORIZON_BIOME_REPORTS=<reports dir> \
//!     cargo test -p horizon-server --release decoration_matches_java -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use super::decorate::{NoStructures, apply_biome_decoration};
use super::sort::StepFeatureData;
use crate::block::{BlockId, BlockState};
use crate::core::position::ChunkPos;
use crate::registry::{DataPack, Registries, load_tags, registry_dir};
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::serial::ChunkStatus;
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::biome::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use crate::worldgen::codec::{object, required};
use crate::worldgen::density::DensityContext;
use crate::worldgen::placement::RegionWorld;
use crate::worldgen::placement::survival::BlockSurvivalRules;
use crate::worldgen::proto_chunk::{GenerationRegion, ProtoChunk, RegionChunk};
use crate::worldgen::surface::SurfaceSystem;
use crate::worldgen::surface::oracle_tests::{BiomeChunks, Stem};
use crate::worldgen::terrain::NoiseTerrain;
use crate::worldgen::terrain::oracle_tests::{env_path, expand, load_real};

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_DECORATION_ORACLE (tools/leaf_server/README.md)"]
fn decoration_matches_java() {
    let oracle = env_path("HORIZON_DECORATION_ORACLE").expect("HORIZON_DECORATION_ORACLE");
    let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
    let reports = env_path("HORIZON_BIOME_REPORTS").expect("HORIZON_BIOME_REPORTS");
    let mut jar = VanillaJar::open(env_path("HORIZON_VANILLA_JAR").unwrap().as_ref()).unwrap();
    let pack = DataPack::vanilla(&mut jar).unwrap();
    let packs = std::slice::from_ref(&pack);
    let block_tags = load_tags(packs, &registry_dir("minecraft:block"), |id| {
        real.blocks.block_by_name(id).map(|b| b.0 as i32)
    });
    let tag = |name: &str| -> HashSet<BlockId> {
        (block_tags.iter())
            .find(|(t, _)| t == name)
            .map(|(_, ids)| ids.iter().map(|&i| BlockId(i as u32)).collect())
            .unwrap_or_default()
    };
    let table = HeightmapTable::vanilla(
        &real.blocks,
        &real.state_data,
        &tag("minecraft:blocks_motion_in_heightmap"),
        &tag("minecraft:blocks_motion_in_heightmap_no_leaves"),
    );
    let registries = Registries::build(
        Vec::new(),
        &[DataPack::vanilla(&mut jar).unwrap(), DataPack::paper()],
    )
    .unwrap();
    let biome_registry = registries.synced("minecraft:worldgen/biome").unwrap();
    let biomes = BiomeIds::new(biome_registry);
    let biome_count = biome_registry.entries.len() as u32;
    let presets = BiomePresets::load(reports.as_ref(), &real.data, &biomes).unwrap();
    let world_presets = real.data.raw("minecraft:worldgen/world_preset").unwrap();
    let registry = real.data.features();
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
    let text = std::fs::read_to_string(oracle).expect("the oracle file");

    let mut stems: HashMap<(String, String, String, i64), Stem> = HashMap::new();
    let mut steps: Vec<StepFeatureData> = Vec::new();
    let mut context = DensityContext::cached();
    let (mut chunks, mut blocks, mut changed) = (0, 0u64, 0u64);
    let mut decorate_time = Duration::ZERO;
    let mut failures = Vec::new();
    let mut by_note: BTreeMap<String, usize> = BTreeMap::new();
    let mut expected_counts = 0usize;
    // The nine chunks after decoration: (dx, dz) -> chunk, and the label.
    type Decorated = (HashMap<(i32, i32), ProtoChunk>, String);
    let mut current: Option<Decorated> = None;

    let only = std::env::var("HORIZON_DECORATION_CHUNKS").ok();
    let mut skipping = false;
    for line in text.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        match words[0] {
            "stem" => {
                let seed: i64 = words[4].parse().unwrap();
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
                // The generator's feature order is built over `possibleBiomes()` in its order.
                let ours = stem.source.possible_biomes();
                let theirs: Vec<u32> = words[5..]
                    .iter()
                    .map(|name| biomes.id(name).expect("a biome"))
                    .collect();
                assert_eq!(ours, theirs, "possibleBiomes() of {}", words[1]);
                steps = registry
                    .features_per_step(&ours)
                    .expect("the feature order");
            }
            "chunk" => {
                let seed: i64 = words[4].parse().unwrap();
                let (cx, cz): (i32, i32) = (words[5].parse().unwrap(), words[6].parse().unwrap());
                // HORIZON_DECORATION_CHUNKS="cx,cz;cx,cz" runs only these centres.
                skipping = only
                    .as_ref()
                    .is_some_and(|only| !only.split(';').any(|c| c == format!("{cx},{cz}")));
                if skipping {
                    current = None;
                    continue;
                }
                let key = (
                    words[1].to_owned(),
                    words[2].to_owned(),
                    words[3].to_owned(),
                    seed,
                );
                let stem = &stems[&key];
                let settings = real.data.noise_settings(words[1]).unwrap();
                let (min_y, height): (i32, i32) =
                    (words[7].parse().unwrap(), words[8].parse().unwrap());
                let center = ChunkPos::new(cx, cz);
                let bc = BiomeChunks::build_radius(&real, stem, biome_count, center, seed, 2);
                let system = SurfaceSystem::new(
                    &real.data,
                    &stem.density,
                    settings,
                    &real.blocks,
                    seed,
                    false,
                )
                .expect("the surface system");
                let mut all: Vec<ProtoChunk> = Vec::new();
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let pos = ChunkPos::new(cx + dx, cz + dz);
                        let mut chunk = real.chunk(pos, min_y, height);
                        chunk.fill_biomes_from_noise(|q| {
                            bc.chunk(ChunkPos::new(q[0] >> 2, q[2] >> 2))
                                .noise_biome(q[0], q[1], q[2])
                        });
                        context.reset();
                        NoiseTerrain::new(&stem.density, settings, &real.blocks)
                            .unwrap()
                            .fill_with_aquifer(&mut chunk, &mut context, &real.state_data, &table)
                            .expect("fill")
                            .expect("a world with height");
                        system.build_surface(
                            &mut chunk,
                            &mut context,
                            &real.state_data,
                            &table,
                            |at| bc.biome_at(at),
                            None,
                        );
                        chunk.set_status(ChunkStatus::Terrain);
                        all.push(chunk);
                    }
                }
                let before: Vec<Vec<u32>> = all
                    .iter()
                    .map(crate::worldgen::surface::oracle_tests::snapshot)
                    .collect();
                let level = WorldGenerationContext::new(min_y, height, settings.sea_level);
                let rules = BlockSurvivalRules {
                    rules: &survival,
                    blocks: &real.blocks,
                    data: &real.state_data,
                };
                let start = Instant::now();
                let source_biomes =
                    crate::worldgen::structure::BiomeSet::new(stem.source.possible_biomes());
                {
                    let region_chunks: Vec<RegionChunk<'_>> =
                        all.iter_mut().map(RegionChunk::Writable).collect();
                    let mut region = GenerationRegion::new(center, 1, region_chunks);
                    let mut world = RegionWorld {
                        region: &mut region,
                        heightmaps: &table,
                        blocks: &real.blocks,
                        data: &real.state_data,
                        level,
                        generator: level,
                        zoom: bc.zoom,
                        survival: rules,
                        seed,
                        random: crate::worldgen::placement::world::RegionRandom::new(
                            seed, false, center,
                        ),
                    };
                    // Skipped on both sides: the types not ported, by the oracle's `PORTED`.
                    let skipped = super::decorate::unported_features(registry, &steps);
                    apply_biome_decoration(
                        &mut world,
                        registry,
                        &steps,
                        Some(&source_biomes),
                        seed,
                        &mut NoStructures,
                        |p| skipped.contains(&p),
                    )
                    .unwrap_or_else(|e| panic!("{e} ({} seed {seed})", words[1]));
                }
                decorate_time += start.elapsed();
                for (chunk, before) in all.iter().zip(&before) {
                    let after = crate::worldgen::surface::oracle_tests::snapshot(chunk);
                    changed += after.iter().zip(before).filter(|(a, b)| a != b).count() as u64;
                }
                chunks += 1;
                *by_note.entry(words[11].to_owned()).or_default() += 1;
                let label = format!("{} {seed} chunk {cx} {cz} ({})", words[1], words[11]);
                let map = all
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| ((i as i32 % 3 - 1, i as i32 / 3 - 1), c))
                    .collect();
                current = Some((map, label));
            }
            "count" => expected_counts += usize::from(!skipping),
            "sec" if skipping => {}
            "sec" => {
                let (map, label) = current.as_ref().unwrap();
                let (dx, dz): (i32, i32) = (words[1].parse().unwrap(), words[2].parse().unwrap());
                let section: usize = words[3].parse().unwrap();
                let chunk = &map[&(dx, dz)];
                let ids = expand(&words[4..]);
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
                                "{label}: chunk ({dx}, {dz}) block {x} {y} {z} is {} not {id}",
                                chunk.block_state(x, y, z).0
                            ));
                        }
                        bad += 1;
                    }
                }
                blocks += 4096;
            }
            other => panic!("unknown row {other}"),
        }
    }
    println!(
        "{chunks} chunks ({blocks} blocks, {changed} changed by the ported features, {expected_counts} \
         (chunk, feature) placements counted by Java); {} failures; FEATURES {:.1} ms per chunk (this build)",
        failures.len(),
        decorate_time.as_secs_f64() * 1000.0 / f64::from(chunks.max(1))
    );
    println!("chunks by note: {by_note:?}");
    // `updateShape` of the plants a tree's edge touches is `feature::shape`'s; every chunk is equal.
    assert!(
        failures.is_empty(),
        "{} failures, the first:\n{}",
        failures.len(),
        failures
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(chunks > 0);
}
