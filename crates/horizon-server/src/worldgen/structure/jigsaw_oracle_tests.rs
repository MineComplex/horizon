//! Jigsaw structures against the real 26.3 server: `LeafJigsawOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs `Structure.generate` for every
//! vanilla jigsaw structure over chunks of two seeds and places the first start of each into the
//! template oracle's synthetic world; this repeats each case through the level generator's STRUCTURE_STARTS
//! path ([`generate_with_worker`]) and compares the start (or none), every piece (element, position,
//! rotation, box, ground level, junctions), the start's NBT and every placement's `setBlock` calls,
//! loot seeds and the random's next draw.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafJigsawOracle.java jigsaw.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_JIGSAW_ORACLE=jigsaw.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     HORIZON_BIOME_REPORTS=<reports dir> \
//!     cargo test -p horizon-server --release jigsaw_matches_java -- --ignored --nocapture
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use super::piece_place::{PieceTables, PieceTarget, intersects, reference_of, writable_area};
use super::pool::ElementKind;
use super::start::{StructurePiece, StructureStart};
use super::template::StructureTemplates;
use super::template_oracle_tests::{Fixture, World};
use crate::block::flags::FLAG_RANDOMIZABLE_CONTAINER;
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, JavaString, Tag};
use crate::registry::{DataPack, Registries};
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

/// The canonical NBT form the oracle writes: sorted keys, typed numbers.
fn canonical(tag: &Tag, out: &mut String) {
    use std::fmt::Write;
    match tag {
        Tag::Compound(map) => {
            out.push('{');
            for (i, (key, value)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&key.to_string_lossy());
                out.push(':');
                canonical(value, out);
            }
            out.push('}');
        }
        Tag::List(list) => {
            out.push('[');
            for (i, value) in list.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(value, out);
            }
            out.push(']');
        }
        Tag::String(s) => {
            let _ = write!(out, "\"{}\"", s.to_string_lossy());
        }
        Tag::Byte(v) => {
            let _ = write!(out, "{v}b");
        }
        Tag::Short(v) => {
            let _ = write!(out, "{v}s");
        }
        Tag::Int(v) => {
            let _ = write!(out, "{v}");
        }
        Tag::Long(v) => {
            let _ = write!(out, "{v}L");
        }
        Tag::Float(bits) => {
            let _ = write!(out, "f{bits:x}");
        }
        Tag::Double(bits) => {
            let _ = write!(out, "d{bits:x}");
        }
        Tag::IntArray(values) => {
            let v: Vec<String> = values.iter().map(i32::to_string).collect();
            let _ = write!(out, "[I;{}]", v.join(","));
        }
        Tag::LongArray(values) => {
            let v: Vec<String> = values.iter().map(i64::to_string).collect();
            let _ = write!(out, "[L;{}]", v.join(","));
        }
        Tag::ByteArray(values) => {
            let v: Vec<String> = values.iter().map(|b| (*b as i8).to_string()).collect();
            let _ = write!(out, "[B;{}]", v.join(","));
        }
        Tag::End => out.push('?'),
    }
}

pub(crate) fn canonical_string(tag: &Tag) -> String {
    let mut out = String::new();
    canonical(tag, &mut out);
    out
}

fn text(tag: &Compound, key: &str) -> String {
    match tag.get(&JavaString::from(key)) {
        Some(Tag::String(s)) => s.to_string_lossy(),
        _ => String::new(),
    }
}

/// The oracle's element key.
fn key(element: &Compound) -> String {
    match text(element, "element_type").as_str() {
        "minecraft:single_pool_element" => format!("S:{}", text(element, "location")),
        "minecraft:legacy_single_pool_element" => format!("L:{}", text(element, "location")),
        "minecraft:feature_pool_element" => format!("F:{}", text(element, "feature")),
        "minecraft:list_pool_element" => {
            let children: Vec<String> = match element.get(&JavaString::from("elements")) {
                Some(Tag::List(list)) => list
                    .iter()
                    .map(|c| match c {
                        Tag::Compound(c) => key(c),
                        _ => String::new(),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            format!("[{}]", children.join(","))
        }
        _ => "E".to_owned(),
    }
}

fn rotation_name(rotation: crate::block::transform::Rotation) -> &'static str {
    [
        "NONE",
        "CLOCKWISE_90",
        "CLOCKWISE_180",
        "COUNTERCLOCKWISE_90",
    ][rotation as usize]
}

/// The NBT with every inline processor list removed, to tell element differences apart.
fn without_inline_processors(tag: &Tag) -> Tag {
    match tag {
        Tag::Compound(map) => Tag::Compound(
            map.iter()
                .filter(|(k, v)| {
                    !(k.to_string_lossy() == "processors" && matches!(v, Tag::Compound(_)))
                })
                .map(|(k, v)| (k.clone(), without_inline_processors(v)))
                .collect(),
        ),
        Tag::List(list) => Tag::List(list.iter().map(without_inline_processors).collect()),
        other => other.clone(),
    }
}

fn has_feature(registry: &super::StructureRegistry, element: super::pool::PoolElementId) -> bool {
    match &registry.pools.element(element).kind {
        ElementKind::Feature { .. } => true,
        ElementKind::List(children) => children.iter().any(|&c| has_feature(registry, c)),
        _ => false,
    }
}

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_JIGSAW_ORACLE (tools/leaf_server/README.md)"]
fn jigsaw_matches_java() {
    let oracle = env_path("HORIZON_JIGSAW_ORACLE").expect("HORIZON_JIGSAW_ORACLE");
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
    let templates = StructureTemplates::load(&mut jar, None, &blocks, &state_data).unwrap();
    data.set_templates(templates);
    let state_count = blocks.state_count();
    // Only the WG surface is read (`getFirstFreeHeight` with `WORLD_SURFACE_WG`).
    let heightmaps = Arc::new(HeightmapTable::new(state_count, |s| {
        if state_data.is_air(s) { 0 } else { 0b11 }
    }));
    let (data, blocks, flags) = (Arc::new(data), Arc::new(blocks), Arc::new(state_data));
    let registry = data.structures();
    let fixture = Fixture::load();
    let survival =
        Arc::new(crate::block::survival::BlockSurvival::new(&blocks, &flags, &[], &[]).unwrap());
    let mut generators: HashMap<(bool, i64), Arc<LevelGenerator>> = HashMap::new();
    let mut generator_for = |nether: bool, seed: i64| {
        generators
            .entry((nether, seed))
            .or_insert_with(|| {
                let stem_key = if nether {
                    "minecraft:the_nether"
                } else {
                    "minecraft:overworld"
                };
                let raw = required(&object(&dimensions[stem_key]).unwrap(), "generator")
                    .unwrap()
                    .to_owned();
                let stem = decode_noise_generator(&raw, &biomes, &presets).unwrap();
                let (min_y, height) = if nether { (0, 256) } else { (-64, 384) };
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
    let (mut cases, mut valid, mut pieces_seen, mut nbt_equal, mut placements, mut sets_seen) =
        (0, 0, 0, 0, 0, 0usize);
    let mut nbt_processor_only = 0;
    let mut failures: Vec<String> = Vec::new();
    let mut starts: HashMap<String, StructureStart> = HashMap::new();
    let started = std::time::Instant::now();
    while let Some(line) = lines.next() {
        let f: Vec<&str> = line.split('\t').collect();
        match f[0] {
            "case" => {
                cases += 1;
                let (name, seed) = (f[1], f[2].parse::<i64>().unwrap());
                let chunk = ChunkPos::new(f[3].parse().unwrap(), f[4].parse().unwrap());
                let want: usize = f[5].parse().unwrap();
                let nether = name == "minecraft:bastion_remnant";
                let generator = generator_for(nether, seed);
                let mut worker = generator.worker().unwrap();
                worker.reset_search();
                let structure = registry.structure_id(name).unwrap();
                let heights = if nether { (0, 256) } else { (-64, 384) };
                let start = generate_with_worker(&mut worker, structure, chunk, heights, 0);
                let label = format!("{name} {seed} {chunk:?}");
                let got = start.as_ref().map_or(0, |s| s.children.len());
                if got != want {
                    failures.push(format!("{label}: {got} pieces, not {want}"));
                }
                if want == 0 {
                    continue;
                }
                valid += 1;
                let mut first = true;
                for i in 0..want {
                    let row: Vec<&str> = lines.next().unwrap().split('\t').collect();
                    assert_eq!(row[0], "piece");
                    pieces_seen += 1;
                    let Some(StructurePiece::Jigsaw(piece)) =
                        start.as_ref().and_then(|s| s.children.get(i))
                    else {
                        continue;
                    };
                    let element = registry.pools.element_tag(piece.element);
                    let (min, max) = (piece.bounding_box.minimum(), piece.bounding_box.maximum());
                    let junctions: String = (piece.junctions.iter())
                        .map(|j| {
                            format!(
                                "{},{},{},{},{};",
                                j.source_x,
                                j.source_ground_y,
                                j.source_z,
                                j.delta_y,
                                j.dest_projection.name()
                            )
                        })
                        .collect();
                    let mine = [
                        key(&element),
                        format!(
                            "{},{},{}",
                            piece.position.x, piece.position.y, piece.position.z
                        ),
                        rotation_name(piece.rotation).to_owned(),
                        format!(
                            "{},{},{},{},{},{}",
                            min[0], min[1], min[2], max[0], max[1], max[2]
                        ),
                        piece.ground_level_delta.to_string(),
                        junctions,
                    ];
                    let java: Vec<String> = row[1..].iter().map(|s| (*s).to_owned()).collect();
                    if first && mine[..] != java[..] {
                        failures.push(format!(
                            "{label}: piece {i}: rust {mine:?}\n    java {java:?}"
                        ));
                        first = false;
                    }
                }
                let row: Vec<&str> = lines.next().unwrap().split('\t').collect();
                assert_eq!(row[0], "nbt");
                if let Some(start) = start {
                    let Tag::Compound(saved) = Tag::Compound(start.create_tag(registry, chunk))
                    else {
                        unreachable!()
                    };
                    // Loading the tag back gives the same pieces (boxes recomputed from the
                    // elements, as vanilla does: equal unless the expansion hack grew them).
                    // Elements are compared by what they save: a pool element listed by several
                    // pools has one copy per pool, and loading finds the first.
                    let loaded = StructureStart::load(registry, &saved, 0).unwrap().unwrap();
                    let same = |a: &StructurePiece, b: &StructurePiece| match (a, b) {
                        (StructurePiece::Jigsaw(a), StructurePiece::Jigsaw(b)) => {
                            registry.pools.element_tag(a.element)
                                == registry.pools.element_tag(b.element)
                                && a.position == b.position
                                && a.rotation == b.rotation
                                && a.junctions == b.junctions
                                && a.ground_level_delta == b.ground_level_delta
                        }
                        _ => false,
                    };
                    if loaded.children.len() != start.children.len()
                        || !loaded
                            .children
                            .iter()
                            .zip(&start.children)
                            .all(|(a, b)| same(a, b))
                    {
                        failures.push(format!("{label}: the saved start loads back differently"));
                    }
                    let tag = Tag::Compound(saved);
                    let mine = canonical_string(&tag);
                    if mine == row[1] {
                        nbt_equal += 1;
                    } else {
                        // Parse the Java form back through our SNBT? Compare without inline processors.
                        let stripped = canonical_string(&without_inline_processors(&tag));
                        let java_stripped = strip_inline_processors(row[1]);
                        if stripped == java_stripped {
                            nbt_processor_only += 1;
                        } else {
                            failures.push(format!(
                                "{label}: start nbt differs at {}",
                                first_difference(row[1], &mine)
                            ));
                        }
                    }
                    starts.insert(format!("{name}\t{seed}\t{}\t{}", f[3], f[4]), start);
                }
            }
            "place" => {
                placements += 1;
                let seed: i64 = f[2].parse().unwrap();
                let label = format!(
                    "place {} {seed} {},{} in {},{}",
                    f[1], f[3], f[4], f[5], f[6]
                );
                let Some(start) = starts.get(&format!("{}\t{}\t{}\t{}", f[1], f[2], f[3], f[4]))
                else {
                    failures.push(format!("{label}: no start"));
                    continue;
                };
                let target = ChunkPos::new(f[5].parse().unwrap(), f[6].parse().unwrap());
                let nether = f[1] == "minecraft:bastion_remnant";
                let (min_y, height) = if nether { (0, 256) } else { (-64, 384) };
                let chunk_box = writable_area(target, min_y, height);
                let tables = PieceTables {
                    structures: registry,
                    processors: data.processor_lists(),
                    features: data.features(),
                    seed,
                    known_shape: false,
                };
                let mut world = World {
                    f: &fixture,
                    writes: HashMap::new(),
                    sets: Vec::new(),
                    ticks: Vec::new(),
                    level_random: None,
                };
                let mut random = WorldgenRandom::new(XoroshiroRandom::new(seed ^ target.pack()));
                let reference = reference_of(start).unwrap();
                let mut placed = Vec::new();
                for piece in &start.children {
                    let StructurePiece::Jigsaw(piece) = piece else {
                        continue;
                    };
                    if !intersects(piece.bounding_box, chunk_box)
                        || has_feature(registry, piece.element)
                    {
                        continue;
                    }
                    let place_target = PieceTarget {
                        chunk_box,
                        reference,
                        keep_jigsaws: false,
                    };
                    if let Err(e) = tables.place_piece(
                        piece,
                        &mut world,
                        &mut random,
                        place_target,
                        &mut placed,
                    ) {
                        failures.push(format!("{label}: rust error {e}"));
                    }
                }
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
                let mut seeds: std::collections::BTreeMap<String, String> = Default::default();
                for (p, nbt) in placed.iter().flat_map(|p| &p.block_entities) {
                    let state = world.block_state(*p).unwrap();
                    let name = fixture.blocks.block_name(fixture.blocks.block(state));
                    let randomizable = fixture.data.flags(state) & FLAG_RANDOMIZABLE_CONTAINER != 0
                        || matches!(
                            name,
                            "minecraft:suspicious_sand" | "minecraft:suspicious_gravel"
                        );
                    let table = matches!(
                        nbt.get(&JavaString::from("LootTable")),
                        Some(Tag::String(_))
                    );
                    let pos = format!("{},{},{}", p.x, p.y, p.z);
                    match nbt.get(&JavaString::from("LootTableSeed")) {
                        Some(Tag::Long(seed)) if randomizable && table && *seed != 0 => {
                            seeds.insert(pos, seed.to_string());
                        }
                        _ => {
                            seeds.remove(&pos);
                        }
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
            }
            other => panic!("unknown row {other}"),
        }
    }
    println!(
        "{cases} cases ({valid} starts, {pieces_seen} pieces, {nbt_equal} start tags equal, \
         {nbt_processor_only} equal but for inline processor lists), {placements} placements \
         ({sets_seen} setBlock calls) in {:?}; {} failures",
        started.elapsed(),
        failures.len()
    );
    for failure in failures.iter().take(30) {
        println!("  {failure}");
    }
    assert!(failures.is_empty());
}

/// The oracle's canonical text with each `processors:{...}` value removed (as
/// [`without_inline_processors`] removes them from ours).
fn strip_inline_processors(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("processors:{") {
        out.push_str(&rest[..at]);
        let mut depth = 0;
        let mut end = at + "processors:".len();
        for (i, c) in rest[end..].char_indices() {
            match c {
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end += i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &rest[end..];
        // The separator before the removed field, or after it when it came first.
        if out.ends_with(',') {
            out.pop();
        } else if rest.starts_with(',') {
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

pub(super) fn first_difference(java: &str, rust: &str) -> String {
    let at = java
        .bytes()
        .zip(rust.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(java.len().min(rust.len()));
    let from = at.saturating_sub(60);
    format!(
        "{at}: java …{}… rust …{}…",
        &java[from..(at + 80).min(java.len())],
        &rust[from.min(rust.len())..(at + 80).min(rust.len())]
    )
}
