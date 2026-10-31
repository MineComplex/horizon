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
- 4.B.1 chunk holding (`src/server/chunk_system/`): Moonrise tickets + propagation, holders and full
  statuses, unload queue with save-on-unload, Paper's incremental autosave, player chunk loader, Leaf's
  tick order and stop sequence. `LeafChunkTicketsOracle` (125 rows) equal; Moonrise parts derived from
  sources. Real binary checked with `real_binary_session` (join, 9 chunks, walk, clean stop).

- User lists (`src/server/user_lists/`): whitelist and ops as MineComplex's Leaf has them (its cleanup removed
  the ban lists, so none): files as Gson writes them, Paper's `ConcurrentHashMap` save order, `.backup` of
  unreadable files, login order (whitelist with operators past it, then full server with `bypassesPlayerLimit`),
  op level in the join, Spigot `messages.whitelist` with `§` codes as a component (`server/legacy_text.rs`,
  Adventure's legacy section serializer, derived from its 5.2.0 sources: not in the jar's libraries).
  `LeafUserListsOracle`: 118 rows equal (the `ConcurrentHashMap` rows run the JDK class).

## Reusable Rust pieces (use these, don't add parallel ones)
`src/util/{random,jmath,number_text}`, `src/core/{position,geometry}`, `src/nbt.rs`, `src/azalea/{buf,chat,
crypto,language}`, `src/registry`, `src/server/config`, `src/world/{chunk,storage,chunk_io}`,
`src/worldgen/{noise,density,data,codec,proto_chunk,feature,placement}`. Crates: tokio, serde_json, saphyr, tracing, rayon.

## Next
WP2 rest (biome predicates; wire BiomeSource/LevelDensity into `Level`, one `ClimateSearch` per worker, spawn origin once per level; read the flat-bedrock flag per world and pass it to `SurfaceSystem::new`); WP4 rest (trees and the other feature implementations, the decoration driver); WP5 rest (pieces: jigsaw, templates, processors, legacy; ring search on the real biome source, moving `find_biome_horizontal` to `worldgen/biome`; `structures` tag in `world/chunk/serial.rs` and its private int reader → `Tag::int_value`; structure check, `/locate`); WP6 driver + Moonrise-shaped scheduler (carvers' light sources).
Core: portals and `/execute in` (the level change itself is `move_player_to_level`), commands (registry args, vanilla tree, executor, ops), block/fluid ticks,
redstone, Starlight light, entities, play basics (recipes, inventory, block entities in chunk packets).

## Open issues
- User lists: no commands to edit them; `enforce-whitelist` not ported; Gson's lenient reading and bins of 8+
  colliding keys differ; saves are synchronous (Leaf's are async).
- Oracles run the vanilla jar only: Paper/Leaf/Moonrise-only behaviour is derived from sources.
- Chunk holding stand-ins: FULL promoted up to a tick earlier than Moonrise's main-thread task; pending
  block/fluid ticks don't mark a chunk unsaved; a missing chunk stalls sending (Moonrise's rule for players
  who can't generate) until WP6; `paper-world.yml` per world and `chunk-loading-*` rates not read.
- Levels: one `Level` per stem (overworld, nether, end, then custom ones) with its own region I/O, holders,
  weather and dimension type; ticked/saved/stopped in `keysInOrder` order (Leaf's parallel world ticking is
  off by default, not ported). A player joins the level its file's `Dimension` names; an unknown one gets
  the respawn data's level (Paper's warn). Sea level is the stand-in 63/32/0/-63 (noise settings unread).
  Per level, as Paper keeps it (`server/level_data.rs`, 26.3 sources): `paper:level_overrides` (game time,
  spawn, `initialized`, game type, difficulty/hardcore/locked; overworld also synced to `level.dat`),
  `world_border`, `world_clocks` (day time; per-world, `time.affects-all-worlds` true not ported),
  `weather`, written only when dirty; ticked while the game runs normally (game time, clocks, moving
  border), game time resent every 20 ticks. The server's spawn is the respawn level's (`paperSpawnDimension`).
  `misc.enable-nether` (`paper-global.yml`) and `settings.allow-end` (`bukkit.yml`, read, not written) off:
  that stem is not a level. Not ported: `setInitialSpawn` for `initialized: false` levels (needs worldgen),
  the border-adjusted respawn of a spawn outside the border, Purpur's day/night length (rate 1 assumed),
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
