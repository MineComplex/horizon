//! Surface rules without the 26.3 data: decoding, the clay bands, temperatures.

use serde_json::value::RawValue;

use super::rules::{Condition, MaterialRules, Rule};
use super::temperature::{BiomeClimate, BiomeClimates};
use super::{ClayColors, clay_bands};
use crate::block::BlockState;
use crate::core::position::BlockPos;
use crate::util::random::LegacyRandom;
use crate::worldgen::codec::DensityLoader;
use crate::worldgen::density::DensityGraph;
use crate::worldgen::oracle_tests::settings_blocks;
use crate::worldgen::structure::BiomeLookup;

fn raw(text: &str) -> Box<RawValue> {
    serde_json::from_str(text).unwrap()
}

fn decode(
    rules: &[(&str, &str)],
    conditions: &[(&str, &str)],
) -> Result<MaterialRules, crate::worldgen::codec::DecodeError> {
    let list = |elements: &[(&str, &str)]| -> Vec<(String, Box<RawValue>)> {
        elements
            .iter()
            .map(|(id, json)| ((*id).to_owned(), raw(json)))
            .collect()
    };
    let ids = [
        "minecraft:plains".to_owned(),
        "minecraft:eroded_badlands".to_owned(),
    ];
    let tags = [("minecraft:is_badlands".to_owned(), vec![1])];
    let biomes = BiomeLookup {
        ids: &ids,
        tags: &tags,
    };
    let mut graph = DensityGraph::new();
    MaterialRules::decode(
        &list(rules),
        &list(conditions),
        &biomes,
        &mut DensityLoader::new(&mut graph),
        &settings_blocks(),
    )
}

#[test]
fn references_share_nodes_and_forward_references_resolve() {
    let rules = decode(
        &[
            (
                "minecraft:top",
                r#"{"type":"minecraft:sequence","sequence":["minecraft:bedrock_floor",
                    {"type":"minecraft:condition","if_true":"minecraft:later","then_run":"minecraft:stone"}]}"#,
            ),
            ("minecraft:bedrock_floor", r#"{"type":"minecraft:block","result_state":"minecraft:stone"}"#),
            ("minecraft:stone", r#"{"type":"minecraft:block","result_state":{"id":"minecraft:stone"}}"#),
        ],
        &[(
            "minecraft:later",
            r##"{"type":"minecraft:not","invert":{"type":"minecraft:biome","biome_is":"#minecraft:is_badlands"}}"##,
        )],
    )
    .unwrap();
    let top = rules.element("minecraft:top").unwrap();
    let Rule::Sequence(items) = rules.rule(top) else {
        panic!("a sequence")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(rules.element("minecraft:bedrock_floor"), Some(items[0]));
    assert!(matches!(rules.rule(items[0]), Rule::Block(state) if *state != BlockState::AIR));
    assert_eq!(rules.eroded_badlands, Some(1));
    assert_eq!(rules.frozen_oceans, [None, None]);
    // The not's inner biome set is the tag's members.
    let inner = rules
        .conditions()
        .iter()
        .find_map(|condition| match condition {
            Condition::Not(inner) => Some(*inner),
            _ => None,
        })
        .expect("a not condition");
    let Condition::Biome(set) = rules.condition(inner) else {
        panic!("a biome condition")
    };
    assert!(set.contains(1) && !set.contains(0));
}

#[test]
fn papers_bedrock_condition_decodes_as_a_gradient_with_its_roof_flag() {
    let rules = decode(
        &[(
            "minecraft:bedrock_roof",
            r#"{"type":"minecraft:condition","if_true":{"type":"minecraft:not","invert":
                {"type":"paper:optionally_flat_bedrock_material_condition","is_roof":true,
                 "false_at_and_above":{"below_top":0},"random_name":"minecraft:bedrock_roof",
                 "true_at_and_below":{"below_top":5}}},
                "then_run":{"type":"minecraft:block","result_state":"minecraft:stone"}}"#,
        )],
        &[],
    )
    .unwrap();
    assert_eq!(rules.random_names(), ["minecraft:bedrock_roof"]);
    assert!(rules.conditions().iter().any(|condition| matches!(
        condition,
        Condition::Gradient(gradient) if gradient.paper_roof == Some(true) && gradient.random == 0
    )));
}

#[test]
fn bad_registries_fail_the_load() {
    let block = r#"{"type":"minecraft:block","result_state":"minecraft:stone"}"#;
    let error = |rules: &[(&str, &str)], conditions: &[(&str, &str)]| {
        decode(rules, conditions).unwrap_err().to_string()
    };
    assert!(
        error(
            &[("a:b", r#"{"type":"minecraft:sequence","sequence":["a:b"]}"#)],
            &[]
        )
        .contains("cyclic")
    );
    assert!(
        error(
            &[
                (
                    "a:b",
                    r#"{"type":"minecraft:condition","if_true":"a:missing","then_run":"a:c"}"#
                ),
                ("a:c", block)
            ],
            &[]
        )
        .contains("Missing material condition")
    );
    assert!(
        error(&[("a:b", r#"{"type":"minecraft:sparkles"}"#)], &[]).contains("minecraft:sparkles")
    );
    assert!(error(
        &[("a:b", block)],
        &[("a:c", r#"{"type":"minecraft:y_above","anchor":{"absolute":1},"surface_depth_multiplier":21,"add_stone_depth":false}"#)]
    )
    .contains("surface_depth_multiplier"));
    assert!(
        error(
            &[(
                "a:b",
                r#"{"type":"minecraft:block","result_state":"minecraft:nothing"}"#
            )],
            &[]
        )
        .contains("unknown block state")
    );
}

#[test]
fn clay_bands_use_the_seven_colours_in_vanillas_layering() {
    let colors = ClayColors {
        terracotta: BlockState(1),
        orange: BlockState(2),
        yellow: BlockState(3),
        brown: BlockState(4),
        red: BlockState(5),
        white: BlockState(6),
        light_gray: BlockState(7),
    };
    let bands = clay_bands(&mut LegacyRandom::new(42), colors);
    assert_eq!(bands.len(), 192);
    let count = |color: u32| bands.iter().filter(|b| b.0 == color).count();
    assert!(bands.iter().all(|b| (1..=7).contains(&b.0)));
    // Orange every few layers, then 6-15 yellow, brown and red bands, 9-15 white ones.
    assert!(count(1) > 0 && count(2) > 10 && count(6) > 3 && count(6) <= 15);
    // The draws are the seed's: the same seed gives the same bands, another one does not.
    assert_eq!(bands, clay_bands(&mut LegacyRandom::new(42), colors));
    assert_ne!(bands, clay_bands(&mut LegacyRandom::new(43), colors));
}

#[test]
fn temperature_falls_above_the_snow_line_and_frozen_biomes_have_ice_patches() {
    let climates = BiomeClimates::new(vec![
        BiomeClimate {
            temperature: 0.8,
            frozen: false,
        },
        BiomeClimate {
            temperature: 0.0,
            frozen: true,
        },
    ]);
    // Up to sea level + 17 the base temperature; above, cooler by (noise * 8 + y - line) * 0.05 / 40.
    let below = BlockPos::new(100, 80, -40);
    assert_eq!(climates.temperature(0, below, 63), 0.8);
    let high = BlockPos::new(100, 300, -40);
    // The simplex term is within +-2, times 8.
    let cooled = climates.temperature(0, high, 63);
    assert!(cooled > 0.8 - (300.0 - 80.0 + 16.0) * 0.05 / 40.0 - 1e-4);
    assert!(cooled < 0.8 - (300.0 - 80.0 - 16.0) * 0.05 / 40.0 + 1e-4);
    assert!(
        climates.warm_enough_to_rain(0, below, 63) && !climates.cold_enough_to_snow(0, below, 63)
    );
    assert!(climates.cold_enough_to_snow(0, high, 63) == (cooled < 0.15));
    // A frozen biome is 0.2 inside ice patches and its base (0) elsewhere; over a sample, both
    // happen, and only the patches melt an iceberg slightly (above 0.1).
    let spots: Vec<BlockPos> = (0..400)
        .map(|i| BlockPos::new(i * 37, 64, i * 91 - 5000))
        .collect();
    let values: Vec<f32> = spots
        .iter()
        .map(|&p| climates.temperature(1, p, 63))
        .collect();
    assert!(values.iter().all(|&v| v == 0.0 || v == 0.2));
    assert!(values.contains(&0.0) && values.contains(&0.2));
    for (&spot, &value) in spots.iter().zip(&values) {
        assert_eq!(climates.melts_iceberg_slightly(1, spot, 63), value == 0.2);
    }
}
