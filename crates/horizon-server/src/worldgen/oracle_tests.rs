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

/// The blocks the noise settings name (their states do not matter to densities); the real block
/// table when `HORIZON_SERVER_BLOCKS_JSON` names `blocks.json`, which the feature registry needs
/// (its block predicates and tags name any block).
pub(crate) fn settings_blocks() -> BlockStates {
    if let Some(path) = std::env::var_os("HORIZON_SERVER_BLOCKS_JSON") {
        return BlockStates::load(path.as_ref()).expect("blocks.json");
    }
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
                    .compile_level_with(noise_settings, seed, false, &noise_refs)
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

/// The bench workload both density implementations run (this program and Horizon's `Arc<dyn>`
/// sampler tree, `crates/horizon-world`): the overworld's `final_density` over 256 chunks as
/// noise-cell corner volumes (5x49x5, steps 4/8/4, a fresh cached context per chunk, as a chunk's
/// fill does), then every router field at 4,096 points with an uncached context. Prints the
/// best of five runs and a checksum of the values' bits, which must equal the other side's.
///
/// ```sh
/// HORIZON_VANILLA_JAR=<26.3 jar> cargo test -p horizon-server --release density_program_speed \
///     -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a benchmark; needs the 26.3 jar"]
fn density_program_speed() {
    use std::time::{Duration, Instant};
    let data = load_jar_data().expect("HORIZON_VANILLA_JAR is not set");
    let settings = data.noise_settings("minecraft:overworld").unwrap();
    let density = data.compile_level(settings, 12345, false).unwrap();
    let program = &density.program;
    let final_density = density.router(RouterField::FinalDensity);
    let mut best = (Duration::MAX, Duration::MAX);
    let mut sums = (0u64, 0u64);
    for _ in 0..5 {
        let start = Instant::now();
        let mut sum = 0u64;
        let mut out = vec![0.0; 5 * 49 * 5];
        for cz in 0..16 {
            for cx in 0..16 {
                let volume =
                    DensityVolume::new([5, 49, 5], [cx * 16, -64, cz * 16], [4, 8, 4]).unwrap();
                let mut context = DensityContext::cached();
                program.fill(&mut context, final_density, &mut out, volume);
                sum = out
                    .iter()
                    .fold(sum, |s, v| s.wrapping_add(u64::from(v.to_bits())));
            }
        }
        let volumes = start.elapsed();
        let start = Instant::now();
        let mut points = 0u64;
        let mut context = DensityContext::uncached();
        for i in 0..4096i32 {
            let position = [
                i.wrapping_mul(7919) % 30_000,
                (i % 384) - 64,
                i.wrapping_mul(104_729) % 30_000,
            ];
            for field in RouterField::ALL {
                let value = program.sample(&mut context, density.router(field), position);
                points = points.wrapping_add(u64::from(value.to_bits()));
            }
        }
        let scalar = start.elapsed();
        best = (best.0.min(volumes), best.1.min(scalar));
        sums = (sum, points);
    }
    eprintln!(
        "program: volumes {:?} ({:?}/chunk), points {:?}; checksums {:x} {:x}",
        best.0,
        best.0 / 256,
        best.1,
        sums.0,
        sums.1
    );
}
