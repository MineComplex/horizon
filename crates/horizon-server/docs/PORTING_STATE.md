# Leaf 26.3 → Rust: porting state (read this instead of the whole repo)

Short, current. Rules: `../PORTING_RULES.md`. Design: `../ARCHITECTURE.md`. Generator plan: `worldgen-plan.md`.
Real-data checks: `tools/leaf_server/README.md` (`setup_reference.sh` → `realdata.env`).

## Decisions (user)
- One crate `crates/horizon-server`, no dependency on other Horizon crates; `#![forbid(unsafe_code)]`.
- World generator **copied** from `crates/horizon-world/src/worldgen` + `crates/horizon-core/src` and adapted
  (2026-10-31; accepted exception to rule 16). Drop Horizon's modpack `horizon:*` code.
- Cut vs Leaf: no EULA check, no `custom_enchantments` pack, `hex_color` syntax error for non-hex, no
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
- Worldgen WP5 slice (`src/worldgen/structure`): structure/structure_set registries + tags, random spread,
  frequency reducers, exclusion zones, ring geometry (biome search as a closure), Spigot `seed-*` salts
  (derived from sources), `createStructures` choice, StructureStart/references NBT in Leaf's orders.
  `LeafStructurePlacementOracle`: 97,412 answers, all equal. Matches Leaf (user, 2026-10-31): a malformed
  inner start id fails the chunk load, inline structures/sets decode, dotted `spigot.yml` world names nest.
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
`src/worldgen/{noise,density,data,codec,proto_chunk}`. Crates: tokio, serde_json, saphyr, tracing, rayon.

## Next
WP2 rest (biome predicates, Purpur end void rings; wire BiomeSource/LevelDensity into `Level`, one `ClimateSearch` per worker, spawn origin once per level) → WP3 (surface rules + Paper flat bedrock, carvers); WP4 (features,
placement, trees); WP5 rest (pieces: jigsaw, templates, processors, legacy; ring search on the real biome source, moving `find_biome_horizontal` to `worldgen/biome`; `structures` tag in `world/chunk/serial.rs` and its private int reader → `Tag::int_value`; structure check, `/locate`); WP6 driver + Moonrise-shaped scheduler.
Small cleanups: `obfuscate_seed` (server/runtime.rs) → `worldgen/biome/zoom.rs`; `find_biome_horizontal`
(structure/placement.rs) → `worldgen/biome`; `world/chunk/serial.rs` private int reader → `Tag::int_value`.
Core: Nether/End levels, commands (registry args, vanilla tree, executor, ops), block/fluid ticks,
redstone, Starlight light, entities, play basics (recipes, inventory, block entities in chunk packets).

## Open issues
- User lists: no commands to edit them; `enforce-whitelist` not ported; Gson's lenient reading and bins of 8+
  colliding keys differ; saves are synchronous (Leaf's are async).
- Oracles run the vanilla jar only: Paper/Leaf/Moonrise-only behaviour is derived from sources.
- Chunk holding stand-ins: FULL promoted up to a tick earlier than Moonrise's main-thread task; pending
  block/fluid ticks don't mark a chunk unsaved; a missing chunk stalls sending (Moonrise's rule for players
  who can't generate) until WP6; `paper-world.yml` per world and `chunk-loading-*` rates not read.
- Stand-ins: no movement checks; default game rules/world border/clocks; only the overworld; no block
  entities in chunk packets; `bukkit.yml` not read; YAML comments dropped on save.
