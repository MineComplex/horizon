//! The piece-based structures against the real 26.3 server: `LeafLegacyStructuresOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs `Structure.generate` for the desert
//! pyramid, jungle temple, swamp hut, buried treasure, mineshafts, stronghold, nether fortress and ocean
//! monument over chunks of two seeds and places the first start of each, chunk after chunk, into the
//! template oracle's synthetic world; this repeats each case through the level generator's
//! STRUCTURE_STARTS path ([`generate_with_worker`]) and compares the start (or none), every piece's
//! saved NBT, the start's NBT and every placement's `setBlock` calls, block entities (loot tables and
//! seeds, spawner entities), entities and the random draws that follow.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafLegacyStructuresOracle.java legacy.txt
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_LEGACY_STRUCTURES_ORACLE=legacy.txt \
//!     HORIZON_SERVER_BLOCKS_JSON=<blocks.json> HORIZON_SERVER_BLOCK_STATES=<block_states.bin> \
//!     HORIZON_BIOME_REPORTS=<reports dir> \
//!     cargo test -p horizon-server --release legacy_structures_match_java -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::jigsaw_oracle_tests::{canonical_string, first_difference};
use super::legacy_piece::{PieceLevel, PieceOutput, place_pieces};
use super::piece_place::{reference_of, writable_area};
use super::start::StructureStart;
use super::template::StructureTemplates;
use super::template_oracle_tests::{Fixture, World};
use crate::block::flags::FLAG_HAS_BLOCK_ENTITY;
use crate::block::transform::Mirror;
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::geometry::Direction;
use crate::core::position::{BlockPos, ChunkPos};
use crate::nbt::{Compound, JavaString, Tag};
use crate::registry::{DataPack, Registries};
use crate::server::vanilla_jar::VanillaJar;
use crate::util::random::{LegacyRandom, RandomSource, WorldgenRandom, XoroshiroRandom};
use crate::world::chunk::ContainerFactory;
use crate::world::chunk::serial::Heightmap;
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::biome::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use crate::worldgen::codec::{object, required};
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::generator::structures::generate_with_worker;
use crate::worldgen::generator::{GeneratorTables, LevelGenerator, LevelGeneratorSettings};
use crate::worldgen::placement::{PlacementWorld, WorldError};
use crate::worldgen::terrain::oracle_tests::{env_path, load_real};

/// The template oracle's synthetic world with the level's own random (`WorldGenLevel.getRandom`).
struct LegacyWorld<'a> {
    inner: World<'a>,
    level_random: LegacyRandom,
}

impl PlacementWorld for LegacyWorld<'_> {
    fn blocks(&self) -> &BlockStates {
        self.inner.blocks()
    }
    fn state_data(&self) -> &BlockStateData {
        self.inner.state_data()
    }
    fn generation(&self) -> WorldGenerationContext {
        self.inner.generation()
    }
    fn level_generation(&self) -> WorldGenerationContext {
        self.inner.level_generation()
    }
    fn height(&self, kind: Heightmap, x: i32, z: i32) -> Result<i32, WorldError> {
        self.inner.height(kind, x, z)
    }
    fn block_state(&self, pos: BlockPos) -> Result<BlockState, WorldError> {
        self.inner.block_state(pos)
    }
    fn biome(&self, _: BlockPos) -> Result<u32, WorldError> {
        // The oracle's `getBiome` is plains, which blocks nothing.
        Ok(0)
    }
    fn block_survives(&self, state: BlockState, pos: BlockPos) -> Result<bool, WorldError> {
        self.inner.block_survives(state, pos)
    }
}

impl FeatureWorld for LegacyWorld<'_> {
    fn set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: i32,
    ) -> Result<bool, WorldError> {
        self.inner.set_block(pos, state, flags)
    }
    fn schedule_tick(&mut self, pos: BlockPos, delay: i32) -> Result<(), WorldError> {
        self.inner.schedule_tick(pos, delay)
    }
    fn schedule_fluid_tick(
        &mut self,
        pos: BlockPos,
        fluid: u8,
        delay: i32,
    ) -> Result<(), WorldError> {
        self.inner.schedule_fluid_tick(pos, fluid, delay)
    }
    fn mark_postprocessing(&mut self, pos: BlockPos) -> Result<(), WorldError> {
        self.inner.mark_postprocessing(pos)
    }
    fn ensure_can_write(&self, pos: BlockPos) -> Result<bool, WorldError> {
        self.inner.ensure_can_write(pos)
    }
    fn section_block(&self, pos: BlockPos) -> Result<Option<BlockState>, WorldError> {
        self.inner.section_block(pos)
    }
    fn set_section_block(&mut self, pos: BlockPos, state: BlockState) -> Result<(), WorldError> {
        self.inner.set_section_block(pos, state)
    }
    fn update_shape_at_face(
        &mut self,
        pos: BlockPos,
        direction: Direction,
        flags: i32,
    ) -> Result<(), WorldError> {
        self.inner.update_shape_at_face(pos, direction, flags)
    }
    fn store_beehive_occupants(&mut self, pos: BlockPos, ticks: &[i32]) -> Result<(), WorldError> {
        self.inner.store_beehive_occupants(pos, ticks)
    }
    fn level_boolean(&mut self) -> Result<bool, WorldError> {
        Ok(self.level_random.next_boolean())
    }
    fn level_float(&mut self) -> Result<f32, WorldError> {
        Ok(self.level_random.next_float())
    }
}

fn text(tag: &Compound, key: &str) -> String {
    match tag.get(&JavaString::from(key)) {
        Some(Tag::String(s)) => s.to_string_lossy(),
        _ => String::new(),
    }
}

/// The block entities as the oracle prints them: the last write per position, kept when the block
/// there still has a block entity.
fn block_entities(output: &PieceOutput, world: &LegacyWorld<'_>) -> String {
    let mut last: BTreeMap<String, String> = BTreeMap::new();
    let mut keyed: HashMap<BlockPos, &Compound> = HashMap::new();
    for (pos, nbt) in &output.block_entities {
        keyed.insert(*pos, nbt);
    }
    for (pos, nbt) in keyed {
        let state = world.inner.block_state(pos).unwrap();
        if world.inner.f.data.flags(state) & FLAG_HAS_BLOCK_ENTITY == 0 {
            continue;
        }
        let seed = match nbt.get(&JavaString::from("LootTableSeed")) {
            Some(Tag::Long(seed)) => *seed,
            _ => 0,
        };
        let spawner = match nbt.get(&JavaString::from("SpawnData")) {
            Some(Tag::Compound(data)) => match data.get(&JavaString::from("entity")) {
                Some(Tag::Compound(entity)) => text(entity, "id"),
                _ => String::new(),
            },
            _ => String::new(),
        };
        last.insert(
            format!("{},{},{}", pos.x, pos.y, pos.z),
            format!(
                "{},{},{seed},{spawner}",
                text(nbt, "id"),
                text(nbt, "LootTable")
            ),
        );
    }
    last.iter().map(|(p, v)| format!("{p},{v};")).collect()
}

/// The entities as the oracle prints them (it cannot make a witch, a cat or an elder guardian without a server).
fn entities(output: &PieceOutput) -> String {
    let mut out = String::new();
    for nbt in &output.entities {
        let id = text(nbt, "id");
        if matches!(
            id.as_str(),
            "minecraft:witch" | "minecraft:cat" | "minecraft:elder_guardian"
        ) {
            continue;
        }
        let Some(Tag::List(pos)) = nbt.get(&JavaString::from("Pos")) else {
            continue;
        };
        let at: Vec<f64> = pos
            .iter()
            .map(|t| match t {
                Tag::Double(bits) => f64::from_bits(*bits),
                _ => 0.0,
            })
            .collect();
        out.push_str(&format!("{id},{:?},{:?},{:?};", at[0], at[1], at[2]));
    }
    out
}

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_LEGACY_STRUCTURES_ORACLE (tools/leaf_server/README.md)"]
fn legacy_structures_match_java() {
    let oracle =
        env_path("HORIZON_LEGACY_STRUCTURES_ORACLE").expect("HORIZON_LEGACY_STRUCTURES_ORACLE");
    let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
    let reports = env_path("HORIZON_BIOME_REPORTS").expect("HORIZON_BIOME_REPORTS");
    let mut jar = VanillaJar::open(env_path("HORIZON_VANILLA_JAR").unwrap().as_ref()).unwrap();
    let pack = DataPack::vanilla(&mut jar).unwrap();
    let registries = Registries::build(Vec::new(), &[pack, DataPack::paper()]).unwrap();
    let biome_registry = registries.synced("minecraft:worldgen/biome").unwrap();
    let biomes = BiomeIds::new(biome_registry);
    let biome_count = biome_registry.entries.len() as u32;
    let plains = biomes.id("minecraft:plains").unwrap();
    let presets = BiomePresets::load(reports.as_ref(), &real.data, &biomes).unwrap();
    let normal = (real
        .data
        .raw("minecraft:worldgen/world_preset")
        .unwrap()
        .elements
        .iter())
    .find(|(id, _)| id == "minecraft:normal")
    .map(|(_, raw)| raw.to_owned())
    .unwrap();
    let dimensions = object(required(&object(&normal).unwrap(), "dimensions").unwrap()).unwrap();
    let crate::worldgen::terrain::oracle_tests::Real {
        mut data,
        blocks,
        state_data,
    } = real;
    let templates = StructureTemplates::load(&mut jar, None, &blocks, &state_data).unwrap();
    data.set_templates(templates);
    let state_count = blocks.state_count();
    // Only the WG surface is read (`getFirstFreeHeight` with `WORLD_SURFACE_WG`).
    let heightmaps = Arc::new(HeightmapTable::new(state_count, |s| {
        if state_data.is_air(s) { 0 } else { 0b11 }
    }));
    let (data, blocks, flags) = (Arc::new(data), Arc::new(blocks), Arc::new(state_data));
    let registry = data.structures();
    let fixture = Fixture::load();
    let survival =
        Arc::new(crate::block::survival::BlockSurvival::new(&blocks, &flags, &[], &[]).unwrap());
    let mut generators: HashMap<(bool, i64), Arc<LevelGenerator>> = HashMap::new();
    let mut generator_for = |nether: bool, seed: i64| {
        generators
            .entry((nether, seed))
            .or_insert_with(|| {
                let stem_key = if nether {
                    "minecraft:the_nether"
                } else {
                    "minecraft:overworld"
                };
                let raw = required(&object(&dimensions[stem_key]).unwrap(), "generator")
                    .unwrap()
                    .to_owned();
                let stem = decode_noise_generator(&raw, &biomes, &presets).unwrap();
                let (min_y, height) = if nether { (0, 256) } else { (-64, 384) };
                Arc::new(
                    LevelGenerator::new(
                        GeneratorTables {
                            data: data.clone(),
                            blocks: blocks.clone(),
                            flags: flags.clone(),
                            heightmaps: heightmaps.clone(),
                            survival: survival.clone(),
                        },
                        LevelGeneratorSettings {
                            stem,
                            seed,
                            generate_structures: true,
                            flat_bedrock: false,
                            end_void_rings: false,
                            spigot_seeds: Default::default(),
                            min_section_y: min_y >> 4,
                            section_count: (height / 16) as usize,
                            factory: ContainerFactory::new(state_count, biome_count, plains),
                        },
                    )
                    .expect("the generator"),
                )
            })
            .clone()
    };
    let text_file = std::fs::read_to_string(oracle).expect("the oracle file");
    let mut lines = text_file.lines();
    let (mut cases, mut valid, mut pieces_seen, mut placements, mut sets_seen) =
        (0, 0, 0, 0, 0usize);
    let mut per_structure: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut starts: HashMap<String, StructureStart> = HashMap::new();
    let started = std::time::Instant::now();
    // `HORIZON_LEGACY_ONLY=desert_pyramid,monument` keeps those structures' rows (for work in progress).
    let only: Option<Vec<String>> = std::env::var("HORIZON_LEGACY_ONLY")
        .ok()
        .map(|v| v.split(',').map(|n| format!("minecraft:{n}")).collect());
    let wanted = |name: &str| {
        only.as_ref()
            .is_none_or(|only| only.iter().any(|n| n == name))
    };
    while let Some(line) = lines.next() {
        let f: Vec<&str> = line.split('\t').collect();
        if !wanted(f[1]) {
            if f[0] == "case" {
                // Its piece rows and the start's tag.
                for _ in 0..f[5].parse::<usize>().unwrap() + usize::from(f[5] != "0") {
                    lines.next();
                }
            }
            continue;
        }
        match f[0] {
            "case" => {
                cases += 1;
                let (name, seed) = (f[1], f[2].parse::<i64>().unwrap());
                let chunk = ChunkPos::new(f[3].parse().unwrap(), f[4].parse().unwrap());
                let want: usize = f[5].parse().unwrap();
                let nether = name == "minecraft:fortress";
                let generator = generator_for(nether, seed);
                let mut worker = generator.worker().unwrap();
                worker.reset_search();
                let structure = registry.structure_id(name).unwrap();
                let heights = if nether { (0, 256) } else { (-64, 384) };
                let start = generate_with_worker(&mut worker, structure, chunk, heights, 0);
                let label = format!("{name} {seed} {chunk:?}");
                let got = start.as_ref().map_or(0, |s| s.children.len());
                if got != want {
                    failures.push(format!("{label}: {got} pieces, not {want}"));
                }
                if want == 0 {
                    continue;
                }
                valid += 1;
                per_structure.entry(name.to_owned()).or_default().0 += 1;
                let mut first = true;
                for i in 0..want {
                    let row: Vec<&str> = lines.next().unwrap().split('\t').collect();
                    assert_eq!(row[0], "piece");
                    pieces_seen += 1;
                    per_structure.entry(name.to_owned()).or_default().1 += 1;
                    let Some(piece) = start.as_ref().and_then(|s| s.children.get(i)) else {
                        continue;
                    };
                    let mine = canonical_string(&Tag::Compound(piece.create_tag(registry)));
                    if first && mine != row[1] {
                        failures.push(format!(
                            "{label}: piece {i} differs at {}",
                            first_difference(row[1], &mine)
                        ));
                        first = false;
                    }
                }
                let row: Vec<&str> = lines.next().unwrap().split('\t').collect();
                assert_eq!(row[0], "nbt");
                if let Some(start) = start {
                    let saved = start.create_tag(registry, chunk);
                    let mine = canonical_string(&Tag::Compound(saved.clone()));
                    if mine != row[1] {
                        failures.push(format!(
                            "{label}: start nbt differs at {}",
                            first_difference(row[1], &mine)
                        ));
                    }
                    // Loading the tag back and saving it again gives the same tag.
                    match StructureStart::load(registry, &saved, seed) {
                        Ok(Some(loaded)) => {
                            // A monument's rooms are not saved: loading makes them again
                            // (`regeneratePiecesAfterLoad`), equal to the generated ones.
                            if name == "minecraft:monument" && loaded.children != start.children {
                                failures
                                    .push(format!("{label}: the loaded monument's rooms differ"));
                            }
                            let again = canonical_string(&Tag::Compound(
                                loaded.create_tag(registry, chunk),
                            ));
                            if again != mine {
                                failures.push(format!(
                                    "{label}: the saved start loads back differently at {}",
                                    first_difference(&mine, &again)
                                ));
                            }
                        }
                        other => failures.push(format!("{label}: the saved start loads {other:?}")),
                    }
                    starts.insert(format!("{name}\t{seed}\t{}\t{}", f[3], f[4]), start);
                }
            }
            "place" => {
                placements += 1;
                let seed: i64 = f[2].parse().unwrap();
                let label = format!(
                    "place {} {seed} {},{} in {},{}",
                    f[1], f[3], f[4], f[5], f[6]
                );
                let Some(start) =
                    starts.get_mut(&format!("{}\t{}\t{}\t{}", f[1], f[2], f[3], f[4]))
                else {
                    failures.push(format!("{label}: no start"));
                    continue;
                };
                let target = ChunkPos::new(f[5].parse().unwrap(), f[6].parse().unwrap());
                let nether = f[1] == "minecraft:fortress";
                let (min_y, height) = if nether { (0, 256) } else { (-64, 384) };
                let chunk_box = writable_area(target, min_y, height);
                let mut world = LegacyWorld {
                    inner: World {
                        f: &fixture,
                        writes: HashMap::new(),
                        sets: Vec::new(),
                        ticks: Vec::new(),
                        level_random: None,
                    },
                    level_random: LegacyRandom::new(seed ^ target.pack()),
                };
                let mut random = WorldgenRandom::new(XoroshiroRandom::new(seed ^ target.pack()));
                let reference = reference_of(start).unwrap();
                let structure = start.structure.unwrap();
                let kind = registry.structure(structure).kind.clone();
                let mut output = PieceOutput::default();
                if let Err(e) = place_pieces(
                    &mut start.children,
                    &kind,
                    &mut world,
                    &mut random,
                    chunk_box,
                    reference,
                    PieceLevel {
                        seed,
                        mineshaft_blocking: &registry.mineshaft_blocking,
                    },
                    &mut output,
                ) {
                    failures.push(format!("{label}: rust error {e}"));
                }
                let sets = world.inner.sets.join(";");
                sets_seen += world.inner.sets.len();
                if sets != f[7] {
                    failures.push(format!(
                        "{label}: {} setBlock calls, java {}; first difference at {}",
                        world.inner.sets.len(),
                        f[7].split(';').filter(|s| !s.is_empty()).count(),
                        first_difference(f[7], &sets)
                    ));
                }
                let mine = block_entities(&output, &world);
                if mine != f[8] {
                    failures.push(format!(
                        "{label}: block entities differ at {}",
                        first_difference(f[8], &mine)
                    ));
                }
                let mine = entities(&output);
                if mine != f[9] {
                    failures.push(format!("{label}: entities {mine} not {}", f[9]));
                }
                let next = random.next_long().to_string();
                if next != f[10] {
                    failures.push(format!("{label}: next long {next} not {}", f[10]));
                }
                let next = world.level_random.next_long().to_string();
                if next != f[11] {
                    failures.push(format!(
                        "{label}: level random next long {next} not {}",
                        f[11]
                    ));
                }
                let _ = Mirror::None;
            }
            other => panic!("unknown row {other}"),
        }
    }
    println!(
        "{cases} cases ({valid} starts, {pieces_seen} pieces), {placements} placements \
         ({sets_seen} setBlock calls) in {:?}; {} failures",
        started.elapsed(),
        failures.len()
    );
    for (name, (starts, pieces)) in &per_structure {
        println!("  {name}: {starts} starts, {pieces} pieces");
    }
    for failure in failures.iter().take(30) {
        println!("  {failure}");
    }
    assert!(failures.is_empty());
}
