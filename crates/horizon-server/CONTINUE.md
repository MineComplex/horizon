# Continue the Leaf 26.3 → Rust port (prompt for Claude Code)

Paste this file (or point Claude Code at it) to continue the work. WP1 of the world generator (noise,
density program, worldgen data, proto-chunk) landed in `70afcbbe8` and was verified on 2026-10-31
(section 4.A step 0: bit-exact against the real 26.3 classes, benchmarked); check `git log` first.

---

You are continuing the port of the Minecraft server **Leaf 26.3** (MineComplex's fork of Leaf, itself a
Paper/Purpur/Gale fork) from Java to native Rust, in `crates/horizon-server` of the Horizon repository.
Branch: `claude/leaf-rust-migration-refactor-ey9obv` (in the `horizon` repo; also checked out: `Leaf`,
`Client`, `Velocity`, MineComplex's Java repos, used as references only).

## 0. Read before anything else

1. `crates/horizon-server/PORTING_RULES.md` — the user's rules. Port Minecraft/Leaf **behaviour**, not
   Java architecture: no JDK ports, no Java object model (`JRef`, `Mut<T>`/`UnsafeCell`, monitors,
   runtime casts), no ported shaded libraries (Netty, fastutil, Guava, Gson, Brigadier, Adventure, DFU,
   Configurate…); reuse maintained Rust crates and existing Horizon code first (search crates.io and the
   workspace before writing anything; say what you compared); `#![forbid(unsafe_code)]`; every `Arc`,
   lock and trait object has a written reason; the tick thread owns the game, workers get owned data
   and answer with messages; don't change tick or packet ordering.
2. `crates/horizon-server/ARCHITECTURE.md` — scope, the audit, ownership and threads, data layout, the
   "Done so far" and "Next" lists, and **the user's decisions** (deliberate differences from Leaf: no
   EULA check; MineComplex's `custom_enchantments` datapack cut; `hex_color` gives a syntax error for
   non-hex input; no packet-rate kick, only our own inbound byte budget with backpressure).
3. `CLAUDE.md` at the repo root: never commit Mojang files, game data, generated tables of Mojang data or
   decompiled code (`tools/check_public_tree.py` runs on commit; `git config core.hooksPath .githooks`);
   commits end with `Co-Authored-By: Claude <model> <noreply@anthropic.com>`; run servers/clients only in
   fresh `mktemp -d` folders with `HOME`/`XDG_*` pointed into them.
4. `crates/horizon-server/docs/worldgen-plan.md` — the world generator survey and its six work
   packages (WP1–WP6).
5. `tools/leaf_server/README.md` — how to rebuild the reference sources, run the Java oracles and the
   Velocity test client (if that folder isn't there yet, the steps are in section 2 below).

## 1. Where things stand

`cargo test -p horizon-server` (250+ tests) and `cargo clippy -p horizon-server --all-targets` (zero
warnings) pass; the workspace builds (`cargo check --workspace`, needs `libasound2-dev libudev-dev`).
A client logs in through Velocity modern forwarding, gets through configuration (registries, tags),
joins the overworld of an existing world and receives its chunks (checked with the real binary on a
world made by vanilla 26.3: 25 chunks, player file saved, clean `stop`).

Ported and checked against the real 26.3 classes on Java 25 (Paper/Leaf changes applied from their
patches, because Leaf itself can't be built here):

| Area | Modules |
|---|---|
| Commands: native Brigadier (arena tree), packet, 14 registry-free Minecraft argument types | `src/command/` |
| `.properties`, Leaf's `server.properties`, YAML configs (`paper-global`, `spigot`, `purpur`, `leaf-global`), startup order, vanilla jar + `en_us` | `src/server/{properties_file,server_properties,config,bootstrap,vanilla_jar}.rs` |
| Tick rate (`/tick`) and Paper's tick loop timing, TPS/MSPT | `src/server/{tick_rate,tick_loop}.rs` |
| Runtime: tick thread, tokio network, logins, keep-alive, console `stop`, shutdown | `src/server/runtime.rs` |
| Network: frames, compression, encryption, handshake, Velocity login, configuration state, play packets for a join, Moonrise chunk-sending math | `src/network/` |
| Registries + tags from the jar's vanilla datapack (+ Paper's pack) | `src/registry/` |
| Text components (JSON + NBT) | `src/azalea/chat/` |
| Blocks: state table (`blocks.json`), per-state flags and shapes (`block_states.bin`) from the prepare step | `src/block/`, `tools/leaf_server_reports.py` |
| Chunks: paletted containers (Moonrise save form), sections, 26.3 chunk NBT, `LevelChunk` + heightmaps | `src/world/chunk/`, `src/world/level_chunk.rs` |
| World storage: `session.lock`, `level.dat`, region I/O workers (LZ4 default) | `src/world/{level_storage,chunk_io}.rs` |
| Join path: levels, players, player files (`players/data/<uuid>.dat`) | `src/server/{level,player,player_data}.rs` |
| World generator WP1 (`70afcbbe8`): noise, density program, worldgen data, proto-chunk. `LeafDensityOracle` on the real 26.3 jar (sha256 `a362163e…8ae6`), all 7 noise settings x 2 seeds, every router/aquifer root and every noise: 52,520 values, 2,502 cached values, 719,140 volume values, all bit-equal | `src/worldgen/` |

Known stand-ins (documented in the code): chunks held by a per-player refcount instead of Moonrise
tickets; no movement checks; default game rules/world border/clocks; only the overworld; no block
entities in chunk packets; whitelist/ops/bans files not ported; `bukkit.yml` not read (its shutdown
message default is hard-coded); comments admins add to YAML files are dropped on save.

## 2. Reference setup (rebuild it in a new environment)

- **Vanilla 26.3 sources** (unobfuscated jar): decompile the server jar with Vineflower 1.11.1
  (`https://maven-central.storage-download.googleapis.com/maven2/org/vineflower/vineflower/1.11.1/vineflower-1.11.1.jar`,
  sha1 `77767cdbc76d116c6d5e1565ec387e531085b9f4`) into a scratch folder, never into a repo:
  `java -jar vineflower-1.11.1.jar --silent=true --thread-count=8 <server.jar> <out>/`.
  The extracted server jar is at `Leaf/leaf-server/.gradle/caches/paperweight/data/bundler/server.jar`
  after Leaf's Gradle ran once; its 39 libraries are in `~/.gradle/caches/modules-2/files-2.1/`.
  The 26.3 client's decompiled sources (shared packet classes) are in the `Client` repo.
- **Paper's patches**: `Leaf/.gradle/caches/paperweight/upstreams/paper/paper-server/patches/{sources,features}/`
  and Paper's own code under `.../paper-server/src/main/java` (cloned by `./gradlew applyAllPatches`).
  **Leaf's patches**: `Leaf/leaf-server/minecraft-patches/features/*.patch` (vanilla classes; Purpur is
  `0001`, MineComplex's Cleanup is `0317`/`0318`), `Leaf/leaf-server/paper-patches/features/*.patch`,
  Leaf's own code `Leaf/leaf-server/src/main/java`. Final behaviour = vanilla + Paper + Leaf, in order.
- **JDK 25** (Leaf's toolchain): `apt-get install openjdk-25-jdk-headless`. Oracles are single-file
  Java programs run with `java -cp <server.jar>:<libs> Oracle.java`, printing expected values that
  `#[ignore]`/env-gated Rust tests compare (pattern: `LeafConfigurationOracle.java`).
- **Prepare step** (data the server reads at run time, never committed):
  `python3 tools/leaf_server_reports.py --jar <server jar> [--libraries <dir>] --server-dir <dir> --java <java 25>`
  writes `<dir>/cache/vanilla-26.3/reports/` and `block_states.bin`.
- **Known environment problems**: Maven Central answers this kind of container with HTTP 429, so
  Leaf's `applyAllPatches` fails at its dependency download (Paper's patches were applied before it
  failed). Pointing Gradle at Google's Maven Central mirror needs the user's permission (a global init
  script was refused once). GitHub over curl returns 403, but `git` works.

## 3. How to work

- Split work into **parallel agents**, each in its own git worktree, on non-overlapping files; give each
  the context above, its references, exact verification steps, a size target (~1,000–1,300 lines) and
  "commit on your branch, don't push". Worktrees are sometimes created from `main` (no shared history):
  every agent first checks `git log` and, if needed, `git reset --hard origin/<branch>`.
- Merge each finished branch with `git cherry-pick`, resolve conflicts (usually ARCHITECTURE.md and
  `Cargo.toml`), run `cargo test -p horizon-server`, `cargo clippy -p horizon-server --all-targets`
  (zero warnings), the gated real-data tests, and for runtime work the real binary with the Velocity
  client; then push. Remove finished worktrees; keep `.claude/worktrees/` out of git
  (`.git/info/exclude`).
- Every behaviour claim is either verified against the real classes (say how many cases) or marked as
  derived from the sources. Report honestly what was not run.
- When Leaf's behaviour is surprising or a decision is needed (a deliberate difference from Leaf,
  committing anything derived from Mojang or GPL code, changing global build settings), ask the user.

## 4. Next steps, in order

### A. World generator — see `docs/worldgen-plan.md`
0. ~~Finish verifying WP1~~ — **done 2026-10-31**: `cargo test -p horizon-server` 274 passed (+2 ignored),
   clippy zero warnings (fixed one new `chunks_exact_to_as_chunks` lint of clippy 1.99), the density oracle
   bit-equal (see section 1). Speed (`density_program_speed`, an ignored test; the same workload on a
   temporary copy of Horizon's `Arc<dyn DensitySampler>` tree gave the same checksums): overworld
   `final_density` cell-corner volumes 572 µs/chunk on both (equal within 0.3%); 4,096 points x 8 router
   fields, uncached: 365 ms vs 401 ms (the program ~9% faster). Apple Silicon, release, best of 5, 3 runs.
   So the enum program stays; no `Box<dyn>` fallback is needed.
1. In parallel: **WP2** biomes, climate, terrain fill, aquifer; **WP4** features, placement, trees (split
   4a/4b if two agents); **WP5** structures (templates from the jar, Spigot's structure salts, structure
   start NBT). Each copies Horizon's code from `crates/horizon-world/src/worldgen/` (drop Horizon's
   modpack `horizon:*` code), adapts it to horizon-server's types and data, and fixes the Java-shaped
   spots the plan lists (Mutex fields on structure pieces, `Rc<RefCell>` in jigsaw, `thread_local`,
   `Box::leak`, lazy decode caches, `Arc<dyn>` graphs).
2. Then **WP3** surface rules (with Paper's flat-bedrock condition) and carvers.
3. Then **WP6**: the generator driver and a status scheduler shaped like Moonrise's (parallel up to
   TERRAIN, area-locked FEATURES/SPAWN, FULL on the tick thread), Paper's `WorldGenRegion.setBlock`
   change, flat and debug generators, integration with `ChunkIo` and the join path (generate missing
   chunks instead of skipping them).
4. Verify with Java oracles over whole chunks for fixed seeds (fix the decoration order on both sides,
   because Moonrise's order differs from vanilla's), and Horizon's parity notes in
   `docs/world-generation.md`.

### B. Gameplay core (can run in parallel with A, on other files)
1. **Chunk holding**: Moonrise's tickets and holders, unloading with saving, autosave
   (`next_autosave_interval` exists), `/save-all`; then the Nether and the End as more `Level`s, and
   dimension switching.
2. **Commands**: argument types that need registries/world (entity selectors `@a`/`@e`/…, block states,
   items, resources, NBT paths, components, `message`), the vanilla command tree from `Commands`, and
   vanilla's executor (`net.minecraft.commands.execution`) over `ContextChain`; console commands;
   `ops.json`, whitelist and ban lists (`horizon-server-files` in the old Horizon tree has vanilla ports
   to reuse, minus their hand-rolled JDK parts).
3. **Block updates**: block ticks and fluid ticks (scheduled), neighbour updates, redstone (Leaf/Paper's
   choice of redstone implementation — check the patches), random ticks (sections already count them).
4. **Light**: Moonrise's Starlight engine (the light data already reads/writes in its format).
5. **Entities**: a generational arena per level (`EntityId`), tracking and entity packets, movement and
   collision physics (shapes are loaded), then mobs, AI goals/brains and pathfinding (Leaf's async
   pathfinding only where it keeps vanilla's ordering).
6. **Play basics still skipped at join**: recipes, recipe book, inventory/menus, effects, scoreboard,
   player info removal and leave message, block entities in chunk packets, world border/clocks/game
   rules from saved data, `bukkit.yml` (shutdown message), Leaf's `leaf-global.yml` options as their
   systems arrive.

### C. Throughout
- Keep ARCHITECTURE.md's "Done so far"/"Next" current with every merge.
- Commit new oracles and test drivers (Java under `mods/horizon/src/test/java/dev/horizonlab/leafserver/`
  with unique class names; Python/shell under `tools/leaf_server/`), never their Mojang-derived output
  beyond small expected-value fixtures.
- Benchmark before claiming performance (the density program, chunk sending, palettes' list lookup).

## 5. Report at the end of a session

What was ported; which JDK/shaded dependencies were replaced and by what; files changed; what was
verified against Java and how; test/clippy results; what's left and the open questions for the user.
