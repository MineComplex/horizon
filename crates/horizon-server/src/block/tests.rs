//! A synthetic table built here (no Mojang data in the repository), plus a check against the real
//! 26.3 report when `HORIZON_SERVER_BLOCKS_JSON` names a `blocks.json` (made by
//! `tools/leaf_server_reports.py`); that test passes without checking anything otherwise.

use std::collections::BTreeMap;

use super::*;

const BOOL: &[&str] = &["true", "false"];
const FACING: &[&str] = &["north", "south", "west", "east"];

/// Shaped like vanilla's start of the registry, with a stairs-like and a chest-like block.
fn synthetic() -> BlockStates {
    let mut b = BlockStatesBuilder::new();
    b.add_block("minecraft:air", &[]).unwrap();
    b.add_block("minecraft:stone", &[]).unwrap();
    // Given out of order: the table sorts by name (facing, half, shape, waterlogged).
    let stairs = b
        .add_block(
            "minecraft:oak_stairs",
            &[
                ("waterlogged", BOOL),
                (
                    "shape",
                    &[
                        "straight",
                        "inner_left",
                        "inner_right",
                        "outer_left",
                        "outer_right",
                    ],
                ),
                ("facing", FACING),
                ("half", &["top", "bottom"]),
            ],
        )
        .unwrap();
    // facing=north, half=bottom, shape=straight, waterlogged=false: 0*20 + 1*10 + 0*2 + 1.
    b.set_default(stairs, BlockState(2 + 11)).unwrap();
    let chest = b
        .add_block(
            "minecraft:chest",
            &[
                ("type", &["single", "left", "right"]),
                ("facing", FACING),
                ("waterlogged", BOOL),
            ],
        )
        .unwrap();
    b.set_default(chest, BlockState(2 + 80 + 1)).unwrap();
    b.add_block("minecraft:cave_air", &[]).unwrap();
    b.finish()
}

#[test]
fn ids_follow_vanilla_order() {
    let t = synthetic();
    assert_eq!(t.state_count(), 2 + 80 + 24 + 1);
    assert_eq!(t.block_count(), 5);
    let stairs = t.block_by_name("minecraft:oak_stairs").unwrap();
    assert_eq!(stairs, BlockId(2));
    assert_eq!(t.states(stairs).next(), Some(BlockState(2)));
    assert_eq!(t.states(stairs).len(), 80);
    let names: Vec<&str> = t.properties(stairs).map(|p| t.property_name(p)).collect();
    assert_eq!(names, ["facing", "half", "shape", "waterlogged"]);
    let default = t.default_state(stairs);
    assert_eq!(default, BlockState(13));
    let values: Vec<(&str, &str)> = t.values(default).collect();
    assert_eq!(
        values,
        [
            ("facing", "north"),
            ("half", "bottom"),
            ("shape", "straight"),
            ("waterlogged", "false")
        ]
    );
    // Last state: east, bottom, outer_right, false.
    assert_eq!(t.value(BlockState(81), "facing"), Some("east"));
    assert_eq!(t.value(BlockState(81), "shape"), Some("outer_right"));
    assert_eq!(t.block(BlockState(81)), stairs);
    assert_eq!(
        t.block(BlockState(82)),
        t.block_by_name("minecraft:chest").unwrap()
    );
    // Chest: facing slowest, then type, then waterlogged.
    assert_eq!(t.value(BlockState(82 + 6), "facing"), Some("south"));
    assert_eq!(t.value(BlockState(82 + 2), "type"), Some("left"));
}

#[test]
fn set_value_is_id_arithmetic() {
    let t = synthetic();
    let stairs = t.block_by_name("minecraft:oak_stairs").unwrap();
    let d = t.default_state(stairs);
    let east = t.with_property(d, "facing", "east").unwrap();
    assert_eq!(east, BlockState(13 + 3 * 20));
    assert_eq!(t.with_property(east, "facing", "north"), Some(d));
    assert_eq!(
        t.with_property(d, "shape", "outer_left")
            .map(BlockState::id),
        Some(13 + 3 * 2)
    );
    // Where vanilla throws: no such property, or a value it does not allow.
    assert_eq!(t.with_property(d, "type", "left"), None);
    assert_eq!(t.with_property(d, "facing", "up"), None);
    let facing = t.property(stairs, "facing").unwrap();
    assert_eq!(t.with_value_index(d, facing, 4), None);
    assert_eq!(t.value_index(east, facing), Some(3));
    assert_eq!(t.cycle(east, facing), Some(d));
    // `trySetValue` on a block without the property keeps the state.
    let stone = t.default_state(t.block_by_name("minecraft:stone").unwrap());
    assert_eq!(t.try_with_value_index(stone, facing, 1), stone);
    assert_eq!(t.value_index(stone, facing), None);
    // Partial property lists start from the default state.
    assert_eq!(
        t.state_with(stairs, [("half", "top"), ("facing", "west")]),
        Some(BlockState(2 + 40 + 1))
    );
    // Every state round-trips through its own values.
    for state in (0..t.state_count()).map(BlockState) {
        let block = t.block(state);
        let values: Vec<(&str, &str)> = t.values(state).collect();
        assert_eq!(t.state_with(block, values), Some(state));
    }
}

#[test]
fn shares_properties_and_flags() {
    let t = synthetic();
    let stairs = t.block_by_name("minecraft:oak_stairs").unwrap();
    let chest = t.block_by_name("minecraft:chest").unwrap();
    assert_eq!(t.property(stairs, "facing"), t.property(chest, "facing"));
    assert_eq!(
        t.property(stairs, "waterlogged"),
        t.property(chest, "waterlogged")
    );
    assert!(t.is_air(BlockState::AIR));
    assert!(t.is_air(BlockState(t.state_count() - 1)));
    assert!(!t.is_air(BlockState(1)));
    assert!(t.contains(BlockState(106)) && !t.contains(BlockState(107)));
    assert_eq!(<BlockState as PaletteValue>::from_id(7).id(), 7);
}

#[test]
fn builder_rejects_bad_blocks() {
    let mut b = BlockStatesBuilder::new();
    b.add_block("minecraft:air", &[]).unwrap();
    assert_eq!(
        b.add_block("minecraft:air", &[]),
        Err(BlockDataError::DuplicateBlock("minecraft:air".into()))
    );
    assert!(b.add_block("minecraft:a", &[("x", &[])]).is_err());
    assert!(b.add_block("minecraft:b", &[("x", &["1", "1"])]).is_err());
    assert!(
        b.add_block("minecraft:c", &[("x", BOOL), ("x", BOOL)])
            .is_err()
    );
    assert!(b.set_default(BlockId(0), BlockState(1)).is_err());
}

#[test]
fn reads_a_report() {
    // Keys out of order as the generator writes them (`type` first).
    let json = r#"{
      "minecraft:stone": {"states": [{"default": true, "id": 1}]},
      "minecraft:air": {"states": [{"default": true, "id": 0}]},
      "minecraft:chest": {"properties": {"type": ["single", "left"], "facing": ["north", "south"]},
        "states": [
          {"id": 2, "properties": {"type": "single", "facing": "north"}},
          {"id": 3, "default": true, "properties": {"type": "left", "facing": "north"}},
          {"id": 4, "properties": {"type": "single", "facing": "south"}},
          {"id": 5, "properties": {"type": "left", "facing": "south"}}]}
    }"#;
    let t = BlockStates::from_report(json).unwrap();
    assert_eq!(t.block_name(BlockId(2)), "minecraft:chest");
    assert_eq!(t.default_state(BlockId(2)), BlockState(3));
    assert_eq!(t.value(BlockState(4), "facing"), Some("south"));
    // A state where the order does not put it.
    let swapped = json.replace(r#""id": 3, "default": true"#, r#""id": 9, "default": true"#);
    assert!(matches!(
        BlockStates::from_report(&swapped),
        Err(BlockDataError::StateOrder { .. })
    ));
    let no_default = json.replace(r#""default": true, "id": 1"#, r#""id": 1"#);
    assert_eq!(
        BlockStates::from_report(&no_default).err(),
        Some(BlockDataError::NoDefault("minecraft:stone".into()))
    );
}

#[derive(serde::Deserialize)]
struct ReportBlock {
    #[serde(default)]
    properties: BTreeMap<String, Vec<String>>,
    states: Vec<ReportState>,
}
#[derive(serde::Deserialize)]
struct ReportState {
    id: u32,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    properties: BTreeMap<String, String>,
}

/// `HORIZON_SERVER_BLOCKS_JSON=<server dir>/cache/vanilla-26.3/reports/blocks.json cargo test -p
/// horizon-server block::tests::real_26_3_report -- --nocapture`
#[test]
fn real_26_3_report() {
    let Some(path) = std::env::var_os("HORIZON_SERVER_BLOCKS_JSON") else {
        eprintln!("HORIZON_SERVER_BLOCKS_JSON is not set: the 26.3 report is not checked");
        return;
    };
    let path = std::path::PathBuf::from(path);
    let t = BlockStates::load(&path).unwrap();
    assert_eq!(t.state_count(), 35723);
    assert_eq!(t.block_count(), 1286);
    let block = |name| t.block_by_name(name).unwrap();
    assert_eq!(t.default_state(block("minecraft:air")), BlockState(0));
    assert_eq!(t.default_state(block("minecraft:stone")), BlockState(1));
    let stairs = block("minecraft:oak_stairs");
    let d = t.default_state(stairs);
    assert_eq!(d, BlockState(5474));
    let values: Vec<(&str, &str)> = t.values(d).collect();
    assert_eq!(
        values,
        [
            ("facing", "north"),
            ("half", "bottom"),
            ("shape", "straight"),
            ("waterlogged", "false")
        ]
    );
    assert_eq!(
        t.with_property(d, "facing", "east").map(BlockState::id),
        Some(5474 + 60)
    );
    let airs: Vec<&str> = (0..t.block_count())
        .map(BlockId)
        .filter(|&b| t.is_air(t.default_state(b)))
        .map(|b| t.block_name(b))
        .collect();
    assert_eq!(
        airs,
        ["minecraft:air", "minecraft:void_air", "minecraft:cave_air"]
    );

    // Every state, property and value: `setValue` against the report's own ids.
    let report: BTreeMap<String, ReportBlock> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut checked = 0u64;
    for (name, rb) in &report {
        let b = block(name);
        let by_values: BTreeMap<&BTreeMap<String, String>, u32> =
            rb.states.iter().map(|s| (&s.properties, s.id)).collect();
        for s in &rb.states {
            let state = BlockState(s.id);
            assert_eq!(t.block(state), b);
            assert_eq!(s.default, t.default_state(b) == state, "{name} {}", s.id);
            for (prop, values) in &rb.properties {
                assert_eq!(t.value(state, prop), Some(s.properties[prop].as_str()));
                for v in values {
                    let mut want = s.properties.clone();
                    want.insert(prop.clone(), v.clone());
                    assert_eq!(
                        t.with_property(state, prop, v),
                        Some(BlockState(by_values[&want]))
                    );
                    checked += 1;
                }
            }
        }
    }
    eprintln!("{checked} setValue results match the report");

    // Block ids are the registry's, when `registries.json` sits next to the report.
    if let Ok(text) = std::fs::read_to_string(path.with_file_name("registries.json")) {
        let registries: serde_json::Value = serde_json::from_str(&text).unwrap();
        let entries = registries["minecraft:block"]["entries"]
            .as_object()
            .unwrap();
        assert_eq!(entries.len() as u32, t.block_count());
        for (name, entry) in entries {
            assert_eq!(
                u64::from(block(name.as_str()).0),
                entry["protocol_id"].as_u64().unwrap()
            );
        }
        eprintln!("block ids match registries.json");
    }
}
