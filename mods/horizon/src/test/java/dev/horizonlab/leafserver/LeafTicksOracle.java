// Writes what 26.3's scheduled block ticks do, for horizon-server's tick containers
// (crates/horizon-server/src/world/ticks.rs, tests in ticks_oracle_tests.rs): synthetic schedules
// driven through the real `LevelTicks<Block>` and `LevelChunkTicks<Block>`, and `SavedTick`'s codec
// as the chunk NBT uses it. Run with the server owner's own 26.3 jar and its libraries on the class
// path (Java's single-file source launcher): `java -cp <jar:libs> LeafTicksOracle.java <out.txt>`;
// the file is then given to the ignored test `ticks_match_java` (HORIZON_TICKS_ORACLE).
// Original Horizon code. The file is the script and its answers: the Rust test replays each `do`
// line and compares what follows it (lines starting `= `).
//
// Block types are indexes 0..3 (stone, dirt, sand, gravel). Rows:
//   scenario <n> <normal|tolerant>        a fresh LevelTicks, game time 100, sub-tick counter 0;
//                                         tolerant: only the set of ticks run per `tick` is compared
//                                         (containers unpacked from disk tie in drain order, which
//                                         vanilla leaves to its hash maps)
//   do container <cx> <cz> <n>            addContainer(new LevelChunkTicks(pending)); n `pend` rows follow:
//   pend <type> <x> <y> <z> <delay> <priority value>
//   do ticking <cx> <cz> <0|1>            the tickCheck's answer for that chunk
//   do unpack <cx> <cz>                   the container's unpack(game time)
//   do schedule <type> <x> <y> <z> <delay> <priority value>   sub-tick order = the counter
//   do has <type> <x> <y> <z>             = has <bool>
//   do tick <max>                         = ran <type> <x> <y> <z> / will <type> <x> <y> <z> <bool>
//                                         per tick run, then = count <n>. The callback asks
//                                         willTickThisTick of the next block east, and for
//                                         (x + z) mod 5 == 0 schedules the same type one up with
//                                         delay x mod 3, priority (y mod 7) - 3 (`tolerant`: no callback)
//   do advance                            game time + 1
//   do remove <cx> <cz>                   = removed <bool>
//   do pack <cx> <cz>                     = saved <type> <x> <y> <z> <delay> <priority> ..., = packed <n>
//   do clear <x0> <y0> <z0> <x1> <y1> <z1>
//   do copy <x0> <y0> <z0> <x1> <y1> <z1> <dx> <dy> <dz>
//   do count                              = count <n>
//   do codec_read <cx> <cz> <snbt>        SavedTick.codec(BLOCK).listOf() read, then filterTickListForChunk:
//                                         = saved <type name> <x> <y> <z> <delay> <priority> ..., = read <n>
//   do codec_write <snbt of list>         the same list read and written back: = tag <snbt>

package dev.horizonlab.leafserver;

import com.mojang.serialization.Codec;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.Set;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.nbt.TagParser;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.ticks.LevelChunkTicks;
import net.minecraft.world.ticks.LevelTicks;
import net.minecraft.world.ticks.SavedTick;
import net.minecraft.world.ticks.ScheduledTick;
import net.minecraft.world.ticks.TickPriority;

public final class LeafTicksOracle {
    // Set after the bootstrap (the blocks need it).
    private static Block[] TYPES;
    private static Codec<List<SavedTick<Block>>> CODEC;

    private final PrintWriter out;
    private LevelTicks<Block> ticks;
    private final Set<Long> ticking = new HashSet<>();
    private final Map<Long, LevelChunkTicks<Block>> containers = new HashMap<>();
    private long time;
    private long subTicks;
    private boolean callback;

    private LeafTicksOracle(PrintWriter out) {
        this.out = out;
    }

    private static int typeIndex(Block block) {
        for (int i = 0; i < TYPES.length; i++) {
            if (TYPES[i] == block) {
                return i;
            }
        }
        throw new IllegalStateException(String.valueOf(block));
    }

    private void result(String line) {
        out.println("= " + line);
    }

    private String saved(SavedTick<Block> t, boolean byName) {
        String type = byName ? BuiltInRegistries.BLOCK.getKey(t.type()).toString() : String.valueOf(typeIndex(t.type()));
        return "saved " + type + " " + t.pos().getX() + " " + t.pos().getY() + " " + t.pos().getZ() + " " + t.delay() + " " + t.priority().getValue();
    }

    private void scenario(int n, boolean tolerant) {
        out.println("scenario " + n + " " + (tolerant ? "tolerant" : "normal"));
        ticking.clear();
        containers.clear();
        time = 100;
        subTicks = 0;
        callback = !tolerant;
        ticks = new LevelTicks<>(ticking::contains);
    }

    private void addContainer(int cx, int cz, List<SavedTick<Block>> pending) {
        out.println("do container " + cx + " " + cz + " " + pending.size());
        for (SavedTick<Block> t : pending) {
            out.println("pend " + typeIndex(t.type()) + " " + t.pos().getX() + " " + t.pos().getY() + " " + t.pos().getZ() + " " + t.delay() + " " + t.priority().getValue());
        }
        LevelChunkTicks<Block> container = pending.isEmpty() ? new LevelChunkTicks<>() : new LevelChunkTicks<>(pending);
        containers.put(ChunkPos.pack(cx, cz), container);
        ticks.addContainer(new ChunkPos(cx, cz), container);
    }

    private void schedule(Block type, BlockPos pos, int delay, TickPriority priority, boolean print) {
        if (print) {
            out.println("do schedule " + typeIndex(type) + " " + pos.getX() + " " + pos.getY() + " " + pos.getZ() + " " + delay + " " + priority.getValue());
        }
        ticks.schedule(new ScheduledTick<>(type, pos, time + delay, priority, subTicks++));
    }

    private void runTick(int max) {
        out.println("do tick " + max);
        ticks.tick(time, max, (pos, type) -> {
            result("ran " + typeIndex(type) + " " + pos.getX() + " " + pos.getY() + " " + pos.getZ());
            if (!callback) {
                return;
            }
            BlockPos east = pos.offset(1, 0, 0);
            result("will " + typeIndex(type) + " " + east.getX() + " " + east.getY() + " " + east.getZ() + " " + ticks.willTickThisTick(east, type));
            if (Math.floorMod(pos.getX() + pos.getZ(), 5) == 0) {
                BlockPos up = pos.offset(0, 1, 0);
                int delay = Math.floorMod(pos.getX(), 3);
                TickPriority priority = TickPriority.byValue(Math.floorMod(pos.getY(), 7) - 3);
                out.println("# follow-up " + typeIndex(type) + " " + up.getX() + " " + up.getY() + " " + up.getZ() + " " + delay + " " + priority.getValue());
                schedule(type, up, delay, priority, false);
            }
        });
        result("count " + ticks.count());
    }

    private void randomScenario(int n, long seed, boolean tolerant, int pendingContainers) {
        Random r = new Random(seed);
        scenario(n, tolerant);
        for (int cx = -1; cx <= 1; cx++) {
            for (int cz = -1; cz <= 1; cz++) {
                List<SavedTick<Block>> pending = new ArrayList<>();
                if ((cx + 1) * 3 + (cz + 1) < pendingContainers) {
                    int count = 1 + r.nextInt(6);
                    for (int i = 0; i < count; i++) {
                        pending.add(new SavedTick<>(TYPES[r.nextInt(4)], new BlockPos(cx * 16 + r.nextInt(16), 60 + r.nextInt(5), cz * 16 + r.nextInt(16)), r.nextInt(8) - 1, TickPriority.byValue(r.nextInt(7) - 3)));
                    }
                }
                addContainer(cx, cz, pending);
                out.println("do ticking " + cx + " " + cz + " 1");
                ticking.add(ChunkPos.pack(cx, cz));
            }
        }
        for (int step = 0; step < 90; step++) {
            int roll = r.nextInt(100);
            int x = r.nextInt(48) - 24;
            int z = r.nextInt(48) - 24;
            int y = 60 + r.nextInt(5);
            if (roll < 45) {
                schedule(TYPES[r.nextInt(4)], new BlockPos(x, y, z), r.nextInt(5), TickPriority.byValue(r.nextInt(7) - 3), true);
            } else if (roll < 68) {
                int max = tolerant ? 1_000_000 : new int[] {1, 2, 3, 5, 1000}[r.nextInt(5)];
                runTick(max);
                out.println("do advance");
                time++;
            } else if (roll < 73) {
                int t = r.nextInt(4);
                out.println("do has " + t + " " + x + " " + y + " " + z);
                result("has " + ticks.hasScheduledTick(new BlockPos(x, y, z), TYPES[t]));
            } else if (roll < 77) {
                int cx = r.nextInt(3) - 1;
                int cz = r.nextInt(3) - 1;
                boolean on = r.nextBoolean();
                out.println("do ticking " + cx + " " + cz + " " + (on ? 1 : 0));
                if (on) {
                    ticking.add(ChunkPos.pack(cx, cz));
                } else {
                    ticking.remove(ChunkPos.pack(cx, cz));
                }
            } else if (roll < 81) {
                int cx = r.nextInt(3) - 1;
                int cz = r.nextInt(3) - 1;
                LevelChunkTicks<Block> container = containers.get(ChunkPos.pack(cx, cz));
                if (container != null) {
                    out.println("do unpack " + cx + " " + cz);
                    container.unpack(time);
                }
            } else if (roll < 85) {
                int x1 = x + r.nextInt(12);
                int z1 = z + r.nextInt(12);
                out.println("do clear " + x + " 60 " + z + " " + x1 + " 64 " + z1);
                ticks.clearArea(new BoundingBox(x, 60, z, x1, 64, z1));
            } else if (roll < 88) {
                int x1 = x + r.nextInt(12);
                int z1 = z + r.nextInt(12);
                int dx = r.nextInt(33) - 16;
                int dz = r.nextInt(33) - 16;
                out.println("do copy " + x + " 60 " + z + " " + x1 + " 64 " + z1 + " " + dx + " 0 " + dz);
                ticks.copyArea(new BoundingBox(x, 60, z, x1, 64, z1), new net.minecraft.core.Vec3i(dx, 0, dz));
            } else if (roll < 91) {
                int cx = r.nextInt(3) - 1;
                int cz = r.nextInt(3) - 1;
                out.println("do remove " + cx + " " + cz);
                boolean had = containers.remove(ChunkPos.pack(cx, cz)) != null;
                ticks.removeContainer(new ChunkPos(cx, cz));
                result("removed " + had);
                if (r.nextBoolean()) {
                    addContainer(cx, cz, new ArrayList<>());
                }
            } else if (roll < 96) {
                int cx = r.nextInt(3) - 1;
                int cz = r.nextInt(3) - 1;
                LevelChunkTicks<Block> container = containers.get(ChunkPos.pack(cx, cz));
                out.println("do pack " + cx + " " + cz);
                if (container == null) {
                    result("packed 0");
                } else {
                    List<SavedTick<Block>> packed = container.pack(time);
                    packed.forEach(t -> result(saved(t, false)));
                    result("packed " + packed.size());
                }
            } else {
                out.println("do count");
                result("count " + ticks.count());
            }
        }
    }

    private void codecRead(int cx, int cz, String snbt) throws Exception {
        out.println("do codec_read " + cx + " " + cz + " " + snbt);
        Tag tag = TagParser.parseCompoundFully("{v:" + snbt + "}").get("v");
        List<SavedTick<Block>> read = CODEC.parse(NbtOps.INSTANCE, tag).result().orElse(List.of());
        read = SavedTick.filterTickListForChunk(read, new ChunkPos(cx, cz));
        read.forEach(t -> result(saved(t, true)));
        result("read " + read.size());
    }

    private void codecWrite(String snbt) throws Exception {
        out.println("do codec_write " + snbt);
        Tag tag = TagParser.parseCompoundFully("{v:" + snbt + "}").get("v");
        List<SavedTick<Block>> read = CODEC.parse(NbtOps.INSTANCE, tag).result().orElse(List.of());
        result("tag " + CODEC.encodeStart(NbtOps.INSTANCE, read).getOrThrow());
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        TYPES = new Block[] {Blocks.STONE, Blocks.DIRT, Blocks.SAND, Blocks.GRAVEL};
        CODEC = SavedTick.codec(BuiltInRegistries.BLOCK.byNameCodec()).listOf();
        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            LeafTicksOracle o = new LeafTicksOracle(out);
            for (int i = 0; i < 12; i++) {
                o.randomScenario(i, 1000 + i, false, i % 3 == 0 ? 0 : 1);
            }
            // Several containers unpacked from disk: ties in the drain order.
            o.randomScenario(12, 5000, true, 9);
            o.randomScenario(13, 5001, true, 4);
            String ok = "{i:\"minecraft:stone\",x:3,y:64,z:5,t:7,p:-1}";
            String[] reads = {
                "[" + ok + ",{i:\"dirt\",x:20,y:64,z:5,t:0,p:0},{i:\"minecraft:sand\",x:-1,y:-60,z:15,t:-3,p:3}]",
                "[" + ok + ",{i:\"minecraft:stone\",x:3,y:64,z:5,t:7}]",
                "[" + ok + ",{i:\"minecraft:nothing\",x:3,y:64,z:5,t:7,p:0}]",
                "[{i:\"minecraft:stone\",x:3,y:64,z:5,t:7,p:7},{i:\"minecraft:stone\",x:4,y:64,z:5,t:7,p:-9},{i:\"minecraft:stone\",x:5,y:64,z:5,t:7,p:2}]",
                "[{i:\"minecraft:stone\",x:3.9d,y:-1.5d,z:5.5f,t:4294967297L,p:1b}]",
                "[{i:\"minecraft:stone\",x:\"3\",y:64,z:5,t:7,p:0}]",
                "[{i:5,x:3,y:64,z:5,t:7,p:0}]",
                "[3]",
                "{i:\"minecraft:stone\",x:3,y:64,z:5,t:7,p:0}",
                "[]",
                "[{i:\"MINECRAFT:stone\",x:3,y:64,z:5,t:7,p:0}]",
                "[{i:\"minecraft:stone\",x:3,y:64,z:5,t:7,p:0},{i:\"minecraft:stone\",x:3,y:64,z:5,t:9,p:1}]",
                "\"text\"",
            };
            for (String snbt : reads) {
                o.codecRead(0, 0, snbt);
            }
            o.codecRead(-1, 0, reads[0]);
            for (String snbt : new String[] {reads[0], reads[3], reads[4], reads[11], "[]"}) {
                o.codecWrite(snbt);
            }
        }
    }
}
