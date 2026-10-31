//! Block survival, state providers and simple features against the real 26.3 server:
//! `LeafFeatureOracle.java` (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs
//! `BlockState.canSurvive`, the providers and the features on vanilla's own classes; this compares
//! each result. Worlds are given by formulas repeated here.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafFeatureOracle.java feature.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_FEATURE_ORACLE=feature.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     cargo test -p horizon-server --release features_match_java -- --ignored --nocapture
//! ```

use std::collections::HashMap;

use serde_json::value::RawValue;

use super::{Feature, FeatureWorld, PlacedFeatureId};
use crate::block::survival::{BlockSurvival, SurvivalError, SurvivalWorld};
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::position::BlockPos;
use crate::registry::{DataPack, for_each_element, load_tags, registry_dir};
use crate::server::vanilla_jar::VanillaJar;
use crate::util::random::{RandomSource, XoroshiroRandom};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::data::WorldgenData;
use crate::worldgen::placement::{DecodeContext, FLUIDS, PlacementWorld, WorldError};
use crate::worldgen::state_provider::{
    BlockStateProvider, NamedProviders, ProviderContext, ProviderError,
};

/// The oracle's `NAMES`, `CONFIGS` and `BRIGHTNESS`.
const NAMES: &str = "grass_block dirt coarse_dirt podzol mycelium rooted_dirt mud muddy_mangrove_roots \
moss_block farmland dirt_path sand red_sand gravel clay stone deepslate netherrack soul_sand soul_soil \
crimson_nylium warped_nylium nether_wart_block snow snow_block powder_snow ice packed_ice glass oak_leaves \
oak_log oak_planks water lava air cave_air void_air cactus sugar_cane kelp kelp_plant big_dripleaf \
big_dripleaf_stem small_dripleaf tall_grass short_grass oak_sapling wheat pumpkin_stem bamboo \
bamboo_sapling chorus_plant chorus_flower end_stone magma_block bubble_column hopper scaffolding piston \
sticky_piston moving_piston piston_head cave_vines cave_vines_plant twisting_vines weeping_vines vine \
pointed_dripstone dripstone_block oak_fence_gate oak_slab oak_stairs oak_trapdoor lily_pad frogspawn \
azalea flowering_azalea sculk bone_block crimson_roots crimson_fungus sea_pickle seagrass tall_seagrass \
torch lantern glowstone amethyst_block oak_door iron_bars cobweb spore_blossom hanging_roots glow_lichen \
moss_carpet brown_mushroom red_mushroom sweet_berry_bush cocoa fire soul_fire rail stone_pressure_plate \
repeater oak_sign oak_wall_sign white_banner white_carpet wall_torch ladder lever mangrove_roots \
mangrove_propagule cactus_flower leaf_litter bell candle brain_coral brain_coral_wall_fan redstone_wire \
grindstone tripwire_hook amethyst_cluster cake red_sand pink_petals";
const CONFIGS: i32 = 40;
const BRIGHTNESS: [i32; 6] = [0, 7, 8, 12, 13, 15];

fn mix(h: i32) -> i32 {
    let h = h ^ ((h as u32) >> 7) as i32;
    let h = h.wrapping_mul(0x2545F491);
    h ^ ((h as u32) >> 13) as i32
}

/// The palette index at an offset from the candidate, `None` for air (`neighbour` in the oracle).
fn neighbour(cfg: i32, size: usize, d: [i32; 3]) -> Option<usize> {
    let pick = (cfg / 4) as usize % size;
    let hash = cfg
        .wrapping_mul(1_000_003)
        .wrapping_add((d[0] + 8) * 9176)
        .wrapping_add((d[1] + 8) * 131)
        .wrapping_add((d[2] + 8) * 7);
    let hashed = mix(hash).rem_euclid(size as i32) as usize;
    let below = d == [0, -1, 0];
    let above = d == [0, 1, 0];
    match cfg % 4 {
        0 => Some(pick),
        1 => below.then_some(pick),
        2 if above => Some(pick),
        2 => below.then_some(hashed),
        _ => Some(hashed),
    }
}

struct Fixture {
    blocks: BlockStates,
    data: BlockStateData,
    survival: BlockSurvival,
    block_tags: Vec<(String, Vec<i32>)>,
    fluid_tags: Vec<(String, Vec<i32>)>,
    named: Vec<(String, Box<RawValue>)>,
    worldgen: WorldgenData,
}

impl Fixture {
    fn names(&self) -> DecodeContext<'_> {
        DecodeContext {
            blocks: &self.blocks,
            block_tags: &self.block_tags,
            biomes: &[],
            biome_tags: &[],
            fluid_tags: &self.fluid_tags,
        }
    }

    fn load() -> Self {
        let var =
            |name: &str| std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set"));
        let mut jar = VanillaJar::open(var("HORIZON_VANILLA_JAR").as_ref()).expect("the 26.3 jar");
        let pack = DataPack::vanilla(&mut jar).expect("the vanilla pack");
        let packs = std::slice::from_ref(&pack);
        let blocks = BlockStates::load(var("HORIZON_SERVER_BLOCKS_JSON").as_ref()).unwrap();
        let data =
            BlockStateData::load(var("HORIZON_SERVER_BLOCK_STATES").as_ref(), &blocks).unwrap();
        let block_tags = load_tags(packs, &registry_dir("minecraft:block"), |id| {
            blocks.block_by_name(id).map(|b| b.0 as i32)
        });
        let fluid_tags = load_tags(packs, &registry_dir("minecraft:fluid"), |id| {
            FLUIDS.iter().position(|f| *f == id).map(|i| i as i32)
        });
        let survival = BlockSurvival::new(&blocks, &data, &block_tags, &fluid_tags).unwrap();
        let mut named = Vec::new();
        for_each_element(packs, "minecraft:worldgen/block_state_provider", |file| {
            named.push((
                file.id.to_owned(),
                serde_json::from_slice(file.bytes).unwrap(),
            ));
            Ok::<_, ()>(())
        })
        .unwrap();
        let worldgen = WorldgenData::load(packs, &blocks).expect("the worldgen registries");
        Self {
            blocks,
            data,
            survival,
            block_tags,
            fluid_tags,
            named,
            worldgen,
        }
    }

    /// The palette: every state of `NAMES` (dynamic-shape ones left out), then of `own`.
    fn palette(&self, own: BlockState) -> Vec<BlockState> {
        let mut palette = Vec::new();
        let mut add = |block| {
            for state in self.blocks.states(block) {
                if !self
                    .data
                    .has(state, crate::block::flags::FLAG_DYNAMIC_SHAPE)
                {
                    palette.push(state);
                }
            }
        };
        for name in NAMES.split_whitespace() {
            add(self
                .blocks
                .block_by_name(&format!("minecraft:{name}"))
                .unwrap());
        }
        add(self.blocks.block(own));
        palette
    }
}

struct Level<'a> {
    palette: &'a [BlockState],
    cfg: i32,
    origin: [i32; 3],
}

impl SurvivalWorld for Level<'_> {
    type Error = std::convert::Infallible;
    fn block_state(&self, x: i32, y: i32, z: i32) -> Result<BlockState, Self::Error> {
        let d = [x - self.origin[0], y - self.origin[1], z - self.origin[2]];
        Ok(neighbour(self.cfg, self.palette.len(), d).map_or(BlockState::AIR, |i| self.palette[i]))
    }
    fn raw_brightness(&self, _: i32, _: i32, _: i32) -> Result<i32, Self::Error> {
        Ok(BRIGHTNESS[self.cfg as usize % BRIGHTNESS.len()])
    }
}

fn check_survival(f: &Fixture, id: u32, expected: &str) -> (usize, usize) {
    let state = BlockState(id);
    let palette = f.palette(state);
    let origin = [5, 64, 7];
    let mut yes = 0;
    let mut got = String::new();
    for cfg in 0..CONFIGS {
        let level = Level {
            palette: &palette,
            cfg,
            origin,
        };
        let answer = f
            .survival
            .can_survive(&f.blocks, &f.data, &level, state, origin);
        got.push(match answer {
            Ok(true) => '1',
            Ok(false) => '0',
            Err(SurvivalError::World(never)) => match never {},
            Err(SurvivalError::Unsupported(_)) => 'x',
        });
        yes += usize::from(got.ends_with('1'));
    }
    assert_eq!(
        got,
        expected,
        "canSurvive of {} {:?}",
        id,
        f.blocks.values(state).collect::<Vec<_>>()
    );
    (CONFIGS as usize, yes)
}

/// `SYNTH` of the oracle.
const SYNTH: &str = "air air air air air air cave_air water water water stone stone stone stone granite \
diorite andesite tuff tuff deepslate deepslate netherrack basalt blackstone dirt dirt grass_block \
grass_block sand red_sand gravel clay snow magma_block mud moss_block podzol short_grass oak_leaves";

/// The oracle's level: blocks by formula under the writes, and a log of the writes, ticks and marks.
const SOLID: &str = "netherrack netherrack basalt blackstone stone stone granite tuff deepslate dirt \
grass_block sand gravel clay";

struct TestWorld<'a> {
    f: &'a Fixture,
    /// 1: solid up to y 50, then water up to y 80 (the second and fourth seeds' world).
    mode: i32,
    synth: Vec<BlockState>,
    solid: Vec<BlockState>,
    water: BlockState,
    writes: HashMap<(i32, i32, i32), BlockState>,
    log: Vec<String>,
    marks: Vec<String>,
}

impl<'a> TestWorld<'a> {
    fn new(f: &'a Fixture, mode: i32) -> Self {
        let state = |n: &str| {
            let block = f.blocks.block_by_name(&format!("minecraft:{n}")).unwrap();
            f.blocks.default_state(block)
        };
        Self {
            f,
            mode,
            synth: SYNTH.split_whitespace().map(state).collect(),
            solid: SOLID.split_whitespace().map(state).collect(),
            water: state("water"),
            writes: HashMap::new(),
            log: Vec::new(),
            marks: Vec::new(),
        }
    }
    fn at(&self, x: i32, y: i32, z: i32) -> BlockState {
        if let Some(state) = self.writes.get(&(x, y, z)) {
            return *state;
        }
        let hash =
            mix(x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ z.wrapping_mul(83492791));
        if self.mode == 1 && (51..=80).contains(&y) {
            return self.water;
        }
        let palette = if self.mode == 1 && y <= 50 {
            &self.solid
        } else {
            &self.synth
        };
        palette[hash.rem_euclid(palette.len() as i32) as usize]
    }
}

struct Reader<'a, 'b>(&'a TestWorld<'b>);

impl SurvivalWorld for Reader<'_, '_> {
    type Error = WorldError;
    fn block_state(&self, x: i32, y: i32, z: i32) -> Result<BlockState, WorldError> {
        Ok(self.0.at(x, y, z))
    }
    fn raw_brightness(&self, _: i32, _: i32, _: i32) -> Result<i32, WorldError> {
        Ok(0)
    }
}

impl PlacementWorld for TestWorld<'_> {
    fn blocks(&self) -> &BlockStates {
        &self.f.blocks
    }
    fn state_data(&self) -> &BlockStateData {
        &self.f.data
    }
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(-64, 384, 63)
    }
    fn level_generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(-64, 384, 63)
    }
    fn height(&self, _: Heightmap, _: i32, _: i32) -> Result<i32, WorldError> {
        Err(WorldError::Unsupported("heightmaps"))
    }
    fn block_state(&self, pos: BlockPos) -> Result<BlockState, WorldError> {
        Ok(self.at(pos.x, pos.y, pos.z))
    }
    fn biome(&self, _: BlockPos) -> Result<u32, WorldError> {
        Err(WorldError::Unsupported("biomes"))
    }
    fn block_survives(&self, state: BlockState, pos: BlockPos) -> Result<bool, WorldError> {
        let at = [pos.x, pos.y, pos.z];
        (self.f.survival)
            .can_survive(&self.f.blocks, &self.f.data, &Reader(self), state, at)
            .map_err(|e| match e {
                SurvivalError::World(e) => e,
                SurvivalError::Unsupported(what) => WorldError::Unsupported(what),
            })
    }
}

impl FeatureWorld for TestWorld<'_> {
    fn set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: i32,
    ) -> Result<bool, WorldError> {
        self.writes.insert((pos.x, pos.y, pos.z), state);
        self.log.push(format!(
            "set {},{},{},{},{flags}",
            pos.x, pos.y, pos.z, state.0
        ));
        Ok(true)
    }
    fn schedule_tick(&mut self, pos: BlockPos, delay: i32) -> Result<(), WorldError> {
        self.log
            .push(format!("tick {},{},{},{delay}", pos.x, pos.y, pos.z));
        Ok(())
    }
    fn mark_postprocessing(&mut self, pos: BlockPos) -> Result<(), WorldError> {
        if (-64..320).contains(&pos.y) {
            self.marks.push(format!("{},{},{}", pos.x, pos.y, pos.z));
        }
        Ok(())
    }
}

fn origin(text: &str) -> BlockPos {
    let v: Vec<i32> = text.split(' ').map(|n| n.parse().unwrap()).collect();
    BlockPos::new(v[0], v[1], v[2])
}

fn check_provider(f: &Fixture, fields: &[&str]) {
    let json = fields[1];
    let raw: Box<RawValue> = serde_json::from_str(json).unwrap();
    let names = f.names();
    let ctx = ProviderContext {
        names: &names,
        named: NamedProviders(&f.named),
    };
    let Ok(provider) = BlockStateProvider::decode(&raw, &ctx) else {
        assert_eq!(fields[4], "decode error", "decoding {json}");
        return;
    };
    let world = TestWorld::new(f, 0);
    let mut random = XoroshiroRandom::new(fields[2].parse().unwrap());
    let pos = origin(fields[3]);
    let mut draws = String::new();
    let mut run = |draws: &mut String| -> Result<(), ProviderError> {
        for _ in 0..4 {
            match provider.optional_state(&world, &mut random, pos)? {
                Some(state) => *draws += &format!("{};", state.0),
                None => *draws += "null;",
            }
            *draws += &format!("{};", provider.state(&world, &mut random, pos)?.0);
        }
        Ok(())
    };
    if run(&mut draws).is_err() {
        assert_eq!(
            fields[4],
            format!("{draws}error"),
            "provider {json} at {pos:?}"
        );
        return;
    }
    assert_eq!(
        fields[4], draws,
        "provider {json} seed {} at {pos:?}",
        fields[2]
    );
    assert_eq!(
        fields[5],
        random.next_long().to_string(),
        "provider {json} tail"
    );
}

fn check_feature(f: &Fixture, fields: &[&str]) -> bool {
    let [_, name, seed, pos, result, log, marks, next] = fields else {
        panic!("a feature row")
    };
    let registry = f.worldgen.features();
    let id = registry.configured_id(name).unwrap();
    let feature: &Feature = (registry.configured(id).feature.as_ref()).expect("a ported feature");
    let mut world = TestWorld::new(
        f,
        i32::from(*seed == "987654321012" || *seed == "4242424242"),
    );
    let mut random = XoroshiroRandom::new(seed.parse().unwrap());
    let got = match feature.place(&mut world, &mut random, origin(pos)) {
        Ok(placed) => placed.to_string(),
        Err(_) => "error".into(),
    };
    world.marks.sort();
    let tail = random.next_long();
    assert_eq!(
        format!(
            "{got}\t{}\t{}\t{tail}",
            world.log.join(";"),
            world.marks.join(";")
        ),
        format!("{result}\t{log}\t{marks}\t{next}"),
        "feature {name} seed {seed} at {pos}"
    );
    !log.is_empty()
}

/// `a:0,1 b:1,0`: source names with their features, as indices into the oracle's five trees.
fn check_cycle(f: &Fixture, fields: &[&str]) {
    let trees = [
        "oak_checked",
        "birch_checked",
        "spruce_checked",
        "fancy_oak_checked",
        "pine_checked",
    ];
    let registry = f.worldgen.features();
    let (mut names, mut sources) = (Vec::new(), Vec::new());
    for source in fields[1].split(' ') {
        let (name, features) = source.split_once(':').unwrap();
        names.push(name);
        let ids: Vec<PlacedFeatureId> = (features.split(','))
            .map(|i| {
                let tree = trees[i.parse::<usize>().unwrap()];
                registry.placed_id(&format!("minecraft:{tree}")).unwrap()
            })
            .collect();
        sources.push(vec![ids]);
    }
    let error = super::sort::build_features_per_step(&sources).unwrap_err();
    let message = error.message(|i| names[i].to_owned());
    assert_eq!(message, fields[2], "cycle {}", fields[1]);
}

#[test]
#[ignore = "needs HORIZON_FEATURE_ORACLE (tools/leaf_server/README.md)"]
fn features_match_java() {
    let Some(path) = std::env::var_os("HORIZON_FEATURE_ORACLE") else {
        eprintln!("HORIZON_FEATURE_ORACLE is not set");
        return;
    };
    let f = Fixture::load();
    let text = std::fs::read_to_string(path).unwrap();
    let (mut survive_rows, mut survive_cases, mut survive_true) = (0, 0, 0);
    let (mut providers, mut features, mut wrote, mut cycles) = (0, 0, 0, 0);
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "survive" => {
                let (cases, yes) = check_survival(&f, fields[1].parse().unwrap(), fields[2]);
                survive_rows += 1;
                survive_cases += cases;
                survive_true += yes;
            }
            "cycle" => {
                check_cycle(&f, &fields);
                cycles += 1;
            }
            "provider" => {
                check_provider(&f, &fields);
                providers += 1;
            }
            "feature" => {
                wrote += usize::from(check_feature(&f, &fields));
                features += 1;
            }
            other => panic!("unknown row {other}"),
        }
    }
    eprintln!(
        "features_match_java: {survive_rows} states, {survive_cases} canSurvive cases ({survive_true} true), {cycles} sorter cycles, {providers} provider rows, {features} feature rows ({wrote} wrote blocks), all equal"
    );
}
