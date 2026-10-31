// Writes what 26.3's block changes and fluids do, for horizon-server's block-change path and fluids
// (crates/horizon-server/src/world/{block_update,fluid}.rs, tests in fluid_oracle_tests.rs):
// scripted worlds (pools, falls, flows over edges, lava meeting water, waterlogged slabs, sources in
// the Nether, plants washed away or losing their ground, random rooms) are built in a map-backed
// headless ServerLevel, changed with the real `Level.setBlock` and ticked N game ticks through the
// real `LevelTicks` and `FlowingFluid`. Run with the server owner's own 26.3 jar and its libraries on
// the class path (Java's single-file source launcher): `java -cp <jar:libs> LeafFluidOracle.java
// <out.txt>`; the file is then given to the ignored test `fluids_match_java` (HORIZON_FLUID_ORACLE).
// Original Horizon code (after the parity harness's headless level). Nothing it writes is committed.
//
// Rows (the Rust test replays the `scene`, `cell` and `op` rows and compares every `=` row):
//   scene <name> <overworld|the_nether> <seed>     a fresh level; its random is `LegacyRandomSource(seed)`
//   cell <x> <y> <z> <state id>                    put without updates
//   op set <x> <y> <z> <state id> <flags>          Level.setBlock
//   op ticks <n>                                   n game ticks: time + 1, block ticks, fluid ticks
//   = set <x> <y> <z> <state id> <flags>           a setBlock that returned true (once it returns)
//   = upd <x> <y> <z>                              the neighbour updater's debug listener
//   = chg <x> <y> <z>                              sendBlockUpdated
//   = ev <type> <x> <y> <z> <data>                 levelEvent
//   = tick <game time>                             after each game tick, then the world's state:
//   = state <x> <y> <z> <state id>                 every cell that differs from the scene's start
//   = ft|bt <x> <y> <z> <type> <delay> <priority>  the pending fluid / block ticks (sorted)
//   = rng <long>                                   the level random's next long at the scene's end

package dev.horizonlab.leafserver;

import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.RandomSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.redstone.CollectingNeighborUpdater;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.ticks.LevelChunkTicks;
import net.minecraft.world.ticks.LevelTicks;

public final class LeafFluidOracle {
    static Object unsafe;
    static Method allocate;
    static BlockState AIR;
    static Map<String, DimensionType> TYPES = new HashMap<>();

    @SuppressWarnings("unchecked")
    static <T> T make(Class<T> type) {
        try {
            return (T) allocate.invoke(unsafe, type);
        } catch (Exception e) {
            throw new AssertionError(e);
        }
    }

    static void set(Class<?> owner, Object target, String name, Object value) throws Exception {
        Field f = owner.getDeclaredField(name);
        f.setAccessible(true);
        f.set(target, value);
    }

    static String pos(BlockPos p) {
        return p.getX() + " " + p.getY() + " " + p.getZ();
    }

    /** The one chunk every position is in: `LevelChunk.setBlockState` over the level's map. */
    static final class Chunk extends LevelChunk {
        World world;

        Chunk() {
            super(null, null);
        }

        @Override
        public BlockState setBlockState(BlockPos pos, BlockState state, int flags) {
            BlockState old = world.cells.getOrDefault(pos, AIR);
            if (old == state) {
                return null;
            }
            if (state == AIR) {
                world.cells.remove(pos);
            } else {
                world.cells.put(pos.immutable(), state);
            }
            Block newBlock = state.getBlock();
            boolean blockChanged = !old.is(newBlock);
            boolean movedByPiston = (flags & 64) != 0;
            if ((blockChanged || newBlock instanceof net.minecraft.world.level.block.BaseRailBlock) && ((flags & 1) != 0 || movedByPiston)) {
                old.affectNeighborsAfterRemoval(world, pos, movedByPiston);
            }
            if (!world.getBlockState(pos).is(newBlock)) {
                return null;
            }
            if ((flags & 512) == 0) {
                state.onPlace(world, pos, old, movedByPiston);
            }
            return old;
        }

        @Override public void markUnsaved() { }
        @Override public BlockState getBlockState(BlockPos pos) { return world.getBlockState(pos); }
        @Override public FluidState getFluidState(BlockPos pos) { return world.getFluidState(pos); }
        @Override public net.minecraft.server.level.FullChunkStatus getFullStatus() { return net.minecraft.server.level.FullChunkStatus.ENTITY_TICKING; }
    }

    static final class World extends ServerLevel {
        Map<BlockPos, BlockState> cells;
        RandomSource rng;
        long time;
        long subTick;
        Chunk chunk;
        LevelTicks<Block> blockTicks;
        LevelTicks<Fluid> fluidTicks;
        GameRules rules;
        DimensionType type;
        ResourceKey<Level> key;
        net.minecraft.world.attribute.EnvironmentAttributeSystem attributes;
        Server server;
        PrintWriter out;

        World() {
            super(null, null, null, null, null, null, false, 0L, List.of(), false);
        }

        void emit(String line) {
            out.println("= " + line);
        }

        @Override public BlockState getBlockState(BlockPos p) {
            if (this.isOutsideBuildHeight(p)) return Blocks.VOID_AIR.defaultBlockState();
            return cells.getOrDefault(p, AIR);
        }
        @Override public FluidState getFluidState(BlockPos p) { return getBlockState(p).getFluidState(); }
        @Override public boolean setBlock(BlockPos p, BlockState s, int flags, int limit) {
            boolean changed = super.setBlock(p, s, flags, limit);
            if (changed) emit("set " + pos(p) + " " + Block.getId(s) + " " + flags);
            return changed;
        }
        @Override public LevelChunk getChunkAt(BlockPos p) { return chunk; }
        @Override public LevelChunk getChunk(int x, int z) { return chunk; }
        @Override public net.minecraft.world.level.chunk.ChunkAccess getChunk(int x, int z, net.minecraft.world.level.chunk.status.ChunkStatus status, boolean load) { return chunk; }
        @Override public boolean isLoaded(BlockPos p) { return true; }
        @Override public boolean hasChunkAt(BlockPos p) { return true; }
        @Override public boolean hasChunk(int x, int z) { return true; }
        @Override public boolean isClientSide() { return false; }
        @Override public RandomSource getRandom() { return rng; }
        @Override public int getMinY() { return type.minY(); }
        @Override public int getHeight() { return type.height(); }
        @Override public DimensionType dimensionType() { return type; }
        @Override public ResourceKey<Level> dimension() { return key; }
        @Override public long getGameTime() { return time; }
        @Override public long nextSubTickCount() { return subTick++; }
        @Override public LevelTicks<Block> getBlockTicks() { return blockTicks; }
        @Override public LevelTicks<Fluid> getFluidTicks() { return fluidTicks; }
        @Override public void sendBlockUpdated(BlockPos p, BlockState a, BlockState b, int f) { emit("chg " + pos(p)); }
        @Override public void setBlocksDirty(BlockPos p, BlockState a, BlockState b) { }
        @Override public void updatePOIOnBlockStateChange(BlockPos p, BlockState a, BlockState b) { }
        @Override public net.minecraft.core.RegistryAccess registryAccess() { return net.minecraft.core.RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY); }
        @Override public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() { return FeatureFlags.DEFAULT_FLAGS; }
        @Override public GameRules getGameRules() { return rules; }
        @Override public net.minecraft.server.MinecraftServer getServer() { return server; }
        @Override public net.minecraft.world.level.border.WorldBorder getWorldBorder() { return new net.minecraft.world.level.border.WorldBorder(); }
        @Override public int getSeaLevel() { return 63; }
        @Override public net.minecraft.world.attribute.EnvironmentAttributeSystem environmentAttributes() {
            if (attributes == null) attributes = net.minecraft.world.attribute.EnvironmentAttributeSystem.builder().addConstantLayer(type.attributes()).build();
            return attributes;
        }
        @Override public void levelEvent(Entity source, int event, BlockPos p, int data) { emit("ev " + event + " " + pos(p) + " " + data); }
        @Override public void gameEvent(Holder<net.minecraft.world.level.gameevent.GameEvent> e, Vec3 p, net.minecraft.world.level.gameevent.GameEvent.Context c) { }
        @Override public void playSeededSound(Entity except, double x, double y, double z, Holder<net.minecraft.sounds.SoundEvent> sound, net.minecraft.sounds.SoundSource source, float volume, float pitch, long seed) { }
        @Override public boolean addFreshEntity(Entity entity) { return true; }
        @Override public List<net.minecraft.server.level.ServerPlayer> players() { return List.of(); }
    }

    /** Loot tables are all empty here: drops are not compared (and draw nothing). */
    static final class Loot extends net.minecraft.server.ReloadableServerRegistries.Holder {
        Loot() {
            super(null);
        }
        @Override
        public net.minecraft.world.level.storage.loot.LootTable getLootTable(ResourceKey<net.minecraft.world.level.storage.loot.LootTable> id) {
            return net.minecraft.world.level.storage.loot.LootTable.EMPTY;
        }
    }

    static final class Server extends net.minecraft.server.MinecraftServer {
        Loot loot;
        Server() {
            super(null, null, null, null, null, null, null, null, null, false, null);
        }
        @Override public net.minecraft.server.ReloadableServerRegistries.Holder reloadableRegistries() { return loot; }
        @Override protected boolean initServer() { return false; }
        @Override public int getMaxPlayers() { return 0; }
        @Override public net.minecraft.server.permissions.LevelBasedPermissionSet operatorUserPermissions() { return null; }
        @Override public net.minecraft.server.permissions.PermissionSet getFunctionCompilationPermissions() { return null; }
        @Override public boolean shouldRconBroadcast() { return false; }
        @Override protected net.minecraft.util.debugchart.SampleLogger getTickTimeLogger() { return null; }
        @Override public boolean isTickTimeLoggingEnabled() { return false; }
        @Override public net.minecraft.SystemReport fillServerSystemReport(net.minecraft.SystemReport r) { return r; }
        @Override public boolean isDedicatedServer() { return true; }
        @Override public int getRateLimitPacketsPerSecond() { return 0; }
        @Override public int getCommandSpamThresholdSeconds() { return 0; }
        @Override public int getChatSpamThresholdSeconds() { return 0; }
        @Override public boolean useNativeTransport() { return false; }
        @Override public boolean isPublished() { return false; }
        @Override public boolean shouldInformAdmins() { return false; }
        @Override public boolean isSingleplayerOwner(net.minecraft.server.players.NameAndId id) { return false; }
    }

    /** A dimension type from the vanilla data pack (its attributes: the nether's fast lava). */
    static DimensionType loadDimensionType(ResourceKey<DimensionType> key) {
        var repo = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        repo.reload();
        repo.setSelected(List.of("vanilla"));
        var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA, repo.openAllSelected());
        var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
        var staticTags = net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(net.minecraft.server.RegistryLayer.STATIC));
        var context = net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLD), staticTags);
        var world = net.minecraft.resources.RegistryDataLoader.load(resources, context, net.minecraft.resources.RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        staticTags.forEach(net.minecraft.core.Registry.PendingTags::apply);
        return world.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(key).value();
    }

    // ---- scenes

    final PrintWriter out;
    World w;
    Map<BlockPos, BlockState> start;

    LeafFluidOracle(PrintWriter out) {
        this.out = out;
    }

    static BlockState state(String text) {
        try {
            return net.minecraft.commands.arguments.blocks.BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, text, false).blockState();
        } catch (Exception e) {
            throw new IllegalArgumentException(text, e);
        }
    }

    void scene(String name, String dimension, long seed) throws Exception {
        out.println("scene " + name + " " + dimension + " " + seed);
        w = make(World.class);
        w.out = out;
        w.cells = new HashMap<>();
        w.rng = new LegacyRandomSource(seed);
        w.time = 1000;
        w.subTick = 0;
        w.rules = new GameRules(FeatureFlags.DEFAULT_FLAGS);
        w.type = TYPES.get(dimension);
        w.key = dimension.equals("the_nether") ? Level.NETHER : Level.OVERWORLD;
        w.server = make(Server.class);
        w.server.loot = make(Loot.class);
        set(Level.class, w, "random", w.rng);
        // Sounds draw their seeds from a random of their own (a candle put out plays one).
        set(Level.class, w, "soundSeedGenerator", new LegacyRandomSource(0));
        CollectingNeighborUpdater updater = new CollectingNeighborUpdater(w, 1000000);
        World level = w;
        updater.setDebugListener(p -> level.emit("upd " + pos(p)));
        set(Level.class, w, "neighborUpdater", updater);
        w.chunk = make(Chunk.class);
        w.chunk.world = w;
        w.blockTicks = new LevelTicks<>(l -> true);
        w.fluidTicks = new LevelTicks<>(l -> true);
        for (int x = -3; x <= 3; x++) {
            for (int z = -3; z <= 3; z++) {
                w.blockTicks.addContainer(new ChunkPos(x, z), new LevelChunkTicks<>());
                w.fluidTicks.addContainer(new ChunkPos(x, z), new LevelChunkTicks<>());
            }
        }
        start = null;
    }

    void cell(int x, int y, int z, BlockState s) {
        out.println("cell " + x + " " + y + " " + z + " " + Block.getId(s));
        if (s != AIR) w.cells.put(new BlockPos(x, y, z), s);
    }

    void box(int x0, int y0, int z0, int x1, int y1, int z1, BlockState s) {
        for (int x = x0; x <= x1; x++) for (int y = y0; y <= y1; y++) for (int z = z0; z <= z1; z++) cell(x, y, z, s);
    }

    void op(int x, int y, int z, BlockState s, int flags) {
        if (start == null) start = new HashMap<>(w.cells);
        out.println("op set " + x + " " + y + " " + z + " " + Block.getId(s) + " " + flags);
        w.setBlock(new BlockPos(x, y, z), s, flags);
    }

    void op(int x, int y, int z, BlockState s) {
        op(x, y, z, s, 3);
    }

    void ticks(int n) throws Exception {
        if (start == null) start = new HashMap<>(w.cells);
        out.println("op ticks " + n);
        for (int i = 0; i < n; i++) {
            w.time++;
            w.blockTicks.tick(w.time, 65536, (p, b) -> {
                BlockState s = w.getBlockState(p);
                if (s.is(b)) s.tick(w, p, w.rng);
            });
            w.fluidTicks.tick(w.time, 65536, (p, f) -> {
                BlockState s = w.getBlockState(p);
                FluidState fs = s.getFluidState();
                if (fs.is(f)) fs.tick(w, p, s);
            });
            snapshot();
        }
    }

    @SuppressWarnings("unchecked")
    void snapshot() throws Exception {
        w.emit("tick " + w.time);
        List<BlockPos> all = new ArrayList<>(start.keySet());
        for (BlockPos p : w.cells.keySet()) if (!start.containsKey(p)) all.add(p);
        all.sort((a, b) -> a.getX() != b.getX() ? Integer.compare(a.getX(), b.getX()) : a.getY() != b.getY() ? Integer.compare(a.getY(), b.getY()) : Integer.compare(a.getZ(), b.getZ()));
        for (BlockPos p : all) {
            BlockState a = start.getOrDefault(p, AIR), b = w.cells.getOrDefault(p, AIR);
            if (a != b) w.emit("state " + pos(p) + " " + Block.getId(b));
        }
        List<String> rows = new ArrayList<>();
        Field f = LevelTicks.class.getDeclaredField("allContainers");
        f.setAccessible(true);
        for (var pair : List.of(new Object[] {"bt", w.blockTicks}, new Object[] {"ft", w.fluidTicks})) {
            var map = (it.unimi.dsi.fastutil.longs.Long2ObjectMap<LevelChunkTicks<Object>>) f.get(pair[1]);
            for (var container : map.values()) {
                container.getAll().forEach(t -> {
                    String type = t.type() instanceof Block b ? BuiltInRegistries.BLOCK.getKey(b).toString() : BuiltInRegistries.FLUID.getKey((Fluid) t.type()).toString();
                    rows.add(pair[0] + " " + pos(t.pos()) + " " + type + " " + (t.triggerTick() - w.time) + " " + t.priority().getValue());
                });
            }
        }
        Collections.sort(rows);
        for (String r : rows) w.emit(r);
    }

    void end() {
        w.emit("rng " + w.rng.nextLong());
    }

    void run() throws Exception {
        BlockState stone = state("stone"), water = state("water"), lava = state("lava"), glass = state("glass");
        // A pool: a walled 9 x 9 basin, a source in the middle.
        scene("pool", "overworld", 1);
        box(-4, 0, -4, 4, 0, 4, stone);
        box(-5, 1, -5, 5, 1, -5, stone);
        box(-5, 1, 5, 5, 1, 5, stone);
        box(-5, 1, -4, -5, 1, 4, stone);
        box(5, 1, -4, 5, 1, 4, stone);
        op(0, 1, 0, water);
        ticks(60);
        end();

        // A fall onto a floor, and the flow cut by removing the source.
        scene("fall", "overworld", 2);
        box(-6, 0, -6, 6, 0, 6, stone);
        box(0, 8, 0, 0, 8, 0, stone);
        op(0, 9, 0, water);
        ticks(70);
        op(0, 9, 0, AIR);
        ticks(60);
        end();

        // Over the edges of a platform, down to a floor with a hole.
        scene("edge", "overworld", 3);
        box(-8, 0, -8, 8, 0, 8, stone);
        cell(3, 0, -3, AIR);
        box(-2, 4, -2, 2, 4, 2, stone);
        op(-2, 5, -2, water);
        ticks(90);
        end();

        // Two sources make a third; a channel fills.
        scene("infinite", "overworld", 4);
        box(-4, 0, -4, 4, 0, 4, stone);
        box(-4, 1, -2, 4, 1, -2, stone);
        box(-4, 1, 2, 4, 1, 2, stone);
        op(-2, 1, 0, water);
        op(2, 1, 0, water);
        ticks(40);
        end();

        // Lava on a floor (slow, with its random spread delays), then cut.
        scene("lava", "overworld", 5);
        box(-6, 0, -6, 6, 0, 6, stone);
        op(0, 1, 0, lava);
        ticks(200);
        op(0, 1, 0, AIR);
        ticks(150);
        end();

        // Lava and water meet: cobblestone and obsidian; lava falling into water: stone.
        scene("meet", "overworld", 6);
        box(-8, 0, -8, 8, 0, 8, stone);
        op(-3, 1, 0, water);
        op(3, 1, 0, lava);
        box(-1, 6, 4, 1, 6, 6, glass);
        op(0, 7, 5, lava);
        op(0, 1, 5, water);
        ticks(220);
        end();

        // Basalt: lava over soul soil next to blue ice.
        scene("basalt", "overworld", 7);
        box(-3, 0, -3, 3, 0, 3, state("soul_soil"));
        cell(1, 1, 0, state("blue_ice"));
        op(0, 1, 0, lava);
        op(-2, 1, 0, lava);
        ticks(80);
        end();

        // Waterlogged slabs: a source flows into bottom slabs, not a double one; a waterlogged
        // slab next to air starts flowing when its neighbour changes.
        scene("waterlogged", "overworld", 8);
        box(-6, 0, -6, 6, 0, 6, stone);
        cell(1, 1, 0, state("oak_slab[type=bottom,waterlogged=false]"));
        cell(-1, 1, 0, state("oak_slab[type=double,waterlogged=false]"));
        cell(0, 1, 1, state("stone_slab[type=top,waterlogged=false]"));
        cell(4, 1, 4, state("oak_slab[type=bottom,waterlogged=true]"));
        op(0, 1, 0, water);
        op(4, 1, 3, stone);
        op(4, 1, 3, AIR);
        ticks(60);
        end();

        // Containers between two sources take a converted source: a slab, a lit candle (put out
        // with flags 11); a double slab and kelp-less blocks refuse.
        scene("waterlog_convert", "overworld", 13);
        box(-6, 0, -6, 6, 0, 6, stone);
        cell(1, 1, 0, state("oak_slab[type=bottom,waterlogged=false]"));
        cell(1, 1, 3, state("candle[candles=2,lit=true,waterlogged=false]"));
        cell(1, 1, -3, state("oak_slab[type=double,waterlogged=false]"));
        op(0, 1, 0, water);
        op(2, 1, 0, water);
        op(0, 1, 3, water);
        op(2, 1, 3, water);
        op(0, 1, -3, water);
        op(2, 1, -3, water);
        ticks(40);
        end();

        // Lava rising (a second source next to flowing lava): the random spread delays.
        scene("lava_rise", "overworld", 14);
        box(-8, 0, -8, 8, 0, 8, stone);
        op(0, 1, 0, lava);
        ticks(120);
        op(2, 1, 0, lava);
        op(0, 1, 2, lava);
        ticks(150);
        op(-2, 1, -1, lava);
        ticks(100);
        end();

        // Plants: water washes grass and a torch away; grass whose ground goes away breaks.
        scene("plants", "overworld", 9);
        box(-6, 0, -6, 6, 0, 6, state("grass_block"));
        cell(1, 1, 0, state("short_grass"));
        cell(2, 1, 0, state("poppy"));
        cell(0, 1, 2, state("torch"));
        cell(-3, 1, -3, state("short_grass"));
        op(0, 1, 0, water);
        op(-3, 0, -3, AIR);
        ticks(50);
        end();

        // The bottom of the world: water above nothing falls to the lowest y.
        scene("void", "overworld", 10);
        cell(0, -60, 0, stone);
        op(0, -59, 0, water);
        op(2, -63, 2, water);
        ticks(40);
        end();

        // The Nether: fast lava (faster, farther), water too.
        scene("nether_lava", "the_nether", 11);
        box(-8, 10, -8, 8, 10, 8, state("netherrack"));
        op(0, 11, 0, lava);
        ticks(120);
        op(0, 11, 0, AIR);
        ticks(80);
        end();
        scene("nether_meet", "the_nether", 12);
        box(-6, 10, -6, 6, 10, 6, state("netherrack"));
        op(-2, 11, 0, lava);
        op(2, 11, 0, water);
        ticks(100);
        end();

        // Random rooms: solid blocks, slabs and glass, then sources.
        String[] solids = {"stone", "glass", "oak_slab[type=bottom,waterlogged=false]", "oak_slab[type=top,waterlogged=false]", "dirt", "cobblestone"};
        for (int n = 0; n < 10; n++) {
            Random r = new Random(1000 + n);
            boolean nether = n % 4 == 3;
            int floor = nether ? 20 : 0;
            scene("room" + n, nether ? "the_nether" : "overworld", 2000 + n);
            box(-7, floor, -7, 7, floor, 7, stone);
            for (int i = 0; i < 70; i++) {
                cell(r.nextInt(15) - 7, floor + 1 + r.nextInt(4), r.nextInt(15) - 7, state(solids[r.nextInt(solids.length)]));
            }
            int sources = 2 + r.nextInt(3);
            for (int i = 0; i < sources; i++) {
                op(r.nextInt(11) - 5, floor + 2 + r.nextInt(4), r.nextInt(11) - 5, r.nextInt(3) == 0 ? lava : water);
            }
            ticks(60 + r.nextInt(80));
            end();
        }
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        AIR = Blocks.AIR.defaultBlockState();
        TYPES.put("overworld", loadDimensionType(BuiltinDimensionTypes.OVERWORLD));
        TYPES.put("the_nether", loadDimensionType(BuiltinDimensionTypes.NETHER));
        var allocator = Class.forName("sun.misc.Unsafe");
        var field = allocator.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        unsafe = field.get(null);
        allocate = allocator.getMethod("allocateInstance", Class.class);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            new LeafFluidOracle(out).run();
        }
    }
}
