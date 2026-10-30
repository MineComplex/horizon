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
- Ported: Leaf's NMS, Leaf/Gale/Purpur/Pufferfish/Lithium/Moonrise server code, the Bukkit/Paper API (same shape
  for plugin authors, implemented directly on the server's own state, no CraftBukkit wrapper layer).
- Not ported (no Java library is ported; Rust crates or code taken from azalea instead):
  JDK -> std/tokio/rayon/chrono/...; netty -> tokio + `azalea::buf`; Gson -> `serde_json`; JOML -> `glam`;
  Guava/fastutil/Caffeine -> std/`moka`; YAML configs -> `serde_yaml`; log4j/slf4j -> `log`;
  jline -> `rustyline`; joptsimple -> `clap`; Brigadier -> `azalea::brigadier`; Adventure -> `azalea::chat`;
  translations -> `azalea::language` (`en_us.json` read at run time, never committed); encryption ->
  `azalea::crypto`. DFU `Codec`s become plain (de)serialization functions/`serde` over JSON and NBT.
- Left out entirely: data fixing (26.3 worlds only: DFU datafixers, `util.datafix`, DataConverter), Sentry,
  spark, oshi, the status protocol (Velocity answers pings), Query, RCON, JSON-RPC management, JFR, secure
  seed, and the API remnants of these (list-ping events, server icon/MOTD, remote console sender).
- `src/azalea/`: code taken from azalea (MIT) as modules, not dependencies.

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
packets), `server` (tick loop, players, commands, config: server.properties, paper/leaf YAML), `api`
(Bukkit/Paper API for plugins), `azalea` (taken code).
