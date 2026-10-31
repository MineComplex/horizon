//! Carver decoding and the carving paths on invented configurations (the real ones are checked
//! against the jar by `oracle_tests.rs`).

use serde_json::value::RawValue;

use super::float_provider::FloatProvider;
use super::mask::CarvingMask;
use super::world::WorldCarver;
use crate::core::position::ChunkPos;
use crate::util::random::{LegacyRandom, RandomSource};
use crate::worldgen::anchor::WorldGenerationContext;

fn raw(json: &str) -> Box<RawValue> {
    serde_json::from_str(json).expect("valid JSON")
}

const CAVE: &str = r#"{
    "type": "minecraft:cave", "probability": 0.5,
    "y": {"type": "minecraft:uniform", "min_inclusive": {"absolute": 0}, "max_inclusive": {"absolute": 40}},
    "count": {"type": "minecraft:uniform", "min_inclusive": 3, "max_inclusive": 6},
    "thickness": {"type": "minecraft:trapezoid", "min": 0.0, "max": 3.0, "plateau": 1.0},
    "room_vertical_radius_multiplier": 0.5, "horizontal_radius_multiplier": 1.0,
    "vertical_radius_multiplier": 1.0, "floor_level": -0.7, "weird_thickness_bias": true
}"#;

const CANYON: &str = r#"{
    "type": "minecraft:canyon", "probability": 0.5,
    "y": {"type": "minecraft:uniform", "min_inclusive": {"absolute": 10}, "max_inclusive": {"absolute": 60}},
    "vertical_rotation": {"type": "minecraft:uniform", "min_inclusive": -0.125, "max_exclusive": 0.125},
    "shape": {
        "distance_factor": {"type": "minecraft:uniform", "min_inclusive": 0.75, "max_exclusive": 1.0},
        "thickness": {"type": "minecraft:trapezoid", "min": 0.0, "max": 6.0, "plateau": 2.0},
        "width_smoothness": 3,
        "horizontal_radius_factor": {"type": "minecraft:clamped_normal", "mean": 1.0, "deviation": 0.1, "min": 0.8, "max": 1.2},
        "vertical_radius_default_factor": 1.0, "vertical_radius_center_factor": 0.0, "y_scale": 3.0
    }
}"#;

fn context() -> WorldGenerationContext {
    WorldGenerationContext::new(-64, 384, 63)
}

fn mask() -> CarvingMask {
    CarvingMask::new(-63, 384 - 64 - 1 - 7)
}

#[test]
fn float_providers_decode_and_validate() {
    assert_eq!(
        FloatProvider::decode(&raw("0.25")).unwrap(),
        FloatProvider::Constant(0.25)
    );
    let trapezoid = FloatProvider::decode(&raw(
        r#"{"type":"minecraft:trapezoid","min":0,"max":6,"plateau":2}"#,
    ))
    .unwrap();
    assert_eq!((trapezoid.min(), trapezoid.max()), (0.0, 6.0));
    for bad in [
        r#"{"type":"minecraft:uniform","min_inclusive":1.0,"max_exclusive":1.0}"#,
        r#"{"type":"minecraft:trapezoid","min":0,"max":2,"plateau":3}"#,
        r#"{"type":"minecraft:clamped_normal","mean":0,"deviation":1,"min":2,"max":1}"#,
        r#"{"type":"minecraft:nope"}"#,
        r#""text""#,
    ] {
        assert!(FloatProvider::decode(&raw(bad)).is_err(), "{bad}");
    }
    assert!(FloatProvider::decode_at_least(&raw("-0.5"), 0.0).is_err());
    assert!(FloatProvider::decode_within(&raw("1.5"), -1.0, 1.0).is_err());
    assert!(FloatProvider::decode_within(&raw("-1"), -1.0, 1.0).is_ok());
}

#[test]
fn float_providers_draw_in_vanillas_order() {
    // Trapezoid: min + a * plateauEnd + b * plateauStart, a drawn first.
    let provider = FloatProvider::Trapezoid {
        min: 1.0,
        max: 7.0,
        plateau: 2.0,
    };
    let mut random = LegacyRandom::new(42);
    let mut twin = LegacyRandom::new(42);
    let (a, b) = (twin.next_float(), twin.next_float());
    assert_eq!(provider.sample(&mut random), 1.0 + a * 4.0 + b * 2.0);
    let uniform = FloatProvider::Uniform {
        min: -1.0,
        max: 3.0,
    };
    assert_eq!(uniform.sample(&mut random), twin.next_float() * 4.0 + -1.0);
    // A constant draws nothing.
    let before = random.clone().next_long();
    FloatProvider::Constant(2.0).sample(&mut random);
    assert_eq!(random.next_long(), before);
}

#[test]
fn carvers_decode_and_reject_bad_configurations() {
    assert!(matches!(
        WorldCarver::decode(&raw(CAVE)).unwrap(),
        WorldCarver::Cave(_)
    ));
    assert!(matches!(
        WorldCarver::decode(&raw(CANYON)).unwrap(),
        WorldCarver::Canyon(_)
    ));
    let broken = [
        CAVE.replace("\"probability\": 0.5", "\"probability\": 1.5"),
        CAVE.replace("\"floor_level\": -0.7", "\"floor_level\": -1.5"),
        CAVE.replace("\"min\": 0.0, \"max\": 3.0", "\"min\": -1.0, \"max\": 3.0"),
        CAVE.replace("\"min_inclusive\": 3", "\"min_inclusive\": -3"),
        CAVE.replace("minecraft:cave", "minecraft:tunnel"),
        CANYON.replace("\"width_smoothness\": 3", "\"width_smoothness\": 0"),
        CANYON.replace("\"y_scale\"", "\"y_scaling\""),
    ];
    for json in broken {
        assert!(WorldCarver::decode(&raw(&json)).is_err(), "{json}");
    }
    // `start_vertical_radius_multiplier` and `weird_thickness_bias` are optional.
    let plain = CAVE.replace(", \"weird_thickness_bias\": true", "");
    assert!(WorldCarver::decode(&raw(&plain)).is_ok());
}

fn carve(json: &str, seed: i64, chunk: ChunkPos, source: ChunkPos) -> CarvingMask {
    let carver = WorldCarver::decode(&raw(json)).unwrap();
    let mut mask = mask();
    let mut random = LegacyRandom::new(seed);
    carver
        .carve(context(), &mut random, chunk, source, &mut mask)
        .unwrap();
    mask
}

#[test]
fn caves_and_canyons_dig_only_inside_the_mask_and_repeat() {
    let chunk = ChunkPos::new(3, -2);
    for json in [CAVE, CANYON] {
        let mut dug = 0;
        for seed in 0..40 {
            let first = carve(json, seed, chunk, ChunkPos::new(3, -2));
            let again = carve(json, seed, chunk, ChunkPos::new(3, -2));
            assert_eq!(first.runs(), again.runs());
            for [x, z, bottom, top] in first.runs() {
                assert!((0..16).contains(&x) && (0..16).contains(&z));
                assert!(bottom >= -62 && top <= 384 - 64 - 1 - 7 && bottom <= top);
            }
            dug += first.runs().len();
        }
        assert!(dug > 0, "nothing dug in 40 tries of {json}");
    }
}

#[test]
fn a_source_far_away_digs_nothing() {
    // Paths are at most 112 blocks long: eight chunks away is out of reach.
    for json in [CAVE, CANYON] {
        for seed in 0..20 {
            assert!(
                carve(json, seed, ChunkPos::new(0, 0), ChunkPos::new(8, 8)).is_empty(),
                "{json}"
            );
        }
    }
}

#[test]
fn cave_floors_stay_where_the_floor_level_puts_them() {
    // `yd <= floorLevel` skips the bottom of every ellipsoid, so a cave of floor level 0.9 only
    // digs the top tenth of what a floor of -0.9 digs.
    let low = CAVE.replace("\"floor_level\": -0.7", "\"floor_level\": -0.9");
    let high = CAVE.replace("\"floor_level\": -0.7", "\"floor_level\": 0.9");
    let (mut low_bits, mut high_bits) = (0usize, 0usize);
    for seed in 0..30 {
        for [_, _, bottom, top] in
            carve(&low, seed, ChunkPos::new(0, 0), ChunkPos::new(0, 0)).runs()
        {
            low_bits += (top - bottom + 1) as usize;
        }
        for [_, _, bottom, top] in
            carve(&high, seed, ChunkPos::new(0, 0), ChunkPos::new(0, 0)).runs()
        {
            high_bits += (top - bottom + 1) as usize;
        }
    }
    assert!(
        low_bits > 0 && high_bits < low_bits / 2,
        "{low_bits} {high_bits}"
    );
}

#[test]
fn a_start_test_is_a_single_float_against_the_probability() {
    let carver = WorldCarver::decode(&raw(CAVE)).unwrap();
    for seed in 0..50 {
        let mut random = LegacyRandom::new(seed);
        let expected = LegacyRandom::new(seed).next_float() <= 0.5;
        assert_eq!(carver.is_start_chunk(&mut random), expected);
        // Exactly one draw.
        let mut twin = LegacyRandom::new(seed);
        twin.next_float();
        assert_eq!(random.next_long(), twin.next_long());
    }
}
