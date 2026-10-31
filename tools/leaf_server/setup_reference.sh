#!/usr/bin/env bash
# Builds everything horizon-server's real-data checks need, into a scratch folder outside the repo,
# and writes <out>/realdata.env to source before `cargo test -p horizon-server` (README.md here).
#
#   tools/leaf_server/setup_reference.sh <out dir>
#
# Environment: LEAF_DIR (default: ../Leaf next to this repo), JAVA (a Java 25 `java`; default: `java`).
# Nothing here is committed: the outputs are Mojang data or derived from it.
set -euo pipefail

out=${1:?usage: setup_reference.sh <out dir>}
repo=$(cd "$(dirname "$0")/../.." && pwd)
leaf=${LEAF_DIR:-$repo/../Leaf}
java=${JAVA:-java}
mkdir -p "$out"
out=$(cd "$out" && pwd)

cache=$leaf/leaf-server/.gradle/caches/paperweight
jar=$cache/data/bundler/server.jar
libs=$cache/jars/minecraft
[ -f "$jar" ] || { echo "no 26.3 server jar at $jar (run Leaf's Gradle once)" >&2; exit 1; }
"$java" -version 2>&1 | grep -q '"25' || { echo "JAVA must be Java 25" >&2; exit 1; }
cp=$jar:$(find "$libs" -name '*.jar' | sort | paste -sd: -)
oracles=$repo/mods/horizon/src/test/java/dev/horizonlab/leafserver

echo "== prepare step (reports/, block_states.bin)"
python3 "$repo/tools/leaf_server_reports.py" --jar "$jar" --libraries "$libs" \
    --server-dir "$out/serverdir" --java "$java"

echo "== oracles"
mkdir -p "$out/oracle-run"
(cd "$out/oracle-run" &&
    "$java" -cp "$cp" "$oracles/LeafConfigurationOracle.java" "$out/config.tsv" &&
    "$java" -cp "$cp" "$oracles/LeafDensityOracle.java" "$out/density.txt" &&
    "$java" -cp "$cp" "$oracles/LeafTerrainOracle.java" "$out/terrain.txt" &&
    "$java" -cp "$cp" "$oracles/LeafSurfaceOracle.java" "$out/surface.txt" &&
    "$java" -cp "$cp" "$oracles/LeafCarverOracle.java" "$out/carver.txt" &&
    "$java" -cp "$cp" "$oracles/LeafBiomeOracle.java" "$out/biomes.txt" &&
    "$java" -cp "$cp" "$oracles/LeafStructurePlacementOracle.java" "$out/structures.txt" &&
    "$java" -cp "$cp" "$oracles/LeafPlacementOracle.java" "$out/placement.txt" &&
    "$java" -cp "$cp" "$oracles/LeafFeatureOracle.java" "$out/feature.txt" &&
    "$java" -cp "$cp" "$oracles/LeafChunkTicketsOracle.java" "$out/tickets.txt" &&
    "$java" -cp "$cp" "$oracles/LeafUserListsOracle.java" "$out/user_lists.jsonl" &&
    "$java" -cp "$cp" "$oracles/LeafLevelDataOracle.java" "$out/level_data.jsonl") 2>&1 |
    grep -v -i warning || true

echo "== vanilla fixture world (crates/horizon-server/src/world/storage_tests.rs)"
world=$out/vanilla-world-flat
rm -rf "$world"
mkdir -p "$world/home"
printf 'eula=true\n' >"$world/eula.txt"
printf 'level-type=minecraft\\:flat\nlevel-seed=12345\ndifficulty=hard\ngamemode=creative\nonline-mode=false\nserver-port=25599\n' \
    >"$world/server.properties"
(
    cd "$world"
    {
        sleep 25
        echo 'forceload add 0 0 15 15'
        sleep 5
        echo 'setblock 3 -60 3 minecraft:oak_leaves'
        echo 'setblock 4 -55 4 minecraft:stone'
        sleep 3
        echo stop
    } | HOME=$world/home XDG_DATA_HOME=$world/home XDG_CONFIG_HOME=$world/home \
        "$java" -Xmx2G -cp "$cp" net.minecraft.server.Main --nogui >"$world/server.log" 2>&1
)
grep -q 'Changed the block at 4, -55, 4' "$world/server.log" ||
    { echo "the fixture world was not made; see $world/server.log" >&2; exit 1; }

reports=$out/serverdir/cache/vanilla-26.3
cat >"$out/realdata.env" <<EOF
export CARGO_INCREMENTAL=0
export HORIZON_VANILLA_JAR=$jar
export HORIZON_SERVER_JAR=$jar
export HORIZON_SERVER_BLOCKS_JSON=$reports/reports/blocks.json
export HORIZON_SERVER_BLOCK_STATES=$reports/block_states.bin
export HORIZON_SERVER_VANILLA_WORLD=$world/world
export HORIZON_REGISTRIES_JSON=$reports/reports/registries.json
export HORIZON_ORACLE_TSV=$out/config.tsv
export HORIZON_DENSITY_ORACLE=$out/density.txt
export HORIZON_TERRAIN_ORACLE=$out/terrain.txt
export HORIZON_SURFACE_ORACLE=$out/surface.txt
export HORIZON_CARVER_ORACLE=$out/carver.txt
export HORIZON_LEVEL_DATA_ORACLE=$out/level_data.jsonl
export HORIZON_BIOME_ORACLE=$out/biomes.txt
export HORIZON_BIOME_REPORTS=$reports/reports
export HORIZON_STRUCTURE_ORACLE=$out/structures.txt
export HORIZON_PLACEMENT_ORACLE=$out/placement.txt
export HORIZON_FEATURE_ORACLE=$out/feature.txt
export HORIZON_TICKETS_ORACLE=$out/tickets.txt
export HORIZON_USER_LISTS_ORACLE=$out/user_lists.jsonl
EOF
echo "== done: . $out/realdata.env"
