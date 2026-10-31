# horizon-server: architecture

A native Rust Minecraft 26.3 server whose **behaviour** is Leaf 26.3's (Paper/Purpur/Gale fork). Leaf's Java
sources (`~/Desktop/new/Leaf`: the applied NMS under `leaf-server/src/minecraft/java`, Leaf's own code under
`leaf-server/src/main/java`, Paper under `paper-server`, the API under `paper-api`) are the specification of
game logic, algorithms, protocol and invariants — **not** a template for the architecture.

**Port behaviour, not Java architecture.** No Java object model: no `Arc` per object, no `UnsafeCell` fields,
no runtime casts reproducing inheritance, no `&self` everywhere, no monitor per object, no `unsafe` to dodge the
borrow checker. Every `Arc`, lock and trait object has a concrete reason, written next to it.

## Scope (decisions made by the user)

- One crate, `horizon-server`; it depends on no other Horizon crate.
- Ported: Leaf's NMS and Leaf/Gale/Purpur/Pufferfish/Lithium/Moonrise server code, as native Rust designs.
- **No plugins yet.** Java plugin compatibility is not required: no JVM plugin loading, class loaders or
  reflection. A native plugin system comes later, by a separate decision: none is chosen now (no WASM, no
  dynamic native plugins, no plugin framework); the server core keeps clean module boundaries for it.
  The Bukkit/Paper API is not ported for now; its event points in Leaf's code are noted, not implemented.
- Not ported (no Java library is ported; Rust crates or code taken from azalea instead):
  JDK -> std/tokio/rayon/chrono/...; netty -> tokio + `azalea::buf`; Gson -> `serde_json`; JOML -> `glam`;
  Guava/fastutil/Caffeine -> std/`moka`; YAML configs -> `saphyr` (SnakeYAML's YAML 1.1 typing on top, `server/config/yaml.rs`); log4j/slf4j -> `log`;
  jline -> `rustyline`; joptsimple -> `clap`; Brigadier -> `command` (Horizon's own Brigadier core, arena tree); Adventure -> `azalea::chat`;
  translations -> `azalea::language` (`en_us.json` loaded and installed once at startup, never committed); encryption ->
  `azalea::crypto`. DFU `Codec`s become plain (de)serialization functions/`serde` over JSON and NBT.
- Left out entirely: data fixing (26.3 worlds only: DFU datafixers, `util.datafix`, DataConverter), Sentry,
  spark, oshi, the status protocol (Velocity answers pings), Query, RCON, JSON-RPC management, JFR, secure
  seed, and the API remnants of these (list-ping events, server icon/MOTD, remote console sender).
- Deliberate differences from Leaf (decided by the user, 2026-10-30):
  - no EULA check (Leaf deletes it too);
  - MineComplex's built-in `custom_enchantments` datapack (Leaf's `0070-Custom-datapack.patch`) is cut:
    only the vanilla pack and Paper's `paper` pack are loaded, so enchantments and their tags are
    vanilla's;
  - `hex_color` command arguments: any non-hex character is the argument's syntax error
    (`argument.hexcolor.invalid`) instead of vanilla's `NumberFormatException` reported as
    `command.exception`;
  - no packet-rate kick, as in Leaf (it deletes Paper's limiter); our own per-connection inbound byte
    budget only applies backpressure.
- The world generator is **copied** from `crates/horizon-world`/`horizon-core` into horizon-server and
  adapted (`docs/worldgen-plan.md`), not shared with or depended on (decided by the user, 2026-10-31).
  This is the accepted exception to PORTING_RULES §16 (no duplicate implementations): horizon-server
  stays independent of the other Horizon crates; inside horizon-server there is still one
  implementation per responsibility.
- `src/azalea/`: code taken from azalea (MIT) as modules, not dependencies.

## What gets ported, and what never does

Before porting a Java class, classify it: (1) Minecraft server code (NMS), (2) Paper/Leaf/Purpur/Gale/
Pufferfish/Lithium/Moonrise patches and code, (3) the Bukkit/Paper API (deferred, see Scope) — these are ported, as native Rust
designs; (4) the JDK/JVM, (5) third-party libraries, shaded or relocated (netty, fastutil, Guava, Gson, Log4j,
SLF4J, Apache Commons, Caffeine, JCTools, Brigadier, DFU, Adventure, ASM, ByteBuddy, Configurate, jline,
joptsimple, Spottedleaf's concurrentutil...), (6) generated or build-time code — these are **never
translated**: their uses are replaced by Rust std, a maintained crate, or a narrow native component for the
Minecraft-visible behaviour only (command parsing/suggestions, text components, NBT/JSON formats).
There is no `java`/`jvm`/`jdk` module and no compatibility layer: no reflection, class loaders, monitors,
Java exceptions or reference semantics. Java's wrapping `int` arithmetic is written explicitly
(`wrapping_*`) where the game relies on it.

Dependencies are added only when a subsystem uses them, after checking maintenance, correctness, allocation
and thread-safety; logging is `tracing`.

### Audit (2026-10-30)

| Path | Origin | Why it is here | Used by | Status |
|---|---|---|---|---|
| `src/azalea/buf` | azalea-buf (Rust) | protocol primitives (VarInt, strings, UUIDs) | network codecs (to come) | kept; stable-Rust edits: no specialization, no simdnbt, no backtrace capture |
| `src/azalea/chat` | azalea-chat (Rust) | text components (Adventure's role) | chat, commands (to come) | **reworked (2026-10-30):** 26.3's `ComponentSerialization` in `chat/codec.rs`, one set of rules over JSON (serde) and NBT (`nbt.rs`, the network form), checked against the 26.3 classes (`chat/testdata/components.tsv`); keybind/score/selector/nbt contents added; azalea's simdnbt paths, its serde derives (an empty `with` always written, `show_entity` with an int id, unsigned shadow colors, no click/hover reading) and the `§` splitting of literals removed |
| `src/azalea/language` | azalea-language (Rust) | translations | chat | **reworked (2026-10-30):** an explicit `Language` value, installed once at startup like vanilla's `Language.inject`; no hidden file read on first use (it read `lang/en_us.json` relative to the working directory and silently fell back to an empty table); lang JSON read as `Language.loadFromJson` does |
| `src/azalea/crypto` | azalea-crypto (Rust) | login encryption, AES/CFB8 | network login (to come) | kept |
| former `src/azalea/brigadier` | azalea-brigadier (Rust) | command parsing | — | **replaced (2026-10-30)** by `src/command`: it was a line-by-line port of Java Brigadier: nodes as `Arc<RwLock<CommandNode>>` with parent links, parsed arguments as `Arc<dyn Any>` read back with `downcast_ref` (runtime type casts), commands/requirements/modifiers as `Arc<dyn Fn>`, English messages built from `{:?}` instead of Minecraft's translation keys, and no tests |
| `src/command` | Horizon's `horizon-command` core (checked against vanilla by Horizon's parity harnesses) | Brigadier's behaviour | commands (to come) | native: one arena (`Vec<Node>`, `NodeId`s), closed enums for argument types and values, a server-chosen key `K` instead of stored closures (the tree is plain `Send + Sync` data, the commands packet's shape), contexts borrow the tree and input; Minecraft's `BrigadierExceptions` keys; checked by Brigadier's own test cases |
| `src/util/number_text` | Horizon's `horizon-client-core::snbt` | the text Java prints for `double`/`float` (`1.0`, `1.0E7`) | command errors, chat arguments (and SNBT to come) | native |
| former `src/java`, generated skeleton, `tools/leaf_port` | JDK / Java object model (`JRef`, `JObject`, `Mut<T>` over `UnsafeCell`, `jobject!`, `cast_arc`, `instance_of`, monitors) | first attempt | — | removed (ab3326d1); the crate now has `#![forbid(unsafe_code)]` |

Checked and kept: the `LazyLock` tables in `azalea/chat` (immutable constants), `AzBuf for Arc<T>` in
`azalea/buf` (a codec impl, not storage), the temp-name counter in `world/storage/region.rs` (an atomic
for unique file names). Unused crates after these changes (`compact_str`, `parking_lot`) were removed.

### Done so far, and next steps

Done (each checked against Java 25 running the real 26.3 classes, with Paper's/Leaf's changes applied
from their patches): the command tree and its packet (`command`, `command/network.rs`);
`.properties` files and Leaf's `server.properties` (`server/properties_file.rs`,
`server/server_properties.rs`; no EULA check and no `eula.txt`: Leaf deletes it, and the user decided to keep it out); `/tick` and the
tick loop's timing with Paper's TPS/MSPT (`server/tick_rate.rs`, `server/tick_loop.rs`); paletted
containers with Moonrise's save form (`world/chunk`); the block state table read from the data
generator's `blocks.json` (`block`, with `tools/leaf_server_reports.py`), and each state's cached
flags and shapes (`block/flags.rs`, `LeafBlockStatesExporter.java` under
`mods/horizon/src/test/java/dev/horizonlab/leafserver/`); startup (`server/bootstrap.rs`, `main.rs`): Leaf's
order and log lines; the YAML settings read so far (`server/config`: Velocity, catch-up ticks,
`tab-complete`/`send-namespaced`, the lagging threshold, Leaf's 5 s TPS; YAML typing checked against
SnakeYAML 2.6, the libraries' coercions derived from their sources) and the vanilla jar with `en_us.json`
and `deprecated.json` (`server/vanilla_jar.rs`); chunk sections with
Moonrise's block counting and ticking list, and the 26.3 chunk NBT with Paper's version guard and
Moonrise's Starlight light tags (`world/chunk/section.rs`, `world/chunk/serial.rs`; block entities,
entities, ticks, post-processing and structures stay raw NBT until their systems are ported); the
network side up to the configuration state (`network/`: frames, compression, encryption, handshake,
Velocity modern forwarding); Minecraft's registry-free command argument types; 26.3 text components
in JSON and network NBT (`azalea/chat/codec.rs`); the tick thread and tokio network runtime
(`server/runtime.rs`); registries and the configuration state (`registry/`, `network/configuration.rs`);
level storage, region I/O workers and `LevelChunk` (`world/`); the join's play packets and Moonrise's
chunk-sending rules (`network/play/`); the world generator's foundations (`worldgen/`: noise, the density
program as one arena of closed `enum` nodes per level instead of an `Arc<dyn>` tree, the worldgen
registries from the jar, the proto chunk), bit-equal to 26.3's `RandomState` samplers on 774,162 values
(`LeafDensityOracle.java`) and as fast as Horizon's `Arc<dyn>` tree on chunk volumes, ~9% faster on
single points (`density_program_speed`, 2026-10-31).

**Biomes (WP2, first slice, 2026-10-31).** `worldgen/climate` (parameters, the six-field climate
sampler on the level's density program, the R-tree with Moonrise's search; the previous result is a
`ClimateSearch` the worker owns instead of a thread-local) and `worldgen/biome` (the four biome
sources as one enum over biome registry ids; the level stem's `biome_source` from JSON or
`level.dat` NBT; the multi-noise presets from the data generator's `biome_parameters` reports; the
BIOMES step filling a `ProtoChunk` with Paper's section order; Leaf's `BiomeManager` zoom; the
noise spawn finder). Checked against the 26.3 classes (`LeafBiomeOracle.java`): 7,599 preset
entries, 23,980 climate targets, 23,980 quart biomes, 880 chunk sections (biomes and container
bytes), 6 spawn positions and 52,096 zoomed quarts, all equal, over two seeds and five stems.

Structures, first slice (WP5, 2026-10-31, `worldgen/structure/`): the `worldgen/structure` and
`worldgen/structure_set` registries with their tags, decoded eagerly; where structures start
(random spread, the frequency reducers, exclusion zones, dimension origin, concentric rings as one
per-level computation whose biome searches the caller runs) with Paper's `spigot.yml` structure
seeds (`world-settings.*.seed-*`, the mineshaft/buried treasure salt overrides, `seed-stronghold`);
`createStructures`' choice of structures (piece generation is a callback); structure starts and
references in chunk NBT with Leaf's insertion-ordered maps and the hash order of loaded references;
`createReferences`. Checked against the 26.3 classes on 97,412 answers (`LeafStructurePlacementOracle.java`);
Paper's seeds are checked against the patched sources' arithmetic (Paper's code isn't in the vanilla jar).

**A client gets into the world (2026-10-30).** Startup loads the registries (one `Arc`), opens
`level-name` with its `session.lock` and `level.dat` (a missing world is not created: generation is
not ported, said in the log) and the overworld's region I/O workers with `region-file-compression`
(`server/level.rs`). The tick thread drives each connection's `Configuration`; `PrepareSpawn` reads
`players/data/<uuid>.dat` (26.3's folder; `server/player_data.rs`) and holds the 7x7 spawn chunks
until loaded; `Play` places the player (`join_sequence`, the commands packet with Paper's empty tree,
markers of unported systems skipped at debug level) and switches the connection to play, where
`server/player.rs` handles teleport acceptance, movement (re-centring the chunk view), client
information, player loaded and Paper's keep-alive (shared with configuration), and sends chunks by
Moonrise's distances, nearest first under its rate limiter, with the saved Starlight light. Player
files are saved on leaving and on stop (typed fields over the file's tag). Stand-ins, each written
where it is: `PlayerSpawnFinder` reduced to the respawn column's `MOTION_BLOCKING` top; no movement checks;
game rules, world border, clocks, join message, ops and `force-gamemode` at their defaults.

**Chunk holding (2026-10-31, `server/chunk_system/`).** Moonrise's tickets, levels and chunk holders
as plain data on the tick thread (one `ChunkHolderManager` per level; the region workers only do I/O,
by message, behind the `ChunkStorage` trait): ticket types with their timeouts (vanilla's checked on
the 26.3 jar by `LeafChunkTicketsOracle.java`, 110 rows; Paper's and Moonrise's derived from their
sources), the `UNKNOWN` ticket that keeps a removed level one tick, level propagation (the result
of Moonrise's propagator, checked against `source + Chebyshev distance` on 253,920 random cases),
full status transitions from levels and full neighbours, the unload queue with Paper's minimum
count/fraction, saving on unload and autosave (only unsaved chunks; `chunks.*` of
`paper-world-defaults.yml`), `level.dat` every `autosavePeriod` ticks and on the empty pause, every
chunk saved on stop with Moonrise's log lines. Players hold their chunks through Moonrise's
`PlayerChunkLoaderData` (load, generate, tick and send stages and queues, delayed tickets on leaving
an area for `delay-chunk-unloads-by`), joins through `CHUNK_LOAD`, `PLAYER_SPAWN` and `PLAYER_JOIN`
tickets. Without a generator, a chunk not on disk stays missing (`awaiting_generation` is WP6's
hook) and the send queue waits on its neighbours, as Moonrise's does when a player can't generate.

Next, in order:
1. Play state, continued: player info removal and the leave message, the unported join steps
   (recipes, recipe book, inventory, effects, scoreboard, entity tracking), block entities in chunk
   packets, the world border/clocks/game rules from the saved data, `ops.json` and permissions.
2. The nether and the end: one more `Level` each (its own `ChunkIo` over
   `dimensions/minecraft/<name>/region`), dimension switching, `paperSpawnDimension`.
3. Moonrise's chunk system, continued: the nether and the end as more `Level`s, `/save-all` (with
   the command executor), entity and POI chunks, `FORCED`/`PORTAL`/`POST_TELEPORT` tickets with
   their systems, then light and generation (the holders' `awaiting_generation` hook).
4. Commands: Minecraft's argument types as `ArgumentType` variants (and parser ids in
   `command/network.rs`), the vanilla tree from `Commands`, the executor over `ContextChain`.
5. The entity arena and movement physics, following "Data" below.

## Ownership and threads

- **The tick thread owns the game.** `Server` owns every `Level` (one per dimension); a `Level` owns its
  chunks, entities, block ticks, block entities, weather/time. Game code gets `&mut Level` / `&mut Server`;
  nothing in the world is shared mutably across threads.
- **Other threads talk through queues.** Network tasks (tokio) decode packets and send them to the tick thread
  over a channel per connection; the tick thread sends outgoing packets back the same way. Chunk generation,
  lighting and I/O run on a worker pool (rayon) on *owned* data (a chunk being generated plus read-only
  neighbours/registries) and hand results back as messages, preserving Moonrise's scheduling order and
  Leaf's async features (async pathfinding, tracker, spawning) only where they keep vanilla's ordering.
- **Read-only shared data is `Arc`'d once:** registries, block/item/entity type tables, loot tables, recipes,
  datapack contents — built at startup/reload, immutable afterwards, swapped whole on `/reload`.

## Data

- **Blocks:** `BlockState` is a `u32` id into a flat static table (properties, flags, shapes); `Block` is an
  id with behaviour by enum/`match` (or a `&'static` behaviour table per block where hooks are many).
  No objects per state.
- **Where block data comes from (2026-10-30):** the state table is Mojang's data, so it is neither
  committed nor generated into Rust sources, and the server runs without a JVM. Reading the jar
  directly is not possible: blocks and their properties exist only as `Blocks`'s static-initializer
  bytecode. So a one-time prepare step, `tools/leaf_server_reports.py`, runs vanilla's own data
  generator (`net.minecraft.data.Main --reports`, Java needed only there) on the owner's 26.3 server
  jar and writes `reports/` to `<server dir>/cache/vanilla-26.3/reports/`; at startup the server reads
  `blocks.json` (`block::BlockStates::load`, ~7 MB, checked state by state against vanilla's order).
  Paper/Purpur/Leaf patches do not change state ids or properties (Moonrise only replaces the
  neighbour table by the same index arithmetic), so the vanilla report is Leaf's table. The report
  has no flags or shapes: `isAir` comes from the three `Properties.air()` blocks. The same prepare
  step then runs `LeafBlockStatesExporter.java` (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`)
  on the jar, which writes
  `cache/vanilla-26.3/block_states.bin` (~1 MB, versioned binary documented in `block/flags.rs`):
  per state what `BlockStateBase`/`initCache` cache (flags, light emission/dampening, push reaction,
  fluid state, destroy speed, map colour, instrument, face sturdiness, the `Cache`'s collision shape
  and the occlusion shape, de-duplicated: 529 shapes for 35,723 states) plus Moonrise's and Leaf's
  derived flags (`occludesFullBlock`, `isSpecialCollidingBlock`, Leaf's constant collision shape);
  `block::BlockStateData` keeps them as flat arrays by state id. Sounds and the like come later.
- **Positions:** `BlockPos`/`ChunkPos`/`SectionPos` are `Copy` value types (`i32` fields, Java's long packing
  as functions); `Vec3`/`AABB` are `Copy` `f64` structs.
- **Chunks:** paletted containers (Java's bit layout, for the protocol and region format), sections in a
  `Vec`, chunks in a `HashMap<ChunkPos, Chunk>` per level (Moonrise's ticket/holder logic as plain data).
- **Entities:** stored in a generational arena per level (`EntityId` = index + generation, also Java's
  network int id table); common state in one struct (position, motion, bounding box, flags, synched data),
  kind-specific state in an enum (`EntityKind`); behaviour by `match` on kind, with shared helpers for
  families (living, mob, projectile...). AI goals/brains are data + functions over `&mut Level` and an id.
  References between entities are `EntityId`s, checked on use.
- **Items:** `ItemStack` is a value (item id, count, component patch); components are typed enums.
- **NBT/JSON:** NBT is a plain enum tree (`nbt` module) with Java's exact binary/SNBT formats; data
  formats Java expressed as `Codec`s are explicit read/write functions with the same error messages.

## Behaviour checks

- Same RNG algorithms and draw order (`util::random`: Java's LCG, Xoroshiro128++, positional factories),
  same `Mth`/float vs double arithmetic, same tick phase order as `MinecraftServer.tickServer`/`ServerLevel.tick`.
- Every ported component gets unit tests; numeric/algorithmic code is checked against values computed by the
  Java code (fixtures produced from Leaf, never vanilla sources committed).

## Layout

Modules follow the server's subsystems, not Java packages: `core` (positions, directions, shapes math),
`util` (mth, random, noise), `nbt`, `resources` (identifiers, registries, data loading), `block`, `item`,
`world` (level, chunk, storage, generation, lighting), `entity`, `network` (tokio transport, protocol,
packets), `server` (tick loop, players, commands, config: server.properties, paper/leaf YAML), `azalea`
(taken code). The full porting rules from the user are in PORTING_RULES.md.
