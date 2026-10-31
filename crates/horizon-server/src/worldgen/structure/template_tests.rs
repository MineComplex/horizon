//! Templates, placement transforms and processor decoding on small synthetic tables (the real
//! data is `template_oracle_tests.rs`).

use serde_json::value::RawValue;

use super::processor::{
    BlockEntityModifier, PosRuleTest, Processor, ProcessorList, json_to_nbt, parse_state,
};
use super::template::{
    JointType, StructureTemplates, bounding_box, transform, transform_vec,
    zero_position_with_transform,
};
use crate::block::transform::{Mirror, Rotation};
use crate::block::{BlockState, BlockStateData, BlockStates, BlockStatesBuilder};
use crate::core::position::BlockPos;
use crate::nbt::{Compound, JavaString, Limits, NamedTag, Tag};
use crate::util::random::LegacyRandom;
use crate::world::storage::compression::Compression;
use crate::worldgen::placement::DecodeContext;

/// Four states as the synthetic `block_states.bin` has them: air, stone (a full collision
/// block), and the two states of a jigsaw.
fn table() -> (BlockStates, BlockStateData) {
    let mut b = BlockStatesBuilder::new();
    b.add_block("minecraft:air", &[]).unwrap();
    b.add_block("minecraft:stone", &[]).unwrap();
    b.add_block(
        "minecraft:jigsaw",
        &[("orientation", &["down_east", "north_up"])],
    )
    .unwrap();
    let blocks = b.finish();
    let data = BlockStateData::from_bytes(&crate::block::flags_tests::synthetic(4), 4).unwrap();
    (blocks, data)
}

fn s(text: &str) -> Tag {
    Tag::String(text.into())
}
fn ints(v: [i32; 3]) -> Tag {
    Tag::List(v.into_iter().map(Tag::Int).collect())
}
fn compound(entries: Vec<(&str, Tag)>) -> Tag {
    Tag::compound(entries)
}

fn template_file(palette_first: &str) -> Vec<u8> {
    let block = |pos, state, nbt: Option<Tag>| {
        let mut entries = vec![("pos", ints(pos)), ("state", Tag::Int(state))];
        entries.extend(nbt.map(|n| ("nbt", n)));
        compound(entries)
    };
    let root = compound(vec![
        ("size", ints([2, 2, 2])),
        (
            "palette",
            Tag::List(vec![
                compound(vec![("id", s(palette_first))]),
                compound(vec![
                    ("id", s("jigsaw")),
                    ("properties", compound(vec![("orientation", s("north_up"))])),
                ]),
                compound(vec![("id", s("minecraft:no_such_block"))]),
            ]),
        ),
        (
            "blocks",
            Tag::List(vec![
                block([1, 1, 0], 0, None),
                block(
                    [0, 0, 1],
                    1,
                    Some(compound(vec![("pool", s("village/plains"))])),
                ),
                block([1, 0, 0], 0, None),
                block([0, 1, 1], 2, None),
                block([0, 0, 0], 7, None),
            ]),
        ),
        (
            "entities",
            Tag::List(vec![compound(vec![
                ("pos", Tag::List(vec![Tag::Double(0.5f64.to_bits()); 3])),
                ("blockPos", ints([0, 0, 0])),
                ("nbt", compound(vec![("id", s("minecraft:villager"))])),
            ])]),
        ),
    ]);
    let named = NamedTag {
        name: JavaString::default(),
        tag: root,
    };
    let raw = crate::nbt::write_named(&named, Limits::default()).unwrap();
    Compression::Gzip.encode(&raw).unwrap()
}

#[test]
fn templates_order_blocks_and_read_jigsaws() {
    let (blocks, data) = table();
    let templates = StructureTemplates::decode_all(
        vec![(
            "minecraft:t".into(),
            "vanilla".into(),
            template_file("stone"),
        )],
        &blocks,
        &data,
    );
    let template = templates.get("minecraft:t").unwrap();
    assert_eq!(template.size(), [2, 2, 2]);
    let palette = &template.palettes()[0];
    // Full blocks (stone) by y, x, z; then the rest (air: an unknown block and a bad index read
    // as air too); then the block with NBT.
    let order: Vec<([i32; 3], u32)> = palette
        .blocks()
        .iter()
        .map(|b| ([b.pos.x, b.pos.y, b.pos.z], b.state.0))
        .collect();
    assert_eq!(
        order,
        [
            ([1, 0, 0], 1),
            ([1, 1, 0], 1),
            ([0, 0, 0], 0),
            ([0, 1, 1], 0),
            ([0, 0, 1], 3)
        ]
    );
    let jigsaw = &palette.jigsaws()[0];
    assert_eq!(jigsaw.joint, JointType::Aligned);
    assert_eq!(jigsaw.pool, "minecraft:village/plains");
    assert_eq!(jigsaw.name, "minecraft:empty");
    assert_eq!(template.entities()[0].pos, [0.5; 3]);
}

#[test]
fn the_first_source_that_decodes_wins() {
    let (blocks, data) = table();
    let files = vec![
        (
            "minecraft:t".into(),
            "generated".into(),
            b"not gzip".to_vec(),
        ),
        ("minecraft:t".into(), "vanilla".into(), template_file("air")),
        ("minecraft:t".into(), "later".into(), template_file("stone")),
    ];
    let templates = StructureTemplates::decode_all(files, &blocks, &data);
    assert_eq!(templates.failures.len(), 1);
    assert_eq!(templates.ids().collect::<Vec<_>>(), ["minecraft:t"]);
    let first = &templates.get("minecraft:t").unwrap().palettes()[0].blocks()[0];
    assert_eq!(first.state, BlockState(0));
}

#[test]
fn transforms_match_vanilla_formulas() {
    let p = BlockPos::new(3, 5, 7);
    let pivot = BlockPos::new(1, 0, 2);
    assert_eq!(
        transform(p, Mirror::None, Rotation::Clockwise90, pivot),
        BlockPos::new(-4, 5, 4)
    );
    assert_eq!(
        transform(p, Mirror::LeftRight, Rotation::None, pivot),
        BlockPos::new(3, 5, -7)
    );
    assert_eq!(
        transform(p, Mirror::FrontBack, Rotation::Counterclockwise90, pivot),
        BlockPos::new(6, 5, 6)
    );
    assert_eq!(
        transform_vec(
            [0.5, 1.0, 0.25],
            Mirror::None,
            Rotation::Clockwise180,
            pivot
        ),
        [2.5, 1.0, 4.75]
    );
    let zero = BlockPos::new(10, 64, 10);
    assert_eq!(
        zero_position_with_transform(zero, Mirror::LeftRight, Rotation::Clockwise90, 4, 6),
        BlockPos::new(10, 64, 10)
    );
    let b = bounding_box(
        zero,
        Rotation::Clockwise90,
        BlockPos::default(),
        Mirror::None,
        [4, 3, 6],
    );
    assert_eq!((b.minimum(), b.maximum()), ([5, 64, 10], [10, 66, 13]));
}

#[test]
fn processor_lists_decode_both_forms() {
    let (blocks, _) = table();
    let names = DecodeContext {
        blocks: &blocks,
        block_tags: &[],
        biomes: &[],
        biome_tags: &[],
        fluid_tags: &[],
    };
    let raw = |json: &str| -> Box<RawValue> { serde_json::from_str(json).unwrap() };
    let list = ProcessorList::decode(
        &raw(
            r#"{"processors":[{"processor_type":"minecraft:block_rot","integrity":0.5},
            {"processor_type":"capped","delegate":{"processor_type":"minecraft:nop"},"limit":3}]}"#,
        ),
        &names,
    )
    .unwrap();
    assert!(
        matches!(list.0[0], Processor::BlockRot { integrity, rottable: None } if integrity == 0.5)
    );
    assert!(list.0[1].evaluates_entire_piece());
    let bare = ProcessorList::decode(&raw(r#"[{"processor_type":"minecraft:gravity"}]"#), &names);
    assert!(matches!(
        bare.unwrap().0[0],
        Processor::Gravity { offset: 0, .. }
    ));
    // Out of range or zero-width: the codec fails.
    for bad in [
        r#"[{"processor_type":"minecraft:block_rot","integrity":1.5}]"#,
        r#"[{"processor_type":"minecraft:capped","delegate":{"processor_type":"minecraft:nop"},"limit":0}]"#,
        r#"[{"processor_type":"minecraft:unknown"}]"#,
    ] {
        assert!(ProcessorList::decode(&raw(bad), &names).is_err(), "{bad}");
    }
    let rule = ProcessorList::decode(
        &raw(r#"[{"processor_type":"minecraft:rule","rules":[{"input_predicate":{"predicate_type":"minecraft:always_true"},
            "location_predicate":{"predicate_type":"minecraft:always_true"},"output_state":{"id":"minecraft:stone"},
            "position_predicate":{"predicate_type":"minecraft:linear_pos","min_dist":3,"max_dist":3},
            "block_entity_modifier":{"type":"minecraft:nonsense"}}]}]"#),
        &names,
    )
    .unwrap();
    // Lenient optional fields fall back to their defaults.
    let Processor::Rule(rules) = &rule.0[0] else {
        panic!()
    };
    assert_eq!(rules[0].position, PosRuleTest::AlwaysTrue);
    assert_eq!(rules[0].modifier, BlockEntityModifier::Passthrough);
}

#[test]
fn position_rules_draw_once() {
    let test = PosRuleTest::Linear {
        min_chance: 0.0,
        max_chance: 1.0,
        min_dist: 0,
        max_dist: 10,
    };
    let mut random = LegacyRandom::new(1);
    let mut copy = LegacyRandom::new(1);
    let roll = crate::util::random::RandomSource::next_float(&mut copy);
    // Distance 5: chance 0.5.
    let hit = test.test(BlockPos::new(5, 0, 0), BlockPos::default(), &mut random);
    assert_eq!(hit, roll <= 0.5);
}

#[test]
fn json_data_narrows_numbers_like_nbt_ops() {
    let value: serde_json::Value = serde_json::from_str(
        r#"{"a":1,"b":300,"c":70000,"d":1.5,"e":0.1,"f":true,"g":[1,2],"h":"x"}"#,
    )
    .unwrap();
    let Tag::Compound(tag) = json_to_nbt(&value) else {
        panic!()
    };
    let get = |k: &str| tag.get(&JavaString::from(k)).unwrap().clone();
    assert_eq!(get("a"), Tag::Byte(1));
    assert_eq!(get("b"), Tag::Short(300));
    assert_eq!(get("c"), Tag::Int(70000));
    assert_eq!(get("d"), Tag::Float(1.5f32.to_bits()));
    assert_eq!(get("e"), Tag::Double(0.1f64.to_bits()));
    assert_eq!(get("f"), Tag::Byte(1));
    assert_eq!(get("g"), Tag::ByteArray(vec![1, 2]));
    assert_eq!(get("h"), Tag::String("x".into()));
    let _: Compound = tag;
}

#[test]
fn jigsaw_final_states_parse_like_the_command_parser() {
    let (blocks, _) = table();
    assert_eq!(parse_state(&blocks, "minecraft:stone"), Some(BlockState(1)));
    assert_eq!(
        parse_state(&blocks, "jigsaw[ orientation = north_up ]"),
        Some(BlockState(3))
    );
    assert_eq!(
        parse_state(&blocks, "jigsaw[orientation=\"down_east\"]{a:1b}"),
        Some(BlockState(2))
    );
    // Unknown block, property or value, a repeated property, an unclosed list, bad NBT.
    for bad in [
        "minecraft:gold_block",
        "jigsaw[facing=up]",
        "jigsaw[orientation=up]",
        "jigsaw[orientation=north_up,orientation=north_up]",
        "jigsaw[orientation=north_up",
        "stone{a:",
    ] {
        assert_eq!(parse_state(&blocks, bad), None, "{bad}");
    }
    // What follows the state is not read.
    assert_eq!(parse_state(&blocks, "stone trailing"), Some(BlockState(1)));
}
