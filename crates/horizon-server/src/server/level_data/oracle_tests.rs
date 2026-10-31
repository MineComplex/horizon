//! The per-level data against the real classes: `LeafLevelDataOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs the 26.3 jar's `WorldBorder`,
//! `ClockState` and `DifficultySettings`; this compares every row.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafLevelDataOracle.java level_data.jsonl
//! HORIZON_LEVEL_DATA_ORACLE=level_data.jsonl \
//!     cargo test -p horizon-server level_data_matches_java -- --ignored --nocapture
//! ```
//! `PaperLevelOverrides` and `ServerClockManager` are Paper's and not in the jar: derived from the
//! sources (their codecs are made of the fields tested here).

use serde_json::Value;

use super::*;

fn settings(v: &Value) -> BorderSettings {
    let d = |k: &str| v[k].as_f64().unwrap();
    BorderSettings {
        center_x: d("center_x"),
        center_z: d("center_z"),
        damage_per_block: d("damage_per_block"),
        safe_zone: d("safe_zone"),
        warning_blocks: v["warning_blocks"].as_i64().unwrap() as i32,
        warning_time: v["warning_time"].as_i64().unwrap() as i32,
        size: d("size"),
        lerp_time: v["lerp_time"].as_i64().unwrap(),
        lerp_target: d("lerp_target"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
#[ignore = "needs HORIZON_LEVEL_DATA_ORACLE (LeafLevelDataOracle.java on the 26.3 jar)"]
fn level_data_matches_java() {
    let path = std::env::var_os("HORIZON_LEVEL_DATA_ORACLE").expect("HORIZON_LEVEL_DATA_ORACLE");
    let text = std::fs::read_to_string(path).expect("the oracle's output");
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let kind = row["kind"].as_str().unwrap().to_owned();
        *counts.entry(kind.clone()).or_default() += 1;
        match kind.as_str() {
            "border" => {
                // The settings go through the codec (as Java's did), are applied, and ticked.
                let given = settings(&row["settings"]);
                let read = BorderSettings::parse(&given.to_tag()).expect("decodes");
                let mut border = WorldBorder::new(read);
                border.apply_initial_settings();
                for _ in 0..row["ticks"].as_i64().unwrap() {
                    border.tick();
                }
                assert_eq!(border.size(), row["size"].as_f64().unwrap(), "{line}");
                assert_eq!(
                    border.to_settings(),
                    settings(&row["saved"]),
                    "saved: {line}"
                );
                let p = border.packet(1.0);
                assert_eq!(p.lerp_time, row["lerp_time"].as_i64().unwrap(), "{line}");
                assert_eq!(p.new_size, row["lerp_target"].as_f64().unwrap(), "{line}");
                let bytes = crate::network::play::clientbound::initialize_border(&p);
                // Our packet starts with its id; Java's codec bytes do not.
                assert_eq!(hex(&bytes[1..]), row["packet"].as_str().unwrap(), "{line}");
            }
            "border_far" => {
                let mut tag = BorderSettings::DEFAULT.to_tag();
                tag.insert("center_x".into(), Tag::Double(3.0e7f64.to_bits()));
                assert_eq!(
                    BorderSettings::parse(&tag).is_some(),
                    row["ok"].as_bool().unwrap()
                );
            }
            "clock" => {
                let input = &row["input"];
                let mut tag = Compound::new();
                tag.insert(
                    "total_ticks".into(),
                    Tag::Long(input["total_ticks"].as_i64().unwrap()),
                );
                for key in ["partial_tick", "rate"] {
                    if let Some(v) = input[key].as_f64() {
                        tag.insert(key.into(), Tag::Float((v as f32).to_bits()));
                    }
                }
                if let Some(v) = input["paused"].as_bool() {
                    tag.insert("paused".into(), Tag::Byte(v.into()));
                }
                let ours = ClockState::parse(&Tag::Compound(tag));
                assert_eq!(ours.is_some(), row["ok"].as_bool().unwrap(), "{line}");
                if let Some(state) = ours {
                    let Tag::Compound(back) = state.to_tag() else {
                        panic!()
                    };
                    let output = &row["output"];
                    let mut keys: Vec<String> = back.keys().map(|k| k.to_string_lossy()).collect();
                    keys.sort();
                    let mut expected: Vec<String> =
                        output.as_object().unwrap().keys().cloned().collect();
                    expected.sort();
                    assert_eq!(keys, expected, "{line}");
                    assert_eq!(
                        storage::number(back.get(&"total_ticks".into())),
                        output["total_ticks"].as_i64()
                    );
                }
            }
            "difficulty" | "difficulty_missing_locked" => {
                let mut tag =
                    LevelOverrides::from_root(&LevelData::parse(&Compound::new())).to_tag();
                let name = row["input"].as_str().unwrap_or("easy");
                let mut settings = Compound::new();
                settings.insert("difficulty".into(), Tag::String(name.into()));
                settings.insert("hardcore".into(), Tag::Byte(0));
                if kind == "difficulty" {
                    settings.insert("locked".into(), Tag::Byte(1));
                }
                tag.insert("difficulty_settings".into(), Tag::Compound(settings));
                assert_eq!(
                    LevelOverrides::parse(&tag).is_some(),
                    row["ok"].as_bool().unwrap(),
                    "{line}"
                );
            }
            other => panic!("unknown row kind {other}"),
        }
    }
    eprintln!("{counts:?}");
}
