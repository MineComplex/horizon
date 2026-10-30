# Porting Horizon's world generator into horizon-server (Leaf 26.3)

Read-only survey, 2026-10-30. Source: `crates/horizon-world/src/worldgen/` (+ `worldgen.rs`) and what it pulls from
`crates/horizon-core` and the rest of `crates/horizon-world`. Target: `crates/horizon-server` (ARCHITECTURE.md,
PORTING_RULES.md: native Rust, no `unsafe`, every Arc/lock justified, no dependency on other Horizon crates, data
from the admin's jar + prepare step, behaviour = vanilla 26.3 + Paper + Leaf).

Paths below: `W/` = `crates/horizon-world/src/worldgen/`, `HW/` = `crates/horizon-world/src/`, `HC/` =
`crates/horizon-core/src/`, `S/` = `crates/horizon-server/src/`, `PAPER/` =
`/home/user/Leaf/.gradle/caches/paperweight/upstreams/paper/paper-server/patches/`, `LEAF/` =
`/home/user/Leaf/leaf-server/minecraft-patches/features/`.

---

## 1. Inventory

`W/` is 51,715 lines in 124 files, plus `HW/worldgen.rs` (299: module list, `DecodeError`, Gson-exact JSON number
helpers). Grouped by subsystem (line counts include unit tests):

| Subsystem | Files (lines) | Total |
|---|---|---|
| **Codec/infra** | `worldgen.rs` 299, `settings.rs` 45, `tags.rs` 369 (tag resolution in vanilla order), `weighted.rs` 105, `value_provider.rs` 275, `height_provider.rs` 149, `anchor.rs` 102 (VerticalAnchor, WorldGenerationContext), `long_set.rs` 83, `write_zone.rs` 45, `bulk_section.rs` 113 | 1,585 |
| **Noise/density/terrain fill** | `density.rs` 578 (registry JSON → `horizon_core::density::graph`), `router.rs` 168 (NoiseRouter), `terrain.rs` 521 (`TerrainChunk`, NoiseChunk fill, `PreLightChunkStage`) | 1,267 |
| **Aquifer** | `aquifer.rs` 95, `aquifer/config.rs` 127, `aquifer/noise_based.rs` 608 | 830 |
| **Surface rules** | `material_condition.rs` 694 + `material_condition/{codec 468, density 278, library 359, noise 112, rule 393}`, `material_extensions.rs` 229 (badlands/iceberg extensions), `material_stage.rs` 360, `material_surface.rs` 142 (clay bands, surface noise), `biome_temperature.rs` 195, `biome_weather.rs` 94 | 3,324 |
| **Biome source/climate** | `biome.rs` 153, `biome_chunk.rs` 202 (quart storage, `BiomeRegion`), `biome_predicate.rs` 81, `biome_source.rs` 92 + `biome_source/{codec 262, runtime 170}`, `climate.rs` 118 + `climate/{codec 59, index 342}` (RTree), `spawn_finder.rs` 128 | 1,607 |
| **Carvers** | `carver.rs` 296 + `carver/{canyon 161, generation 167}`, `carving.rs` 122 (apply carving mask with surface rules) | 746 |
| **Feature/placement infra** | `feature.rs` 162, `feature_catalog.rs` 304, `feature_graph.rs` 1,106, `feature_region.rs` 661 (`TerrainRegion`: 3×3 proto-chunk view), `feature_sort.rs` 116 (FeatureSorter), `decoration.rs` 172, `decoration_order.rs` 416 (vanilla scheduler model), `placement.rs` 505 + `placement/{chain 270, context 380, layer 68}`, `block_predicate.rs` 376, `fluid_predicate.rs` 107, `rule_test.rs` 220, `state_provider.rs` 593 + `state_provider/{catalog 64, noise 231, properties 206}`, `block_feature.rs` 385 | 6,342 |
| **Feature implementations** | `bamboo` 156, `block_blob` 140, `block_column` 186, `block_pile` 160, `blue_ice` 156, `bonus_chest` 102, `cave.rs` 700 (lakes, dungeons, geodes), `chorus_plant` 204, `delta` 202, `disk` 168, `dripstone` 803, `end_gateway` 123, `end_podium` 174, `end_spike` 243, `fungus` 300, `iceberg` 390, `ore_feature` 370, `projected.rs` 277 (basalt columns, patchy squares), `replace_blobs` 170, `root_system` 230, `sculk` 727, `simple_block` 109, `snow_freeze` 115, `speleothem` 210, `spike` 187, `spring` 116, `underwater_magma` 187, `vegetation` 847, `vines` 68 | 7,820 |
| **Trees** | `tree.rs` 832 + `tree/{decorator 699, fallen 182, foliage 631, trunk 811}`, `tree_blocks.rs` 471 | 3,626 |
| **Live features** (gameplay: saplings, bone meal) | `live.rs` 333 | 333 |
| **Structures, vanilla** | `structure.rs` 398 (templates store, StateTransforms), `structure/{start 1537, start_checks*, jigsaw 1368, template 373, processor 2167, legacy 978, simple 623, temple 751, mineshaft 813, stronghold 1153, fortress 935, monument 1452, mansion 982, end_city 467, ocean_ruin 360, ruined_portal 423, nether_fossil 117, features 231}` | 15,128 |
| **Structures, Horizon modpack-only (drop)** | `structure/formations.rs` 405, `structure/piece_limited.rs` 1,067, `structure/start_checks.rs` 462, `structure/enhanced_terrain.rs` 358, `structure/processor/{litho 841, pillars 942, sea_level 448}`; plus `"horizon:*"` branches in `jigsaw.rs:56-172`, `processor.rs`, `rule_test.rs:190`, `start.rs:60-110,378`, `tree/decorator.rs:24` | 4,523 |
| **Generator driver** | `generator.rs` 2,868 (`GeneratorData::load`, `Pipeline`, `OverworldGenerator`, `GeneratorWorker`; tests from 2325), `generator_blocks.rs` 168, `generator_decoration.rs` 1,229 (FEATURES step, `World` region view), `generator_structures.rs` 618 (starts cache, rings) | 4,883 |

### Internal dependencies (computed from `use super::…`, tests excluded)

- infra ← everything. terrain → aquifer, biome (climate sampler). aquifer → terrain (density fields).
- surface → infra, density (`material_condition/density.rs`). carver → surface (`carving.rs` applies the material
  rule), terrain (aquifer/NoiseChunk), featinfra (`FeatureBlockBindings`, height/value providers).
- featinfra ↔ featimpl (the catalog dispatches to every impl: 27 edges; impls use infra: 87 edges), featinfra →
  tree, struct (`template`/`fossil` features in `structure/features.rs`), biome (biome-filter placement), terrain.
- struct → featinfra (placement context, `rule_test`, `block_predicate`), biome, infra.
- driver → all.

### Dependencies outside `worldgen/`

| Used | Where | horizon-server equivalent |
|---|---|---|
| `horizon_core::random` (Legacy, Xoroshiro, `WorldgenRandom`, positional, `java_string_hash`) | ~40 files | **Have** `S/util/random{,/hash,/positional,/worldgen}.rs` — byte-identical to `HC/random/*` except `crate::core::position` paths and the test-only `rng-trace` thread_local |
| `horizon_core::jmath` (sin/cos/pow/log) | ~15 | **Have** `S/util/jmath.rs` (identical) |
| `horizon_core::geometry::{Direction, trig}` | ~20 | **Have** `S/core/geometry/` (identical modulo paths) |
| `horizon_core::position::{BlockPos, ChunkPos}` | ~10 | **Have** `S/core/position.rs` (+`iter.rs`) |
| `horizon_core::noise` (`HC/noise/`: perlin, simplex, normal, stack, volume) | ~25 | **Missing** — 1,572 lines |
| `horizon_core::density` (`HC/density/`: samplers, graph+compiler+seeded, splines, interpolation, cache, beardifier, end islands, intervals) | ~15 | **Missing** — 6,395 lines (≈1,513 tests) |
| `horizon_core::biome` (`HC/biome.rs` 227 + `biome/seed_hash.rs` 66: BiomeManager fuzzy zoom, `obfuscate_seed`) | biome | **Missing** |
| `horizon_core::carving` (`HC/carving.rs` 164 + `carving/ellipsoid.rs` 124: CarvingMask, ellipsoid skip) | carver | **Missing** |
| `HW/registry.rs` (285: `BlockRegistry`, `BlockStateId`, `BlockStateJsonCodec`) + `registry/{facts,json}.rs` | 55 files | **Have, different API**: `S/block/mod.rs` `BlockStates`/`BlockState(u32)` (same ids: both are `Block.BLOCK_STATE_REGISTRY` order) |
| `HW/nbt.rs` (984: `Compound`, `Tag`, `JavaString`) | 20 | **Have** `S/nbt.rs` (1,087) |
| `HW/heightmap.rs` (249: `HeightmapType`, `Heightmap`, `HeightmapState`) | 11 | **Partly**: `S/world/chunk/serial.rs:90` `Heightmap` enum, `S/world/level_chunk.rs:26` `HeightmapTable` |
| `HW/chunk.rs` (842: `BlockSection` paletted section, `BlockState` descriptor) | 5 | **Have** `S/world/chunk/{section,paletted}.rs` (Moonrise counting) |
| `HW/chunk_status.rs` (100) | 2 | **Have** `S/world/chunk/serial.rs:35` `ChunkStatus` |
| `HW/fluid.rs` (322: `FluidKind`, `FluidState`) | 11 | **Partly**: `S/block/flags.rs:103` `StateFluid` per state |
| `HW/block_properties.rs` 246, `block_collision.rs` 284, `collision.rs` 232, `block_support.rs` 210, `block_survival.rs` 1,220, `block_placement.rs` 376 | feature placement checks | **Partly**: `S/block/flags.rs` has collision/occlusion shapes, `is_face_sturdy(SupportType)`, replaceable, can-occlude; **no survival rules, no placement definitions** |
| `HW/level/` (`Behaviours::{update_shape, can_survive, is_face_sturdy}`, `LevelAccess`, `Ticks`/`CapturedTick`, `data_2d`) — `level/{mod 998, shapes 1268, survive 589, states 351, data 1189, classes 338, ticks 189}` | `generator_decoration.rs:465-830`, `structure/processor.rs:1703`, `tree.rs:787`, `placement/context.rs:34` | **Missing** (block behaviour not ported yet) |
| `HW/retrogen.rs` (177, BelowZeroRetrogen) | 2 | Missing; only needed for chunks carrying `below_zero_retrogen` (pre-1.18 upgrades) |
| `HW/java_hash_map.rs` (791, Java HashMap iteration order) | `tree_blocks.rs:383-420` (`PosSet`), `piece_limited.rs` (dropped) | Missing (see §3) |
| `HW/region.rs`, `structure_save.rs` 22 | structure NBT | **Have** `S/world/storage/region.rs`; structure NBT stays raw in `S/world/chunk/serial.rs` today |
| `serde_json` (`RawValue`, `preserve_order`) | everywhere | **Have** (`preserve_order`, `float_roundtrip`) |

---

## 2. Data inputs

`GeneratorData::load(dir, registry)` (`W/generator.rs:306-557`) reads `<resources>/worldgen/` produced by
`crates/horizon-prepare/src/lib.rs::prepare_worldgen` (lines 1087-1186). Mapping to horizon-server:

| File in `.local/horizon-client/26.3/worldgen/` | Format / origin | horizon-server source |
|---|---|---|
| `noises.json`, `densities.json`, `rules.json`, `conditions.json`, `settings.json`, `carvers.json`, `biomes.json`, `world_presets.json`, `features.json`, `placed_features.json`, `state_providers.json`, `processor_lists.json`, `template_pools.json`, `structures.json`, `structure_sets.json` | `{id: "<json text>"}` bundles of the vanilla datapack dirs `data/minecraft/worldgen/{noise, density_function, material_rule, material_condition, noise_settings, carver, biome, world_preset, feature, placed_feature, block_state_provider, processor_list, template_pool, structure, structure_set}` (`WORLDGEN_BUNDLES`, lib.rs:1055); prepare also checks client and server jars agree | **Jar, directly.** `S/registry/mod.rs` `DataPack::vanilla` (line 168) today keeps only `SYNCHRONIZED` dirs + `tags/`; extend it to RegistryDataLoader's worldgen registries (66 noise, 63 density_function, 8 noise_settings, 68 biome, 242 feature, 274 placed_feature, 5 carver, 53 structure, 22 structure_set, 306 template_pool, 41 processor_list, 8 world_preset, 46 material_rule, 9 material_condition, 9 block_state_provider, 10 flat_level_generator_preset, 3 multi_noise_biome_source_parameter_list in the 26.3 jar). Biome and block_state_provider are already synced entries (`Entry.json`). |
| Paper's data overrides | `PAPER/resources/data/minecraft/worldgen/material_rule/bedrock_{floor,roof}.json.patch` (full text in `/home/user/Leaf/leaf-server/src/minecraft/resources/data/minecraft/worldgen/material_rule/`) | **Must be applied by the server** (Leaf's jar differs from vanilla here): add them to the built-in `paper` pack like `PAPER_RAW_CHAT_TYPE` (`S/registry/mod.rs:73`), and decode the `paper:optionally_flat_bedrock_material_condition` type |
| `block_tags.json`, `fluid_tags.json`, `biome_tags.json`, `feature_tags.json` | `tags/{block, fluid, worldgen/biome, worldgen/feature}` | **Jar**: `S/registry/tags.rs` already resolves tags for builtin registries (block, fluid) and synced ones (biome) — as protocol ids. Add tags for the non-synced worldgen registries (`tags/worldgen/{feature, structure, world_preset, flat_level_generator_preset}`) |
| `structure/minecraft/**.nbt` | gzip NBT templates copied from the server jar (1,511 files, 4.2 MB compressed) | **Jar** via `S/server/vanilla_jar.rs` `VanillaJar::read`, plus the world's `generated/<ns>/structure/` dir first (StructureTemplateManager source order; Leaf's Cleanup only removes the gametest sources) |
| `biome_parameters/*.json` (or `presets.json` fallback) | data generator `--reports` `biome_parameters/minecraft/{overworld,nether,…}.json`; quantized in Rust by `quantize_parameter_report` (generator.rs:237) | **Prepare step already has it**: `tools/leaf_server_reports.py` copies all of `reports/` to `<server>/cache/vanilla-26.3/reports/` |
| `block_flags.json` | `[is_air, has_fluid]` per state, from `facts.json` (Horizon's facts exporter) | **Have**: `S/block/flags.rs` `FLAG_AIR`, `fluid()` from `block_states.bin` (LeafBlockStatesExporter) |
| `BlockRegistry` | `facts.json` (`registry/facts.rs`) | **Have**: `S/block/mod.rs` `BlockStates` from `reports/blocks.json` (same state ids) |
| `blocks/block-properties.nbt`, `blocks/cached-collision.nbt`, `blocks/cached-support.nbt` | `VanillaSimpleBlockOracle export` (parity mod), loaded by `W/generator_blocks.rs` | **Mostly have** in `block_states.bin`: collision shape, face sturdiness per `SupportType`, replaceable, flags. Check per field when porting `generator_blocks.rs` (e.g. `isCollisionShapeFullBlock`, `blocksMotion`, `isSolid` legacy flag present as `FLAG_LEGACY_SOLID`) |
| `blocks/survival-rules.json`, `survival-extra-definitions.json`, `placement-definitions.json` | `VanillaSimpleBlockOracle` + `survival::bind_definitions` (Java class → `SurvivalRule` names, `generator_blocks.rs:34-90`) | **Missing**: needs an exporter in the prepare step (extend `mods/horizon/src/test/java/dev/horizonlab/leafserver/LeafBlockStatesExporter.java` → `block_states.bin` v2 with a survival-rule id per block, or a sibling exporter) — or native `canSurvive` per block once block behaviour is ported |
| `blocks/structure-blocks.json` | `VanillaStructureBlockExporter.java` (49 lines): per state `rotate(3)`, `mirror(2)`, full-block class, randomizable container, block-entity type | **Missing**: add to the server exporter (rotate/mirror are per-block code, not derivable from `blocks.json` reliably) |
| `../block-behaviours.json` | `VanillaBlockBehaviourExporter.java` (255 lines): block classes, instance config, per-state facts for `level::Behaviours` (updateShape during template/tree placement, captured ticks) | **Missing**: part of the server's future block-behaviour port; worldgen needs `update_shape`, `can_survive`, `CAN_OCCLUDE`, scheduled ticks from shape updates |
| dimension heights | hard-coded in `W/generator.rs:704` `dimension_type_height` | **Have**: synced `minecraft:dimension_type` entries (min_y/height) |
| world seed / level stems | world creation | **Have**: `S/world/level_storage.rs:757` `WorldGenSettings` (stems raw), `S/server/server_properties.rs` (`level-seed`, `generate-structures`, `generator-settings`, `level-type`) |

Config inputs the server must read for generation (YAML via `S/server/config`): spigot.yml `world-settings.*.seed-*`
(`SpigotWorldConfig.initWorldGenSeeds`), paper world `environment.generate-flat-bedrock`,
`environment.locate-structures-outside-world-border`, `feature-seeds.*`; purpur.yml
`settings.generate-end-void-rings`; leaf-global.yml `performance.faster-random-generator.*`, `fixes.mc-152094`
(see §5).

---

## 3. Java-shaped spots

All uses in `W/` and the needed `HC/` modules (tests excluded unless noted). Classes:
**(A)** genuinely shared read-only data — fine; **(B)** lazily-filled caches/holders emulating Java's lazy
registries — decode eagerly at load, then immutable; **(C)** shared mutable state across worker threads standing in
for the chunk system — replace with data owned by the scheduler and passed to jobs; **(D)** interior mutability
emulating mutable Java fields on shared objects; **(E)** hidden globals / thread-locals — make explicit;
**(F)** `Arc<dyn Trait>` object graphs.

| Site | What | Class | Native design |
|---|---|---|---|
| `W/generator.rs:161-193` `GeneratorData` held as `Arc<Self>`, `GenBlockStates(Arc<[..]>)` :128, `biome_names: Vec<Arc<str>>` | seed-independent tables | A | Keep one `Arc<WorldgenData>` built at startup/`/reload` (ARCHITECTURE "read-only shared data is Arc'd once"); `Arc<str>` → `Box<str>`/indices |
| `W/generator.rs:182` `structures: Option<Arc<StructureResources>>`, :187 `behaviours: Option<Arc<Behaviours>>`, `generator_blocks.rs:23-24` `Arc<BlockSupportRegistry>`/`Arc<BlockCollisionRegistry>` | sub-tables shared inside the data | A | Plain fields of `WorldgenData` (one Arc at the top suffices) |
| `W/generator.rs:190` `ring_cache: Mutex<Vec<…>>`, :195 `Arc<OnceLock<Rings>>`; :193/:199 `decorations: Mutex<Vec<(String, Arc<OnceLock<Result<…>>>)>>` | per-(seed, dimension) results cached on the global data | B/C | Build per level at level creation: `LevelGenerator { rings, decoration, … }` owned by the `Level`, handed to jobs as `Arc<LevelGenerator>` (read-only after build) |
| `W/generator.rs:746` `carver_cache: Mutex<HashMap>`, :757-777 `ChunkWork(Mutex<HashMap<_, Weak<Mutex<()>>>>)`, :778 `BiomeCache = Mutex<HashMap<_, Arc<BiomeChunk>>>` | neighbour biome chunks computed on demand with per-key locks (replacing the chunk pyramid) | C | Follow the 26.3 `ChunkPyramid` (BIOMES before TERRAIN with radius 1; STRUCTURE_STARTS radius 8): the scheduler stores each proto chunk's biomes; a TERRAIN job receives its 3×3 neighbours' biomes as read-only copies/`Arc<BiomeChunk>` snapshots. No locks |
| `W/generator.rs:751` `spawn_position: OnceLock<[i32;2]>` | NoiseSpawnFinder result | B | Compute once when the level is created (MinecraftServer.setInitialSpawn needs it anyway); store in level data |
| `W/generator_structures.rs:124-140` `StartCache` (64 `RwLock<HashMap>` shards), :35 `start_work: ChunkWork` | structure starts by chunk, built on demand by any worker | C | Starts live in the proto chunk (STRUCTURE_STARTS) and are persisted in chunk NBT (vanilla); STRUCTURE_REFERENCES/FEATURES jobs get the 17×17 neighbours' starts as `Arc<StructureStart>` (justified: one start is read by every chunk it covers) |
| `W/generator_structures.rs:39-72` `RingWork` (`AtomicUsize`, `Vec<OnceLock>`, `Mutex<usize>`, `Condvar`), :41 `ring_work: OnceLock` | stronghold ring biome searches shared out between workers | C | One job per level (vanilla `generateRingPositions` runs its searches as background tasks): `par_iter` (or the gen pool) over the searches, collected in order, before the first STRUCTURE_STARTS job; result immutable |
| `W/generator_decoration.rs:73` `named: Mutex<HashMap<String, Option<Arc<PlacedFeatureDefinition>>>>`; `W/live.rs:26` `Cache<T> = Mutex<HashMap<String, Option<Arc<T>>>>` | lazy decode of placed/configured features | B | Decode all 274 placed / 242 configured features at load into `Vec<…>` indexed by registry id; `Arc` disappears |
| `W/structure.rs:278` `templates: RwLock<HashMap<String, Option<Arc<StructureTemplate>>>>` | lazy template loading | B | Load all templates at startup (parallel decode; ~4 MB) into an immutable map, or a loader owned by the gen pool that answers by message. Vanilla also reads the world's `generated/` dir first |
| `W/structure/jigsaw.rs:473` `Pools.decoded: RwLock<HashMap<…>>`, :401 `max_size: OnceLock<i32>`; `W/structure/template.rs:29` `jigsaws: OnceLock<Vec<JigsawInfo>>`; `W/state_provider.rs:38-41` three `OnceLock`s; `W/block_feature.rs:33` `tree_blocks: OnceLock<Arc<TreeBlocks>>` | lazily computed derived fields | B | Compute at decode/load time; plain fields |
| `HC/density/graph/seeded.rs:22` `instances: Mutex<HashMap<NoiseId, Arc<dyn Noise>>>` | lazy per-seed noise instantiation (RandomState.getOrCreateNoise) | B | Instantiate all noises eagerly in `RandomState::new` (deterministic: each is seeded from `fromHashOf(name)`), store `Vec<NormalNoise>` by `NoiseId` |
| `W/structure/simple.rs:22` `found: Mutex<Option<[i32;3]>>` (buried treasure), :426 `frame: Mutex<PieceFrame>` + :430 `height_position: Mutex<i32>` (ScatteredFeaturePiece), :529 `spawned: Mutex<bool>` (witch hut); `fortress.rs:60` `flag: Mutex<bool>`; `legacy.rs:819` `state: Mutex<Option<i32>>`; `mineshaft.rs:52` `placed_spider: Mutex<bool>`; `stronghold.rs:166` `placed: Mutex<bool>`; `temple.rs:18` `placed_chest: Mutex<[bool;4]>`, :446 `placed: Mutex<[bool;4]>` | Java pieces' mutable fields, written in `postProcess` by whichever chunk places them; saved in start NBT | D | Moonrise runs FEATURES of non-overlapping areas in parallel, so a start's pieces can be post-processed concurrently (as in Leaf). Use **set-once `OnceLock`** for values fixed on first use (heightPosition + moved frame, buried-treasure position, template height state) and **`AtomicBool`/`[AtomicBool;4]`** for "placed" flags (check-and-set = `swap`), each with a one-line justification. No `Mutex` |
| `W/structure/jigsaw.rs:953, 1125, 1172, 1188, 1217` `Rc<RefCell<Free>>` (also `piece_limited.rs:286, 683-731`, dropped) | JigsawPlacement's shared `MutableObject<VoxelShape>` free space | D | Arena: `Vec<Free>` in the assembler + `FreeId(u32)` in `PieceState` |
| `W/feature_graph.rs:178-179` `depth/remaining: Cell<usize>`; `W/placement/chain.rs:188, 206` `&Cell<usize>` | recursion budget counters | (fine) | Local; could be `&mut usize`. Keep or simplify |
| `W/structure/processor.rs:881` `thread_local!(static DEPTH: Cell<u32>)` | nesting guard for processor-list references | E | Pass `depth: u32` through `decode`/`decode_at` |
| `W/biome_temperature.rs:37` `static NOISES: OnceLock<TemperatureNoises>`; `HC/density/beardifier.rs:323` `static KERNEL: OnceLock<[f32;13824]>`; `W/structure/jigsaw.rs:687-689` `static GRAVITY/IGNORE_STRUCTURE_BLOCK/IGNORE_STRUCTURE_AND_AIR: OnceLock<Processor>`; `start.rs:508`, `generator_structures.rs:271` `static NONE` | constants (Biome's seeded TEMPERATURE/FROZEN/INFO noises, beard kernel, built-in processors) | A | Fine as `LazyLock` constants (immutable, seed-independent); or fields of `WorldgenData` |
| `&'static BlockRegistry` throughout (`generator.rs:162`, `BlockBehavior<'static>`, `FeatureBlockBindings<'static>`, `StructureResources.registry`), tests `Box::leak` (generator.rs:2339) | global-lifetime registry | E | Borrow `&BlockStates` from the server's `Arc`'d tables, or store the `Arc`; no leaks |
| `W/generator.rs:1721` `trait StructureMobs: Send + Sync` stored as `Arc<dyn StructureMobs>` (`generator_decoration.rs:455`) | callback into gameplay to create mobs during generation | F/E | Vanilla puts generated entities into the proto chunk (`WorldGenRegion.addFreshEntity` → `ProtoChunk.addEntity` tag). Emit entity NBT into the proto chunk; the tick thread loads them at FULL (Paper's duplicate-UUID check in `ChunkStatusTasks.postLoadProtoChunk`) |
| `Arc<dyn SeededPipeline>` (3 uses, generator.rs) | erases `Pipeline<R: RandomSource>` | F | `enum LevelGenerator { Legacy(Pipeline<LegacyRandom>), Xoroshiro(Pipeline<Xoroshiro>) }` or generic monomorphisation; owned by the level |
| `Arc<dyn DensitySampler>` (75 uses), `Arc<dyn Noise>` (34), `Arc<dyn DensityBlender>` (2) across `W/` and `HC/density/` | compiled density functions as a tree of trait objects (DensityFunction objects) | F | The graph already is an arena (`HC/density/graph/mod.rs` `DensityGraph`, `FunctionId`): compile to one `DensityProgram { nodes: Vec<Node> }` (closed `enum Node`, children by index, caches by id in `SamplerContext`), shared as one `Arc<DensityProgram>` per level. Measure with `HW/../examples/chunkgen_bench.rs` before/after; if the enum interpreter is slower, keep `Box<dyn>` inside the program (owned, not `Arc` per node) |
| `W/generator.rs:281-292` `std::thread::Builder` "Worldgen loader" threads, `generator_blocks.rs` `thread::scope` | parallel resource loading | (ok) | Use the server's startup pool (std scoped threads are fine; ARCHITECTURE plans rayon for the worker pool) |
| `W/tree_blocks.rs:383-420` `PosSet` over `HW/java_hash_map.rs` (791 lines) | iteration order of `HashSet<BlockPos>` in TreeFeature (affects placement/decorator order) | behaviour, not a JDK port | Keep the behaviour with a narrow helper: "Java HashSet<BlockPos> iteration order" (bucket index of `h ^ h>>>16` for the current capacity, insertion order within a bucket, resize/treeify rules), ~150 lines, tested against the tree harness; do not port the general map |

---

## 4. Parity status

All harnesses pin **26.3** through `research-lock.json` (`minecraft.version = "26.3"`, protocol 777, data 5023);
`test_noise_parity.py`/`test_density_parity.py` run "both checksum-pinned vanilla JARs" (client and server). The Leaf
cache jar `/home/user/Leaf/leaf-server/.gradle/caches/paperweight/data/bundler/server.jar` has sha256
`a362163e…8ae6`, the same 26.3 server-game jar Horizon's benchmarks record (`benchmarks/*/scoped-parity.json`
`jar_sha256`), and `versions.list` confirms `26.3/server-26.3.jar`; its 39 libraries are under
`/home/user/Leaf/leaf-server/.gradle/caches/paperweight/jars/minecraft/`. `.local/` does not exist in this checkout.

| Subsystem | Harnesses (`tools/`) → oracle (`mods/horizon/src/test/java/dev/horizonlab/parity/`) → probe |
|---|---|
| RNG | `test_worldgen_random_parity.py` → VanillaWorldgenRandomOracle; `test_value_provider_parity.py`, `test_height_provider_parity.py`, `test_anchor_parity.py` |
| Noise | `test_noise_parity.py` → VanillaNoiseOracle (horizon-core probe) |
| Density | `test_density_parity.py` → VanillaDensityOracle; `test_structure_density_parity.py` → VanillaStructureDensityOracle (beardifier) |
| Codecs | `test_worldgen_parity.py` → VanillaWorldgenOracle (every noise/density JSON, Gson number edge cases) |
| Biome/climate | `test_biome_source_parity.py`, `test_biome_source_codec_parity.py`, `test_biome_generation_parity.py`, `test_biome_chunk_parity.py`, `test_biome_zoom_parity.py`, `test_biome_temperature_parity.py`, `test_biome_placement_parity.py` |
| Terrain + aquifer | `test_terrain_parity.py` → VanillaTerrainOracle (+VanillaHeightmapOracle); `test_heightmap_parity.py`, `test_bulk_section_parity.py` |
| Surface rules | `test_material_condition_parity.py`, `test_material_rule_library_parity.py`, `test_material_stage_parity.py`, `test_material_surface_parity.py`, `test_material_extension_parity.py` |
| Carvers | `test_carving_geometry_parity.py`, `test_carving_mask_parity.py`, `test_cave_carver_parity.py`, `test_canyon_carver_parity.py` |
| Features/placement | `test_placement_parity.py`, `test_feature_placement_parity.py`, `test_feature_graph_parity.py`, `test_feature_catalog_parity.py`, `test_feature_region_parity.py`, `test_feature_sort_parity.py`, `test_decoration_region_parity.py`, `test_basic_feature_parity.py`, `test_scattered_ore_parity.py`, `test_replace_block_parity.py`, `test_fill_layer_parity.py`, `test_rule_test_parity.py`, `test_state_provider_parity.py`, `test_spring_parity.py`, `test_end_gateway_parity.py`, `test_end_podium_parity.py`, `test_sculk_parity.py` (gameplay), block facts: `test_simple_block_parity.py`, `test_block_survival_parity.py`, `test_block_support_parity.py` |
| Trees | `test_tree_feature_parity.py` → VanillaTreeFeatureOracle (8,898 fixtures, all equal per docs) |
| Structures | `test_structure_start_nbt_parity.py` → VanillaStructureStartOracle; structure pieces via `test_chunk_generation_parity.py --phase structures` |
| Proto-chunk ticks | `test_generation_tick_parity.py`, `test_retrogen_chunk_parity.py`, `test_chunk_stage_parity.py` |
| **Whole chunks** | `test_chunk_generation_parity.py` → VanillaChunkGenerationOracle (real `ChunkGenerator` stages through real `WorldGenRegion`s) vs `HW/../examples/probe/chunk_dump.rs`; `test_decorated_region_parity.py` (13×13 regions decorated in a fixed order); `HORIZON_ORDER` file fixes decoration order on both sides |
| World creation | `test_server_world_gen_parity.py` (level-type/generator-settings from server.properties), `test_world_spawn_parity.py` |

Recorded status (`docs/world-generation.md` §"Whole-chunk differential harness"): seed 12345, 42 overworld sites /
372 chunks plus Nether and End: structure pieces 0 differences on all 35 structure sites; full generation **4 differing
chunks (3 blocks, 1 block entity)**; Nether/End terrain 0 differences; decorated regions 21 sites / 3,549 chunks, 0
differences. **Known remaining differences**: "a handful of single blocks near structure pieces in full generation
(abandoned camps, shipwreck, villages) that need per-feature tracing". Harness normalisations: sections above the
noise height absent in Horizon (all air in vanilla), template NBT integer width, `DUMMY` block entities, stray
`LootTableSeed`. Decoration is order dependent (`docs/world-generation.md` "Decoration order"; `W/decoration_order.rs`
models *vanilla's* scheduler).

Stale/incomplete notes: `docs/rust-parity.md:6-300` and the headers of `W/../worldgen.rs:1-2`, `HC/density/mod.rs:1-4`,
`W/terrain.rs:1-2` still say "complete terrain generation remains pending" (superseded by the whole-chunk harness);
`docs/feature-placement.md:306-310` notes ore-vein double trig not validated (code now uses `jmath::sin/cos`,
`W/ore_feature.rs:158`). `W/structure/start.rs:100,133,378-414,1204` returns `Unsupported` for unknown structure
types/placements (vanilla data has none); `W/generator_decoration.rs:256,395` counts unsupported features
(`unsupported_features()`) — the server should treat any as a load error. Not implemented anywhere in this tree:
**FlatLevelSource (Superflat) and DebugLevelSource** (the old `horizon-server-core::flatgen` named in CLAUDE.md is
gone), **old-world blending** (`Blender`, not needed: 26.3 worlds only), **lighting** (separate Starlight port).
`docs/known-issues.md` has nothing on generation.

---

## 5. Leaf / Paper / Moonrise deltas

### Changes to generated output (with defaults)

| Delta | Source | Default effect | Port |
|---|---|---|---|
| Bedrock floor/roof rules use `paper:optionally_flat_bedrock_material_condition` | `PAPER/resources/data/minecraft/worldgen/material_rule/bedrock_{floor,roof}.json.patch`; `paper-server/src/main/java/io/papermc/paper/world/worldgen/OptionallyFlatBedrockCondition.java`; registered in `PAPER/sources/net/minecraft/server/Bootstrap.java.patch:16`; needs `WorldGenerationContext.level()` (`PAPER/sources/.../levelgen/WorldGenerationContext.java.patch`, `NoiseBasedChunkGenerator.java.patch` @@ -220, -241; `placement/PlacementContext.java.patch`) | With `environment.generate-flat-bedrock: false` identical to vanilla `VerticalGradientCondition` (same random factory, `Mth.map`, `nextFloat`); when true, bedrock is flat at `min(trueAt,falseAt)` (floor) / `max(..)-1` (roof) | Decode the type in `material_condition/codec.rs`; flag from paper world config |
| Structure-set salts from spigot.yml `seed-*` | `PAPER/sources/.../chunk/ChunkGeneratorStructureState.java.patch` (`injectSpigot`, `KeyedRandomSpreadStructurePlacement`, stronghold `strongholdSeed`), `ChunkGenerator.java.patch` @@ -116 / -527, `structure/placement/{StructurePlacement,AbstractSpreadingStructurePlacement,DimensionOriginStructurePlacement}.java.patch`, `structure/StructureCheck.java.patch` (`getSaltOverride` for mineshaft/buried treasure) | Defaults (`SpigotWorldConfig.initWorldGenSeeds`) equal the 26.3 jar's salts for all 19 `minecraft:` random-spread sets (checked against `worldgen/structure_set/*.json`); `seed-mineshaft`/`seed-stronghold` default "default" = no override; buried treasure 10387320 = `HIGHLY_ARBITRARY_RANDOM_SALT` | Salt table from config, only for `minecraft:` sets; override paths in the three frequency reducers exactly as patched (`probabilityReducer`/`legacyPillagerOutpostReducer` ignore it) |
| Configurable feature seeds | `PAPER/sources/.../chunk/ChunkGenerator.java.patch` @@ -424 (`featureSeeds.features.getLong(feature)`, `setDecorationSeed(configSeed, x, z)`) | Default map empty (`-1`), `generate-random-seeds-for-all: false` → vanilla | Per-configured-feature override in the FEATURES step |
| Paper: clear block entity before a `DUMMY` one | `PAPER/sources/net/minecraft/server/level/WorldGenRegion.java.patch` (setBlock: remove BE if the block changes) and `UPDATE_SKIP_POI` flag | **Always on**: changes block entities vs vanilla when a feature/structure replaces a BE block with a different BE block in a proto chunk (PaperMC/Paper#10750) | Implement in the region view's `set_block` (`W/generator_decoration.rs` `World`) |
| Purpur end void rings | `LEAF/0001-Purpur-Server-Minecraft-Changes.patch` (`EndIslandFunction`, `doffs`) | `settings.generate-end-void-rings: false` → vanilla | One branch in `HC/density/end_islands.rs` |
| Leaf FastRNG for worldgen | `LEAF/0112-Faster-random-generator.patch` (RandomState, WorldgenRandom, Biome noises, structure placement, Geode, noise state providers, OceanMonument, NoiseBasedChunkGenerator) | `performance.faster-random-generator.enabled: false`, `enable-for-worldgen: false` → vanilla (`worldgen = enableForWorldgen && enabled`) | Out of scope initially (changes world output; Xoroshiro-backed `FasterRandomSource`); keep a config hook |
| MC-152094 End city ship bounds | `LEAF/0144-Fix-MC-152094.patch` (ThreadLocal `TOWER_BOUNDS`, `addShipWithinReferenceBounds`) | `fixes.mc-152094: false` → vanilla | Optional; pass bounds explicitly instead of the ThreadLocal |
| Secure seed (Matter) | `LEAF/0110-Matter-Secure-Seed.patch`, `0111` (16 files: ChunkGenerator, structure placement, Geode, Jigsaw, EndCity, Mineshaft, WorldOptions…) | `SecureSeed.enabled = false` | **Left out** (ARCHITECTURE scope; `S/server/server_properties.rs:11` already drops `feature-level-seed`) |
| Bukkit generator/populator/events (`CustomChunkGenerator`, `BlockPopulator`, `AsyncStructureSpawnEvent`, `StructuresLocateEvent`) | `NoiseBasedChunkGenerator.java.patch` @@ -365, `ChunkGenerator.java.patch` @@ -160/-452/-623 | No plugins → vanilla | Note event points only (API deferred) |
| CraftBukkit spawner/loot hooks in structure pieces (`placeCraftSpawner`, `setCraftLootTable`) | `PAPER/sources/.../structure/StructurePiece.java.patch:42-72`, Stronghold/Mineshaft/Fortress/Igloo/Shipwreck/EndCity/OceanRuin/SwampHut/DesertPyramid patches | Output-equivalent: `getOrCreateNextSpawnData` with empty `spawnPotentials` draws nothing; loot seed still `random.nextLong()` | Port vanilla behaviour |
| Paper locate perf + border | `ChunkGenerator.java.patch` @@ -264 (`locateStructuresOutsideWorldBorder`), @@ -295 (ring iteration visits the same positions in the same order) | `environment.locate-structures-outside-world-border: false` (`WorldConfiguration.java:449`) → Paper skips ring/spread candidates outside the world border (differs from vanilla only near a shrunk border); the edge-only loop is equivalent | `/locate`, explorer maps, eyes of ender only |

### Optimisations that do not change results (port the behaviour; optimise natively)

- Moonrise (`PAPER/features/0003-Moonrise-optimisation-patches.patch`): Beardifier arrays and per-case switch (lines
  36828-36938; identical arithmetic), Heightmap raw `char[]` mirror (36938-37001), thread-local density buffer arenas
  (`chunk_gen/buffer_alloc/*`, NoiseChunk 37014, ChunkGenerator 34577, MaterialRuleContext/MaterialSystem 37112-37222),
  PerlinNoise/SmearedPerlin/GradientNoise rewrites (37367-37860), `BiomeManager.getFiddle` (33604: `((r>>24)&1023)-512)*(0.9/1024)`
  — bit-identical to vanilla), Biome temperature cache removal (33578), `possibleBiomes` via `forEachInPalette` (34577 @@ -387,
  set membership only), StructureCheck thread-local climate sampler + bounded synchronised maps (37222), AllOf/AnyOf
  RuleTest arrays (37323-37367), hash palettes everywhere.
- Leaf: `0049` cached sea level, `0007` positive modulo in `Blender` (only `-0.0` vs `0.0`; blending not ported),
  `0087` Carpet-Fixes `getBiome`, `0218` cached biome zoom seed, `0156` C2ME OreFeature allocations, `0158` Lithium
  cached iterate-outwards (DeltaFeature, ReplaceBlobs: same order), `0222` Paw (`Direction.VALUES`, DesertPyramid
  list copy), `0234` Lithium combined heightmap update, `0252` Pluto plane faces, `0211` structure map, `0318` Cleanup
  (profiling, gametest template sources), `0097` faster future sequencing for ring positions (on by default; list
  order kept — joins in order).

### Threading / scheduling (Moonrise chunk system)

- 26.3 pyramid (vanilla `ChunkPyramid.GENERATION_PYRAMID`): EMPTY → STRUCTURE_STARTS → STRUCTURE_REFERENCES (starts r8)
  → BIOMES (starts r8) → TERRAIN (starts r8, biomes r1, write r0; noise+surface+carvers) → FEATURES (starts r8,
  terrain r1, write r1) → INITIALIZE_LIGHT → LIGHT → SPAWN (biomes r1) → FULL. Moonrise drops LIGHT's neighbour
  requirement (`0003` @@ ChunkPyramid.java:40, "starlight does not need neighbours").
- `ChunkTaskScheduler` (0003, lines 9122-9190): write radii (FEATURES 1, LIGHT 2, others 0); **parallel-capable**:
  EMPTY, STRUCTURE_STARTS, STRUCTURE_REFERENCES, BIOMES, TERRAIN, INITIALIZE_LIGHT — run on `parallelGenExecutor`;
  FEATURES and SPAWN go through `radiusAwareScheduler = AreaDependentQueue(parallelGenExecutor, 4)` (tasks whose
  write areas do not overlap run concurrently); LIGHT on the light executor; FULL on the main thread. Empty-load
  statuses (EMPTY, STRUCTURE_REFERENCES, BIOMES, TERRAIN, FEATURES, SPAWN) decide load-vs-generate. Priorities
  come from tickets (`PriorityHolder`, `ChunkHolderManager`).
- `NoiseBasedChunkGenerator.fillFromNoise`/`createBiomes` run inline (`Runnable::run`) instead of Mojang's
  background executors (0003 @@ 37001, 34577).
- Consequence: Leaf's **decoration order is Moonrise's** (priority + area-dependent queue), not the vanilla
  scheduler modelled by `W/decoration_order.rs`. Both are non-deterministic across runs; parity must fix the order
  (`HORIZON_ORDER`) on both sides, and the server's scheduler should follow Moonrise's priority rules, not
  `decoration_order.rs`.
- Concurrency hazards the server must reproduce safely: concurrent FEATURES on chunks sharing a structure start
  (piece flags, §3 D); StructureCheck caches (make them per-level, owned by the scheduler or sharded with a stated reason).
- Leaf `0176` SparklyPaper parallel world ticking: one tick thread per level; generation is already per level.

---

## 6. Work packages

Target layout (ARCHITECTURE "Layout": `util` gets noise; `world` gets generation): `S/util/noise/`,
`S/world/gen/{data, density, biome, terrain, aquifer, surface, carver, feature/, placement/, tree/, structure/, pipeline}`.
Each package owns its directory; shared types are introduced by WP1 only. Dependency order:

```
WP1 ─┬─> WP2 ──> WP3 ─┐
     ├─> WP4 ─────────┼─> WP6
     └─> WP5 ─────────┘
```
WP2, WP4, WP5 run in parallel after WP1; WP3 starts when WP2's `NoiseChunk`/aquifer API is merged (or against
WP1's trait stubs); WP6 integrates.

**Verification method for all packages** (`.local/` is absent here):
1. Preferred: `python3 tools/lab.py prepare` to populate `.local/` (needs Mojang downloads and JDK 25), then run the
   listed harnesses with a server probe. Add a server probe example, `crates/horizon-server/examples/gen_probe.rs`,
   that speaks the same stdin/stdout protocols as `crates/horizon-world/examples/probe/<name>_fixture.rs` (copy the
   fixture modules alongside each package), and a switch in `tools/test_rust_parity.py` / each harness's build line
   (e.g. `HORIZON_PROBE=horizon-server:gen_probe`, today hard-coded at `test_chunk_generation_parity.py:74`).
2. Offline (works now): compile and run the Java oracle with the Leaf cache jar —
   `/usr/lib/jvm/java-25-openjdk-amd64/bin/java -cp /home/user/Leaf/leaf-server/.gradle/caches/paperweight/data/bundler/server.jar:$(find /home/user/Leaf/leaf-server/.gradle/caches/paperweight/jars/minecraft -name '*.jar' | paste -sd:) <Oracle>.java <out>`
   — and check the output in an `#[ignore]` Rust test reading `HORIZON_ORACLE_*` (the pattern of
   `mods/horizon/src/test/java/dev/horizonlab/leafserver/LeafConfigurationOracle.java` and
   `S/world/chunk/section_tests.rs`). Oracles needing Paper behaviour (flat bedrock on, spigot salts) get a Leaf-side
   variant in `mods/horizon/src/test/java/dev/horizonlab/leafserver/`.

### WP1 — Foundations: noise, density program, codecs, worldgen data, proto-chunk model (~11k lines)

- **Copy**: `HC/noise/*` (1,572) → `S/util/noise/`; `HC/density/*` (6,395, tests included) → `S/world/gen/density/`;
  `HW/worldgen.rs` helpers (299), `W/{density.rs 578, router.rs 168, settings.rs, weighted.rs, value_provider.rs,
  height_provider.rs, anchor.rs, long_set.rs, write_zone.rs}`; `W/tags.rs` only where the server's
  `S/registry/tags.rs` lacks name-level resolution; `HW/heightmap.rs` (249) merged into `S/world/chunk/serial.rs`
  `Heightmap`/`S/world/level_chunk.rs` `HeightmapTable`; proto-chunk storage: `W/terrain.rs`'s `TerrainChunk`
  types, `W/feature_region.rs` (661), `W/bulk_section.rs` (113), `W/biome_chunk.rs` (202) rebuilt on
  `S/world/chunk/{section,paletted}.rs` (not `HW/chunk.rs`'s `BlockSection`).
- **Adapt**: `horizon_core::{random,jmath,geometry,position}` → `crate::util::random`, `crate::util::jmath`,
  `crate::core::{geometry,position}`; `BlockRegistry/BlockStateId` → `crate::block::{BlockStates, BlockState}`;
  `crate::nbt` → `S/nbt.rs`. Data layer: extend `S/registry/mod.rs` `DataPack::vanilla` (line 168) with the worldgen
  registry dirs and `tags/worldgen/*`; new `S/world/gen/data.rs` `WorldgenData::load(&Registries, &BlockStates,
  &BlockStateData, reports_dir, &mut VanillaJar)` replacing `GeneratorData::load` (no files under `.local`); add
  Paper's bedrock rules to `DataPack::paper()`; dimension heights from the synced `dimension_type` entries.
- **Java-shape fixes**: density compile to an arena `DensityProgram` (§3 F); eager noise instantiation
  (`graph/seeded.rs:22`); `beardifier.rs:323` kernel stays a `LazyLock` constant; no `&'static` registry.
- **Verify**: `test_noise_parity.py` (VanillaNoiseOracle), `test_density_parity.py` (VanillaDensityOracle),
  `test_worldgen_parity.py` (VanillaWorldgenOracle), `test_worldgen_random_parity.py`, `test_value_provider_parity.py`,
  `test_height_provider_parity.py`, `test_anchor_parity.py`, `test_heightmap_parity.py`, `test_bulk_section_parity.py`;
  copy the `HC/density/tests.rs` and `graph/tests.rs` unit tests; a benchmark of the program vs the trait-object tree.

### WP2 — Biomes, climate, terrain fill, aquifer (BIOMES + noise part of TERRAIN) (~5k lines)

- **Copy**: `W/{biome.rs, biome_predicate.rs, biome_source.rs + biome_source/*, climate.rs + climate/*, spawn_finder.rs,
  terrain.rs (fill), aquifer.rs + aquifer/*}` (≈3.1k), `HC/biome.rs` + `HC/biome/seed_hash.rs` (293, BiomeManager
  zoom), `HC/density/end_islands.rs` already in WP1 (add Purpur's void-rings flag here).
- **Adapt**: biome ids = synced `worldgen/biome` registry order (protocol ids), presets from
  `reports/biome_parameters/minecraft/*.json` (`quantize_parameter_report`, `W/generator.rs:237`); `RandomState`
  (per level, legacy vs xoroshiro from `noise_settings.legacy_random_source`).
- **Java-shape fixes**: biome cache/`ChunkWork` (`W/generator.rs:746-778`) not carried over: BIOMES writes the
  proto chunk; TERRAIN takes neighbour biomes as job input; spawn origin computed once per level.
- **Verify**: `test_biome_source_parity.py`, `test_biome_source_codec_parity.py`, `test_biome_generation_parity.py`,
  `test_biome_chunk_parity.py`, `test_biome_zoom_parity.py`, `test_terrain_parity.py` (VanillaTerrainOracle: final
  density, aquifers, fluid post-processing, WG heightmaps), `test_chunk_generation_parity.py --phase terrain` once WP3
  lands, `test_world_spawn_parity.py`.

### WP3 — Surface rules and carvers (rest of TERRAIN) (~5k lines)

- **Copy**: `W/material_condition.rs` + `material_condition/*` (2,304), `W/{material_extensions, material_stage,
  material_surface, biome_temperature, biome_weather}.rs`, `W/{carver.rs, carver/*, carving.rs}` (746), `HC/carving.rs`
  + `HC/carving/ellipsoid.rs` (288).
- **Adapt**: add `paper:optionally_flat_bedrock_material_condition` (OptionallyFlatBedrockCondition semantics, flag
  from paper world config `environment.generate-flat-bedrock`; the level reaches the rule context explicitly, not via
  `WorldGenerationContext.level()`); carver configs from the `worldgen/carver` registry; uncarvable tag from block tags.
- **Java-shape fixes**: `biome_temperature.rs:37` static noises → `LazyLock` constant or `WorldgenData` field; carver
  biome cache (`generator.rs:746`) dropped (biomes come with the job).
- **Verify**: `test_material_condition_parity.py`, `test_material_rule_library_parity.py`,
  `test_material_stage_parity.py`, `test_material_surface_parity.py`, `test_material_extension_parity.py`,
  `test_biome_temperature_parity.py`, `test_carving_geometry_parity.py`, `test_carving_mask_parity.py`,
  `test_cave_carver_parity.py`, `test_canyon_carver_parity.py`, then `test_chunk_generation_parity.py --phase terrain`;
  plus a Leaf-side oracle run with flat bedrock on.

### WP4 — Features, placement, trees (FEATURES decoration, minus structures) (~17k lines; split 4a/4b if two agents)

- **Copy (4a, infra + block facts)**: `W/{feature.rs, feature_catalog.rs, feature_graph.rs, feature_sort.rs,
  decoration.rs, placement.rs + placement/*, block_predicate.rs, fluid_predicate.rs, rule_test.rs (without
  `horizon:matter_phase`), state_provider.rs + state_provider/*, block_feature.rs}` (≈5.1k) and the block-fact layer
  `HW/{block_survival.rs 1,220, block_placement.rs 376}` re-based on `S/block/flags.rs` (collision, support,
  properties come from `block_states.bin`; survival/placement rules need the new exporter fields, §2).
- **Copy (4b, implementations)**: the 29 feature files listed in §1 (7.8k incl. `cave.rs`, `projected.rs`) and
  `W/tree.rs + tree/* + tree_blocks.rs` (3.6k); `live.rs` (333) only when gameplay needs saplings/bone meal.
- **Adapt**: feature registry decoded eagerly from the jar (`worldgen/feature`, `worldgen/placed_feature`,
  `tags/worldgen/feature`); Paper feature-seed override hook (consumed in WP6); `PlacementContext::update_shape_at_face`
  / `block_survives` stay as the boundary trait (`W/placement/context.rs:34`); its implementation comes from the
  server's block behaviour (initially a WP4-owned `S/world/gen/block_rules.rs` fed by the exported survival rules
  and `VanillaBlockBehaviourExporter` facts, to be replaced by the block-behaviour port).
- **Java-shape fixes**: `generator_decoration.rs:73` / `live.rs:26` lazy caches → eager; `state_provider.rs:38-41`,
  `block_feature.rs:33` `OnceLock`s → computed at decode; `PosSet` → narrow Java-HashSet-order helper (§3);
  `feature_graph.rs`/`placement/chain.rs` `Cell` counters → `&mut usize` (optional).
- **Verify**: `test_placement_parity.py`, `test_feature_placement_parity.py`, `test_feature_graph_parity.py`,
  `test_feature_catalog_parity.py`, `test_feature_sort_parity.py`, `test_feature_region_parity.py`,
  `test_basic_feature_parity.py`, `test_scattered_ore_parity.py`, `test_replace_block_parity.py`,
  `test_fill_layer_parity.py`, `test_rule_test_parity.py`, `test_state_provider_parity.py`, `test_spring_parity.py`,
  `test_end_gateway_parity.py`, `test_end_podium_parity.py`, `test_tree_feature_parity.py` (VanillaTreeFeatureOracle),
  `test_simple_block_parity.py`, `test_block_survival_parity.py`, `test_block_support_parity.py`,
  `test_biome_placement_parity.py`.

### WP5 — Structures (STRUCTURE_STARTS, STRUCTURE_REFERENCES, piece placement) (~15k lines, vanilla only)

- **Copy**: `W/structure.rs` and `W/structure/{start, jigsaw, template, processor, legacy, simple, temple, mineshaft,
  stronghold, fortress, monument, mansion, end_city, ocean_ruin, ruined_portal, nether_fossil, features}.rs`
  (15.1k). **Do not copy** `formations.rs`, `piece_limited.rs`, `start_checks.rs`, `enhanced_terrain.rs`,
  `processor/{litho,pillars,sea_level}.rs` (4.5k, modpack-only) nor the `"horizon:*"` keys (`jigsaw.rs:56-172`,
  `start.rs:60-110`, `legacy.rs:798` `horizon:finalize` marker → a typed field).
- **Adapt**: structure/structure_set/template_pool/processor_list registries from the jar; templates from the jar
  plus the world's `generated/` dir; spigot salt table + `KeyedRandomSpread` overrides (mineshaft/buried treasure
  `saltOverride`, stronghold `strongholdSeed`) per §5; block transforms (`StateTransforms`) from the new
  `structure-blocks` export; structure start NBT read/write into `S/world/chunk/serial.rs` (today raw) matching
  `StructureStart.createTag`; beardifier inputs (pieces/junctions) to WP1's density program; generated entities as
  proto-chunk entity NBT (replaces `StructureMobs`).
- **Java-shape fixes**: piece mutable fields → `OnceLock` (set-once) / `AtomicBool` (flags) (§3 D, all sites listed);
  `Rc<RefCell<Free>>` → arena; `Pools`/`templates` `RwLock` caches → eager immutable maps; `processor.rs:881`
  thread-local depth → parameter; ring positions (`generator_structures.rs:39-72`) → one per-level job.
- **Verify**: `test_structure_start_nbt_parity.py` (VanillaStructureStartOracle), `test_structure_density_parity.py`,
  `test_chunk_generation_parity.py --phase structures` (0 differences is the recorded baseline), a Leaf-side oracle
  for spigot salt overrides and (optional) MC-152094.

### WP6 — Generator driver, status pipeline, level integration (~6k lines + new scheduler code)

- **Copy**: `W/generator.rs` (pipeline parts only, ~1.8k of 2.9k; the rest is loading/caches replaced by WP1-WP5),
  `W/generator_decoration.rs` (1,229), `W/generator_structures.rs` (618, minus caches), `W/generator_blocks.rs` (168,
  mostly replaced by `block_states.bin`), proto-chunk tick capture (`HW/level/ticks.rs` `CapturedTick` subset),
  `HW/retrogen.rs` only if worlds with `below_zero_retrogen` must load; `crates/horizon-world/examples/probe/chunk_dump.rs`
  (319) → server probe.
- **New**: Moonrise-shaped scheduler as plain data on the tick thread (§5): status pyramid with the 26.3 requirements,
  write radii, parallel-capable statuses on the worker pool with owned `ProtoChunk` + read-only neighbour snapshots,
  FEATURES/SPAWN through an area-dependent queue (non-overlapping 3×3 write areas run concurrently), FULL on the tick
  thread; load-vs-generate per status from `S/world/chunk_io.rs`; results back as messages. `LevelGenerator` enum
  (legacy/xoroshiro) per level, built from `WorldGenSettings` stems; FlatLevelSource and DebugLevelSource (not in
  Horizon; write from vanilla). Paper deltas: `WorldGenRegion.setBlock` BE clearing and `UPDATE_SKIP_POI`, feature
  seeds, `generate-structures` per level (`level.worldGenSettings`), duplicate-UUID handling of proto-chunk entities.
- **Java-shape fixes**: all class-C sites (`generator.rs:746-778`, `generator_structures.rs:35-140`, `:190-199`);
  `Arc<dyn SeededPipeline>` → enum; `StructureMobs` callback removed; `Box::leak` in tests removed.
- **Verify**: `test_chunk_generation_parity.py` (full phase, all three dimensions; baseline: 4 differing chunks of 372
  on seed 12345), `test_decorated_region_parity.py` with `HORIZON_ORDER`, `test_generation_tick_parity.py`,
  `test_chunk_stage_parity.py`, `test_server_world_gen_parity.py`; region files written by the server opened by the
  real Leaf/vanilla 26.3 server; `chunkgen_bench`-style throughput benchmark.

### Size summary

| WP | Copied (from Horizon) | Expected after adaptation |
|---|---|---|
| WP1 | ~10.5k (incl. 1.5k tests) | ~10k + data layer (~0.8k) |
| WP2 | ~3.4k | ~3.5k |
| WP3 | ~4.4k | ~4.5k |
| WP4 | ~16.5k (+1.6k block rules) | ~17k |
| WP5 | ~15.1k | ~14.5k |
| WP6 | ~4k + 0.3k probe | ~5k + scheduler ~2k |
| Dropped | 4.5k modpack structures, `"horizon:*"` branches, `HW/java_hash_map.rs` (791) | — |
