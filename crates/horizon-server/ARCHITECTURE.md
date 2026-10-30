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
  Guava/fastutil/Caffeine -> std/`moka`; YAML configs -> `serde_yaml`; log4j/slf4j -> `log`;
  jline -> `rustyline`; joptsimple -> `clap`; Brigadier -> `command` (Horizon's own Brigadier core, arena tree); Adventure -> `azalea::chat`;
  translations -> `azalea::language` (`en_us.json` loaded and installed once at startup, never committed); encryption ->
  `azalea::crypto`. DFU `Codec`s become plain (de)serialization functions/`serde` over JSON and NBT.
- Left out entirely: data fixing (26.3 worlds only: DFU datafixers, `util.datafix`, DataConverter), Sentry,
  spark, oshi, the status protocol (Velocity answers pings), Query, RCON, JSON-RPC management, JFR, secure
  seed, and the API remnants of these (list-ping events, server icon/MOTD, remote console sender).
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
| `src/azalea/chat` | azalea-chat (Rust) | text components (Adventure's role) | chat, commands (to come) | kept |
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
`server/server_properties.rs`; Leaf deletes the EULA check, so there is no `eula.txt`); `/tick` and the
tick loop's timing with Paper's TPS/MSPT (`server/tick_rate.rs`, `server/tick_loop.rs`); paletted
containers with Moonrise's save form (`world/chunk`).

Next, in order:
1. Bootstrap: read `server.properties`, Paper/Purpur/Leaf YAML (`paper-global.yml` Velocity forwarding,
   `leaf-global.yml`), load and `install` `en_us.json`, and run the tick thread over `TickClock`.
2. Network: tokio transport with `azalea::buf` codecs, one channel per connection into the tick thread;
   login with Velocity modern forwarding; configuration and play states.
3. Commands: Minecraft's argument types as `ArgumentType` variants (and parser ids in
   `command/network.rs`), the vanilla tree from `Commands`, the executor over `ContextChain`.
4. Blocks (`BlockState` table from the server jar's reports), chunk sections over `world/chunk`, chunk
   NBT, then the entity arena, following "Data" below.

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
