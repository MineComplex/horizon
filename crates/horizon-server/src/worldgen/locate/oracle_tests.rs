//! Locating against the real classes: `LeafLocateOracle.java` (and `LeafStructurePlacementOracle`'s
//! `realring` rows) with the real 26.3 data (`tools/leaf_server/README.md`).
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafLocateOracle.java locate.txt
//! HORIZON_VANILLA_JAR=... HORIZON_LOCATE_ORACLE=locate.txt cargo test -p horizon-server --release \
//!     locate_matches_java -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::{Locator, find_closest_biome_3d};
use crate::core::position::{BlockPos, ChunkPos};
use crate::registry::{DataPack, Registries, load_tags, registry_dir};
use crate::server::vanilla_jar::VanillaJar;
use crate::world::chunk::ContainerFactory;
use crate::world::level_chunk::HeightmapTable;
use crate::worldgen::biome::codec::{BiomeIds, BiomePresets, decode_noise_generator};
use crate::worldgen::codec::{object, required};
use crate::worldgen::generator::{GeneratorTables, LevelGenerator, LevelGeneratorSettings};
use crate::worldgen::structure::StructureId;
use crate::worldgen::structure::template::StructureTemplates;
use crate::worldgen::terrain::oracle_tests::{env_path, load_real};

const STEMS: [&str; 3] = [
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

/// The real data of the normal world preset's three levels.
pub(crate) struct RealWorld {
    tables: GeneratorTables,
    registries: Registries,
    normal: Box<serde_json::value::RawValue>,
    presets: BiomePresets,
    state_count: u32,
    biome_count: u32,
    generators: HashMap<(usize, i64), Arc<LevelGenerator>>,
}

impl RealWorld {
    pub(crate) fn load() -> Self {
        let real = load_real().expect("HORIZON_VANILLA_JAR and the block data");
        let reports = env_path("HORIZON_BIOME_REPORTS").expect("HORIZON_BIOME_REPORTS");
        let mut jar = VanillaJar::open(env_path("HORIZON_VANILLA_JAR").unwrap().as_ref()).unwrap();
        let vanilla = DataPack::vanilla(&mut jar).unwrap();
        let registries =
            Registries::build(Vec::new(), &[vanilla.clone(), DataPack::paper()]).unwrap();
        let biome_registry = registries.synced("minecraft:worldgen/biome").unwrap();
        let biomes = BiomeIds::new(biome_registry);
        let biome_count = biome_registry.entries.len() as u32;
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
        let crate::worldgen::terrain::oracle_tests::Real {
            mut data,
            blocks,
            state_data,
        } = real;
        let block_tags = load_tags(
            std::slice::from_ref(&vanilla),
            &registry_dir("minecraft:block"),
            |id| blocks.block_by_name(id).map(|b| b.0 as i32),
        );
        let motion: HashSet<_> = block_tags
            .iter()
            .find(|(name, _)| name == "minecraft:blocks_motion_in_heightmap")
            .map(|(_, ids)| {
                ids.iter()
                    .map(|&i| crate::block::BlockId(i as u32))
                    .collect()
            })
            .unwrap_or_default();
        data.set_templates(StructureTemplates::load(&mut jar, None, &blocks, &state_data).unwrap());
        let state_count = blocks.state_count();
        let heightmaps = Arc::new(HeightmapTable::vanilla(
            &blocks,
            &state_data,
            &motion,
            &HashSet::new(),
        ));
        let (data, blocks, flags) = (Arc::new(data), Arc::new(blocks), Arc::new(state_data));
        let survival = Arc::new(
            crate::block::survival::BlockSurvival::new(&blocks, &flags, &[], &[])
                .expect("the survival rules"),
        );
        Self {
            tables: GeneratorTables {
                data,
                blocks,
                flags,
                heightmaps,
                survival,
            },
            registries,
            normal,
            presets,
            state_count,
            biome_count,
            generators: HashMap::new(),
        }
    }

    pub(crate) fn biome_ids(&self) -> BiomeIds<'_> {
        BiomeIds::new(self.registries.synced("minecraft:worldgen/biome").unwrap())
    }

    /// The generator of a stem (0 overworld, 1 nether, 2 end) and seed, with structures.
    pub(crate) fn generator(&mut self, stem: usize, seed: i64) -> Arc<LevelGenerator> {
        if let Some(made) = self.generators.get(&(stem, seed)) {
            return made.clone();
        }
        let biomes = self.biome_ids();
        let dimensions =
            object(required(&object(&self.normal).unwrap(), "dimensions").unwrap()).unwrap();
        let raw = required(&object(&dimensions[STEMS[stem]]).unwrap(), "generator")
            .unwrap()
            .to_owned();
        let generator = decode_noise_generator(&raw, &biomes, &self.presets).unwrap();
        let (min_y, height) = if stem == 0 { (-64, 384) } else { (0, 256) };
        let plains = biomes.id("minecraft:plains").unwrap();
        let made = Arc::new(
            LevelGenerator::new(
                self.tables.clone(),
                LevelGeneratorSettings {
                    stem: generator,
                    seed,
                    generate_structures: true,
                    flat_bedrock: false,
                    end_void_rings: false,
                    spigot_seeds: Default::default(),
                    min_section_y: min_y >> 4,
                    section_count: (height / 16) as usize,
                    factory: ContainerFactory::new(self.state_count, self.biome_count, plains),
                },
            )
            .expect("the generator"),
        );
        self.generators.insert((stem, seed), made.clone());
        made
    }

    fn stem_index(name: &str) -> usize {
        STEMS.iter().position(|s| *s == name).expect("a stem")
    }
}

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_STRUCTURE_ORACLE's realring rows (tools/leaf_server/README.md)"]
fn real_rings_match_java() {
    let oracle = std::fs::read_to_string(
        env_path("HORIZON_STRUCTURE_ORACLE").expect("HORIZON_STRUCTURE_ORACLE"),
    )
    .unwrap();
    let mut world = RealWorld::load();
    let mut rows = 0;
    for line in oracle.lines().filter(|l| l.starts_with("realring ")) {
        let words: Vec<&str> = line.split(' ').collect();
        let seed: i64 = words[1].parse().unwrap();
        let generator = world.generator(0, seed);
        let set = generator.data().structures().set_id(words[2]).unwrap();
        let [index, x, z]: [i32; 3] = [3, 4, 5].map(|i| words[i].parse().unwrap());
        let positions = generator
            .placements()
            .ring_positions(set)
            .expect("ring positions");
        assert_eq!(positions[index as usize], ChunkPos::new(x, z), "{line}");
        rows += 1;
    }
    assert!(rows >= 3 * 128, "realring rows: {rows}");
    println!("realring rows: {rows}");
}

#[test]
#[ignore = "needs the real 26.3 data and HORIZON_LOCATE_ORACLE (tools/leaf_server/README.md)"]
fn locate_matches_java() {
    let oracle =
        std::fs::read_to_string(env_path("HORIZON_LOCATE_ORACLE").expect("HORIZON_LOCATE_ORACLE"))
            .unwrap();
    let mut world = RealWorld::load();
    let language = VanillaJar::open(env_path("HORIZON_VANILLA_JAR").unwrap().as_ref())
        .unwrap()
        .load_language();
    let mut failures: Vec<String> = Vec::new();
    let mut counts: BTreeMap<&str, [usize; 2]> = BTreeMap::new();
    let mut locators: HashMap<(usize, i64), Locator<'static>> = HashMap::new();
    let mut leaked: HashMap<(usize, i64), &'static Arc<LevelGenerator>> = HashMap::new();
    let started = std::time::Instant::now();
    for line in oracle.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f[0] {
            "structure" | "biome" => {
                let stem = RealWorld::stem_index(f[1]);
                let seed: i64 = f[2].parse().unwrap();
                let origin = BlockPos::new(
                    f[3].parse().unwrap(),
                    f[4].parse().unwrap(),
                    f[5].parse().unwrap(),
                );
                let query = f[6];
                // The generators outlive the test; the locators borrow them.
                let generator: &'static LevelGenerator = leaked
                    .entry((stem, seed))
                    .or_insert_with(|| Box::leak(Box::new(world.generator(stem, seed))));
                let got = if f[0] == "structure" {
                    let registry = generator.data().structures();
                    let wanted: Vec<StructureId> = match query.strip_prefix('#') {
                        Some(tag) => registry.tag(tag).expect("a structure tag").to_vec(),
                        None => vec![registry.structure_id(query).expect("a structure")],
                    };
                    let locator = locators
                        .entry((stem, seed))
                        .or_insert_with(|| Locator::new(generator).unwrap());
                    locator
                        .find_nearest_map_structure(&wanted, origin, 100, false)
                        .map_or("none".to_owned(), |(pos, structure)| {
                            format!(
                                "{} {} {} {}",
                                pos.x,
                                pos.y,
                                pos.z,
                                registry.structure(structure).id
                            )
                        })
                } else {
                    let biomes = world.biome_ids();
                    let synced = world.registries.synced("minecraft:worldgen/biome").unwrap();
                    let allowed: HashSet<u32> = match query.strip_prefix('#') {
                        Some(tag) => synced
                            .tags
                            .get(tag)
                            .map(|ids| ids.iter().map(|&i| i as u32).collect())
                            .unwrap_or_default(),
                        None => biomes.id(query).into_iter().collect(),
                    };
                    find_closest_biome_3d(generator, origin, 6400, (32, 64), |b| {
                        allowed.contains(&b)
                    })
                    .map_or("none".to_owned(), |(pos, biome)| {
                        format!(
                            "{} {} {} {}",
                            pos.x,
                            pos.y,
                            pos.z,
                            biomes.name(biome).unwrap()
                        )
                    })
                };
                let entry = counts.entry(if f[0] == "structure" {
                    "structure"
                } else {
                    "biome"
                });
                let entry = entry.or_default();
                entry[usize::from(got == "none")] += 1;
                if got != f[7] {
                    failures.push(format!("{line}\n    ours: {got}"));
                }
            }
            // `LocateCommand.showLocateResult`'s message and result.
            "feedback" => {
                let point = |text: &str| {
                    let v: Vec<i32> = text.split(' ').map(|n| n.parse().unwrap()).collect();
                    BlockPos::new(v[0], v[1], v[2])
                };
                let name = if f[2].starts_with('#') {
                    format!("{} ({})", f[2], f[5])
                } else {
                    f[2].to_owned()
                };
                let (message, distance) = crate::server::runtime::commands::locate::locate_message(
                    &format!("commands.locate.{}.success", f[1]),
                    &name,
                    point(f[3]),
                    point(f[4]),
                    f[1] == "biome",
                );
                let text = message.to_string_in(&language);
                if text != f[7] || distance.to_string() != f[6] {
                    failures.push(format!("{line}\n    ours: {distance} {text}"));
                }
                counts.entry("feedback").or_default()[0] += 1;
            }
            // The not-found messages.
            "error" => {
                let key = match f[1] {
                    "commands.locate.structure.not_found" => "commands.locate.structure.not_found",
                    "commands.locate.structure.invalid" => "commands.locate.structure.invalid",
                    "commands.locate.biome.not_found" => "commands.locate.biome.not_found",
                    "commands.locate.poi.not_found" => "commands.locate.poi.not_found",
                    other => panic!("{other}"),
                };
                let text = crate::command::CommandError::locate(key, f[2])
                    .message()
                    .to_string_in(&language);
                if text != f[3] {
                    failures.push(format!("{line}\n    ours: {text}"));
                }
                counts.entry("error").or_default()[0] += 1;
            }
            _ => {}
        }
    }
    println!(
        "rows [found, none]: {counts:?} in {:?}; failures {}",
        started.elapsed(),
        failures.len()
    );
    for failure in failures.iter().take(20) {
        println!("{failure}");
    }
    assert!(failures.is_empty(), "{} rows differ", failures.len());
}
