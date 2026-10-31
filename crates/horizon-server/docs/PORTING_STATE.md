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

## Reusable Rust pieces (use these, don't add parallel ones)
`src/util/{random,jmath,number_text}`, `src/core/{position,geometry}`, `src/nbt.rs`, `src/azalea/{buf,chat,
crypto,language}`, `src/registry`, `src/server/config`, `src/world/{chunk,storage,chunk_io}`,
`src/worldgen/{noise,density,data,codec,proto_chunk}`. Crates: tokio, serde_json, saphyr, tracing, rayon.

## In progress (wave 1, 2026-10-31; branches merged by cherry-pick)
- WP5 slice: structure/structure_set data, spread placement + Spigot salts, StructureStart NBT (`src/worldgen/structure`).
- 4.B.1: Moonrise tickets/holders, unload with save, autosave (`src/server/level.rs`, `src/world/`).

## Next
WP2 rest (biome predicates, terrain fill, aquifer, Purpur end void rings; wire BiomeSource/LevelDensity into `Level`, one `ClimateSearch` per worker, spawn origin once per level) → WP3 (surface rules + Paper flat bedrock, carvers); WP4 (features,
placement, trees); WP5 pieces (jigsaw, templates, legacy); WP6 driver + Moonrise-shaped scheduler.
Core: Nether/End levels, commands (registry args, vanilla tree, executor, ops/bans), block/fluid ticks,
redstone, Starlight light, entities, play basics (recipes, inventory, block entities in chunk packets).

## Open issues
- Oracles run the vanilla jar only: Paper/Leaf/Moonrise-only behaviour is derived from sources.
- Stand-ins: no movement checks; default game rules/world border/clocks; only the overworld; no block
  entities in chunk packets; no ops/whitelist/bans; `bukkit.yml` not read; YAML comments dropped on save.
