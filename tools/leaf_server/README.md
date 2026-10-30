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
outputs (`config.tsv`, `density.txt`), a flat world made by the real 26.3 server (the fixture that
`world/storage_tests.rs` describes), and `realdata.env` with the variables below.

| Variable | Used by |
|---|---|
| `HORIZON_VANILLA_JAR` | worldgen oracle tests, `real_registries_match_java` |
| `HORIZON_SERVER_JAR`, `HORIZON_SERVER_BLOCKS_JSON`, `HORIZON_SERVER_BLOCK_STATES`, `HORIZON_SERVER_VANILLA_WORLD` | `world/storage_tests.rs` (`reads_a_world_made_by_vanilla`), `world/chunk/section_tests.rs` |
| `HORIZON_REGISTRIES_JSON`, `HORIZON_ORACLE_TSV` | `network/configuration/tests.rs` (`LeafConfigurationOracle.java`) |
| `HORIZON_DENSITY_ORACLE` | `worldgen/oracle_tests.rs` (`LeafDensityOracle.java`); `density_program_speed` is a benchmark |

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
