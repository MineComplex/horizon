//! The structures built from templates against the real 26.3 server: `LeafTemplateStructuresOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs `Structure.generate` for every vanilla
//! shipwreck, ocean ruin, igloo, ruined portal, nether fossil, end city and woodland mansion over chunks of two
//! seeds and places the first start of each (as Paper's `placeInChunk` does) into the template oracle's
//! synthetic world; this repeats each case through the level generator's STRUCTURE_STARTS path
//! ([`generate_with_worker`]) and compares the start (or none), every piece's saved tag, the start's tag, the
//! tag loaded back, and every placement's `setBlock` calls, loot seeds, both randoms' next draws and the
//! pieces' tags after it (what `postProcess` changed).
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafTemplateStructuresOracle.java template_structures.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_TEMPLATE_STRUCTURES_ORACLE=template_structures.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     HORIZON_BIOME_REPORTS=<reports dir> \
//!     cargo test -p horizon-server --release template_structures_match_java -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::jigsaw_oracle_tests::{canonical_string, first_difference};
use super::piece_place::{PieceTables, writable_area};
use super::start::StructureStart;
use super::template::StructureTemplates;
use super::template_oracle_tests::{Fixture, World};
use super::template_pieces::{PieceEvent, PieceOutput};
use crate::block::flags::FLAG_RANDOMIZABLE_CONTAINER;
use crate::core::position::ChunkPos;
use crate::nbt::{JavaString, Tag};
use crate::registry::{DataPack, Registries, load_tags, registry_dir};
use crate::server::vanilla_jar::VanillaJar;
use crate::util::random::{RandomSource, WorldgenRandom, XoroshiroRandom};
use crate::world::chunk::ContainerFactory;
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::biome::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use crate::worldgen::codec::{object, required};
use crate::worldgen::generator::structures::generate_with_worker;
use crate::worldgen::generator::{GeneratorTables, LevelGenerator, LevelGeneratorSettings};
use crate::worldgen::placement::PlacementWorld;
use crate::worldgen::terrain::oracle_tests::{env_path, load_real};

/// The dimension a structure is generated in (the oracle's choice): 0 overworld, 1 nether, 2 end.
fn dimension(name: &str) -> usize {
    match name {
        "minecraft:nether_fossil" | "minecraft:ruined_portal_nether" => 1,
        "minecraft:end_city" => 2,
        _ => 0,
    }
}

/// `(min_y, height)` of a dimension.
fn heights(dimension: usize) -> (i32, i32) {
    if dimension == 0 { (-64, 384) } else { (0, 256) }
}

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_TEMPLATE_STRUCTURES_ORACLE (tools/leaf_server/README.md)"]
fn template_structures_match_java() {
    let oracle =
        env_path("HORIZON_TEMPLATE_STRUCTURES_ORACLE").expect("HORIZON_TEMPLATE_STRUCTURES_ORACLE");
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
    let normal = (real
        .data
        .raw("minecraft:worldgen/world_preset")
        .unwrap()
        .elements
        .iter())
    .find(|(id, _)| id == "minecraft:normal")
    .map(|(_, raw)| raw.to_owned())
    .unwrap();
    let dimensions = object(required(&object(&normal).unwrap(), "dimensions").unwrap()).unwrap();
    let crate::worldgen::terrain::oracle_tests::Real {
        mut data,
        blocks,
        state_data,
    } = real;
    let vanilla = DataPack::vanilla(&mut jar).unwrap();
    // The heightmap predicates: `!isAir()` for the surfaces, `blocks_motion_in_heightmap` for the ocean
    // floors.
    let block_tags = load_tags(
        std::slice::from_ref(&vanilla),
        &registry_dir("minecraft:block"),
        |id| blocks.block_by_name(id).map(|b| b.0 as i32),
    );
    let motion: HashSet<_> = block_tags
        .iter()
        .find(|(name, _)| name == "minecraft:blocks_motion_in_heightmap")
        .map(|(_, ids)| {
            ids.iter()
                .map(|&i| crate::block::BlockId(i as u32))
                .collect()
        })
        .unwrap_or_default();
    let templates = StructureTemplates::load(&mut jar, None, &blocks, &state_data).unwrap();
    data.set_templates(templates);
    let state_count = blocks.state_count();
    let heightmaps = Arc::new(HeightmapTable::vanilla(
        &blocks,
        &state_data,
        &motion,
        &HashSet::new(),
    ));
    let (data, blocks, flags) = (Arc::new(data), Arc::new(blocks), Arc::new(state_data));
    let registry = data.structures();
    let fixture = Fixture::load();
    let survival =
        Arc::new(crate::block::survival::BlockSurvival::new(&blocks, &flags, &[], &[]).unwrap());
    let mut generators: HashMap<(usize, i64), Arc<LevelGenerator>> = HashMap::new();
    let mut generator_for = |dimension: usize, seed: i64| {
        generators
            .entry((dimension, seed))
            .or_insert_with(|| {
                let stem_key = [
                    "minecraft:overworld",
                    "minecraft:the_nether",
                    "minecraft:the_end",
                ][dimension];
                let raw = required(&object(&dimensions[stem_key]).unwrap(), "generator")
                    .unwrap()
                    .to_owned();
                let stem = decode_noise_generator(&raw, &biomes, &presets).unwrap();
                let (min_y, height) = heights(dimension);
                Arc::new(
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
                            min_section_y: min_y >> 4,
                            section_count: (height / 16) as usize,
                            factory: ContainerFactory::new(state_count, biome_count, plains),
                        },
                    )
                    .expect("the generator"),
                )
            })
            .clone()
    };
    let text_file = std::fs::read_to_string(oracle).expect("the oracle file");
    let mut lines = text_file.lines().peekable();
    let mut counts: BTreeMap<String, [usize; 3]> = BTreeMap::new();
    let (mut pieces_seen, mut sets_seen) = (0, 0usize);
    let mut failures: Vec<String> = Vec::new();
    let mut starts: HashMap<String, StructureStart> = HashMap::new();
    let started = std::time::Instant::now();
    while let Some(line) = lines.next() {
        let f: Vec<&str> = line.split('\t').collect();
        match f[0] {
            "case" => {
                let (name, seed) = (f[1], f[2].parse::<i64>().unwrap());
                let chunk = ChunkPos::new(f[3].parse().unwrap(), f[4].parse().unwrap());
                let want: usize = f[5].parse().unwrap();
                let dim = dimension(name);
                let generator = generator_for(dim, seed);
                let mut worker = generator.worker().unwrap();
                worker.reset_search();
                let structure = registry.structure_id(name).unwrap();
                let start = generate_with_worker(&mut worker, structure, chunk, heights(dim), 0);
                let label = format!("{name} {seed} {chunk:?}");
                let got = start.as_ref().map_or(0, |s| s.children.len());
                let entry = counts.entry(name.to_owned()).or_default();
                entry[0] += 1;
                if got != want {
                    failures.push(format!("{label}: {got} pieces, not {want}"));
                }
                if want == 0 {
                    continue;
                }
                entry[1] += 1;
                let mut java_pieces = Vec::new();
                for _ in 0..want {
                    let row: Vec<&str> = lines.next().unwrap().split('\t').collect();
                    assert_eq!(row[0], "piece");
                    java_pieces.push(row[1].to_owned());
                }
                pieces_seen += want;
                let nbt: Vec<&str> = lines.next().unwrap().split('\t').collect();
                assert_eq!(nbt[0], "nbt");
                let loaded: Vec<&str> = lines.next().unwrap().split('\t').collect();
                assert_eq!(loaded[0], "loaded");
                let Some(start) = start else { continue };
                for (i, java) in java_pieces.iter().enumerate() {
                    let Some(piece) = start.children.get(i) else {
                        continue;
                    };
                    let mine = canonical_string(&Tag::Compound(piece.create_tag(registry)));
                    if mine != *java {
                        failures.push(format!(
                            "{label}: piece {i} differs at {}",
                            first_difference(java, &mine)
                        ));
                        break;
                    }
                }
                let saved = start.create_tag(registry, chunk);
                let mine = canonical_string(&Tag::Compound(saved.clone()));
                if mine == nbt[1] {
                    entry[2] += 1;
                } else {
                    failures.push(format!(
                        "{label}: start nbt differs at {}",
                        first_difference(nbt[1], &mine)
                    ));
                }
                // The saved start loaded back, saved again.
                match StructureStart::load(registry, &saved, 0) {
                    Ok(Some(reloaded)) => {
                        let again =
                            canonical_string(&Tag::Compound(reloaded.create_tag(registry, chunk)));
                        if again != loaded[1] {
                            failures.push(format!(
                                "{label}: loaded start differs at {}",
                                first_difference(loaded[1], &again)
                            ));
                        }
                    }
                    other => failures.push(format!("{label}: the start did not load: {other:?}")),
                }
                starts.insert(format!("{name}\t{seed}\t{}\t{}", f[3], f[4]), start);
            }
            "place" => {
                let seed: i64 = f[2].parse().unwrap();
                let label = format!(
                    "place {} {seed} {},{} in {},{}",
                    f[1], f[3], f[4], f[5], f[6]
                );
                let key = format!("{}\t{}\t{}\t{}", f[1], f[2], f[3], f[4]);
                let target = ChunkPos::new(f[5].parse().unwrap(), f[6].parse().unwrap());
                let (min_y, height) = heights(dimension(f[1]));
                let mut chunk_box = writable_area(target, min_y, height);
                let tables = PieceTables {
                    structures: registry,
                    processors: data.processor_lists(),
                    features: data.features(),
                    seed,
                    known_shape: true,
                };
                let mut world = World {
                    f: &fixture,
                    writes: HashMap::new(),
                    sets: Vec::new(),
                    ticks: Vec::new(),
                    level_random: Some(XoroshiroRandom::new(seed ^ 77)),
                };
                let mut random = WorldgenRandom::new(XoroshiroRandom::new(seed ^ target.pack()));
                let mut out = PieceOutput::default();
                let Some(start) = starts.get_mut(&key) else {
                    failures.push(format!("{label}: no start"));
                    continue;
                };
                if let Err(e) =
                    tables.place_start(start, &mut world, &mut random, &mut chunk_box, &mut out)
                {
                    failures.push(format!("{label}: rust error {e}"));
                }
                counts.entry(f[1].to_owned()).or_default();
                let sets = world.sets.join(";");
                sets_seen += world.sets.len();
                if sets != f[7] {
                    failures.push(format!(
                        "{label}: {} setBlock calls, java {}; first difference at {}",
                        world.sets.len(),
                        f[7].split(';').filter(|s| !s.is_empty()).count(),
                        first_difference(f[7], &sets)
                    ));
                }
                let mut seeds: BTreeMap<String, String> = BTreeMap::new();
                let randomizable = |world: &World<'_>, p: crate::core::position::BlockPos| {
                    let state = world.block_state(p).unwrap();
                    let name = fixture.blocks.block_name(fixture.blocks.block(state));
                    fixture.data.flags(state) & FLAG_RANDOMIZABLE_CONTAINER != 0
                        || matches!(
                            name,
                            "minecraft:suspicious_sand" | "minecraft:suspicious_gravel"
                        )
                };
                for event in &out.events {
                    match event {
                        PieceEvent::Template(placed) => {
                            for (p, nbt) in &placed.block_entities {
                                let table = matches!(
                                    nbt.get(&JavaString::from("LootTable")),
                                    Some(Tag::String(_))
                                );
                                let pos = format!("{},{},{}", p.x, p.y, p.z);
                                match nbt.get(&JavaString::from("LootTableSeed")) {
                                    Some(Tag::Long(seed))
                                        if randomizable(&world, *p) && table && *seed != 0 =>
                                    {
                                        seeds.insert(pos, seed.to_string());
                                    }
                                    _ => {
                                        seeds.remove(&pos);
                                    }
                                }
                            }
                        }
                        PieceEvent::Loot { pos, seed, .. } => {
                            if *seed != 0 && randomizable(&world, *pos) {
                                seeds.insert(
                                    format!("{},{},{}", pos.x, pos.y, pos.z),
                                    seed.to_string(),
                                );
                            }
                        }
                        PieceEvent::Spawn { .. } => {}
                    }
                }
                let seeds: String = seeds.iter().map(|(p, s)| format!("{p},{s};")).collect();
                if seeds != f[8] {
                    failures.push(format!("{label}: loot seeds {seeds} not {}", f[8]));
                }
                let next = random.next_long().to_string();
                if next != f[9] {
                    failures.push(format!("{label}: next long {next} not {}", f[9]));
                }
                // A spawned mob's `finalizeSpawn` draws from the level random too: with the entity port.
                let spawned = (out.events.iter()).any(|e| matches!(e, PieceEvent::Spawn { .. }));
                let level_random = world.level_random.as_mut().expect("the level random");
                let level_next = level_random.next_long().to_string();
                if !spawned && level_next != f[10] {
                    failures.push(format!("{label}: level random {level_next} not {}", f[10]));
                }
                let after: Vec<&str> = lines.next().unwrap().split('\t').collect();
                assert_eq!(after[0], "after");
                let mine: Vec<String> = (start.children.iter())
                    .map(|p| canonical_string(&Tag::Compound(p.create_tag(registry))))
                    .collect();
                let mine = mine.join("|");
                if mine != after[7] {
                    failures.push(format!(
                        "{label}: pieces after the placement differ at {}",
                        first_difference(after[7], &mine)
                    ));
                }
            }
            other => panic!("unknown row {other}"),
        }
    }
    let mut report = String::new();
    for (name, [cases, valid, tags]) in &counts {
        report.push_str(&format!(
            "\n  {name}: {cases} cases, {valid} starts, {tags} tags equal"
        ));
    }
    println!(
        "{} pieces, {sets_seen} setBlock calls in {:?}; {} failures{report}",
        pieces_seen,
        started.elapsed(),
        failures.len()
    );
    for failure in failures.iter().take(40) {
        println!("  {failure}");
    }
    assert!(failures.is_empty());
}
