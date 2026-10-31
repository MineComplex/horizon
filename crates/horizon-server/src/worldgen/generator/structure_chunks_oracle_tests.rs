//! Whole chunks with structures against the real 26.3 server: `LeafStructureChunksOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) finds the first start of every
//! overworld structure around the origin (and the nearest stronghold), then takes the 2 x 2 chunks
//! at each start's chunk through STRUCTURE_STARTS (structures on), STRUCTURE_REFERENCES, BIOMES,
//! TERRAIN (with the beardifier) and FEATURES (the starts placed, then the features), one chunk at a
//! time. This runs the same steps in the same order through [`LevelGenerator`]'s worker steps over
//! generation regions, applies each FEATURES step's piece changes to the chunks holding the starts
//! as the chunk system does, and compares every block of the chunks around the decorated ones and
//! the saved `structures` tag of every chunk with starts or references.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafStructureChunksOracle.java structure_chunks.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_STRUCTURE_CHUNKS_ORACLE=structure_chunks.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     HORIZON_BIOME_REPORTS=<reports dir> \
//!     cargo test -p horizon-server --release structure_chunks_match_java -- --ignored --nocapture
//! ```
//!
//! Like the oracle, template pieces place with `setKnownShape(true)` (block behaviour is not
//! ported), entities are not made, and the feature types not ported are skipped on both sides.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{GeneratorTables, LevelGenerator, LevelGeneratorSettings, decorate::decorate_with};
use crate::block::{BlockId, BlockState};
use crate::core::position::ChunkPos;
use crate::nbt::Tag;
use crate::registry::{DataPack, Registries, load_tags, registry_dir};
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::{ChunkStatus, ContainerFactory};
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::biome::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use crate::worldgen::codec::{object, required};
use crate::worldgen::proto_chunk::{GenerationRegion, ProtoChunk, RegionChunk};
use crate::worldgen::structure::jigsaw_oracle_tests::canonical_string;
use crate::worldgen::structure::start::PieceChange;
use crate::worldgen::structure::template::StructureTemplates;
use crate::worldgen::terrain::oracle_tests::{env_path, expand, load_real};

/// Runs `step` over the region of `radius` around `center` whose chunks within `write` are
/// writable, then gives the step's piece changes to the chunks holding their starts.
fn run_region(
    chunks: &mut HashMap<ChunkPos, ProtoChunk>,
    center: ChunkPos,
    radius: i32,
    write: i32,
    step: impl FnOnce(&mut GenerationRegion<'_>),
) -> usize {
    let square = |r: i32| {
        (-r..=r)
            .flat_map(move |dz| (-r..=r).map(move |dx| (dx, dz)))
            .map(move |(dx, dz)| ChunkPos::new(center.x + dx, center.z + dz))
    };
    let writable =
        |pos: ChunkPos| (pos.x - center.x).abs() <= write && (pos.z - center.z).abs() <= write;
    let mut owned: Vec<ProtoChunk> = (square(radius))
        .filter(|&p| writable(p))
        .map(|p| chunks.remove(&p).expect("a writable chunk"))
        .collect();
    let changes: Vec<PieceChange> = {
        let mut owned_iter = owned.iter_mut();
        let slots: Vec<RegionChunk<'_>> = square(radius)
            .map(|p| match writable(p) {
                true => RegionChunk::Writable(owned_iter.next().unwrap()),
                false => RegionChunk::ReadOnly(&chunks[&p]),
            })
            .collect();
        let mut region = GenerationRegion::new(center, radius, slots);
        step(&mut region);
        region.take_piece_changes()
    };
    for chunk in owned {
        chunks.insert(chunk.pos(), chunk);
    }
    let count = changes.len();
    for change in &changes {
        chunks
            .get_mut(&change.origin)
            .expect("the start's chunk")
            .structures
            .apply_change(change);
    }
    count
}

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_STRUCTURE_CHUNKS_ORACLE (tools/leaf_server/README.md)"]
fn structure_chunks_match_java() {
    let oracle =
        env_path("HORIZON_STRUCTURE_CHUNKS_ORACLE").expect("HORIZON_STRUCTURE_CHUNKS_ORACLE");
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
    let registries = Registries::build(Vec::new(), &[pack.clone(), DataPack::paper()]).unwrap();
    let biome_registry = registries.synced("minecraft:worldgen/biome").unwrap();
    let biomes = BiomeIds::new(biome_registry);
    let biome_count = biome_registry.entries.len() as u32;
    let plains = biomes.id("minecraft:plains").unwrap();
    let presets = BiomePresets::load(reports.as_ref(), &real.data, &biomes).unwrap();
    let world_presets = real.data.raw("minecraft:worldgen/world_preset").unwrap();
    let normal = (world_presets.elements.iter())
        .find(|(id, _)| id == "minecraft:normal")
        .map(|(_, raw)| raw)
        .unwrap();
    let dimensions = object(required(&object(normal).unwrap(), "dimensions").unwrap()).unwrap();
    let raw_generator = required(
        &object(&dimensions["minecraft:overworld"]).unwrap(),
        "generator",
    )
    .unwrap()
    .to_owned();
    let state_count = real.blocks.state_count();
    let crate::worldgen::terrain::oracle_tests::Real {
        mut data,
        blocks,
        state_data,
    } = real;
    data.set_templates(StructureTemplates::load(&mut jar, None, &blocks, &state_data).unwrap());
    let (data, blocks, flags) = (Arc::new(data), Arc::new(blocks), Arc::new(state_data));
    let (heightmaps, survival) = (Arc::new(table), Arc::new(survival));
    let text = std::fs::read_to_string(oracle).expect("the oracle file");
    let only = std::env::var("HORIZON_STRUCTURE_CHUNKS_CASES").ok();

    let mut generators: HashMap<i64, LevelGenerator> = HashMap::new();
    let mut failures: Vec<String> = Vec::new();
    let (mut cases, mut sections, mut tags, mut changes) = (0, 0u64, 0, 0);
    let mut per_structure: BTreeMap<String, usize> = BTreeMap::new();
    let mut time = Duration::ZERO;
    // The case being compared: its chunks, label, and the chunks whose structures were compared.
    type Case = (
        HashMap<ChunkPos, ProtoChunk>,
        String,
        HashSet<ChunkPos>,
        (i32, i32),
    );
    let mut current: Option<Case> = None;
    let finish = |current: &mut Option<Case>, failures: &mut Vec<String>| {
        let Some((chunks, label, compared, (x0, z0))) = current.take() else {
            return;
        };
        // Every chunk of the case with starts or references had a row.
        for z in z0 - 9..=z0 + 10 {
            for x in x0 - 9..=x0 + 10 {
                let pos = ChunkPos::new(x, z);
                let structures = &chunks[&pos].structures;
                let any = structures.starts().next().is_some()
                    || structures
                        .all_references()
                        .any(|(_, keys)| !keys.is_empty());
                if any && !compared.contains(&pos) {
                    failures.push(format!("{label}: chunk {x} {z} has structures, java none"));
                }
            }
        }
    };
    let mut skipping = false;
    for line in text.lines() {
        let words: Vec<&str> = line.splitn(5, ' ').collect();
        match words[0] {
            "case" => {
                finish(&mut current, &mut failures);
                let words: Vec<&str> = line.split(' ').collect();
                let seed: i64 = words[1].parse().unwrap();
                let structure = words[2];
                let (x0, z0): (i32, i32) = (words[3].parse().unwrap(), words[4].parse().unwrap());
                let label = format!("{seed} {structure} at {x0} {z0}");
                skipping = only
                    .as_ref()
                    .is_some_and(|o| !o.split(';').any(|c| c == structure));
                if skipping {
                    continue;
                }
                let generator = &*generators.entry(seed).or_insert_with(|| {
                    let stem = decode_noise_generator(&raw_generator, &biomes, &presets).unwrap();
                    LevelGenerator::new(
                        GeneratorTables {
                            data: data.clone(),
                            blocks: blocks.clone(),
                            flags: flags.clone(),
                            heightmaps: heightmaps.clone(),
                            survival: survival.clone(),
                        },
                        LevelGeneratorSettings {
                            stem,
                            seed,
                            generate_structures: true,
                            flat_bedrock: false,
                            end_void_rings: false,
                            spigot_seeds: Default::default(),
                            min_section_y: -4,
                            section_count: 24,
                            factory: ContainerFactory::new(state_count, biome_count, plains),
                        },
                    )
                    .expect("the generator")
                });
                let started = Instant::now();
                let mut worker = generator.worker().unwrap();
                worker.reset_search();
                let mut chunks: HashMap<ChunkPos, ProtoChunk> = HashMap::new();
                let at = |x: i32, z: i32| ChunkPos::new(x, z);
                for z in z0 - 9..=z0 + 10 {
                    for x in x0 - 9..=x0 + 10 {
                        chunks.insert(at(x, z), generator.empty_chunk(at(x, z)));
                        run_region(&mut chunks, at(x, z), 0, 0, |region| {
                            worker.run(ChunkStatus::StructureStarts, region).unwrap();
                        });
                    }
                }
                for z in z0 - 1..=z0 + 2 {
                    for x in x0 - 1..=x0 + 2 {
                        run_region(&mut chunks, at(x, z), 8, 0, |region| {
                            worker
                                .run(ChunkStatus::StructureReferences, region)
                                .unwrap();
                        });
                    }
                }
                for z in z0 - 2..=z0 + 3 {
                    for x in x0 - 2..=x0 + 3 {
                        run_region(&mut chunks, at(x, z), 0, 0, |region| {
                            worker.run(ChunkStatus::Biomes, region).unwrap();
                        });
                    }
                }
                for z in z0 - 1..=z0 + 2 {
                    for x in x0 - 1..=x0 + 2 {
                        run_region(&mut chunks, at(x, z), 8, 0, |region| {
                            worker.run(ChunkStatus::Terrain, region).unwrap();
                        });
                    }
                }
                drop(worker);
                for z in z0..=z0 + 1 {
                    for x in x0..=x0 + 1 {
                        changes += run_region(&mut chunks, at(x, z), 8, 1, |region| {
                            if let Some(e) = decorate_with(generator, region, true).unwrap() {
                                failures.push(format!("{label}: chunk {x} {z} decoration: {e}"));
                            }
                            let center = region.center();
                            region
                                .chunk_mut(center)
                                .unwrap()
                                .set_status(ChunkStatus::Features);
                        });
                    }
                }
                time += started.elapsed();
                cases += 1;
                *per_structure.entry(structure.to_owned()).or_default() += 1;
                current = Some((chunks, label, HashSet::new(), (x0, z0)));
            }
            _ if skipping => {}
            "sec" => {
                let words: Vec<&str> = line.split(' ').collect();
                let (chunks, label, _, _) = current.as_ref().unwrap();
                let pos = ChunkPos::new(words[1].parse().unwrap(), words[2].parse().unwrap());
                let section: i32 = words[3].parse().unwrap();
                let ids = expand(&words[4..]);
                let chunk = &chunks[&pos];
                let base = chunk.min_y() + section * 16;
                let (bx, bz) = (pos.x << 4, pos.z << 4);
                sections += 1;
                for (i, &id) in ids.iter().enumerate() {
                    let (x, y, z) = (
                        bx + (i & 15) as i32,
                        base + (i >> 8) as i32,
                        bz + ((i >> 4) & 15) as i32,
                    );
                    let got = chunk.block_state(x, y, z);
                    if got != BlockState(id) {
                        let bad = (ids.iter().enumerate())
                            .filter(|&(j, &want)| {
                                let (x, y, z) = (
                                    bx + (j & 15) as i32,
                                    base + (j >> 8) as i32,
                                    bz + ((j >> 4) & 15) as i32,
                                );
                                chunk.block_state(x, y, z) != BlockState(want)
                            })
                            .count();
                        failures.push(format!(
                            "{label}: block {x} {y} {z} is {} ({}) not {id} ({}); {bad} in the section",
                            got.0,
                            blocks.block_name(blocks.block(got)),
                            blocks.block_name(blocks.block(BlockState(id)))
                        ));
                        break;
                    }
                }
            }
            "struct" => {
                let (chunks, label, compared, _) = current.as_mut().unwrap();
                let pos = ChunkPos::new(words[1].parse().unwrap(), words[2].parse().unwrap());
                compared.insert(pos);
                tags += 1;
                let registry = data.structures();
                let mine =
                    canonical_string(&Tag::Compound(chunks[&pos].structures.save(registry, pos)));
                if mine != words[3] {
                    let at = (mine.bytes().zip(words[3].bytes()))
                        .position(|(a, b)| a != b)
                        .unwrap_or(mine.len().min(words[3].len()));
                    let from = at.saturating_sub(60);
                    failures.push(format!(
                        "{label}: structures of chunk {} {} differ at {at}: ours …{}… java …{}…",
                        pos.x,
                        pos.z,
                        &mine[from..(at + 80).min(mine.len())],
                        &words[3][from..(at + 80).min(words[3].len())]
                    ));
                }
            }
            other => panic!("unknown row {other}"),
        }
    }
    finish(&mut current, &mut failures);
    println!(
        "{cases} cases ({} structures), {sections} sections, {tags} structures tags, {changes} piece \
         changes; {} failures; {:.0} ms per case",
        per_structure.len(),
        failures.len(),
        time.as_secs_f64() * 1000.0 / f64::from(cases.max(1))
    );
    for (structure, n) in &per_structure {
        println!("  {structure}: {n}");
    }
    for failure in failures.iter().take(usize::MAX) {
        println!("  {failure}");
    }
    assert!(failures.is_empty(), "{} failures", failures.len());
    assert!(only.is_some() || cases >= 40);
}
