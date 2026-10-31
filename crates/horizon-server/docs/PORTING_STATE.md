# Leaf 26.3 → Rust: porting state (read this instead of the whole repo)

Short, current. Rules: `../PORTING_RULES.md`. Design: `../ARCHITECTURE.md`. Generator plan: `worldgen-plan.md`.
Real-data checks: `tools/leaf_server/README.md` (`setup_reference.sh` → `realdata.env`).

## Decisions (user)
- One crate `crates/horizon-server`, no dependency on other Horizon crates; `#![forbid(unsafe_code)]`.
- World generator **copied** from `crates/horizon-world/src/worldgen` + `crates/horizon-core/src` and adapted
  (2026-10-31; accepted exception to rule 16). Drop Horizon's modpack `horizon:*` code.
- Cut vs Leaf: no FastRNG (vanilla random sources only), `white-list` default false, no ban lists, no EULA check, no `custom_enchantments` pack, `hex_color` syntax error for non-hex, no
  packet-rate kick (own inbound byte budget), no plugins/Bukkit API yet, no datafixers, no secure seed,
  no status/Query/RCON/JSON-RPC/JFR/Sentry/spark.
- Agents: 2-3 per wave, narrow briefs; Sonnet for routine porting/tests, Opus for ownership/hard design.

## Reference paths
- Final sources (vanilla + Paper + Leaf applied, Moonrise incl.): `../Leaf/leaf-server/src/minecraft/java/`;
  Leaf's own: `../Leaf/leaf-server/src/main/java`. Client: `../Client`. Proxy: `../Velocity`.
- 26.3 jar: `../Leaf/leaf-server/.gradle/caches/paperweight/data/bundler/server.jar` (sha256 a362163e…8ae6),
  libs `.../paperweight/jars/minecraft/`. Java 25 (macOS): `/Library/Java/JavaVirtualMachines/zulu-25.jdk`.
- Oracles: `mods/horizon/src/test/java/dev/horizonlab/leafserver/Leaf*.java` + env-gated Rust tests.

## Done (verified against real 26.3 classes unless noted)
- Commands (Brigadier core, 14 argument types), configs (properties, Paper/Spigot/Purpur/Leaf YAML),
  tick loop/`/tick`, runtime (tokio net + tick thread), network to play state with Velocity forwarding,
  registries + tags + configuration, text components, block state table + flags/shapes, paletted
  containers/sections/chunk NBT, level storage + region I/O, join path (players see saved chunks).
- Worldgen WP1 (`src/worldgen/`): noise, density program (arena enum), worldgen registries, proto chunk.
  Bit-equal on 774,162 values (`LeafDensityOracle`); as fast as Horizon's `Arc<dyn>` tree on volumes
  (572 µs/chunk), ~9% faster on points.
- Worldgen WP2 slice (`src/worldgen/{climate,biome}`): climate parameters + Moonrise R-tree search,
  multi-noise/fixed/checkerboard/end biome sources, BiomeManager zoom (Leaf's 0087 order), BIOMES step
  (Paper's fill order), NoiseSpawnFinder. `LeafBiomeOracle`: 2 seeds x 5 stems, ~109k rows, all equal.
- Worldgen WP2 terrain (`src/worldgen/{aquifer,terrain}`): `NoiseChunk` fill + aquifer onto the proto chunk, WG
  heightmaps, fluid marks, base column/height. `LeafTerrainOracle`: 224 chunks (15.2M blocks) + 84 columns, all
  equal; 3.5 ms per overworld chunk (release).
- Worldgen WP3 first part (`src/worldgen/surface`): the `material_rule`/`material_condition` registries decoded into
  arenas (with Paper's `optionally_flat_bedrock_material_condition`; its bedrock rules come in the `vanilla` pack),
  `SurfaceSystem::build_surface` (`NoiseBasedChunkGenerator.buildSurface`: rule per block, clay bands, ore veins,
  eroded badlands pillars, frozen ocean icebergs) and biome temperatures (`BiomeClimates`, in `WorldgenData`).
  `LeafSurfaceOracle`: 312 chunks (23.9M blocks, 4.0M changed by the surface; 7 noise settings x 2 seeds over the stems'
  real biome sources, plus chunks of 35 biomes and 16 of the eroded badlands) and 4,154 temperatures, all equal;
  4.6 ms per overworld chunk (release). Flat bedrock (`environment.generate-flat-bedrock`, read by
  `config::load_generate_flat_bedrock`) cannot run on the vanilla jar: `flat_bedrock_is_one_layer` checks Paper's
  arithmetic through the real rules.
- Worldgen WP3 rest (`src/worldgen/carver`): `worldgen/carver` registry + each biome's `carvers` (id, `#tag`, inline or
  list; `WorldgenData::carvers`) and `#minecraft:uncarvable`, `FloatProvider`, `CarvingMask`, `cave` (also `nether_cave`,
  `cave_extra_underground`) and `canyon` carvers, and `CarverStep::carve` = `generateCarvers` + `applyCarvingMask` (17 x 17
  sources, biome at the source's minimum quart `y=0` from the raw source, `setLargeFeatureSeed(seed + index)`, the fill's
  own aquifer via `NoiseTerrain::fill_with_aquifer`, `SurfaceSystem::top_material` under carved grass/mycelium). 26.3 has
  no separate nether carver class. `LeafCarverOracle`: 224 chunks (overworld, large biomes, amplified, Nether x 2 seeds; 205
  carved, 54,365 mask runs, 18.4M blocks, 214,515 changed by the carvers, 15,961 fluid marks), all equal; 2.0 ms per chunk
  (release, incl. the 289 biome reads). Not ported: old-world blending's carving filter and upgrading chunks (mask to the
  top). Not done: `ProtoChunk.setBlockState`'s light-source list (lava etc.) comes with the light work (WP6).
- Worldgen WP5 slice (`src/worldgen/structure`): structure/structure_set registries + tags, random spread,
  frequency reducers, exclusion zones, ring geometry (biome search as a closure), Spigot `seed-*` salts
  (derived from sources), `createStructures` choice, StructureStart/references NBT in Leaf's orders.
  `LeafStructurePlacementOracle`: 97,412 answers, all equal. Matches Leaf (user, 2026-10-31): a malformed
  inner start id fails the chunk load, inline structures/sets decode, dotted `spigot.yml` world names nest.
- Worldgen WP5 templates (`src/worldgen/structure/{template,place,processor}.rs`, `src/block/transform.rs`): `StructureTemplates`
  decodes every template at startup (world `generated/<ns>/structure/` first, then the jar's `data/<ns>/structure/`; a broken
  file falls back to the next source; 1,511 in ~0.2 s release; no datafixer, `.snbt` or `BukkitValues`), palettes in vanilla's
  order, jigsaws parsed at load. `PlaceSettings` + `place_in_world`/`process_block_infos`/transforms/bounding box over
  `FeatureWorld` (barrier-first NBT blocks, waterlogging, the edge walk via `update_shape_at_face`); block entity NBT (with
  `LootTableSeed`) and entity NBT are returned in `PlacedTemplate` until those systems exist; `updateFromNeighbourShapes` is
  WP6. `worldgen/processor_list` (`WorldgenData::processor_lists`) and all processor types as an enum; the settings random is
  shared or per-position (no separate one for `/place` yet). **`block_states.bin` v3** (rotate/mirror/waterlog columns,
  has-block-entity, randomizable-container, full-outline, liquid-container flags; v2 rejected: rerun `setup_reference.sh`).
  `LeafTemplateOracle`: 117 templates, 1,404 placements (40 vanilla + 9 built-in lists, rotations x mirrors, chunk boxes,
  waterlogging), 735,850 `setBlock` calls plus ticks, loot seeds, entity positions and random draws, all equal. The edge walk
  duplicates the tree feature's face traversal (its `Shape` is private to `feature/`): move it to a shared place.
- Worldgen WP4a first part (`src/worldgen/{feature,placement}`): `worldgen/feature` (type + raw JSON) and
  `placed_feature` registries, tags, each biome's features per step, the feature sorter, placement
  modifiers (all 18), block predicates, int/height providers, `FeaturePlacer` over a narrow `PlacementWorld`
  (`RegionWorld` adapts `GenerationRegion`). `LeafPlacementOracle`: 9,100 rows (8,010 chains over a synthetic
  world, 273 vanilla placed features), all equal. `would_survive` is the block-behaviour boundary
  (`SurvivalRules`; the oracle replaces it by `true`). Feature implementations, state providers, rule tests,
  block survival rules and the decoration driver are still WP4.
- Worldgen WP4b first part (`src/block/survival.rs`, `src/worldgen/{state_provider,feature/{simple,rule_test}}`):
  `block_states.bin` version 2 carries each block's `canSurvive` rule (the exporter maps the declaring class, as
  Horizon's prepare step did) and flammable / fence gate / double plant / mossy carpet flags; `BlockSurvival`
  evaluates all 50 rules over the state tables and the data-pack tags (`RegionWorld` + `BlockSurvivalRules` give
  placement the real `would_survive`; light is 0 as in Paper's `WorldGenRegion`). State providers (all 10 types,
  named ones inlined) and the features `simple_block`, `scattered_ore`, `disk`, `block_column` are decoded with the
  registries (`ConfiguredFeature::feature`) and placed through `FeatureWorld` (`RegionWorld` writes the region).
  `LeafFeatureOracle`: 35,723 states x 40 neighbourhoods (1,428,920 `canSurvive` cases), 720 provider rows, 1,200
  feature rows over 50 vanilla features, 4 feature-order cycle messages, all equal; `placement_matches_java` now runs
  the real survival rules (8,010 rows). 26.3's block state JSON is `{"id","properties"}`, not `Name`/`Properties`
  (fixed in `DecodeContext::block_state` and `settings.rs`). Feature-order cycles name their involved sources as
  `FeatureSorter.tryReducingError` does (`FeatureCycle::message`; Java prints a `Biome@<identity hash>`; we print the plain biome id, the user's decision).
- Worldgen WP4 `ore` (`src/worldgen/feature/ore.rs`): `OreFeature` + `canPlaceOre` over `FeatureWorld::{ensure_can_write,
  section_block,set_section_block}` (`BulkSectionAccess`: `RegionWorld` reads and writes the proto chunk's section, no
  heightmap or post-processing; `GenerationRegion::is_writable` is `isWithinWriteZone`). `RuleTest` gained 26.3's
  `all_of`/`any_of`/`not`/`height_match` and takes the position. `random_patch`, `flower` and `no_bonemeal_flower` do
  NOT exist in 26.3 (no such feature type in `FeatureTypes`, none in the data; flowers are `simple_block` features), so
  there is nothing to port. `LeafFeatureOracle` ore rows: every `ore` feature x 4 seeds x 6 origins, two worlds (heightmap
  formula, write zone radius 1 and 0), section writes compared in order.
  Not yet: mossy-carpet `simple_block` (level random), `schedule_tick` on a
  region (the proto chunk keeps no ticks), block entities / POI / `getPostProcessPos` on `setBlock` (WP6).
- Worldgen WP4 rest (`src/worldgen/feature/{tree/,select,misc,patch,decorate}.rs`): `Feature` gained `tree` and
  `fallen_tree` (trunk, foliage and mangrove root placers, all decorators, sizes, `updateLeaves`; copied from Horizon and
  rebuilt on the block tables, `TreeBlocksCell` builds the classifications on the first tree), the selectors (`random_selector`,
  `simple_random_selector`, `random_boolean_selector`, `weighted_random_selector`, `sequence`, `overlay`; their placed features
  are JSON until `FeatureRegistry::load` resolves them), `spring_feature`, `block_pile`, `vines`, `underwater_magma`, `bamboo`,
  `random_neighbor_spread`, `vegetation_patch` and `waterlogged_vegetation_patch`; `FeatureRegistry::{place_configured,
  place_placed}` place nested features. The `HashSet<BlockPos>`s whose order feeds draws (trunks, leaves, a patch's ground) are
  `tree::PosSet` (`tree/pos_set.rs`: `HashSet<BlockPos>`'s buckets, resizes and tree bins, nothing else of the map;
  `LeafPosSetOracle` equal, 2026-10-31). The proto chunk records scheduled ticks (`ProtoChunk::ticks`). `decorate.rs`:
  `apply_biome_decoration` = `ChunkGenerator.addVanillaDecorations` over a `RegionWorld` (3 x 3 chunks' biomes, the generator's
  `FeatureSorter` steps, `setDecorationSeed`/`setFeatureSeed`, `placeWithBiomeCheck`); structures are a `StructureDecorator`
  callback (`NoStructures` for none) and `skip` names placed features not to run. Paper's configurable feature seeds are not read.
  Chunks must have status `terrain` (26.3: writes update the four final heightmaps); the driver primes missing ones first.
  `LeafFeatureOracle`: + `feature2` rows (every tree, fallen tree, selector, patch and small feature over ground levels, 8
  seeds x 8 columns; ticks left out of the rows), `LeafDecorationOracle`: whole chunks (`decorate_oracle_tests.rs`).
  `StructureTemplate.updateShapeAtEdge` is walked face by face (`FeatureWorld::update_shape_at_face`), what a block does on a
  neighbour's change is WP6 (the oracle rows leave the ticks out; chunk rows can differ where it changes a state).
  Not ported feature types (configured features in 26.3's data, by biomes using them): freeze_top_layer, multiface_growth,
  geode, monster_room, lake (each in ~55 overworld biomes), fossil, huge_brown/red_mushroom, huge_fungus, blue_ice, iceberg,
  template, speleothem(_cluster), large_dripstone, root_system, block_blob, netherrack_replace_blobs, delta_feature,
  stepped_column_cluster, sculk_patch, chorus_plant, end_*, spike, single_block_pillar, projected_random_patchy_square,
  coral_tree/claw, no_op, bonus_chest. Mossy-carpet `simple_block` (region random), pale moss patch in the oracles (needs
  `getLevel()`), block entities (beehive occupants are drawn, not stored), POI and `getPostProcessPos` on `setBlock` are WP6.
- Worldgen WP4 terrain features (`src/worldgen/feature/{terrain,ice,lake,mushroom,shape}.rs`): `freeze_top_layer`, `blue_ice`,
  `iceberg`, `lake`, `block_blob`, `huge_brown_mushroom`/`huge_red_mushroom`. `FeatureRegistry` carries each biome's
  `BiomeClimates` and `has_precipitation` (`shouldFreeze`/`shouldSnow`); light 0 as in Paper's `WorldGenRegion`;
  `Math.pow(x, 2.0)` is `x*x`; `huge_red_mushroom`'s `isValidPosition` radius uses (-1, -1) as vanilla does. `feature::shape`
  is `updateShapeAtEdge`'s `updateShape` for vegetation (air where `canSurvive` fails), double plants and vines, chosen by the
  block's survival rule; `RegionWorld::update_shape_at_face` calls it. It was the cause of the 3 differing decoration chunks:
  `decoration_matches_java` is now 48 chunks, 0 failures (no `KNOWN` list). `LeafFeatureOracle`: 47,135 rows; 6,983 ground
  `feature2` rows equal (64 per new type), 1,401 skipped for unported types. 26.3 has no `lake_water` feature: the lake's
  water/ice branch is ported from the source, not exercised. `updateShape` of other blocks (leaves, fences, walls...) is WP6.
  Still not ported: multiface_growth, geode, monster_room, fossil, huge_fungus, netherrack_replace_blobs, delta_feature,
  speleothem(_cluster), large_dripstone, root_system, stepped_column_cluster, sculk_patch, chorus_plant, end_*, spike,
  single_block_pillar, projected_random_patchy_square, coral_tree/claw, no_op, bonus_chest, mossy-carpet simple_block.
- 4.B.1 chunk holding (`src/server/chunk_system/`): Moonrise tickets + propagation, holders and full
  statuses, unload queue with save-on-unload, Paper's incremental autosave, player chunk loader, Leaf's
  tick order and stop sequence. `LeafChunkTicketsOracle` (125 rows) equal; Moonrise parts derived from
  sources. Real binary checked with `real_binary_session` (join, 9 chunks, walk, clean stop).
- WP6 driver (`src/worldgen/generator/`, `src/server/chunk_system/{manager,generation}.rs`, `src/server/level.rs`): the
  26.3 pyramid (`ChunkPyramid`/`ChunkStep` accumulated requirements, `ChunkLevel.generationStatus`, Moonrise's write
  radii, parallel/empty-load statuses, access radii, `byRadius`); Moonrise's scheduler as tick-thread data (requested gen
  status, blocking/waiting neighbours, neighbours using a chunk, loading vs generation pyramid, falling levels clamp the
  request); worker steps (STRUCTURE_STARTS..FEATURES) on a per-level pool (half the cores) over `GenerationRegion`s of
  `Arc<ProtoChunk>` snapshots, results back by channel; a step is handed out only when no running step writes its chunks;
  light steps/SPAWN/FULL on the tick thread. `LevelGenerator` per noise stem (biome source, density with the real sea
  level, surface with Paper's flat bedrock, Purpur void rings, carvers, placements with ring searches + Spigot seeds);
  TERRAIN = fill + surface (region biomes via the zoom, 3x3 palette biomes) + carvers + final heightmaps. New worlds
  (no `level.dat`) are created from `server.properties` (`level-type` preset, `level-seed`, `generate-structures`);
  `setInitialSpawn` per non-initialized level. Proto chunks saved/loaded (`generator/convert.rs`).
  `LeafChunkGenOracle`: pyramid + generationStatus + 96 chunks (overworld/nether/end x 2 seeds, structures off) equal.
  FEATURES runs the decoration (`generator/decorate.rs` over `apply_biome_decoration`, the generator's `FeatureSorter`
  steps over `possibleBiomes()`, the real survival rules; a feature-order cycle fails the level's start with the biome ids).
  Its stand-ins: no structure pieces, feature types not ported skipped (`feature::decorate::unported_features`), a feature
  error ends that chunk's decoration with a warning. Heightmaps as vanilla reads them: a `full` neighbour answers the `_WG`
  kinds from the final ones (`ImposterProtoChunk.fixType`), a missing one is primed on first read and kept
  (`ProtoChunk::height_or_prime`; primed in a read-only neighbour's snapshot, it is not kept past the step).
  Checked live: `real_binary_session` and `real_binary_dimension_change` on a fresh world, no warnings.
  Stand-ins: `structures::generate_start` (pieces) is a no-op awaiting WP5; Starlight not
  ported (chunks reach `light` with `isLightCorrect` false, sent without light); SPAWN mobs, flat/debug generators,
  priorities, FEATURES writes into already-full neighbours (they are read-only views) not ported.

- 4.B.3 scheduled ticks (`src/world/ticks.rs`, `LeafTicksOracle`): `LevelChunkTicks` (by `DRAIN_ORDER`, one per type and
  position, pending saved ticks, Moonrise's dirty flag), `LevelTicks` (collect/drain by `INTRA_TICK_DRAIN_ORDER` with the
  per-tick limit, `will_tick_this_tick`, `clear_area`, `copy_area`), `ProtoChunkTicks` (saved with proto chunks now), typed
  `block_ticks`/`fluid_ticks` in `serial.rs`. `ChunkHolderManager.ticks` registers a chunk's containers at FULL, unpacks
  them at BLOCK_TICKING, packs them at every save and removes them after the unload save; pending ticks keep a chunk unsaved.
  `Level::tick_scheduled_ticks(block, fluid)` runs them in `ServerLevel.tick`'s order (Paper's 65,536 limits) but is NOT
  called yet: no block behaviour, and a no-op would drop saved ticks. `LeafTicksOracle`: 14 scenarios, 1,835 ops, 590 ticks
  run, 13 codec cases, equal; Moonrise's dirty flag derived from sources; ties between containers unpacked from disk break
  by position then chunk (vanilla's order there depends on a hash map). Checked live: two runs of the release binary on one
  fresh world (save, reload). Open: call the runner, per-world `max-block-ticks`/`max-fluid-ticks`, debug-world skip.
- Paper world settings per level (`server/config::load_paper_world`, `level::PaperWorld`): each level creates/reads
  `<dimension folder>/paper-world.yml` as it opens (`createWorldConfig`: new file = Paper's per-world header + `_version`,
  a file without `_version` warned about and given it, saved with its own values only) and reads `chunks.*` and
  `environment.generate-flat-bedrock` from it merged over `paper-world-defaults.yml` (`mergeFrom`; `null` = missing).
  Derived from Paper's sources (no Configurate oracle); unit tests + `real_binary_session` on a fresh world (3 files made,
  clean stop). Not ported: Paper's version transforms of older world files.
- User lists (`src/server/user_lists/`): whitelist and ops as MineComplex's Leaf has them (its cleanup removed
  the ban lists, so none): files as Gson writes them, Paper's `ConcurrentHashMap` save order, `.backup` of
  unreadable files, login order (whitelist with operators past it, then full server with `bypassesPlayerLimit`),
  op level in the join, Spigot `messages.whitelist` with `§` codes as a component (`server/legacy_text.rs`,
  Adventure's legacy section serializer, derived from its 5.2.0 sources: not in the jar's libraries).
  `LeafUserListsOracle`: 118 rows equal (the `ConcurrentHashMap` rows run the JDK class).

## Reusable Rust pieces (use these, don't add parallel ones)
`src/util/{random,jmath,number_text}`, `src/core/{position,geometry}`, `src/nbt.rs`, `src/azalea/{buf,chat,
crypto,language}`, `src/registry`, `src/server/config`, `src/world/{chunk,storage,chunk_io}`,
`src/worldgen/{noise,density,data,codec,proto_chunk,feature,placement}`, `src/worldgen/structure/{template,place,processor}`, `src/block/transform.rs`, `src/world/ticks.rs`. Crates: tokio, serde_json, saphyr, tracing, rayon.

## Next
WP2 rest (biome predicates); WP4 rest (the feature types still listed as not ported under WP4 terrain features); WP5 rest (pools + jigsaw assembly, legacy pieces, `Structure.generate` per type; ring search on the real biome source, moving `find_biome_horizontal` to `worldgen/biome`; `structures` tag in `world/chunk/serial.rs` and its private int reader → `Tag::int_value`; structure check, `/locate`); WP6 rest: wire `generate_start`, flat/debug generators, Starlight (carvers' light sources).
Core: portals and `/execute in` (the level change itself is `move_player_to_level`), commands (registry args, vanilla tree, executor, ops), block/fluid ticks,
redstone, Starlight light, entities, play basics (recipes, inventory, block entities in chunk packets).

## Open issues
- User lists: no commands to edit them; `enforce-whitelist` not ported; Gson's lenient reading and bins of 8+
  colliding keys differ; saves are synchronous (Leaf's are async).
- Oracles run the vanilla jar only: Paper/Leaf/Moonrise-only behaviour is derived from sources.
- Chunk holding stand-ins: FULL promoted up to a tick earlier than Moonrise's main-thread task; a chunk of a level without a generator (flat/debug) never finishes and stalls sending; `chunk-loading-*` rates not read.
- Levels: one `Level` per stem (overworld, nether, end, then custom ones) with its own region I/O, holders,
  weather and dimension type; ticked/saved/stopped in `keysInOrder` order (Leaf's parallel world ticking is
  off by default, not ported). A player joins the level its file's `Dimension` names; an unknown one gets
  the respawn data's level (Paper's warn). Sea level is the generator's noise settings' (stand-in only for flat levels).
  Per level, as Paper keeps it (`server/level_data.rs`, 26.3 sources): `paper:level_overrides` (game time,
  spawn, `initialized`, game type, difficulty/hardcore/locked; overworld also synced to `level.dat`),
  `world_border`, `world_clocks` (day time; per-world, `time.affects-all-worlds` true not ported),
  `weather`, written only when dirty; ticked while the game runs normally (game time, clocks, moving
  border), game time resent every 20 ticks. The server's spawn is the respawn level's (`paperSpawnDimension`).
  `misc.enable-nether` (`paper-global.yml`) and `settings.allow-end` (`bukkit.yml`, read, not written) off:
  that stem is not a level. `setInitialSpawn` runs for non-initialized noise levels (no bonus chest). Not ported: the border-adjusted respawn of a spawn outside the border, Purpur's day/night length (rate 1 assumed),
  game rules (`advance_time` on), vanilla-world root-file migration and the pre-31 `allow-nether` property move.
  `Server::move_player_to_level` = `ServerPlayer.teleport(TeleportTransition)` (respawn, difficulty, op event,
  position, loader distances, abilities, level info, `sendAllPlayerInfo`; markers for attributes, entity
  data, inventory menu, effects until those exist); `HORIZON_TEST_HOOKS=1` adds console
  `horizon-test-move <player> <dimension> <x> <y> <z>` (kept in the release binary, off by default: the user's decision). Portals and `/execute in` are not ported.
- A `messages.whitelist` Adventure cannot deserialize (a `§#rrggbb` overlapping a later code: no input
  found that reaches it) fails `canPlayerLogin` for every login: warn "Failed to handle packet for", the
  login answered "Internal server error" (Java's `ServerConnectionListener.tick` catch).
- Stand-ins: no movement checks; default game rules/world border/clocks; no block
  entities in chunk packets; `bukkit.yml` not read; YAML comments dropped on save.
