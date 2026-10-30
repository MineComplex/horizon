//! Density values against the real 26.3 server: `LeafDensityOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) samples every noise settings
//! element's router and aquifer roots, and every noise, for two seeds through vanilla's own
//! `RandomState`; this compares each value bit for bit.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafDensityOracle.java density.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_DENSITY_ORACLE=density.txt \
//!     cargo test -p horizon-server --release density_matches_java -- --ignored --nocapture
//! ```

use std::collections::HashMap;

use serde_json::value::RawValue;

use super::data::{LevelDensity, WorldgenData};
use super::density::{DensityContext, NodeId};
use super::noise::DensityVolume;
use super::settings::{AquiferField, RouterField};
use crate::block::{BlockStates, BlockStatesBuilder};
use crate::registry::DataPack;
use crate::server::vanilla_jar::VanillaJar;

/// The blocks the noise settings name (their states do not matter to densities).
pub(crate) fn settings_blocks() -> BlockStates {
    let mut builder = BlockStatesBuilder::new();
    for name in [
        "air",
        "stone",
        "water",
        "lava",
        "netherrack",
        "end_stone",
        "void_air",
    ] {
        builder
            .add_block(&format!("minecraft:{name}"), &[])
            .expect("distinct blocks");
    }
    builder.finish()
}

pub(crate) fn load_jar_data() -> Option<WorldgenData> {
    let jar = std::env::var_os("HORIZON_VANILLA_JAR")?;
    let mut jar = VanillaJar::open(jar.as_ref()).expect("the 26.3 jar");
    let pack = DataPack::vanilla(&mut jar).expect("the vanilla pack");
    Some(WorldgenData::load(&[pack], &settings_blocks()).expect("the worldgen registries"))
}

struct Level {
    density: LevelDensity,
    noises: HashMap<String, NodeId>,
}

impl Level {
    fn root(&self, name: &str) -> NodeId {
        if let Some(field) = RouterField::ALL.iter().find(|f| f.name() == name) {
            return self.density.router(*field);
        }
        if let Some(field) = name.strip_prefix("aquifer_") {
            let field = AquiferField::ALL
                .iter()
                .find(|f| f.name() == field)
                .unwrap_or_else(|| panic!("aquifer root {name}"));
            return self.density.aquifer(*field).expect("aquifer roots");
        }
        self.noises[name]
    }
}

#[test]
#[ignore = "needs the 26.3 jar and LeafDensityOracle's output"]
fn density_matches_java() {
    let data = load_jar_data().expect("HORIZON_VANILLA_JAR is not set");
    let oracle_path = std::env::var("HORIZON_DENSITY_ORACLE").expect("HORIZON_DENSITY_ORACLE");
    let oracle = std::fs::read_to_string(oracle_path).unwrap();
    let noise_ids: Vec<String> = data.graph().noise_names().map(str::to_owned).collect();
    let noise_json: Vec<Box<RawValue>> = noise_ids
        .iter()
        .map(|id| {
            serde_json::value::to_raw_value(&serde_json::json!({
                "type": "minecraft:noise", "noise": id, "xz_scale": 1, "y_scale": 1
            }))
            .unwrap()
        })
        .collect();
    let noise_refs: Vec<&RawValue> = noise_json.iter().map(AsRef::as_ref).collect();
    let mut levels: HashMap<(String, i64), Level> = HashMap::new();
    let mut checked: HashMap<&str, usize> = HashMap::new();
    let mut mismatches = Vec::new();
    let mut context = DensityContext::cached();
    for line in oracle.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        let (kind, settings, seed, root) = (fields[0], fields[1], fields[2], fields[3]);
        let seed: i64 = seed.parse().unwrap();
        let level = levels
            .entry((settings.to_owned(), seed))
            .or_insert_with(|| {
                let noise_settings = data.noise_settings(settings).expect("settings");
                let (density, nodes) = data
                    .compile_level_with(noise_settings, seed, &noise_refs)
                    .expect("compiles");
                let noises = noise_ids
                    .iter()
                    .zip(nodes)
                    .map(|(id, node)| (format!("noise:{id}"), node))
                    .collect();
                Level { density, noises }
            });
        let node = level.root(root);
        let program = &level.density.program;
        let int = |i: usize| fields[i].parse::<i32>().unwrap();
        let bits = |text: &str| u32::from_str_radix(text, 16).unwrap();
        match kind {
            "value" | "cached" => {
                let position = [int(4), int(5), int(6)];
                let value = if kind == "value" {
                    program.sample(&mut DensityContext::uncached(), node, position)
                } else {
                    program.sample(&mut context, node, position)
                };
                if value.to_bits() != bits(fields[7]) {
                    mismatches.push(format!("{line}: got {:x} ({value})", value.to_bits()));
                }
                *checked
                    .entry(if kind == "value" { "value" } else { "cached" })
                    .or_default() += 1;
            }
            "volume" => {
                let volume = DensityVolume::new(
                    [int(4), int(5), int(6)],
                    [int(7), int(8), int(9)],
                    [int(10), int(11), int(12)],
                )
                .unwrap();
                context = DensityContext::cached();
                let mut out = vec![0.0; volume.size()];
                program.fill(&mut context, node, &mut out, volume);
                let expected = &fields[13..];
                assert_eq!(expected.len(), out.len());
                let wrong = out
                    .iter()
                    .zip(expected)
                    .filter(|(v, e)| v.to_bits() != bits(e))
                    .count();
                if wrong > 0 {
                    mismatches.push(format!(
                        "volume {} {} {} {:?}: {wrong} of {} values differ",
                        settings,
                        seed,
                        root,
                        &fields[4..13],
                        out.len()
                    ));
                }
                *checked.entry("volume values").or_default() += out.len();
            }
            other => panic!("unknown row {other}"),
        }
    }
    eprintln!("checked: {checked:?}");
    for mismatch in mismatches.iter().take(30) {
        eprintln!("{mismatch}");
    }
    assert!(
        mismatches.is_empty(),
        "{} rows differ from Java",
        mismatches.len()
    );
}
