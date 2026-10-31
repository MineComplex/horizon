//! Structure templates and processors against the real 26.3 server: `LeafTemplateOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) decodes vanilla templates and places
//! them with vanilla's classes into a synthetic world; this repeats each case and compares every
//! `setBlock`, tick, block entity seed, entity position and the random's next draw.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafTemplateOracle.java template.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_TEMPLATE_ORACLE=template.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     cargo test -p horizon-server --release templates_match_java -- --ignored --nocapture
//! ```

use std::borrow::Cow;
use std::collections::HashMap;

use serde_json::value::RawValue;

use super::place::{
    LiquidSettings, PlaceContext, PlaceSettings, filter_blocks, jigsaws, place_entities,
    place_in_world,
};
use super::processor::ProcessorLists;
use super::template::{JigsawInfo, JointType, StructureTemplate, StructureTemplates};
use crate::block::flags::FLAG_RANDOMIZABLE_CONTAINER;
use crate::block::transform::{Mirror, Rotation};
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::nbt::{JavaString, Tag};
use crate::registry::{DataPack, load_tags, registry_dir};
use crate::server::vanilla_jar::VanillaJar;
use crate::util::random::{LegacyRandom, RandomSource, XoroshiroRandom};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::density::StructureBounds;
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::placement::{DecodeContext, PlacementWorld, WorldError};

/// The oracle's `BUILTIN` lists are in its rows (the spec column), so only the formulas are here.
const DEEP: [&str; 7] = [
    "stone",
    "deepslate",
    "lava",
    "gravel",
    "stone",
    "water",
    "andesite",
];
const TOP: [&str; 4] = ["dirt", "grass_block", "sand", "dirt"];

fn mix(h: i32) -> i32 {
    let h = h ^ ((h as u32) >> 7) as i32;
    let h = h.wrapping_mul(0x2545F491);
    h ^ ((h as u32) >> 13) as i32
}

pub(super) fn ground(x: i32, z: i32) -> i32 {
    58 + mix(x.wrapping_mul(31) ^ z.wrapping_mul(17) ^ 0x5bd1).rem_euclid(8)
}

pub(super) struct Fixture {
    pub(super) blocks: BlockStates,
    pub(super) data: BlockStateData,
    pub(super) block_tags: Vec<(String, Vec<i32>)>,
    pub(super) lists: ProcessorLists,
    pub(super) templates: StructureTemplates,
    pub(super) deep: Vec<BlockState>,
    pub(super) top: Vec<BlockState>,
    pub(super) water: BlockState,
    pub(super) air: BlockState,
}

impl Fixture {
    pub(super) fn load() -> Self {
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
        let lists = ProcessorLists::load(packs, &blocks).expect("the processor lists");
        let started = std::time::Instant::now();
        let templates = StructureTemplates::load(&mut jar, None, &blocks, &data).unwrap();
        eprintln!(
            "{} templates in {:?}, {} failures",
            templates.len(),
            started.elapsed(),
            templates.failures.len()
        );
        let state = |name: &str| {
            blocks.default_state(blocks.block_by_name(&format!("minecraft:{name}")).unwrap())
        };
        Self {
            deep: DEEP.iter().map(|n| state(n)).collect(),
            top: TOP.iter().map(|n| state(n)).collect(),
            water: state("water"),
            air: state("air"),
            blocks,
            data,
            block_tags,
            lists,
            templates,
        }
    }

    fn names(&self) -> DecodeContext<'_> {
        DecodeContext {
            blocks: &self.blocks,
            block_tags: &self.block_tags,
            biomes: &[],
            biome_tags: &[],
            fluid_tags: &[],
        }
    }

    fn synth(&self, x: i32, y: i32, z: i32) -> BlockState {
        let g = ground(x, z);
        let hash =
            mix(x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ z.wrapping_mul(83492791));
        if y < g - 3 {
            self.deep[hash.rem_euclid(self.deep.len() as i32) as usize]
        } else if y <= g {
            self.top[hash.rem_euclid(self.top.len() as i32) as usize]
        } else if y <= 62 {
            self.water
        } else {
            self.air
        }
    }
}

pub(super) struct World<'a> {
    pub(super) f: &'a Fixture,
    pub(super) writes: HashMap<BlockPos, BlockState>,
    pub(super) sets: Vec<String>,
    pub(super) ticks: Vec<String>,
    /// `level.getRandom()`, for the oracles that give one.
    pub(super) level_random: Option<XoroshiroRandom>,
}

impl PlacementWorld for World<'_> {
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
    fn height(&self, kind: Heightmap, x: i32, z: i32) -> Result<i32, WorldError> {
        let g = ground(x, z);
        let ocean = matches!(kind, Heightmap::OceanFloor | Heightmap::OceanFloorWg);
        Ok(if ocean { g } else { g.max(62) } + 1)
    }
    fn block_state(&self, pos: BlockPos) -> Result<BlockState, WorldError> {
        Ok(self
            .writes
            .get(&pos)
            .copied()
            .unwrap_or_else(|| self.f.synth(pos.x, pos.y, pos.z)))
    }
    fn biome(&self, _: BlockPos) -> Result<u32, WorldError> {
        Err(WorldError::Unsupported("biomes"))
    }
    fn block_survives(&self, _: BlockState, _: BlockPos) -> Result<bool, WorldError> {
        Err(WorldError::Unsupported("survival"))
    }
}

impl FeatureWorld for World<'_> {
    fn has_level_random(&self) -> bool {
        self.level_random.is_some()
    }
    fn level_boolean(&mut self) -> Result<bool, WorldError> {
        let random = self.level_random.as_mut();
        random
            .map(|r| r.next_boolean())
            .ok_or(WorldError::Unsupported("level random"))
    }
    fn level_float(&mut self) -> Result<f32, WorldError> {
        let random = self.level_random.as_mut();
        random
            .map(|r| r.next_float())
            .ok_or(WorldError::Unsupported("level random"))
    }
    fn level_int_bound(&mut self, bound: i32) -> Result<i32, WorldError> {
        let random = self.level_random.as_mut();
        let random = random.ok_or(WorldError::Unsupported("level random"))?;
        random
            .next_int_bound(bound)
            .map_err(|_| WorldError::Unsupported("a positive bound"))
    }
    fn set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: i32,
    ) -> Result<bool, WorldError> {
        self.writes.insert(pos, state);
        self.sets
            .push(format!("{},{},{},{},{flags}", pos.x, pos.y, pos.z, state.0));
        Ok(true)
    }
    fn schedule_tick(&mut self, pos: BlockPos, delay: i32) -> Result<(), WorldError> {
        let state = self.block_state(pos)?;
        let name = self
            .f
            .blocks
            .block_name(self.f.blocks.block(state))
            .to_owned();
        self.ticks
            .push(format!("{},{},{},{name},{delay}", pos.x, pos.y, pos.z));
        Ok(())
    }
    fn schedule_fluid_tick(
        &mut self,
        pos: BlockPos,
        fluid: u8,
        delay: i32,
    ) -> Result<(), WorldError> {
        let name = self.f.data.fluid_name(fluid).to_owned();
        self.ticks
            .push(format!("{},{},{},{name},{delay}", pos.x, pos.y, pos.z));
        Ok(())
    }
    fn mark_postprocessing(&mut self, _: BlockPos) -> Result<(), WorldError> {
        Ok(())
    }
    fn ensure_can_write(&self, _: BlockPos) -> Result<bool, WorldError> {
        Ok(true)
    }
    fn section_block(&self, pos: BlockPos) -> Result<Option<BlockState>, WorldError> {
        self.block_state(pos).map(Some)
    }
    fn set_section_block(&mut self, pos: BlockPos, state: BlockState) -> Result<(), WorldError> {
        self.writes.insert(pos, state);
        Ok(())
    }
    fn update_shape_at_face(
        &mut self,
        _: BlockPos,
        _: Direction,
        _: i32,
    ) -> Result<(), WorldError> {
        Ok(())
    }
    fn store_beehive_occupants(&mut self, _: BlockPos, _: &[i32]) -> Result<(), WorldError> {
        Ok(())
    }
}

fn ints(text: &str) -> Vec<i32> {
    text.split(',').map(|v| v.parse().unwrap()).collect()
}

fn pos(text: &str) -> BlockPos {
    let v = ints(text);
    BlockPos::new(v[0], v[1], v[2])
}

fn show(p: BlockPos) -> String {
    format!("{},{},{}", p.x, p.y, p.z)
}

fn show_box(b: StructureBounds) -> String {
    let (a, z) = (b.minimum(), b.maximum());
    format!("{},{},{},{},{},{}", a[0], a[1], a[2], z[0], z[1], z[2])
}

fn show_jigsaws(list: &[JigsawInfo]) -> String {
    list.iter()
        .map(|j| {
            let joint = match j.joint {
                JointType::Rollable => "rollable",
                JointType::Aligned => "aligned",
            };
            format!(
                "{},{},{joint},{},{},{},{},{};",
                show(j.pos),
                j.state.0,
                j.name,
                j.pool,
                j.target,
                j.placement_priority,
                j.selection_priority
            )
        })
        .collect()
}

fn blocks_hash(template: &StructureTemplate, palette: usize) -> i64 {
    let mut h = 1i64;
    for b in template.palettes()[palette].blocks() {
        for v in [
            b.pos.x,
            b.pos.y,
            b.pos.z,
            b.state.0 as i32,
            i32::from(b.nbt.is_some()),
        ] {
            h = h.wrapping_mul(31).wrapping_add(v.into());
        }
    }
    h
}

fn bits(v: f64) -> String {
    format!("{:x}", v.to_bits())
}

/// The first difference of two `;` lists, for the failure message.
fn first_difference(java: &str, rust: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (java.split(';').collect(), rust.split(';').collect());
    let i = a
        .iter()
        .zip(&b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    format!(
        "entry {i} of {}/{}: java {:?} rust {:?}",
        a.len(),
        b.len(),
        a.get(i),
        b.get(i)
    )
}

fn check_template(f: &Fixture, fields: &[&str]) -> Vec<String> {
    let Some(template) = f.templates.get(fields[1]) else {
        return vec![format!("{}: not loaded", fields[1])];
    };
    let size = template.size();
    let palettes: String = (0..template.palettes().len())
        .map(|p| {
            format!(
                "{}:{};",
                template.palettes()[p].blocks().len(),
                blocks_hash(template, p)
            )
        })
        .collect();
    let entities: String = template
        .entities()
        .iter()
        .map(|e| {
            format!(
                "{},{},{},{};",
                show(e.block_pos),
                bits(e.pos[0]),
                bits(e.pos[1]),
                bits(e.pos[2])
            )
        })
        .collect();
    let rust = [
        format!("{},{},{}", size[0], size[1], size[2]),
        palettes,
        show_jigsaws(template.palettes()[0].jigsaws()),
        entities,
    ];
    let mut out = Vec::new();
    for (i, (java, rust)) in fields[2..].iter().zip(&rust).enumerate() {
        if java != rust {
            out.push(format!(
                "{} template field {i}: {}",
                fields[1],
                first_difference(java, rust)
            ));
        }
    }
    out
}

fn rotation(name: &str) -> Rotation {
    match name {
        "NONE" => Rotation::None,
        "CLOCKWISE_90" => Rotation::Clockwise90,
        "CLOCKWISE_180" => Rotation::Clockwise180,
        _ => Rotation::Counterclockwise90,
    }
}

fn mirror(name: &str) -> Mirror {
    match name {
        "NONE" => Mirror::None,
        "LEFT_RIGHT" => Mirror::LeftRight,
        _ => Mirror::FrontBack,
    }
}

/// One case; the differing output columns.
fn check_case(f: &Fixture, fields: &[&str]) -> Vec<String> {
    let id = fields[1];
    let template = f.templates.get(id).expect("a template row came first");
    let (rotation, mirror) = (rotation(fields[2]), mirror(fields[3]));
    let position = pos(fields[4]);
    let pivot = pos(fields[5]);
    let raw: Box<RawValue> = serde_json::from_str(fields[6]).unwrap();
    let holder = f.lists.holder(&raw, &f.names());
    let shared = fields[7] == "true";
    let waterlog = fields[8] == "true";
    let bounding_box = (fields[9] != "-").then(|| {
        let v = ints(fields[9]);
        StructureBounds::from_corners([v[0], v[1], v[2]], [v[3], v[4], v[5]])
    });
    let seed: i64 = fields[10].parse().unwrap();
    let level_seed: i64 = fields[11].parse().unwrap();
    let java = &fields[12..];
    if java[0].starts_with("error") {
        return match holder {
            Err(_) => Vec::new(),
            Ok(_) => vec![format!("{id} {}: java {}", fields[6], java[0])],
        };
    }
    let holder = match holder {
        Ok(holder) => holder,
        Err(e) => return vec![format!("{id} {}: rust decode error {e}", fields[6])],
    };
    let list = f.lists.resolve(&holder);
    let mut settings = PlaceSettings {
        mirror,
        rotation,
        rotation_pivot: pivot,
        ignore_entities: true,
        bounding_box,
        liquid: if waterlog {
            LiquidSettings::ApplyWaterlogging
        } else {
            LiquidSettings::IgnoreWaterlogging
        },
        processors: list.0.iter().map(Cow::Borrowed).collect(),
        known_shape: true,
        ..Default::default()
    };
    let mut random = LegacyRandom::new(seed);
    let structure_block = f.blocks.block_by_name("minecraft:structure_block").unwrap();
    let structure_blocks: String = filter_blocks(
        template,
        &f.data,
        &f.blocks,
        position,
        &settings,
        structure_block,
        &mut random,
    )
    .unwrap()
    .iter()
    .map(|b| format!("{},{};", show(b.pos), b.state.0))
    .collect();
    settings.shared_random = shared;
    let mut world = World {
        f,
        writes: HashMap::new(),
        sets: Vec::new(),
        ticks: Vec::new(),
        level_random: None,
    };
    let cx = PlaceContext {
        known: f.lists.known(),
        seed: level_seed,
    };
    let placed = place_in_world(
        &mut world,
        cx,
        template,
        position,
        position,
        &settings,
        &mut random,
        2,
    );
    let placed = match placed {
        Ok(placed) => placed,
        Err(e) => return vec![format!("{id} {}: rust error {e}", fields[6])],
    };
    // The oracle's block entity map: one entry per position, in creation order, last load wins.
    let mut order: Vec<BlockPos> = Vec::new();
    let mut seeds: HashMap<BlockPos, String> = HashMap::new();
    for (p, nbt) in placed.iter().flat_map(|p| &p.block_entities) {
        let state = world.block_state(*p).unwrap();
        // What the block entity saves back: containers and brushable blocks keep a loot table
        // and a non-zero seed.
        let name = f.blocks.block_name(f.blocks.block(state));
        let randomizable = f.data.flags(state) & FLAG_RANDOMIZABLE_CONTAINER != 0
            || matches!(
                name,
                "minecraft:suspicious_sand" | "minecraft:suspicious_gravel"
            );
        let table = matches!(
            nbt.get(&JavaString::from("LootTable")),
            Some(Tag::String(_))
        );
        let seed = match nbt.get(&JavaString::from("LootTableSeed")) {
            Some(Tag::Long(seed)) if randomizable && table && *seed != 0 => seed.to_string(),
            _ => "-".to_owned(),
        };
        if !order.contains(p) {
            order.push(*p);
        }
        seeds.insert(*p, seed);
    }
    let bes: String = order
        .iter()
        .map(|p| format!("{},{};", show(*p), seeds[p]))
        .collect();
    let mut entity_settings = settings.clone();
    entity_settings.ignore_entities = false;
    let entities: String = place_entities(template, position, &entity_settings)
        .iter()
        .map(|e| {
            format!(
                "{},{},{},{};",
                show(e.block_pos),
                bits(e.pos[0]),
                bits(e.pos[1]),
                bits(e.pos[2])
            )
        })
        .collect();
    let rust = [
        show_box(template.bounding_box(position, rotation, pivot, mirror)),
        show(template.zero_position_with_transform(position, mirror, rotation)),
        structure_blocks,
        show_jigsaws(&jigsaws(template, &f.data, position, rotation)),
        placed.is_some().to_string(),
        world.sets.join(";"),
        world.ticks.join(";"),
        bes,
        entities,
        random.next_long().to_string(),
    ];
    const NAMES: [&str; 10] = [
        "bounding box",
        "zero position",
        "structure blocks",
        "jigsaws",
        "placed",
        "setBlock calls",
        "ticks",
        "block entities",
        "entities",
        "next long",
    ];
    let mut out = Vec::new();
    for (i, (java, rust)) in java.iter().zip(&rust).enumerate() {
        if java != rust {
            out.push(format!(
                "{id} {} {} {}: {} {}",
                fields[2],
                fields[3],
                fields[6],
                NAMES[i],
                first_difference(java, rust)
            ));
        }
    }
    out
}

#[test]
#[ignore = "needs HORIZON_TEMPLATE_ORACLE and the 26.3 data (tools/leaf_server/setup_reference.sh)"]
fn templates_match_java() {
    let Some(path) = std::env::var_os("HORIZON_TEMPLATE_ORACLE") else {
        eprintln!("HORIZON_TEMPLATE_ORACLE is not set");
        return;
    };
    let f = Fixture::load();
    let text = std::fs::read_to_string(path).unwrap();
    let (mut templates, mut cases, mut placed, mut writes) = (0, 0, 0, 0usize);
    let mut failures = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "template" => {
                templates += 1;
                failures.extend(check_template(&f, &fields));
            }
            "case" => {
                cases += 1;
                if fields.get(16) == Some(&"true") {
                    placed += 1;
                    writes += fields[17].split(';').filter(|s| !s.is_empty()).count();
                }
                failures.extend(check_case(&f, &fields));
            }
            _ => panic!("unknown row {line}"),
        }
    }
    eprintln!(
        "{templates} templates, {cases} cases ({placed} placed, {writes} setBlock calls), {} differences",
        failures.len()
    );
    for failure in failures.iter().take(40) {
        eprintln!("  {failure}");
    }
    assert!(failures.is_empty());
}
