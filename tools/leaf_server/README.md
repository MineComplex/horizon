# horizon-server: reference setup and real-data checks

horizon-server (`crates/horizon-server`) ports Leaf 26.3's behaviour. Its claims are checked against the
real 26.3 classes: Java oracles run on the server owner's own 26.3 jar, and `#[ignore]`/env-gated Rust
tests compare their output. None of the data below is ever committed (Mojang's data, or derived from it).

## What you need

- Leaf's checkout next to this repo (`../Leaf`, or set `LEAF_DIR`), with Leaf's Gradle run once, so that
  `leaf-server/.gradle/caches/paperweight/data/bundler/server.jar` (the extracted 26.3 server jar,
  sha256 `a362163eec5d1612d520772bc16e5b39c09e3b234fdc045f56bf544284ee8ae6`) and its 39 libraries
  under `.../paperweight/jars/minecraft/` exist.
- Java 25 (Leaf's toolchain) as `JAVA` (macOS: `/Library/Java/JavaVirtualMachines/zulu-25.jdk/Contents/Home/bin/java`;
  Debian: `apt-get install openjdk-25-jdk-headless`), and Python 3.

## One command

```sh
JAVA=<java 25> tools/leaf_server/setup_reference.sh <scratch dir>   # ~2 minutes
. <scratch dir>/realdata.env
cargo test -p horizon-server                                          # env-gated real-data tests run too
cargo test -p horizon-server --release -- --ignored                   # oracle comparisons + benchmarks
```

It writes, under the scratch dir: the prepare step's `serverdir/cache/vanilla-26.3/{reports/,block_states.bin}`
(`tools/leaf_server_reports.py`: vanilla's data generator + `LeafBlockStatesExporter.java`), the oracles'
outputs (`config.tsv`, `density.txt`, `terrain.txt`, `biomes.txt`, `structures.txt`, `placement.txt`, `feature.txt`, `tickets.txt`, `user_lists.jsonl`, `level_data.jsonl`, `chunkgen.txt`), a flat world made by the real 26.3 server (the fixture that
`world/storage_tests.rs` describes), and `realdata.env` with the variables below.

| Variable | Used by |
|---|---|
| `HORIZON_VANILLA_JAR` | worldgen oracle tests (which also need `HORIZON_SERVER_BLOCKS_JSON`: the feature registry resolves every block it names), `real_registries_match_java` |
| `HORIZON_SERVER_JAR`, `HORIZON_SERVER_BLOCKS_JSON`, `HORIZON_SERVER_BLOCK_STATES`, `HORIZON_SERVER_VANILLA_WORLD` | `world/storage_tests.rs` (`reads_a_world_made_by_vanilla`), `world/chunk/section_tests.rs` |
| `HORIZON_REGISTRIES_JSON`, `HORIZON_ORACLE_TSV` | `network/configuration/tests.rs` (`LeafConfigurationOracle.java`) |
| `HORIZON_BIOME_ORACLE`, `HORIZON_BIOME_REPORTS` | `worldgen/biome/oracle_tests.rs` (`LeafBiomeOracle.java`) |
| `HORIZON_STRUCTURE_ORACLE` | `worldgen/structure/oracle_tests.rs` and, for its `realring` rows (the strongholds' rings on the real overworld biome source), `worldgen/locate/oracle_tests.rs` (`LeafStructurePlacementOracle.java`) |
| `HORIZON_LOCATE_ORACLE` | `worldgen/locate/oracle_tests.rs` (`LeafLocateOracle.java`: `findNearestMapStructure` for every structure id and tag, `findClosestBiome3d` for biomes and tags, and `/locate`'s feedback and error messages from `LocateCommand`, over the three levels of the normal preset, two seeds and three origins; the vanilla jar has no Paper, so the search loops and the chunk load are the oracle's copy of `ChunkGenerator`'s, run over the real placements, `StructureCheck` and `Structure.generate`; also needs `HORIZON_VANILLA_JAR`, `HORIZON_SERVER_BLOCKS_JSON`, `HORIZON_SERVER_BLOCK_STATES` and `HORIZON_BIOME_REPORTS`) |
| `HORIZON_TEMPLATE_ORACLE` | `worldgen/structure/template_oracle_tests.rs` (`LeafTemplateOracle.java`: vanilla templates decoded, then placed with rotations, mirrors and processor lists into a synthetic world; every `setBlock`, tick, block entity seed and entity position; also needs `HORIZON_VANILLA_JAR`, `HORIZON_SERVER_BLOCKS_JSON` and a version 3 `HORIZON_SERVER_BLOCK_STATES`) |
| `HORIZON_JIGSAW_ORACLE` | `worldgen/structure/jigsaw_oracle_tests.rs` (`LeafJigsawOracle.java`: `Structure.generate` of every vanilla jigsaw structure over chunks of two seeds, each start's pieces and NBT, and the first start of each placed into the template oracle's synthetic world; also needs `HORIZON_VANILLA_JAR`, `HORIZON_SERVER_BLOCKS_JSON`, `HORIZON_SERVER_BLOCK_STATES` and `HORIZON_BIOME_REPORTS`) |
| `HORIZON_TEMPLATE_STRUCTURES_ORACLE` | `worldgen/structure/template_structures_oracle_tests.rs` (`LeafTemplateStructuresOracle.java`: `Structure.generate` of every vanilla shipwreck, ocean ruin, igloo, ruined portal, nether fossil, end city and woodland mansion over chunks of two seeds, each piece's saved tag, the start's tag and the tag loaded back, and the first start of each placed as Paper's `placeInChunk` does into the template oracle's synthetic world, with the pieces' tags after it; also needs `HORIZON_VANILLA_JAR`, `HORIZON_SERVER_BLOCKS_JSON`, `HORIZON_SERVER_BLOCK_STATES` and `HORIZON_BIOME_REPORTS`) |
| `HORIZON_LEGACY_STRUCTURES_ORACLE` | `worldgen/structure/legacy_oracle_tests.rs` (`LeafLegacyStructuresOracle.java`: `Structure.generate` of the piece-based structures (desert pyramid, jungle temple, swamp hut, buried treasure, mineshafts, stronghold, nether fortress, ocean monument) over chunks of two seeds, each start's pieces and NBT, and the first start of each placed chunk by chunk into the template oracle's synthetic world; also needs `HORIZON_VANILLA_JAR`, `HORIZON_SERVER_BLOCKS_JSON`, `HORIZON_SERVER_BLOCK_STATES` and `HORIZON_BIOME_REPORTS`) |
| `HORIZON_PLACEMENT_ORACLE` | `worldgen/placement/oracle_tests.rs` (`LeafPlacementOracle.java`; also needs `HORIZON_SERVER_BLOCKS_JSON` and `HORIZON_SERVER_BLOCK_STATES`) |
| `HORIZON_FEATURE_ORACLE` | `worldgen/feature/oracle_tests.rs` (`LeafFeatureOracle.java`: `canSurvive` of every block state, state providers, feature sorter cycles, simple features, and over ground levels the trees, fallen trees, selectors and the small features of `feature/misc.rs`; also needs `HORIZON_VANILLA_JAR`, `HORIZON_SERVER_BLOCKS_JSON` and `HORIZON_SERVER_BLOCK_STATES`) |
| `HORIZON_DECORATION_ORACLE` | `worldgen/feature/decorate_oracle_tests.rs` (`LeafDecorationOracle.java`: the FEATURES step of whole chunks, `ChunkGenerator.addVanillaDecorations`'s loop over the real generator's `FeatureSorter` result, 3 x 3 chunks filled and surfaced with the real biomes, without structures, the types not ported skipped on both sides; same extra variables as the carver oracle) |
| `HORIZON_TICKETS_ORACLE` | `server/chunk_system/oracle_tests.rs` (`LeafChunkTicketsOracle.java`) |
| `HORIZON_USER_LISTS_ORACLE` | `server/user_lists/oracle_tests.rs` (`LeafUserListsOracle.java`: whitelist/ops files, `ConcurrentHashMap` order, UUIDs) |
| `HORIZON_LEVEL_DATA_ORACLE` | `server/level_data/oracle_tests.rs` (`LeafLevelDataOracle.java`: `WorldBorder` settings codec, applying them, ticking a moving border, the initialize-border packet; `ClockState` and `DifficultySettings` codecs; Paper's `PaperLevelOverrides` and per-world clocks are derived from sources) |
| `HORIZON_DENSITY_ORACLE` | `worldgen/oracle_tests.rs` (`LeafDensityOracle.java`); `density_program_speed` is a benchmark |
| `HORIZON_TERRAIN_ORACLE` | `worldgen/terrain/oracle_tests.rs` (`LeafTerrainOracle.java`: `doFill` blocks, WG heightmaps, fluid marks, base columns; also needs `HORIZON_SERVER_BLOCKS_JSON`/`_BLOCK_STATES`); `terrain_fill_speed` is a benchmark |
| `HORIZON_SURFACE_ORACLE` | `worldgen/surface/oracle_tests.rs` (`LeafSurfaceOracle.java`: `doFill` then `buildSurface` blocks, WG heightmaps, fluid marks over the stems' real biomes; every biome's temperature; also needs `HORIZON_SERVER_BLOCKS_JSON`/`_BLOCK_STATES` and `HORIZON_BIOME_REPORTS`); `surface_build_speed` is a benchmark, `flat_bedrock_is_one_layer` checks Paper's flag (derived from its source, not an oracle) |
| `HORIZON_CARVER_ORACLE` | `worldgen/carver/oracle_tests.rs` (`LeafCarverOracle.java`: fill, surface, then `generateCarvers`'s body around the generator's own `getBiomeGenerationSettingsForCarver`/`applyCarvingMask`: the carving mask's runs, blocks, WG heightmaps and fluid marks, overworld/large biomes/amplified/Nether; same extra variables as the surface oracle) |
| `HORIZON_CHUNKGEN_ORACLE` | `worldgen/generator/oracle_tests.rs` (`LeafChunkGenOracle.java`: the generation pyramid's steps and `ChunkLevel.generationStatus`, then whole chunks of the normal preset's overworld, Nether and End x 2 seeds through STRUCTURE_STARTS, STRUCTURE_REFERENCES (structures off: the jar cannot place pieces without a level), BIOMES of the 3 x 3 chunks and TERRAIN as `ChunkStatusTasks` runs them: blocks, biomes, all six heightmaps, fluid marks, status, starts and references; same extra variables as the surface oracle) |

## Oracles

Single-file Java programs in `mods/horizon/src/test/java/dev/horizonlab/leafserver/` (original Horizon
code, unique class names), run with Java's source launcher in a scratch folder (they write `logs/`):

```sh
java -cp <server.jar>:<every library jar> LeafSomethingOracle.java <out>
```

Each oracle's header says which Rust test reads its output and through which variable. A new oracle
gets a line in `setup_reference.sh` and a row above.

## Reference sources

- `../Leaf/leaf-server/src/minecraft/java/`: the final server sources (vanilla 26.3 with Paper's and
  Leaf's patches applied, Moonrise under `ca/spottedleaf/moonrise`); Leaf's own code in
  `leaf-server/src/main/java`; the patches themselves in `leaf-server/{minecraft,paper}-patches/features/`
  and Paper's under `.gradle/caches/paperweight/upstreams/paper/paper-server/patches/`.
- `../Client`: the 26.3 client's sources (shared packet classes); `../Velocity`: the proxy.
- Vanilla alone, decompiled: Vineflower 1.11.1 on the server jar into a scratch folder, never into a repo
  (`crates/horizon-server/CONTINUE.md` §2).

The oracles run only the vanilla jar, not a built Leaf, so Paper/Leaf/Moonrise behaviour that is not in
the vanilla jar is checked against these sources and marked as derived from them.
