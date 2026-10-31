//! Structure placement against the real 26.3 server: `LeafStructurePlacementOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) asks vanilla's own classes, and this
//! compares every answer.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafStructurePlacementOracle.java structures.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_STRUCTURE_ORACLE=structures.txt \
//!     cargo test -p horizon-server structure_placement_matches_java -- --ignored --nocapture
//! ```

use std::collections::HashMap;

use super::placement::{LevelParams, LevelPlacements, ring_searches};
use super::start::{StructureStart, hash_set_order};
use super::{BiomeSet, FrequencyReduction, Placement, StructureRegistry, StructureSetId};
use crate::core::position::ChunkPos;
use crate::nbt::{Tag, parse_snbt};
use crate::server::config::SpigotWorldSeeds;
use crate::worldgen::oracle_tests::load_jar_data;

/// The oracle's `CheckerBiomeSource`: plains on rare quarts, deep ocean elsewhere.
fn preferred(qx: i32, qz: i32) -> bool {
    let h = qx.wrapping_mul(73428767) ^ qz.wrapping_mul(912931);
    h.rem_euclid(97) < 2 && (qx >> 5).rem_euclid(3) != 0
}

fn biome_id(registry_biomes: &[String], name: &str) -> u32 {
    registry_biomes
        .iter()
        .position(|b| b == name)
        .expect("biome") as u32
}

/// The level of one seed as the oracle builds it (all sets, the checkerboard biome source).
fn level(registry: &StructureRegistry, biomes: &[String], seed: i64) -> LevelPlacements {
    let (plains, ocean) = (
        biome_id(biomes, "minecraft:plains"),
        biome_id(biomes, "minecraft:deep_ocean"),
    );
    let possible = BiomeSet::new(vec![plains, ocean]);
    let sets: Vec<StructureSetId> = (0..registry.sets.len() as u32)
        .map(StructureSetId)
        .collect();
    let seeds = SpigotWorldSeeds::default();
    let params = LevelParams {
        seed,
        ring_seed: seed,
        origin: ChunkPos::new(0, 0),
        sets: &sets,
        possible_biomes: &possible,
        seeds: &seeds,
    };
    let positions: Vec<ChunkPos> = ring_searches(registry, &params)
        .iter()
        .map(|search| {
            let Placement::ConcentricRings(rings) = &registry.set(search.set).placement else {
                unreachable!("ring searches belong to ring sets")
            };
            search.find(&rings.preferred_biomes, |qx, _, qz| {
                if preferred(qx, qz) { plains } else { ocean }
            })
        })
        .collect();
    LevelPlacements::new(registry, &params, &positions)
}

#[test]
#[ignore = "needs the 26.3 jar and LeafStructurePlacementOracle's output"]
fn structure_placement_matches_java() {
    let data = load_jar_data().expect("HORIZON_VANILLA_JAR is not set");
    let registry = data.structures();
    let biomes = biome_ids();
    let oracle = std::fs::read_to_string(
        std::env::var("HORIZON_STRUCTURE_ORACLE").expect("HORIZON_STRUCTURE_ORACLE"),
    )
    .unwrap();
    let mut levels: HashMap<i64, LevelPlacements> = HashMap::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for line in oracle.lines() {
        let (kind, rest) = line.split_once(' ').unwrap();
        *counts.entry(kind).or_default() += 1;
        let words: Vec<&str> = rest.split(' ').collect();
        match kind {
            "spread" => {
                let seed: i64 = words[0].parse().unwrap();
                let set = registry.set_id(words[1]).unwrap();
                let [x, z, px, pz]: [i32; 4] = [2, 3, 4, 5].map(|i| words[i].parse().unwrap());
                let Placement::RandomSpread(spread) = &registry.set(set).placement else {
                    panic!("{line}");
                };
                let potential = spread.potential_chunk(seed, spread.spreading.salt, x, z);
                assert_eq!(potential, ChunkPos::new(px, pz), "{line}");
                let level = levels
                    .entry(seed)
                    .or_insert_with(|| level(registry, &biomes, seed));
                let placed = level.has_structure_chunk_in_range(registry, set, x, z, 0);
                assert_eq!(placed.to_string(), words[6], "{line}");
            }
            "reducer" => {
                let method = match words[0] {
                    "default" => FrequencyReduction::Default,
                    "legacy_type_1" => FrequencyReduction::LegacyType1,
                    "legacy_type_2" => FrequencyReduction::LegacyType2,
                    "legacy_type_3" => FrequencyReduction::LegacyType3,
                    other => panic!("{other}"),
                };
                let seed: i64 = words[1].parse().unwrap();
                let [salt, x, z, bits]: [i32; 4] = [2, 3, 4, 5].map(|i| words[i].parse().unwrap());
                let probability = f32::from_bits(bits as u32);
                let generate = method.should_generate(seed, salt, x, z, probability, None);
                assert_eq!(generate.to_string(), words[6], "{line}");
            }
            "ring" => {
                let seed: i64 = words[0].parse().unwrap();
                let set = registry.set_id(words[1]).unwrap();
                let [index, x, z]: [i32; 3] = [2, 3, 4].map(|i| words[i].parse().unwrap());
                let level = levels
                    .entry(seed)
                    .or_insert_with(|| level(registry, &biomes, seed));
                let positions = level.ring_positions(set).expect("ring positions");
                assert_eq!(positions[index as usize], ChunkPos::new(x, z), "{line}");
            }
            "hashset" => {
                let (keys, order) = rest.split_once('|').unwrap();
                let parse = |text: &str| -> Vec<i64> {
                    text.split_whitespace()
                        .map(|k| k.parse().unwrap())
                        .collect()
                };
                assert_eq!(hash_set_order(&parse(keys)), parse(order), "{line}");
            }
            "start" => {
                let mut parts = rest.split(" | ");
                let (input, output, bounds) = (
                    parts.next().unwrap(),
                    parts.next().unwrap(),
                    parts.next().unwrap(),
                );
                let Tag::Compound(input) = parse_snbt(input).unwrap().0 else {
                    panic!("{line}");
                };
                let start = StructureStart::load(registry, &input, 0).unwrap();
                match output {
                    "null" => assert!(start.is_none(), "{line}"),
                    output => {
                        let start = start.expect(line);
                        let Tag::Compound(expected) = parse_snbt(output).unwrap().0 else {
                            panic!("{line}");
                        };
                        assert_eq!(
                            start.create_tag(registry, ChunkPos::new(3, -7)),
                            expected,
                            "{line}"
                        );
                        let box_text = start.bounding_box.filter(|_| start.is_valid()).map_or(
                            "none".to_owned(),
                            |b| {
                                let ([a, b2, c], [d, e, f]) = (b.minimum(), b.maximum());
                                format!("{a} {b2} {c} {d} {e} {f}")
                            },
                        );
                        assert_eq!(box_text, bounds, "{line}");
                    }
                }
            }
            // The real overworld's rings: `worldgen::locate`'s `real_rings_match_java`.
            "realring" => {}
            other => panic!("unknown row {other}"),
        }
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort();
    println!("compared: {counts:?}");
    // Five kinds compared here, and the `realring` rows (counted, compared in `worldgen::locate`).
    assert_eq!(counts.len(), 6, "every kind of row");
    // The 26.3 jar's registries, all decoded.
    let villages = registry.tag("minecraft:village").map_or(0, <[_]>::len);
    println!(
        "decoded: {} structures, {} sets, {} structure tags (#village: {villages})",
        registry.structures.len(),
        registry.sets.len(),
        registry.tags.len()
    );
    assert_eq!((registry.structures.len(), registry.sets.len()), (52, 21));
    // Spigot's default seeds are the jar's salts, so a default spigot.yml places as vanilla.
    for (key, set, salt) in crate::server::config::STRUCTURE_SALTS {
        let set = registry.set(registry.set_id(&format!("minecraft:{set}")).expect(key));
        assert_eq!(
            set.placement.spreading().map(|s| s.salt),
            Some(salt),
            "{key}"
        );
    }
}

/// The biome registry's ids in registration order, from the jar.
fn biome_ids() -> Vec<String> {
    use crate::registry::{DataPack, for_each_element};
    use crate::server::vanilla_jar::VanillaJar;
    let jar = std::env::var_os("HORIZON_VANILLA_JAR").expect("HORIZON_VANILLA_JAR");
    let mut jar = VanillaJar::open(jar.as_ref()).unwrap();
    let pack = DataPack::vanilla(&mut jar).unwrap();
    let mut ids = Vec::new();
    for_each_element(&[pack], "minecraft:worldgen/biome", |file| {
        ids.push(file.id.to_owned());
        Ok::<_, ()>(())
    })
    .unwrap();
    ids
}
