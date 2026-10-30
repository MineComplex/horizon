# Leaf 26.3 — Native Rust Porting Instructions

(The user's instructions for this crate, 2026-10-30. ARCHITECTURE.md applies them.)

## Main Objective

We are rewriting the entire Leaf 26.3 Minecraft server from Java to native Rust.

The goal is NOT to mechanically translate Java source code into Rust syntax.

The goal is to build a fully native, memory-safe, high-performance Minecraft server using idiomatic Rust
architecture while preserving the original Minecraft and Leaf behavior.

**CRITICAL: PORT BEHAVIOR, NOT JAVA ARCHITECTURE.**

## 1. Do Not Emulate Java

Do not recreate Java's object model, runtime, memory management, inheritance, or synchronization architecture.

The following patterns are prohibited:

- `JRef<T> = Arc<T>` as a universal Java reference replacement.
- `JObject`, `JObjectRef`, `JGeneric`.
- `Mut<T>` implemented using `UnsafeCell<T>`.
- Universal Java-style `get()` and `set()` field wrappers.
- `jobject!`, `cast_arc`, `instance_of` and runtime class emulation.
- Java-style object monitors.
- Automatic replacement of `synchronized` with `ReentrantMutex`.
- Universal `Arc<dyn Trait>` object storage.
- Using `&self` for all methods to imitate Java.
- Using `unsafe` to bypass Rust ownership rules.
- Recreating Java reflection, garbage collection or class loading.

Do not create a JVM compatibility layer.

Use native Rust ownership, borrowing, composition, enums, traits and generics.

## 2. Do Not Port the Java Standard Library

Do not translate or recreate `java.lang.*`, `java.util.*`, `java.io.*`, `java.nio.*`, `java.net.*`,
`java.time.*`, `java.math.*`, `java.security.*`, `java.util.concurrent.*`, `java.lang.reflect.*`, `javax.*`,
`jdk.internal.*`, `sun.*`.

Use Rust's standard library and appropriate existing crates. For example: `ArrayList` → `Vec`; `HashMap` →
`HashMap` or `hashbrown`; `HashSet` → `HashSet`; `ByteBuffer` → `bytes` or slices; `UUID` → `uuid`;
`ExecutorService` → an appropriate Rust execution model; `CompletableFuture` → Rust futures; `AtomicInteger`
→ Rust atomics; `System.nanoTime()` → `Instant`.

These mappings are examples, not mandatory one-to-one replacements. Preserve the required semantics.

Do not create custom Java compatibility classes merely to make translated code compile.

## 3. Do Not Port Shaded Dependencies

Leaf/Paper includes numerous external Java libraries. **DO NOT rewrite the source code of these libraries into
Rust.** This includes Netty, FastUtil, Guava, Gson, Jackson, SLF4J, Log4j, Apache Commons, Caffeine, JCTools,
Brigadier, Adventure, DataFixerUpper, ASM, ByteBuddy and other shaded or relocated dependencies.

Instead:

1. Identify the original dependency.
2. Determine which functionality Leaf actually uses.
3. Find an appropriate native Rust implementation.
4. Prefer established, maintained Rust crates.
5. Implement only missing Minecraft-specific functionality.
6. Preserve required behavior through tests.

Examples: Netty → Tokio / Mio / Bytes; FastUtil → Hashbrown / Vec / specialized collections; Gson / Jackson →
Serde; SLF4J / Log4j → Tracing; Caffeine → Moka or specialized caching; JCTools → Crossbeam; Guava → Rust std
and focused crates.

Do not assume these replacements are automatically behaviorally equivalent.

For Minecraft-specific functionality such as Brigadier commands, Adventure components or DataFixerUpper
migrations, implement the required semantics natively when no suitable Rust equivalent exists.

Do not port an entire library when only a small subset is required.

## 4. Distinguish Minecraft Code From Dependencies

Before porting any Java class, identify its origin:

1. Mojang Minecraft server implementation.
2. Paper/Leaf patches and server-specific functionality.
3. Bukkit/Paper API contracts.
4. Java standard library.
5. External or shaded dependency.
6. Build-time or generated code.

Categories 1–3 may require native Rust implementations. Categories 4–6 must not be mechanically ported.
Even if a shaded library has been relocated into another package namespace, it remains an external dependency.

## 5. Use Native Rust Architecture

Use the Rust ownership model correctly. Prefer: `T` for owned values; `&T` for immutable access; `&mut T` for
exclusive mutable access; `Box<T>` when heap allocation is necessary; `Arc<T>` only for genuinely shared
ownership; `Cell<T>` / `RefCell<T>` for justified single-threaded interior mutability; `Mutex<T>` /
`RwLock<T>` only when synchronization is necessary; atomics for suitable shared primitive values; generational
handles and IDs for long-lived object references.

Do not introduce reference counting, locks or interior mutability without a concrete reason.
Do not reproduce Java inheritance hierarchies mechanically. Prefer composition, traits, enums and generics.

## 6. Optimize Minecraft Data Structures

Minecraft server performance is a major priority. Pay special attention to: entity storage and ticking; chunk
storage and access; block states and block positions; collision detection; movement physics; pathfinding and
AI; lighting; redstone; block updates; world generation; packet processing; serialization and persistence.

Avoid unnecessary heap allocations, pointer chasing, atomic reference counting and dynamic dispatch in hot paths.
Consider generational arenas, dense arrays, SoA and data-oriented structures where appropriate.
Do not allocate an `Arc` for every Entity, BlockPos or BlockState.
Use profiling and benchmarks to validate performance improvements.

## 7. Threading and Synchronization

Do not reproduce Java synchronization patterns automatically. Use explicit ownership of game state. World,
entity and chunk mutations must follow a well-defined execution model. Asynchronous workers must not
arbitrarily mutate tick-owned state. Use message passing, task queues or appropriate synchronization when
crossing ownership boundaries. Preserve the original ordering requirements of Minecraft ticking and gameplay.
Do not introduce parallel execution that changes game behavior.

## 8. Memory Safety

Avoid `unsafe` by default. Never use `UnsafeCell` simply to imitate Java mutable fields. Never create
overlapping mutable references or unsynchronized data races. Do not implement `Send` or `Sync` manually without
proving their safety invariants. If Rust's borrow checker rejects the design, reconsider ownership and data flow
instead of bypassing it. Any necessary unsafe implementation must have documented invariants, focused tests
and a clear justification.

## 9. Preserve Leaf 26.3 Behavior

The server must preserve: Minecraft protocol compatibility; packet ordering and serialization; tick behavior;
entity mechanics; movement and collision physics; block mechanics; redstone behavior; chunk generation and
persistence; world data compatibility; game rules; relevant Paper/Leaf features.

Do not simplify or remove Minecraft functionality merely because it is difficult to port. The Java source is
the reference for behavior and algorithms, not the required Rust architecture. Add differential tests against
the Java implementation for complex or behavior-sensitive systems.

## 10. Plugin System

Java plugin compatibility is NOT required. Do not implement JVM-based Bukkit/Paper plugin loading. Do not port
Java class loaders, reflection or plugin runtime infrastructure. A native Rust plugin system may be developed
later. **Do not implement or select a plugin runtime architecture yet.** Do not introduce WASM, dynamic native
plugins or a custom plugin framework without a separate architectural decision. For now, focus on the
Minecraft server core and keep reasonable module boundaries for future extensibility.

## 11. Audit Existing Rust Code

Audit the project for: Java runtime emulation; Java object model abstractions; ported JDK classes; ported
shaded dependencies; unsafe interior mutability; excessive Arc usage; Java-style inheritance emulation;
unnecessary synchronization; duplicated functionality already available in Rust crates. For each finding,
determine the correct native Rust replacement. Refactor incrementally without breaking existing functionality.
Do not continue expanding a flawed compatibility layer simply because other modules already depend on it.

## 12. Implementation Workflow

For every component:

1. Read and understand the original Java implementation.
2. Identify its actual responsibilities and dependencies.
3. Determine whether it belongs to Minecraft, Leaf, the JDK or an external library.
4. Skip mechanical porting of JDK and external libraries.
5. Design an idiomatic Rust representation.
6. Establish ownership and lifetime rules.
7. Implement the required behavior.
8. Add correctness and compatibility tests.
9. Check allocations, synchronization and performance.
10. Integrate the component with the native Rust server architecture.

Do not generate large quantities of mechanically translated code. Prefer a smaller amount of well-designed,
correct Rust code over a large Java compatibility framework.

## 13. Required Quality Standards

All implementations must: compile without unnecessary warnings; avoid undefined behavior; use idiomatic Rust;
avoid unnecessary allocations; avoid unnecessary synchronization; preserve required Minecraft behavior; have
clear ownership semantics; be maintainable and testable; use existing Rust crates where appropriate; avoid
introducing Java-specific abstractions. Do not claim performance improvements without measurement. Do not
sacrifice correctness or memory safety for theoretical speed.

## 14. Immediate Task

Before continuing the port: inspect the existing Rust workspace; identify Java compatibility layers and
mechanically ported dependencies; produce an inventory of problematic modules; identify their native Rust
replacements; determine which existing implementations can be reused; create an incremental refactoring plan;
begin replacing Java-specific architecture with native Rust implementations; preserve existing functionality
throughout migration. Do not blindly delete existing code. Do not perform an uncontrolled full-project rewrite.
Keep the project buildable at each reasonable migration milestone.
Maximum Reuse of Existing Rust Crates and Implementations
CRITICAL: Do not reinvent functionality that already exists in the Rust ecosystem.
The Horizon project must reuse existing, proven Rust implementations wherever possible instead of mechanically porting Java code or rewriting functionality from scratch.

### 15 Search Before Implementing

Before implementing or porting any component:
Search the existing Horizon workspace for equivalent functionality.
Inspect currently installed Cargo dependencies.
Search crates.io, docs.rs and relevant open-source Rust repositories.
Identify existing implementations that provide the required functionality.
Compare their behavior with the original Leaf 26.3 implementation.
Prefer reusing existing implementations when compatibility can be verified.
Write custom code only for functionality that is genuinely Minecraft-specific or not adequately covered by existing crates.
Never implement functionality from scratch without first checking whether a suitable Rust implementation already exists.

### 16 Avoid Duplicate Implementations

Horizon must not contain multiple independent implementations of the same functionality without a justified reason.
Examples:
Multiple VarInt implementations.
Multiple NBT parsers.
Multiple packet buffer abstractions.
Multiple UUID implementations.
Multiple compression wrappers.
Multiple Minecraft coordinate representations.
Multiple block-state registries.
Prefer a single well-defined implementation per responsibility.
Refactor callers to use shared native Rust components

## Final Instructions

We are building a fully native Minecraft server in Rust. We are NOT building a Java-to-Rust compatibility
runtime. We are NOT rebuilding the JDK. We are NOT rewriting shaded Java libraries. We are NOT supporting Java
plugins. We are NOT deciding the native plugin architecture yet.

**PORT MINECRAFT BEHAVIOR, NOT JAVA ARCHITECTURE.**
**REPLACE JAVA DEPENDENCIES, DO NOT REIMPLEMENT THE JAVA ECOSYSTEM.**
**USE RUST OWNERSHIP, NOT JAVA REFERENCE SEMANTICS.**
**PRIORITIZE CORRECTNESS, MEMORY SAFETY, PERFORMANCE AND MAINTAINABILITY.**
**REUSE VERIFIED RUST CODE FIRST. ADAPT WHEN NECESSARY. IMPLEMENT FROM SCRATCH ONLY WHEN JUSTIFIED.**
**HORIZON MUST BE A NATIVE RUST MINECRAFT SERVER, NOT A COLLECTION OF REWRITTEN JAVA LIBRARIES**