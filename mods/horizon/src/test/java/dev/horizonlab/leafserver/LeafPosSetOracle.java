// Writes the order of vanilla's `HashSet<BlockPos>` for horizon-server's position set
// (crates/horizon-server/src/worldgen/feature/tree/pos_set.rs), the order tree and vegetation features
// draw random numbers in. Run with the server owner's own 26.3 jar and its libraries on the class
// path (Java's single-file source launcher): `java -cp <jar:libs> LeafPosSetOracle.java <out.txt>`;
// the file is then given to the ignored test `pos_set_matches_java` (HORIZON_POS_SET_ORACLE).
// Original Horizon code.
// Rows (each case starts from an empty set):
//   case <name>
//   insert <x> <y> <z> <true|false>     HashSet.add and its answer
//   pop <x> <y> <z>                     iterator().next() then iterator.remove()
//   order <x> <y> <z>[,<x> <y> <z>...]  the iteration order at that point
// Positions are chosen so that no two distinct positions of a case share a hash code: vanilla orders
// those by identity hash codes inside a tree bin, which no other program can reproduce.

package dev.horizonlab.leafserver;

import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.StringJoiner;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;

public final class LeafPosSetOracle {
    private final PrintWriter out;
    private HashSet<BlockPos> set;
    private Map<Integer, BlockPos> hashes;

    private LeafPosSetOracle(PrintWriter out) {
        this.out = out;
    }

    private void start(String name) {
        set = new HashSet<>();
        hashes = new HashMap<>();
        out.println("case " + name);
    }

    private static String text(BlockPos p) {
        return p.getX() + " " + p.getY() + " " + p.getZ();
    }

    /** Adds `p` unless another position already took its hash code. */
    private void insert(BlockPos p) {
        BlockPos owner = hashes.putIfAbsent(p.hashCode(), p);
        if (owner != null && !owner.equals(p)) {
            return;
        }
        out.println("insert " + text(p) + " " + set.add(p));
    }

    private BlockPos pop() {
        Iterator<BlockPos> it = set.iterator();
        BlockPos first = it.next();
        it.remove();
        out.println("pop " + text(first));
        return first;
    }

    private void order() {
        StringJoiner joined = new StringJoiner(",");
        for (BlockPos p : set) {
            joined.add(text(p));
        }
        out.println("order " + joined);
    }

    /** Positions along (1, 2, 1): their hash codes step by 1024, so they share a bucket in every
     *  table of up to 1024 buckets and make tree bins, with noise around them to grow the table. */
    private void collisionLine(String name, BlockPos base, int length, int noise, long seed) {
        start(name);
        Random random = new Random(seed);
        List<BlockPos> all = new ArrayList<>();
        for (int k = 0; k < length; k++) {
            all.add(base.offset(k, 2 * k, k));
        }
        for (int i = 0; i < noise; i++) {
            all.add(base.offset(random.nextInt(24) - 12, random.nextInt(24) - 12, random.nextInt(24) - 12));
        }
        java.util.Collections.shuffle(all, random);
        for (BlockPos p : all) {
            insert(p);
        }
        order();
        // Pops (tree removals, rebalancing) interleaved with more inserts into the same buckets.
        for (int round = 0; round < length; round++) {
            if (!set.isEmpty() && random.nextInt(3) != 0) {
                pop();
            } else {
                int k = length + round;
                insert(base.offset(k, 2 * k, k));
            }
            if (round % 16 == 0) {
                order();
            }
        }
        order();
        while (!set.isEmpty()) {
            pop();
        }
    }

    /** `updateLeaves`' shape: pop a position, push its neighbours, over a box of leaves. */
    private void spread(String name, BlockPos origin, int radius, int steps, long seed) {
        start(name);
        Random random = new Random(seed);
        insert(origin);
        for (int step = 0; step < steps && !set.isEmpty(); step++) {
            BlockPos p = pop();
            for (Direction direction : Direction.values()) {
                BlockPos n = p.relative(direction);
                if (n.distManhattan(origin) <= radius && random.nextInt(4) != 0) {
                    insert(n);
                }
            }
            if (step % 64 == 0) {
                order();
            }
        }
        order();
    }

    private void blob(String name, BlockPos origin, int count, int size, long seed) {
        start(name);
        Random random = new Random(seed);
        for (int i = 0; i < count; i++) {
            insert(origin.offset(random.nextInt(size), random.nextInt(size), random.nextInt(size)));
        }
        order();
        for (int i = 0; i < count / 2; i++) {
            pop();
        }
        order();
    }

    public static void main(String[] args) throws Exception {
        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            LeafPosSetOracle oracle = new LeafPosSetOracle(out);
            oracle.collisionLine("line_origin", new BlockPos(0, 0, 0), 40, 60, 1);
            oracle.collisionLine("line_far", new BlockPos(29_999_000, -60, -29_998_000), 80, 200, 2);
            oracle.collisionLine("line_long", new BlockPos(-1234, 70, 5678), 160, 600, 3);
            oracle.collisionLine("line_small_table", new BlockPos(17, 64, -9), 12, 0, 4);
            oracle.spread("spread_tree", new BlockPos(100, 80, -300), 6, 2000, 5);
            oracle.spread("spread_far", new BlockPos(-29_999_900, 300, 29_999_900), 9, 4000, 6);
            for (int seed = 0; seed < 8; seed++) {
                oracle.blob("blob_" + seed, new BlockPos(seed * 997 - 4000, seed * 13 - 50, seed * -7919), 300 + seed * 150, 30, 100 + seed);
            }
        }
    }
}
