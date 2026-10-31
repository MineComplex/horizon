use serde_json::value::RawValue;

use super::placement::{LevelParams, LevelPlacements, ring_searches};
use super::start::{ChunkStructures, StructureStart, create_references, hash_set_order};
use super::*;
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, JavaString, Tag};
use crate::server::config::{STRUCTURE_SALTS, SpigotWorldSeeds};
use crate::util::random::{LegacyRandom, RandomSource, WorldgenRandom};
use crate::worldgen::biome::find_biome_horizontal;

const BIOMES: [&str; 3] = ["minecraft:plains", "minecraft:ocean", "minecraft:the_void"];

fn raw(json: &str) -> Box<RawValue> {
    serde_json::from_str(json).unwrap()
}

fn structure_json(biomes: &str, terrain: &str) -> String {
    format!(
        r#"{{"type":"minecraft:jigsaw","biomes":{biomes},"step":"surface_structures","spawn_overrides":{{}},"terrain_adaptation":"{terrain}","size":7}}"#
    )
}

fn spread(salt: i32, spacing: i32, separation: i32, extra: &str) -> String {
    format!(
        r#"{{"type":"minecraft:random_spread","salt":{salt},"spacing":{spacing},"separation":{separation}{extra}}}"#
    )
}

/// A registry made the way `WorldgenData::load` makes it.
fn registry(structures: &[(&str, String)], sets: &[(&str, String)]) -> StructureRegistry {
    let ids: Vec<String> = BIOMES.iter().map(|b| b.to_string()).collect();
    let tags = vec![("minecraft:is_ocean".to_owned(), vec![1])];
    let lookup = BiomeLookup {
        ids: &ids,
        tags: &tags,
    };
    let structures: Vec<Structure> = (structures.iter())
        .map(|(id, json)| decode_structure(id, raw(json), &lookup).unwrap())
        .collect();
    let set_ids: Vec<String> = sets.iter().map(|(id, _)| id.to_string()).collect();
    let sets = (sets.iter())
        .map(|(id, json)| {
            decode_structure_set(
                id,
                &raw(json),
                &structures,
                &set_ids,
                &lookup,
                &mut InlineElements::default(),
            )
            .unwrap()
        })
        .collect();
    StructureRegistry {
        structures,
        sets,
        tags: Vec::new(),
        ..StructureRegistry::default()
    }
}

fn set_json(structures: &[(&str, i32)], placement: &str) -> String {
    let entries: Vec<String> = (structures.iter())
        .map(|(s, w)| format!(r#"{{"structure":"{s}","weight":{w}}}"#))
        .collect();
    format!(
        r#"{{"structures":[{}],"placement":{placement}}}"#,
        entries.join(",")
    )
}

/// Vanilla's villages, pillager outposts, mineshafts and buried treasure sets, and strongholds.
fn vanilla_like() -> StructureRegistry {
    registry(
        &[
            (
                "minecraft:village",
                structure_json("\"minecraft:plains\"", "beard_thin"),
            ),
            (
                "minecraft:outpost",
                structure_json("[\"minecraft:plains\"]", "none"),
            ),
            (
                "minecraft:mineshaft",
                structure_json("\"#minecraft:is_ocean\"", "none"),
            ),
            (
                "minecraft:treasure",
                structure_json("\"#is_ocean\"", "none"),
            ),
            (
                "minecraft:stronghold",
                structure_json("\"minecraft:plains\"", "bury"),
            ),
            (
                "minecraft:void_thing",
                structure_json("\"minecraft:the_void\"", "none"),
            ),
        ],
        &[
            // An exclusion zone may name a set registered after it.
            (
                "minecraft:pillager_outposts",
                set_json(
                    &[("minecraft:outpost", 1)],
                    &spread(
                        165745296,
                        32,
                        8,
                        r#","frequency":0.2,"frequency_reduction_method":"legacy_type_1","exclusion_zone":{"other_set":"villages","chunk_count":10}"#,
                    ),
                ),
            ),
            (
                "minecraft:villages",
                set_json(&[("minecraft:village", 1)], &spread(10387312, 34, 8, "")),
            ),
            (
                "minecraft:mineshafts",
                set_json(
                    &[("minecraft:mineshaft", 1)],
                    &spread(
                        0,
                        1,
                        0,
                        r#","frequency":0.004,"frequency_reduction_method":"legacy_type_3""#,
                    ),
                ),
            ),
            (
                "minecraft:buried_treasures",
                set_json(
                    &[("minecraft:treasure", 1)],
                    &spread(
                        0,
                        1,
                        0,
                        r#","frequency":0.01,"frequency_reduction_method":"legacy_type_2","locate_offset":[9,0,9]"#,
                    ),
                ),
            ),
            (
                "minecraft:strongholds",
                set_json(
                    &[("minecraft:stronghold", 1)],
                    r##"{"type":"minecraft:concentric_rings","salt":0,"distance":32,"spread":3,"count":128,"preferred_biomes":"#minecraft:is_ocean"}"##,
                ),
            ),
            (
                "custom:mineshafts",
                set_json(
                    &[("minecraft:mineshaft", 1)],
                    &spread(
                        0,
                        1,
                        0,
                        r#","frequency":0.004,"frequency_reduction_method":"legacy_type_3""#,
                    ),
                ),
            ),
            (
                "minecraft:void_set",
                set_json(
                    &[("minecraft:void_thing", 1)],
                    r#"{"type":"minecraft:dimension_origin"}"#,
                ),
            ),
        ],
    )
}

fn all_sets(registry: &StructureRegistry) -> Vec<StructureSetId> {
    (0..registry.sets.len() as u32)
        .map(StructureSetId)
        .collect()
}

/// A level of `registry` where plains and ocean can exist; ring searches answer their centre.
fn level(registry: &StructureRegistry, seed: i64, seeds: &SpigotWorldSeeds) -> LevelPlacements {
    let sets = all_sets(registry);
    let possible = BiomeSet::new(vec![0, 1]);
    let params = LevelParams {
        seed,
        ring_seed: seed,
        origin: ChunkPos::new(0, 0),
        sets: &sets,
        possible_biomes: &possible,
        seeds,
    };
    let rings: Vec<ChunkPos> = (ring_searches(registry, &params).iter())
        .map(|search| search.result(None))
        .collect();
    LevelPlacements::new(registry, &params, &rings)
}

fn placement(registry: &StructureRegistry, set: &str) -> RandomSpread {
    match &registry.set(registry.set_id(set).unwrap()).placement {
        Placement::RandomSpread(spread) => *spread,
        other => panic!("{other:?}"),
    }
}

#[test]
fn registries_decode_settings_and_placements() {
    let registry = vanilla_like();
    let village = registry.structure(registry.structure_id("minecraft:village").unwrap());
    assert_eq!(village.kind, "minecraft:jigsaw");
    assert_eq!(village.step, Decoration::SurfaceStructures);
    assert_eq!(village.terrain_adaptation, TerrainAdjustment::BeardThin);
    assert!(village.biomes.contains(0) && !village.biomes.contains(1));
    let mineshaft = registry.structure(registry.structure_id("minecraft:mineshaft").unwrap());
    assert_eq!(mineshaft.biomes.iter().collect::<Vec<_>>(), [1]);
    let outposts = placement(&registry, "minecraft:pillager_outposts");
    assert_eq!(
        outposts.spreading.exclusion_zone,
        Some(ExclusionZone {
            other_set: registry.set_id("minecraft:villages").unwrap(),
            chunk_count: 10
        })
    );
    assert_eq!(
        outposts.spreading.frequency_reduction,
        FrequencyReduction::LegacyType1
    );
    let treasure = registry.set(registry.set_id("minecraft:buried_treasures").unwrap());
    assert_eq!(treasure.placement.locate_offset(), [9, 0, 9]);
    assert_eq!(
        placement::locate_pos(&treasure.placement, ChunkPos::new(-2, 3)),
        crate::core::position::BlockPos::new(-23, 0, 57)
    );
    let Placement::ConcentricRings(rings) = &registry
        .set(registry.set_id("minecraft:strongholds").unwrap())
        .placement
    else {
        panic!()
    };
    assert_eq!((rings.distance, rings.spread, rings.count), (32, 3, 128));
    assert!(rings.preferred_biomes.contains(1));
}

#[test]
fn invalid_elements_are_rejected() {
    let ids: Vec<String> = BIOMES.iter().map(|b| b.to_string()).collect();
    let lookup = BiomeLookup {
        ids: &ids,
        tags: &[],
    };
    let structure = |json: &str| decode_structure("x:y", raw(json), &lookup);
    assert!(structure(&structure_json("\"#minecraft:missing\"", "none")).is_err());
    assert!(structure(&structure_json("\"minecraft:nowhere\"", "none")).is_err());
    assert!(structure(&structure_json("\"minecraft:plains\"", "sideways")).is_err());
    assert!(structure(r#"{"type":"minecraft:igloo","biomes":[],"step":"lakes"}"#).is_err());
    let structures = [structure(&structure_json("[]", "none")).unwrap()];
    let set = |placement: &str| {
        let json = set_json(&[("x:y", 1)], placement);
        decode_structure_set(
            "x:set",
            &raw(&json),
            &structures,
            &["x:set".into()],
            &lookup,
            &mut InlineElements::default(),
        )
    };
    assert!(
        set(&spread(1, 8, 8, "")).is_err(),
        "spacing must exceed separation"
    );
    assert!(set(&spread(-1, 8, 2, "")).is_err(), "negative salt");
    assert!(set(&spread(1, 8, 2, r#","frequency":1.5"#)).is_err());
    assert!(set(&spread(1, 8, 2, r#","locate_offset":[17,0,0]"#)).is_err());
    assert!(
        set(&spread(
            1,
            8,
            2,
            r#","exclusion_zone":{"other_set":"x:set","chunk_count":17}"#
        ))
        .is_err()
    );
    assert!(
        set(&spread(
            1,
            8,
            2,
            r#","exclusion_zone":{"other_set":"x:none","chunk_count":3}"#
        ))
        .is_err()
    );
    assert!(set(r#"{"type":"minecraft:somewhere"}"#).is_err());
    assert!(
        set(&spread(
            1,
            8,
            2,
            r#","exclusion_zone":{"other_set":"x:set","chunk_count":3}"#
        ))
        .is_ok()
    );
    let json = set_json(&[("x:y", 0)], &spread(1, 8, 2, ""));
    let inline = &mut InlineElements::default();
    assert!(decode_structure_set("x:set", &raw(&json), &structures, &[], &lookup, inline).is_err());
}

/// `StructureSet.CODEC` and its entries take a registered id or the element itself.
#[test]
fn inline_structures_and_sets_decode_like_registered_ones() {
    let ids: Vec<String> = BIOMES.iter().map(|b| b.to_string()).collect();
    let lookup = BiomeLookup {
        ids: &ids,
        tags: &[],
    };
    let registered =
        [decode_structure("x:y", raw(&structure_json("[]", "none")), &lookup).unwrap()];
    let set_ids = ["x:set".to_owned(), "x:other".to_owned()];
    let decode = |json: &str, inline: &mut InlineElements| {
        decode_structure_set("x:set", &raw(json), &registered, &set_ids, &lookup, inline)
    };

    // An inline structure takes the next id after the registered ones.
    let mut inline = InlineElements::default();
    let entries = format!(
        r#"[{{"structure":{},"weight":3}},{{"structure":"x:y","weight":1}}]"#,
        structure_json("[]", "beard_thin")
    );
    let json = format!(
        r#"{{"structures":{entries},"placement":{}}}"#,
        spread(1, 8, 2, "")
    );
    let set = decode(&json, &mut inline).unwrap();
    assert_eq!(set.structures, [(StructureId(1), 3), (StructureId(0), 1)]);
    assert_eq!(inline.structures.len(), 1);
    assert_eq!(
        inline.structures[0].terrain_adaptation,
        TerrainAdjustment::BeardThin
    );
    assert!(!set.inline);

    // An inline exclusion set takes the next id after the registered sets and is not a candidate.
    let mut inline = InlineElements::default();
    let zone = format!(
        r#","exclusion_zone":{{"other_set":{},"chunk_count":4}}"#,
        set_json(&[("x:y", 2)], &spread(7, 8, 2, ""))
    );
    let set = decode(
        &set_json(&[("x:y", 1)], &spread(1, 8, 2, &zone)),
        &mut inline,
    )
    .unwrap();
    let Placement::RandomSpread(RandomSpread {
        spreading: Spreading {
            exclusion_zone: Some(zone),
            ..
        },
        ..
    }) = set.placement
    else {
        panic!("a random spread with an exclusion zone")
    };
    assert_eq!(zone.other_set, StructureSetId(2));
    assert_eq!(zone.chunk_count, 4);
    assert_eq!(inline.sets.len(), 1);
    assert!(inline.sets[0].inline);
    assert_eq!(inline.sets[0].structures, [(StructureId(0), 2)]);

    // A malformed inline element is still an error.
    let bad = decode(
        r#"{"structures":[{"structure":{"type":"minecraft:jigsaw"},"weight":1}],"placement":{}}"#,
        &mut InlineElements::default(),
    );
    assert!(bad.is_err());
}

/// `StructureStart.loadStaticStart` parses the inner id (`Identifier.parse` throws); the starts'
/// keys only `tryParse`.
#[test]
fn malformed_start_id_fails_the_load_but_unknown_ones_are_skipped() {
    let registry = vanilla_like();
    let chunk = ChunkPos::new(0, 0);
    let load = |key: &str, id: &str| {
        let tag = Compound::from([(
            "starts".into(),
            Tag::compound([(key, Tag::compound([("id", Tag::String(id.into()))]))]),
        )]);
        ChunkStructures::load(&registry, &tag, chunk)
    };
    assert!(load("minecraft:village", "Not A:Valid Id").is_err());
    assert_eq!(
        load("minecraft:village", "minecraft:nothing")
            .unwrap()
            .starts()
            .count(),
        0
    );
    assert_eq!(
        load("Bad Key", "Not A:Valid Id").unwrap().starts().count(),
        0
    );
}

/// Paper's `injectSpigot` (derived from the patched sources): a `minecraft:` random-spread set
/// takes Spigot's salt, but an exclusion zone checks the other set by the registry's placement.
#[test]
fn spigot_salts_replace_set_salts_but_not_exclusion_zones() {
    let registry = vanilla_like();
    let mut seeds = SpigotWorldSeeds::default();
    let village_key = STRUCTURE_SALTS
        .iter()
        .position(|(_, set, _)| *set == "villages")
        .unwrap();
    seeds.salts[village_key] = 777;
    let seed = 4242;
    let paper = level(&registry, seed, &seeds);
    let villages = registry.set_id("minecraft:villages").unwrap();
    let spread = placement(&registry, "minecraft:villages");
    for (x, z) in [(0, 0), (-35, 70), (100, -100), (5000, 3)] {
        let potential = spread.potential_chunk(seed, 777, x, z);
        assert!(paper.is_structure_chunk(&registry, villages, potential.x, potential.z));
        let vanilla = spread.potential_chunk(seed, 10387312, x, z);
        assert!(paper.has_structure_chunk_in_range(&registry, villages, vanilla.x, vanilla.z, 0));
        if potential != vanilla {
            assert!(!paper.is_structure_chunk(&registry, villages, vanilla.x, vanilla.z));
        }
    }
    // The outposts' zone: Paper's own outposts (unchanged salt) against the jar's villages.
    let outposts = registry.set_id("minecraft:pillager_outposts").unwrap();
    let outpost = placement(&registry, "minecraft:pillager_outposts");
    let default = level(&registry, seed, &SpigotWorldSeeds::default());
    let mut compared = 0;
    for cell_x in -20..20 {
        for cell_z in -20..20 {
            let chunk = outpost.potential_chunk(seed, 165745296, cell_x * 32, cell_z * 32);
            let a = paper.is_structure_chunk(&registry, outposts, chunk.x, chunk.z);
            let b = default.is_structure_chunk(&registry, outposts, chunk.x, chunk.z);
            assert_eq!(a, b, "{chunk:?}");
            compared += usize::from(a);
        }
    }
    assert!(compared > 10, "{compared}");
}

/// Paper's `saltOverride` (derived from the patched sources): `seed-mineshaft` reseeds
/// `legacy_type_3` with `setLargeFeatureWithSalt`, only for the keyed `minecraft:mineshafts`;
/// `seed-buriedtreasure` replaces `legacy_type_2`'s arbitrary salt (default: the same salt).
#[test]
fn spigot_salt_overrides_reach_their_reducers() {
    let registry = vanilla_like();
    let seed = -77;
    let mut seeds = SpigotWorldSeeds {
        mineshaft: Some(31337),
        ..SpigotWorldSeeds::default()
    };
    let paper = level(&registry, seed, &seeds);
    let mineshafts = registry.set_id("minecraft:mineshafts").unwrap();
    let custom = registry.set_id("custom:mineshafts").unwrap();
    let treasures = registry.set_id("minecraft:buried_treasures").unwrap();
    let reference = |x: i32, z: i32, salt: Option<i32>| {
        let mut random = WorldgenRandom::new(LegacyRandom::new(0));
        match salt {
            Some(salt) => random.set_large_feature_with_salt(seed, x, z, salt),
            None => random.set_large_feature_seed(seed, x, z),
        }
        random.next_double() < f64::from(0.004f32)
    };
    let (mut overridden, mut differ) = (0, 0);
    for x in -300..300 {
        for z in [-5, 0, 77] {
            let keyed = paper.is_structure_chunk(&registry, mineshafts, x, z);
            assert_eq!(keyed, reference(x, z, Some(31337)), "{x} {z}");
            assert_eq!(
                paper.is_structure_chunk(&registry, custom, x, z),
                reference(x, z, None)
            );
            overridden += usize::from(keyed);
            differ += usize::from(keyed != reference(x, z, None));
        }
    }
    assert!(overridden > 0 && differ > 0, "{overridden} {differ}");
    // The default buried treasure seed is legacy_type_2's own salt.
    let reducer = FrequencyReduction::LegacyType2;
    for x in -50..50 {
        let vanilla = reducer.should_generate(seed, 0, x, 3, 0.01, None);
        assert_eq!(
            paper.is_structure_chunk(&registry, treasures, x, 3),
            vanilla
        );
        assert_eq!(
            reducer.should_generate(seed, 0, x, 3, 0.01, Some(10387320)),
            vanilla
        );
    }
    seeds.buried_treasure = 5;
    let changed = level(&registry, seed, &seeds);
    let mut any = false;
    for x in -500..500 {
        let expected = reducer.should_generate(seed, 0, x, 3, 0.01, Some(5));
        assert_eq!(
            changed.is_structure_chunk(&registry, treasures, x, 3),
            expected
        );
        any |= expected != reducer.should_generate(seed, 0, x, 3, 0.01, None);
    }
    assert!(any);
}

/// Paper's `strongholdSeed` (derived from the patched sources) seeds the strongholds' rings;
/// other levels' rings use the level seed (0 for flat levels).
#[test]
fn stronghold_seed_replaces_the_ring_seed() {
    let registry = vanilla_like();
    let sets = all_sets(&registry);
    let possible = BiomeSet::new(vec![0, 1]);
    let searches = |seed: i64, ring_seed: i64, seeds: &SpigotWorldSeeds| {
        let params = LevelParams {
            seed,
            ring_seed,
            origin: ChunkPos::new(0, 0),
            sets: &sets,
            possible_biomes: &possible,
            seeds,
        };
        (ring_searches(&registry, &params).iter())
            .map(|s| s.chunk)
            .collect::<Vec<_>>()
    };
    let seeds = SpigotWorldSeeds {
        stronghold: Some(-5),
        ..SpigotWorldSeeds::default()
    };
    assert_eq!(
        searches(1, 1, &seeds),
        searches(1, -5, &SpigotWorldSeeds::default())
    );
    assert_ne!(
        searches(1, 1, &SpigotWorldSeeds::default()),
        searches(1, 0, &SpigotWorldSeeds::default())
    );
    let rings = searches(1, 1, &SpigotWorldSeeds::default());
    assert_eq!(rings.len(), 128);
    // The first ring's three positions lie 4-ish distances out (32 chunks * 4 ± 40).
    for chunk in &rings[..3] {
        let distance = f64::from(chunk.x).hypot(f64::from(chunk.z));
        assert!((88.0..=168.0).contains(&distance), "{chunk:?}");
    }
}

#[test]
fn possible_sets_follow_the_biome_source_and_origin_placement() {
    let registry = vanilla_like();
    let level = level(&registry, 1, &SpigotWorldSeeds::default());
    let names: Vec<&str> = (level.possible_sets())
        .map(|set| registry.set(set).id.as_str())
        .collect();
    assert!(!names.contains(&"minecraft:void_set"), "{names:?}");
    assert_eq!(names.len(), registry.sets.len() - 1);
    let strongholds = registry.set_id("minecraft:strongholds").unwrap();
    let positions = level.ring_positions(strongholds).unwrap();
    assert_eq!(positions.len(), 128);
    assert!(level.is_structure_chunk(&registry, strongholds, positions[5].x, positions[5].z));
    assert!(!level.is_structure_chunk(&registry, strongholds, 0, 0));
    // The void set is not possible here, but its placement is the origin chunk.
    let void = registry.set_id("minecraft:void_set").unwrap();
    assert!(!level.is_structure_chunk(&registry, void, 0, 0));
    assert!(level.has_structure_chunk_in_range(&registry, void, 2, -1, 2));
    assert!(!level.has_structure_chunk_in_range(&registry, void, 3, 0, 2));
    let village = registry.structure_id("minecraft:village").unwrap();
    let possible = BiomeSet::new(vec![0, 1]);
    let villages: Vec<_> = level
        .sets_for_structure(&registry, village, &possible)
        .collect();
    assert_eq!(villages, [registry.set_id("minecraft:villages").unwrap()]);
}

/// `createStructures`: a lone structure is tried once; several are drawn by weight from
/// `setLargeFeatureSeed` until one starts; a set with a start already is skipped.
#[test]
fn create_starts_tries_structures_in_weighted_order() {
    let registry = registry(
        &[
            ("x:a", structure_json("\"minecraft:plains\"", "none")),
            ("x:b", structure_json("\"minecraft:plains\"", "none")),
            ("x:c", structure_json("\"minecraft:plains\"", "none")),
            ("x:d", structure_json("\"minecraft:the_void\"", "none")),
        ],
        &[
            (
                "x:many",
                set_json(&[("x:a", 1), ("x:b", 3), ("x:c", 6)], &spread(5, 3, 1, "")),
            ),
            // Not possible in a level without the void: never tried.
            ("x:one", set_json(&[("x:d", 1)], &spread(6, 2, 1, ""))),
        ],
    );
    let seed = 99;
    let level = level(&registry, seed, &SpigotWorldSeeds::default());
    let many = registry.set_id("x:many").unwrap();
    let chunk = (-20..20)
        .flat_map(|x| (-20..20).map(move |z| ChunkPos::new(x, z)))
        .find(|c| level.is_structure_chunk(&registry, many, c.x, c.z))
        .unwrap();
    let mut tried = Vec::new();
    level.create_starts(
        &registry,
        chunk,
        |_| false,
        |s| {
            tried.push(s.0);
            false
        },
    );
    // Reference: vanilla's loop.
    let mut random = WorldgenRandom::new(LegacyRandom::new(0));
    random.set_large_feature_seed(seed, chunk.x, chunk.z);
    let mut options = vec![(0, 1), (1, 3), (2, 6)];
    let mut total = 10;
    let mut expected = Vec::new();
    while !options.is_empty() {
        let mut choice = random.next_int_bound(total).unwrap();
        let index = options
            .iter()
            .position(|(_, w)| {
                choice -= w;
                choice < 0
            })
            .unwrap();
        expected.push(options[index].0);
        total -= options.remove(index).1;
    }
    assert_eq!(tried, expected);
    let mut tried = Vec::new();
    level.create_starts(
        &registry,
        chunk,
        |s| s.0 == 1,
        |s| {
            tried.push(s);
            true
        },
    );
    assert!(
        tried.is_empty(),
        "a valid start of the set is already there"
    );
    let mut tried = Vec::new();
    level.create_starts(
        &registry,
        chunk,
        |_| false,
        |s| {
            tried.push(s.0);
            true
        },
    );
    assert_eq!(tried, &expected[..1]);
}

#[test]
fn find_biome_horizontal_picks_like_vanilla() {
    // find_closest: the first allowed quart ring by ring, z then x.
    let mut random = LegacyRandom::new(1);
    let found = find_biome_horizontal(
        [100, 64, -100],
        40,
        1,
        |b| b == 1,
        &mut random,
        true,
        |qx, _, qz| u32::from(qx == 27 && qz == -27 || qx == 22 && qz == -28),
    );
    assert_eq!(found, Some(([108, 64, -108], 1)));
    // Nothing allowed: none, and no draws.
    let mut random = LegacyRandom::new(1);
    let none = find_biome_horizontal(
        [0, 0, 0],
        112,
        1,
        |_| false,
        &mut random,
        false,
        |_, _, _| 0,
    );
    assert_eq!(none, None);
    assert_eq!(random.next_int(), LegacyRandom::new(1).next_int());
}

fn piece(bb: [i32; 6]) -> super::start::StructurePiece {
    super::start::StructurePiece::Saved(Compound::from([
        ("id".into(), Tag::String("minecraft:btp".into())),
        ("BB".into(), Tag::IntArray(bb.to_vec())),
        ("O".into(), Tag::Int(-1)),
        ("GD".into(), Tag::Int(0)),
    ]))
}

#[test]
fn chunk_structures_round_trip_and_filter() {
    let registry = vanilla_like();
    let village = registry.structure_id("minecraft:village").unwrap();
    let treasure = registry.structure_id("minecraft:treasure").unwrap();
    let chunk = ChunkPos::new(3, -4);
    let mut structures = ChunkStructures::default();
    let mut start = StructureStart::new(
        &registry,
        village,
        chunk,
        0,
        vec![piece([160, 60, -64, 180, 80, -50])],
    );
    start.bukkit_values = Some(Compound::from([("plugin:key".into(), Tag::Byte(1))]));
    structures.set_start(village, start.clone());
    structures.set_start(treasure, StructureStart::invalid());
    for key in [
        ChunkPos::new(2, -4).pack(),
        0,
        ChunkPos::new(3, -12).pack(),
        0,
    ] {
        structures.add_reference(village, key);
    }
    assert_eq!(
        structures.references(village),
        [ChunkPos::new(2, -4).pack(), 0, ChunkPos::new(3, -12).pack()]
    );
    // Beard: the box grows by 12.
    let bounds = start.bounding_box.unwrap();
    assert_eq!(
        (bounds.minimum(), bounds.maximum()),
        ([148, 48, -76], [192, 92, -38])
    );

    let tag = structures.save(&registry, chunk);
    let starts = match &tag[&JavaString::from("starts")] {
        Tag::Compound(starts) => starts,
        _ => panic!(),
    };
    assert_eq!(
        starts[&JavaString::from("minecraft:treasure")],
        Tag::compound([("id", Tag::String("INVALID".into()))])
    );
    let loaded = ChunkStructures::load(&registry, &tag, chunk).unwrap();
    assert_eq!(loaded.start(village), Some(&start));
    assert_eq!(loaded.start(treasure), Some(&StructureStart::invalid()));
    // Loaded references come in the hash set's order.
    let keys = [ChunkPos::new(2, -4).pack(), 0, ChunkPos::new(3, -12).pack()];
    assert_eq!(loaded.references(village), hash_set_order(&keys));
    assert_eq!(loaded.save(&registry, chunk).len(), 2);

    // Unknown structures and far references are dropped; other tags are skipped.
    let tag = Compound::from([
        (
            "starts".into(),
            Tag::compound([
                (
                    "minecraft:nothing",
                    Tag::compound([("id", Tag::String("minecraft:nothing".into()))]),
                ),
                ("village", Tag::Int(3)),
            ]),
        ),
        (
            "References".into(),
            Tag::compound([
                ("minecraft:nothing", Tag::LongArray(vec![1])),
                (
                    "minecraft:village",
                    Tag::LongArray(vec![
                        ChunkPos::new(12, -4).pack(),
                        ChunkPos::new(11, 4).pack(),
                    ]),
                ),
                ("minecraft:treasure", Tag::IntArray(vec![1])),
            ]),
        ),
    ]);
    let loaded = ChunkStructures::load(&registry, &tag, chunk).unwrap();
    assert_eq!(
        loaded.starts().count(),
        0,
        "an empty start tag has no structure id"
    );
    assert_eq!(loaded.references(village), [ChunkPos::new(11, 4).pack()]);
    assert!(loaded.references(treasure).is_empty());
    assert_eq!(loaded.all_references().count(), 1);
}

#[test]
fn references_reach_starts_whose_box_meets_the_chunk() {
    let registry = vanilla_like();
    let village = registry.structure_id("minecraft:village").unwrap();
    let treasure = registry.structure_id("minecraft:treasure").unwrap();
    let mut near = ChunkStructures::default();
    // 12 blocks of beard reach chunk (0, 0) from x 27; without, x 28 does not.
    near.set_start(
        village,
        StructureStart::new(
            &registry,
            village,
            ChunkPos::new(2, 0),
            0,
            vec![piece([27, 0, 0, 40, 10, 10])],
        ),
    );
    near.set_start(
        treasure,
        StructureStart::new(
            &registry,
            treasure,
            ChunkPos::new(2, 0),
            0,
            vec![piece([28, 0, 0, 40, 10, 10])],
        ),
    );
    let mut far = ChunkStructures::default();
    far.set_start(
        treasure,
        StructureStart::new(
            &registry,
            treasure,
            ChunkPos::new(-8, 8),
            0,
            vec![piece([-128, 0, -5, 0, 5, 3])],
        ),
    );
    far.set_start(village, StructureStart::invalid());
    let found = create_references(ChunkPos::new(0, 0), |chunk| match (chunk.x, chunk.z) {
        (2, 0) => Some(&near),
        (-8, 8) => Some(&far),
        _ => None,
    });
    assert_eq!(
        found,
        [
            (treasure, ChunkPos::new(-8, 8).pack()),
            (village, ChunkPos::new(2, 0).pack())
        ]
    );
}

#[test]
fn hash_set_order_deduplicates_and_puts_zero_first() {
    assert_eq!(hash_set_order(&[]), Vec::<i64>::new());
    assert_eq!(hash_set_order(&[5, 0, 5]), [0, 5]);
    let keys: Vec<i64> = (0..100).map(|i| i * 4_294_967_297 + 3).collect();
    let mut order = hash_set_order(&keys);
    order.sort_unstable();
    assert_eq!(order, keys);
}

#[test]
fn java_round_and_int_value() {
    use crate::util::jmath::round;
    assert_eq!(round(2.5), 3);
    assert_eq!(round(-2.5), -2);
    assert_eq!(round(-2.5000000000000004), -3);
    assert_eq!(round(0.49999999999999994), 0);
    assert_eq!(round(f64::NAN), 0);
    assert_eq!(round(1e300), i64::MAX);
    assert_eq!(round(-1e300), i64::MIN);
    assert_eq!(Tag::Long(1 << 32 | 7).int_value(), Some(7));
    assert_eq!(Tag::Double((-1.5f64).to_bits()).int_value(), Some(-2));
    assert_eq!(Tag::Float(1e20f32.to_bits()).int_value(), Some(i32::MAX));
    assert_eq!(Tag::String("1".into()).int_value(), None);
}
