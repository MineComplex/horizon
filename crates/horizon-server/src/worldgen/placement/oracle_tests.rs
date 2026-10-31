//! Placement against the real 26.3 server: `LeafPlacementOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs the int and height providers, every
//! placed feature's modifier chain (and custom chains covering every modifier and predicate), and the
//! feature sorter on vanilla's own classes; this compares each result. The chains run over a synthetic
//! world given by formulas, repeated here (`Synth`); `would_survive` runs `BlockState.canSurvive` over
//! that world on both sides (light 0, as Paper's `WorldGenRegion` answers during features).
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafPlacementOracle.java placement.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_PLACEMENT_ORACLE=placement.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     cargo test -p horizon-server --release placement_matches_java -- --ignored --nocapture
//! ```

use std::collections::HashMap;

use serde_json::value::RawValue;

use super::{DecodeContext, FLUIDS, FeaturePlacer, HeightProvider, IntProvider, PlacementModifier};
use super::{PlacementWorld, WorldError};
use crate::block::survival::{BlockSurvival, SurvivalError, SurvivalWorld};
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::position::BlockPos;
use crate::registry::{DataPack, for_each_element, load_tags, registry_dir};
use crate::server::vanilla_jar::VanillaJar;
use crate::util::random::{InvalidBound, LegacyRandom, RandomSource, XoroshiroRandom};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::data::WorldgenData;
use crate::worldgen::feature::PlacedFeatureId;

const PALETTE: [&str; 16] = [
    "air",
    "stone",
    "dirt",
    "water",
    "lava",
    "grass_block",
    "cave_air",
    "sand",
    "void_air",
    "oak_leaves",
    "snow",
    "gravel",
    "short_grass",
    "podzol",
    "deepslate",
    "cobblestone",
];

/// The oracle's world: heights, blocks and biomes by formula.
struct Synth<'a> {
    blocks: &'a BlockStates,
    data: &'a BlockStateData,
    palette: Vec<BlockState>,
    bedrock: BlockState,
    level_min: i32,
    level_height: i32,
    generator: WorldGenerationContext,
    /// Biome ids by name order.
    biomes: &'a [u32],
    fixed: Option<u32>,
    survival: &'a BlockSurvival,
}

fn floor_mod(value: i32, modulus: i32) -> i32 {
    value.rem_euclid(modulus)
}

impl Synth<'_> {
    fn height_at(&self, kind: Heightmap, x: i32, z: i32) -> i32 {
        let hash = (x.wrapping_mul(31))
            .wrapping_add(z.wrapping_mul(17))
            .wrapping_add((kind as i32) * 7);
        self.level_min + 24 + floor_mod(hash, 97)
    }

    fn block_at(&self, x: i32, y: i32, z: i32) -> BlockState {
        if y <= self.level_min {
            return self.bedrock;
        }
        if y >= self.height_at(Heightmap::MotionBlocking, x, z) + 4 {
            return self.palette[0];
        }
        let hash = (x.wrapping_mul(31))
            .wrapping_add(y.wrapping_mul(17))
            .wrapping_add(z.wrapping_mul(13))
            .wrapping_add((x >> 2).wrapping_mul(y >> 1));
        self.palette[floor_mod(hash, 16) as usize]
    }
}

/// `canSurvive` reads the same world; light is 0 as in Paper's `WorldGenRegion` during features.
struct SynthReader<'a, 'b>(&'a Synth<'b>);

impl SurvivalWorld for SynthReader<'_, '_> {
    type Error = WorldError;
    fn block_state(&self, x: i32, y: i32, z: i32) -> Result<BlockState, WorldError> {
        Ok(self.0.block_at(x, y, z))
    }
    fn raw_brightness(&self, _: i32, _: i32, _: i32) -> Result<i32, WorldError> {
        Ok(0)
    }
}

impl PlacementWorld for Synth<'_> {
    fn blocks(&self) -> &BlockStates {
        self.blocks
    }
    fn state_data(&self) -> &BlockStateData {
        self.data
    }
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::with_generator(
            self.level_min,
            self.level_height,
            self.generator.min_y,
            self.generator.height,
            self.generator.sea_level,
        )
    }
    fn level_generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(self.level_min, self.level_height, 63)
    }
    fn height(&self, kind: Heightmap, x: i32, z: i32) -> Result<i32, WorldError> {
        Ok(self.height_at(kind, x, z))
    }
    fn block_state(&self, pos: BlockPos) -> Result<BlockState, WorldError> {
        Ok(self.block_at(pos.x, pos.y, pos.z))
    }
    fn biome(&self, pos: BlockPos) -> Result<u32, WorldError> {
        if let Some(fixed) = self.fixed {
            return Ok(fixed);
        }
        let hash = ((pos.x >> 2).wrapping_mul(3))
            .wrapping_add((pos.z >> 2).wrapping_mul(5))
            .wrapping_add((pos.y >> 2).wrapping_mul(7));
        Ok(self.biomes[floor_mod(hash, self.biomes.len() as i32) as usize])
    }
    fn block_survives(&self, state: BlockState, pos: BlockPos) -> Result<bool, WorldError> {
        let at = [pos.x, pos.y, pos.z];
        (self.survival)
            .can_survive(self.blocks, self.data, &SynthReader(self), state, at)
            .map_err(|e| match e {
                SurvivalError::World(e) => e,
                SurvivalError::Unsupported(what) => WorldError::Unsupported(what),
            })
    }
}

struct Fixture {
    data: WorldgenData,
    blocks: BlockStates,
    state_data: BlockStateData,
    /// `worldgen/biome` names by id.
    biome_names: Vec<String>,
    /// Biome ids by name order.
    sorted_biomes: Vec<u32>,
    biome_tags: Vec<(String, Vec<i32>)>,
    block_tags: Vec<(String, Vec<i32>)>,
    fluid_tags: Vec<(String, Vec<i32>)>,
    survival: BlockSurvival,
}

impl Fixture {
    fn load() -> Self {
        let var =
            |name: &str| std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set"));
        let mut jar = VanillaJar::open(var("HORIZON_VANILLA_JAR").as_ref()).expect("the 26.3 jar");
        let pack = DataPack::vanilla(&mut jar).expect("the vanilla pack");
        let blocks = BlockStates::load(var("HORIZON_SERVER_BLOCKS_JSON").as_ref()).unwrap();
        let state_data =
            BlockStateData::load(var("HORIZON_SERVER_BLOCK_STATES").as_ref(), &blocks).unwrap();
        let mut biome_names = Vec::new();
        for_each_element(
            std::slice::from_ref(&pack),
            "minecraft:worldgen/biome",
            |file| {
                biome_names.push(file.id.to_owned());
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        let packs = std::slice::from_ref(&pack);
        let biome_tags = load_tags(packs, &registry_dir("minecraft:worldgen/biome"), |id| {
            biome_names.iter().position(|b| b == id).map(|i| i as i32)
        });
        let block_tags = load_tags(packs, &registry_dir("minecraft:block"), |id| {
            blocks.block_by_name(id).map(|b| b.0 as i32)
        });
        let fluid_tags = load_tags(packs, &registry_dir("minecraft:fluid"), |id| {
            FLUIDS.iter().position(|f| *f == id).map(|i| i as i32)
        });
        let survival = BlockSurvival::new(&blocks, &state_data, &block_tags, &fluid_tags).unwrap();
        let data = WorldgenData::load(packs, &blocks).expect("the worldgen registries");
        let mut sorted_biomes: Vec<u32> = (0..biome_names.len() as u32).collect();
        sorted_biomes.sort_by(|a, b| biome_names[*a as usize].cmp(&biome_names[*b as usize]));
        // The fluid registry's order is the state data's.
        for (id, name) in FLUIDS.iter().enumerate() {
            assert_eq!(state_data.fluid_name(id as u8), *name);
        }
        Self {
            data,
            blocks,
            state_data,
            biome_names,
            sorted_biomes,
            biome_tags,
            block_tags,
            fluid_tags,
            survival,
        }
    }

    fn names(&self) -> DecodeContext<'_> {
        DecodeContext {
            blocks: &self.blocks,
            block_tags: &self.block_tags,
            biomes: &self.biome_names,
            biome_tags: &self.biome_tags,
            fluid_tags: &self.fluid_tags,
        }
    }

    fn biome_id(&self, name: &str) -> u32 {
        self.biome_names
            .iter()
            .position(|b| b == name)
            .expect("a biome") as u32
    }

    fn generator(&self, profile: &str) -> (i32, i32, WorldGenerationContext) {
        let settings = |name| {
            let s = self.data.noise_settings(name).expect("noise settings");
            WorldGenerationContext::new(s.noise.min_y, s.noise.height, s.sea_level)
        };
        match profile {
            "A" => (-64, 384, settings("minecraft:overworld")),
            "B" => (-64, 384, settings("minecraft:nether")),
            "C" => (0, 256, settings("minecraft:overworld")),
            _ => panic!("profile {profile}"),
        }
    }

    fn world(&self, profile: &str, fixed: Option<u32>) -> Synth<'_> {
        let (level_min, level_height, generator) = self.generator(profile);
        let state = |name: &str| {
            let block = self
                .blocks
                .block_by_name(&format!("minecraft:{name}"))
                .unwrap();
            self.blocks.default_state(block)
        };
        Synth {
            blocks: &self.blocks,
            data: &self.state_data,
            palette: PALETTE.iter().map(|n| state(n)).collect(),
            bedrock: state("bedrock"),
            level_min,
            level_height,
            generator,
            biomes: &self.sorted_biomes,
            fixed,
            survival: &self.survival,
        }
    }
}

/// Runs `$body` with `$r` the oracle's random of this kind and seed.
macro_rules! with_random {
    ($kind:expr, $seed:expr, |$r:ident| $body:expr) => {
        if $kind == "legacy" {
            let mut $r = LegacyRandom::new($seed);
            $body
        } else {
            let mut $r = XoroshiroRandom::new($seed);
            $body
        }
    };
}

fn raw(json: &str) -> Box<RawValue> {
    RawValue::from_string(json.to_owned()).unwrap()
}

/// `value,value,...` of 24 draws, `error` if one fails, `decode error` if the JSON does not decode.
fn sample_list<T>(
    decoded: Result<T, ()>,
    mut sample: impl FnMut(&T) -> Result<i32, InvalidBound>,
) -> String {
    let Ok(provider) = decoded else {
        return "decode error".into();
    };
    let mut values = Vec::new();
    for _ in 0..24 {
        match sample(&provider) {
            Ok(v) => values.push(v.to_string()),
            Err(_) => return "error".into(),
        }
    }
    values.join(",")
}

/// One `place` row: the positions, whether anything was placed, and the next long.
fn place_row(fx: &Fixture, fields: &[&str]) -> (String, String, String) {
    let [_, profile, kind, seed, origin, mode, name, ..] = fields else {
        panic!("place row {fields:?}")
    };
    let seed: i64 = seed.parse().unwrap();
    let origin: Vec<i32> = origin.split(',').map(|v| v.parse().unwrap()).collect();
    let origin = BlockPos::new(origin[0], origin[1], origin[2]);
    let fixed = (*mode != "vary").then(|| fx.biome_id(mode));
    let mut world = fx.world(profile, fixed);
    let registry = fx.data.features();
    let custom: Vec<PlacementModifier>;
    let (modifiers, top): (&[PlacementModifier], Option<PlacedFeatureId>) = if name.starts_with('[')
    {
        let items: Vec<Box<RawValue>> = serde_json::from_str(name).unwrap();
        let decoded: Result<Vec<_>, _> = items
            .iter()
            .map(|m| PlacementModifier::decode(m, &fx.names()))
            .collect();
        match decoded {
            Ok(list) => custom = list,
            Err(_) => return ("decode error".into(), String::new(), String::new()),
        }
        (&custom, None)
    } else {
        let id = registry
            .placed_id(name)
            .unwrap_or_else(|| panic!("placed feature {name}"));
        (&registry.placed(id).placement, Some(id))
    };
    let mut placer = FeaturePlacer::default();
    let mut positions = Vec::new();
    with_random!(*kind, seed, |random| {
        let result = placer.place_chain(
            registry,
            modifiers,
            top,
            &mut world,
            &mut random,
            origin,
            |_, r, p| {
                positions.push(p);
                Ok::<_, InvalidBound>(r.next_int_bound(3)? == 0)
            },
        );
        match result {
            Err(_) => ("error".to_owned(), "false".to_owned(), String::new()),
            Ok(any) => {
                let list: Vec<String> = positions
                    .iter()
                    .map(|p| format!("{} {} {}", p.x, p.y, p.z))
                    .collect();
                (
                    list.join(";"),
                    any.to_string(),
                    random.next_long().to_string(),
                )
            }
        }
    })
}

#[test]
#[ignore = "needs the 26.3 jar and LeafPlacementOracle's output"]
fn placement_matches_java() {
    let fx = Fixture::load();
    let oracle = std::fs::read_to_string(
        std::env::var("HORIZON_PLACEMENT_ORACLE").expect("HORIZON_PLACEMENT_ORACLE"),
    )
    .unwrap();
    let registry = fx.data.features();
    let mut sets: Vec<(String, Vec<u32>)> = Vec::new();
    let mut sorted: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    let mut cycles = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut nonempty = 0;
    let mut mismatches = Vec::new();
    let mut check = |what: String, expected: &str, got: String| {
        if expected != got {
            mismatches.push(format!("{what}\n  java: {expected}\n  rust: {got}"));
        }
    };
    for line in oracle.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        *counts.entry(fields[0]).or_default() += 1;
        match fields[0] {
            "int" => {
                let [_, kind, seed, expected, json] = fields[..] else {
                    panic!("{line}")
                };
                let seed: i64 = seed.parse().unwrap();
                let got = with_random!(kind, seed, |random| sample_list(
                    IntProvider::decode(&raw(json)).map_err(|_| ()),
                    |p| p.sample(&mut random)
                ));
                check(format!("int {kind} {seed} {json}"), expected, got);
            }
            "height" => {
                let [_, profile, kind, seed, expected, json] = fields[..] else {
                    panic!("{line}")
                };
                let seed: i64 = seed.parse().unwrap();
                let (min, height, generator) = fx.generator(profile);
                let context = WorldGenerationContext::with_generator(
                    min,
                    height,
                    generator.min_y,
                    generator.height,
                    generator.sea_level,
                );
                let got = with_random!(kind, seed, |random| sample_list(
                    HeightProvider::decode(&raw(json)).map_err(|_| ()),
                    |p| p.sample(&mut random, context)
                ));
                check(
                    format!("height {profile} {kind} {seed} {json}"),
                    expected,
                    got,
                );
            }
            "set" => {
                let ids = (fields[2].split(' '))
                    .filter(|n| !n.is_empty())
                    .map(|n| fx.biome_id(n));
                sets.push((fields[1].to_owned(), ids.collect()));
            }
            "sort" => {
                if fields[2] == "cycle" {
                    cycles.push(fields[1].to_owned());
                } else {
                    let step: usize = fields[2].parse().unwrap();
                    let names = (fields[3].split(' '))
                        .filter(|n| !n.is_empty())
                        .map(str::to_owned);
                    let steps = sorted.entry(fields[1].to_owned()).or_default();
                    assert_eq!(steps.len(), step, "sort rows are in step order");
                    steps.push(names.collect());
                }
            }
            "place" => {
                let (positions, placed, next) = place_row(&fx, &fields);
                if !positions.is_empty() && positions != "error" && positions != "decode error" {
                    nonempty += 1;
                }
                let label = format!("place {}", fields[1..7].join(" "));
                check(
                    label,
                    &fields[7..10].join("|"),
                    [positions, placed, next].join("|"),
                );
            }
            other => panic!("unknown row {other}"),
        }
    }
    for (name, ids) in &sets {
        match registry.features_per_step(ids) {
            Err(_) => check(
                format!("sort {name}"),
                if cycles.contains(name) {
                    "cycle"
                } else {
                    "steps"
                },
                "cycle".into(),
            ),
            Ok(steps) => {
                let steps: Vec<Vec<String>> = steps
                    .iter()
                    .map(|s| {
                        (s.features().iter())
                            .map(|f| registry.placed(*f).id.clone())
                            .collect()
                    })
                    .collect();
                let java = sorted.get(name).cloned().unwrap_or_default();
                check(
                    format!("sort {name}"),
                    &format!("{java:?}"),
                    format!("{steps:?}"),
                );
            }
        }
    }
    println!(
        "placement oracle rows: {counts:?}; {nonempty} place rows with positions; {} sorter sets \
         ({} cyclic); {} placed features, {} features",
        sets.len(),
        cycles.len(),
        registry.placed_len(),
        registry.configured_len()
    );
    let mut by_subject: std::collections::BTreeMap<&str, usize> = Default::default();
    for m in &mismatches {
        let label = m.lines().next().unwrap();
        let subject = label.split(' ').next_back().unwrap();
        *by_subject.entry(subject).or_default() += 1;
    }
    eprintln!("mismatching subjects: {by_subject:#?}");
    for m in mismatches.iter().take(6) {
        eprintln!("{}", m.chars().take(700).collect::<String>());
    }
    assert!(mismatches.is_empty(), "{} mismatches", mismatches.len());
}
