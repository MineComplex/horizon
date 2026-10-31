// Writes 26.3's block survival, block state provider and simple feature results for horizon-server's
// world generation tests (crates/horizon-server/src/worldgen/feature/oracle_tests.rs). Run with the
// server owner's own 26.3 jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafFeatureOracle.java <out.txt>`; the file is then given to the ignored test
// `features_match_java` (HORIZON_FEATURE_ORACLE, with HORIZON_VANILLA_JAR, HORIZON_SERVER_BLOCKS_JSON and
// HORIZON_SERVER_BLOCK_STATES). Original Horizon code. Rows are tab separated:
//   survive <state id> <one char per configuration: 1 survives, 0 does not, x the call threw>
//   provider <seed> <x y z> <json> <states "id;id;..."|error|decode error>  (several draws per row)
//   feature  <config name> <seed> <origin> <placed any> <writes "x y z id flags;..."> <next long>
// `survive` runs BlockState.canSurvive on every block state over CONFIGS neighbourhoods given by
// formulas the Rust test repeats (see `neighbour`): mode 0 a world of one palette state, 1 that state
// below and air elsewhere, 2 it above, a hashed state below and air elsewhere, 3 a hashed state per
// position. The palette is every state of NAMES, then every state of the candidate's own block.

package dev.horizonlab.leafserver;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.io.PrintWriter;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import com.google.gson.JsonElement;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import java.util.stream.Stream;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderSet;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

public final class LeafFeatureOracle {
    static final int CONFIGS = 40;
    static final int[] BRIGHTNESS = {0, 7, 8, 12, 13, 15};
    static final String[] NAMES = {
        "grass_block", "dirt", "coarse_dirt", "podzol", "mycelium", "rooted_dirt", "mud",
        "muddy_mangrove_roots", "moss_block", "farmland", "dirt_path", "sand", "red_sand", "gravel", "clay",
        "stone", "deepslate", "netherrack", "soul_sand", "soul_soil", "crimson_nylium", "warped_nylium",
        "nether_wart_block", "snow", "snow_block", "powder_snow", "ice", "packed_ice", "glass",
        "oak_leaves", "oak_log", "oak_planks", "water", "lava", "air", "cave_air", "void_air", "cactus",
        "sugar_cane", "kelp", "kelp_plant", "big_dripleaf", "big_dripleaf_stem", "small_dripleaf",
        "tall_grass", "short_grass", "oak_sapling", "wheat", "pumpkin_stem", "bamboo", "bamboo_sapling",
        "chorus_plant", "chorus_flower", "end_stone", "magma_block", "bubble_column", "hopper",
        "scaffolding", "piston", "sticky_piston", "moving_piston", "piston_head", "cave_vines",
        "cave_vines_plant", "twisting_vines", "weeping_vines", "vine", "pointed_dripstone",
        "dripstone_block", "oak_fence_gate", "oak_slab", "oak_stairs", "oak_trapdoor", "lily_pad",
        "frogspawn", "azalea", "flowering_azalea", "sculk", "bone_block", "crimson_roots", "crimson_fungus",
        "sea_pickle", "seagrass", "tall_seagrass", "torch", "lantern", "glowstone", "amethyst_block",
        "oak_door", "iron_bars", "cobweb", "spore_blossom", "hanging_roots", "glow_lichen", "moss_carpet",
        "brown_mushroom", "red_mushroom", "sweet_berry_bush", "cocoa", "fire", "soul_fire", "rail",
        "stone_pressure_plate", "repeater", "oak_sign", "oak_wall_sign", "white_banner", "white_carpet",
        "wall_torch", "ladder", "lever", "mangrove_roots", "mangrove_propagule", "cactus_flower",
        "leaf_litter", "bell", "candle", "brain_coral", "brain_coral_wall_fan", "redstone_wire",
        "grindstone", "tripwire_hook", "amethyst_cluster", "cake", "red_sand", "pink_petals",
    };

    static int mix(int h) {
        h ^= h >>> 7;
        h *= 0x2545F491;
        h ^= h >>> 13;
        return h;
    }

    /** The palette index of the state at an offset from the candidate, or -1 for air. */
    static int neighbour(int cfg, int size, int dx, int dy, int dz) {
        int mode = cfg % 4;
        int pick = (cfg / 4) % size;
        int hashed = Math.floorMod(mix(cfg * 1000003 + (dx + 8) * 9176 + (dy + 8) * 131 + (dz + 8) * 7), size);
        boolean below = dx == 0 && dy == -1 && dz == 0;
        boolean above = dx == 0 && dy == 1 && dz == 0;
        return switch (mode) {
            case 0 -> pick;
            case 1 -> below ? pick : -1;
            case 2 -> above ? pick : below ? hashed : -1;
            default -> hashed;
        };
    }

    /** Dynamic-shape states (shulker boxes, moving pistons) are left out: they ask a block entity. */
    static void addStates(List<BlockState> palette, Block block) {
        if (block.hasDynamicShape()) return;
        palette.addAll(block.getStateDefinition().getPossibleStates());
    }

    static List<BlockState> palette(Block own) {
        List<BlockState> palette = new ArrayList<>();
        for (String name : NAMES) {
            Identifier key = Identifier.parse("minecraft:" + name);
            if (!BuiltInRegistries.BLOCK.containsKey(key)) throw new IllegalStateException("no block " + name);
            Block block = BuiltInRegistries.BLOCK.getValue(key);
            addStates(palette, block);
        }
        addStates(palette, own);
        return palette;
    }

    static void survival(PrintWriter out) {
        int[] failures = {0};
        for (int id = 0; id < Block.BLOCK_STATE_REGISTRY.size(); id++) {
            BlockState state = Block.BLOCK_STATE_REGISTRY.byId(id);
            List<BlockState> palette = palette(state.getBlock());
            BlockPos origin = new BlockPos(5, 64, 7);
            StringBuilder row = new StringBuilder();
            for (int cfg = 0; cfg < CONFIGS; cfg++) {
                int config = cfg;
                int brightness = BRIGHTNESS[cfg % BRIGHTNESS.length];
                LevelReader level = (LevelReader) Proxy.newProxyInstance(LevelReader.class.getClassLoader(),
                    new Class<?>[] {LevelReader.class}, (proxy, method, values) -> {
                        switch (method.getName()) {
                            case "getBlockState", "getFluidState" -> {
                                BlockPos at = (BlockPos) values[0];
                                int index = neighbour(config, palette.size(), at.getX() - origin.getX(),
                                    at.getY() - origin.getY(), at.getZ() - origin.getZ());
                                BlockState there = index < 0 ? net.minecraft.world.level.block.Blocks.AIR.defaultBlockState()
                                    : palette.get(index);
                                return method.getName().equals("getBlockState") ? there : there.getFluidState();
                            }
                            case "getRawBrightness" -> {
                                return brightness;
                            }
                            default -> {
                                if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, values);
                                throw new UnsupportedOperationException(method.toString());
                            }
                        }
                    });
                char result;
                try {
                    result = state.canSurvive(level, origin) ? '1' : '0';
                } catch (RuntimeException e) {
                    result = 'x';
                    failures[0]++;
                    if (failures[0] < 5) System.err.println("survive " + id + ": " + e);
                }
                row.append(result);
            }
            out.println("survive\t" + id + "\t" + row);
        }
        System.err.println("survive: " + failures[0] + " throwing cases");
    }

    static RegistryOps<JsonElement> ops;
    static RegistryAccess.Frozen world;

    /** The vanilla data pack's tags and registries, as the server loads them (LeafPlacementOracle). */
    static void bootstrapData() throws Exception {
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        List<PackResources> packs = new ArrayList<>(repository.openAllSelected());
        MultiPackResourceManager resources = new MultiPackResourceManager(PackType.SERVER_DATA, packs);
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags = TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        staticTags.forEach(Registry.PendingTags::apply);
        var lookups = TagLoader.buildUpdatedLookups(initial.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        world = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        HolderLookup.Provider provider = HolderLookup.Provider.create(
            Stream.concat(lookups.stream(), world.registries().map(entry -> (HolderLookup.RegistryLookup<?>) entry.value())));
        ops = RegistryOps.create(JsonOps.INSTANCE, provider);
    }

    // ---- the synthetic level of the provider and feature rows ----

    /** The level's blocks by formula (`synth` in oracle_tests.rs repeats it). */
    static final String[] SYNTH = ("air air air air air air cave_air water water water stone stone stone stone granite "
        + "diorite andesite tuff tuff deepslate deepslate netherrack basalt blackstone dirt dirt grass_block "
        + "grass_block sand red_sand gravel clay snow magma_block mud moss_block podzol short_grass oak_leaves").split(" ");
    static BlockState[] synthStates;

    /** Mode 1 is solid up to y 50 (ores and disks need blocks around them), then water up to y 80. */
    static final String[] SOLID = ("netherrack netherrack basalt blackstone stone stone granite tuff deepslate dirt "
        + "grass_block sand gravel clay").split(" ");
    static BlockState[] solidStates;

    static BlockState synth(int mode, int x, int y, int z) {
        int hash = mix(x * 73856093 ^ y * 19349663 ^ z * 83492791);
        if (mode == 1 && y <= 50) return solidStates[Math.floorMod(hash, solidStates.length)];
        if (mode == 1 && y <= 80) return net.minecraft.world.level.block.Blocks.WATER.defaultBlockState();
        return synthStates[Math.floorMod(hash, synthStates.length)];
    }

    static final long[] SEEDS = {1L, 987654321012L, 31337L, 4242424242L};
    static final int[][] ORIGINS = {{0, 64, 0}, {13, 70, -5}, {-40, 20, 33}, {100, -30, 7}, {8, 0, 8}, {-3, 110, -9}};

    static Object unsafe;
    static java.lang.reflect.Method allocate;

    /** A `ProtoChunk` with just what `markPosForPostProcessing` reads. */
    static net.minecraft.world.level.chunk.ChunkAccess fakeChunk() throws Exception {
        var chunk = (net.minecraft.world.level.chunk.ChunkAccess) allocate.invoke(unsafe,
            net.minecraft.world.level.chunk.ProtoChunk.class);
        var base = net.minecraft.world.level.chunk.ChunkAccess.class;
        setField(base, chunk, "postProcessing", new it.unimi.dsi.fastutil.shorts.ShortList[24]);
        setField(base, chunk, "levelHeightAccessor", net.minecraft.world.level.LevelHeightAccessor.create(-64, 384));
        for (var entry : java.util.Map.of("minY", -64, "height", 384, "maxY", 319, "minSectionY", -4,
                "maxSectionY", 19, "sectionsCount", 24, "minSection", -4, "maxSection", 20).entrySet()) {
            try {
                setField(base, chunk, entry.getKey(), entry.getValue());
            } catch (NoSuchFieldException e) {
                // not a field of this version
            }
        }
        return chunk;
    }

    static void setField(Class<?> type, Object target, String name, Object value) throws Exception {
        var field = type.getDeclaredField(name);
        field.setAccessible(true);
        field.set(target, value);
    }

    /** One level: reads by formula over the writes, and a log of the calls the Rust test repeats. */
    record Level(WorldGenLevel level, java.util.Map<BlockPos, BlockState> writes, List<String> log,
                 java.util.Map<Long, net.minecraft.world.level.chunk.ChunkAccess> chunks) {
    }

    static Level newLevel(int mode) {
        var writes = new java.util.HashMap<BlockPos, BlockState>();
        var log = new ArrayList<String>();
        var chunks = new java.util.HashMap<Long, net.minecraft.world.level.chunk.ChunkAccess>();
        WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),
            new Class<?>[] {WorldGenLevel.class}, (proxy, method, values) -> {
                switch (method.getName()) {
                    case "getBlockState" -> {
                        BlockPos at = ((BlockPos) values[0]).immutable();
                        BlockState written = writes.get(at);
                        return written != null ? written : synth(mode, at.getX(), at.getY(), at.getZ());
                    }
                    case "getFluidState" -> {
                        BlockPos at = ((BlockPos) values[0]).immutable();
                        BlockState written = writes.get(at);
                        return (written != null ? written : synth(mode, at.getX(), at.getY(), at.getZ())).getFluidState();
                    }
                    case "setBlock" -> {
                        BlockPos at = ((BlockPos) values[0]).immutable();
                        writes.put(at, (BlockState) values[1]);
                        log.add("set " + at.getX() + "," + at.getY() + "," + at.getZ() + "," + Block.getId((BlockState) values[1])
                            + "," + values[2]);
                        return true;
                    }
                    case "scheduleTick" -> {
                        BlockPos at = ((BlockPos) values[0]).immutable();
                        log.add("tick " + at.getX() + "," + at.getY() + "," + at.getZ() + "," + values[2]);
                        return null;
                    }
                    case "getRawBrightness" -> {
                        return 0;
                    }
                    case "getChunk" -> {
                        BlockPos at = (BlockPos) values[0];
                        long key = ((long) (at.getX() >> 4) << 32) ^ (at.getZ() >> 4 & 0xFFFFFFFFL);
                        return chunks.computeIfAbsent(key, k -> {
                            try {
                                return fakeChunk();
                            } catch (Exception e) {
                                throw new IllegalStateException(e);
                            }
                        });
                    }
                    case "getMinY" -> {
                        return -64;
                    }
                    case "getHeight" -> {
                        return 384;
                    }
                    case "getSeaLevel" -> {
                        return 63;
                    }
                    default -> {
                        if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, values);
                        throw new UnsupportedOperationException(method.toString());
                    }
                }
            });
        return new Level(level, writes, log, chunks);
    }

    static String postProcessing(Level level) {
        List<String> marks = new ArrayList<>();
        for (var entry : level.chunks().entrySet()) {
            int cx = (int) (entry.getKey() >> 32);
            int cz = (int) (long) entry.getKey();
            var chunk = entry.getValue();
            var lists = chunk.getPostProcessing();
            for (int s = 0; s < lists.length; s++) {
                if (lists[s] == null) continue;
                for (short packed : lists[s]) {
                    int x = (cx << 4) + (packed & 15), y = (s - 4 << 4) + (packed >> 4 & 15), z = (cz << 4) + (packed >> 8 & 15);
                    marks.add(x + "," + y + "," + z);
                }
            }
        }
        java.util.Collections.sort(marks);
        return String.join(";", marks);
    }

    static RandomSource random(long seed) {
        return new net.minecraft.world.level.levelgen.XoroshiroRandomSource(seed);
    }

    static void providers(PrintWriter out) {
        // (the first element of each row's JSON is the provider; handmade ones follow the named ones)
        List<String> json = new ArrayList<>();
        var registry = world.lookupOrThrow(net.minecraft.core.registries.Registries.BLOCK_STATE_PROVIDER);
        registry.listElements().sorted(java.util.Comparator.comparing(h -> h.key().identifier().toString()))
            .forEach(h -> json.add("\"" + h.key().identifier() + "\""));
        json.addAll(List.of(
            "{\"id\":\"minecraft:oak_stairs\",\"properties\":{\"facing\":\"east\"}}",
            "{\"type\":\"minecraft:simple\",\"state\":{\"id\":\"minecraft:stone\"}}",
            "{\"type\":\"minecraft:weighted\",\"entries\":[{\"data\":\"minecraft:dirt\",\"weight\":3},{\"data\":\"minecraft:sand\",\"weight\":1},{\"data\":\"minecraft:gravel\",\"weight\":0}]}",
            "{\"type\":\"minecraft:random_block\",\"blocks\":\"#minecraft:flowers\"}",
            "{\"type\":\"minecraft:random_block\",\"blocks\":[\"minecraft:poppy\",\"minecraft:dandelion\",\"minecraft:fern\"]}",
            "{\"type\":\"minecraft:random_block\",\"blocks\":[]}",
            "{\"type\":\"minecraft:rotated\",\"state\":{\"id\":\"minecraft:oak_log\"}}",
            "{\"type\":\"minecraft:rotated\",\"state\":{\"id\":\"minecraft:furnace\"},\"direction\":\"west\"}",
            "{\"type\":\"minecraft:rotated\",\"state\":{\"id\":\"minecraft:dispenser\"}}",
            "{\"type\":\"minecraft:rotated\",\"state\":{\"id\":\"minecraft:hopper\"},\"direction\":\"up\"}",
            "{\"type\":\"minecraft:rotated\",\"state\":{\"id\":\"minecraft:quartz_pillar\"},\"direction\":\"south\"}",
            "{\"type\":\"minecraft:rotated\",\"state\":{\"id\":\"minecraft:stone\"}}",
            "{\"type\":\"minecraft:copy_properties\",\"source\":{\"id\":\"minecraft:oak_stairs\"}}",
            "{\"type\":\"minecraft:copy_properties\",\"source\":{\"id\":\"minecraft:short_grass\"}}",
            "{\"type\":\"minecraft:randomized_int\",\"source\":{\"id\":\"minecraft:wheat\"},\"property\":\"age\",\"values\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":0,\"max_inclusive\":7}}",
            "{\"type\":\"minecraft:randomized_int\",\"source\":{\"id\":\"minecraft:stone\"},\"property\":\"age\",\"values\":3}",
            "{\"type\":\"minecraft:randomized_int\",\"source\":{\"id\":\"minecraft:wheat\"},\"property\":\"age\",\"values\":9}",
            "{\"type\":\"minecraft:rule_based\",\"rules\":[{\"if_true\":{\"type\":\"minecraft:matching_blocks\",\"blocks\":\"minecraft:stone\"},\"then\":{\"id\":\"minecraft:diamond_block\"}},{\"if_true\":{\"type\":\"minecraft:matching_blocks\",\"blocks\":[\"minecraft:air\",\"minecraft:water\"]},\"then\":{\"type\":\"minecraft:random_block\",\"blocks\":[]}}]}",
            "{\"type\":\"minecraft:rule_based\",\"fallback\":{\"id\":\"minecraft:glass\"},\"rules\":[{\"if_true\":{\"type\":\"minecraft:matching_blocks\",\"blocks\":\"minecraft:dirt\"},\"then\":{\"type\":\"minecraft:weighted\",\"entries\":[{\"data\":\"minecraft:gold_block\",\"weight\":1}]}}]}",
            "{\"type\":\"minecraft:noise\",\"seed\":1234,\"noise\":{\"base_octave\":-4,\"base_amplitude\":1.0},\"scale\":0.5,\"states\":[\"minecraft:stone\",\"minecraft:dirt\",\"minecraft:sand\",\"minecraft:clay\"]}",
            "{\"type\":\"minecraft:noise_threshold\",\"seed\":99,\"noise\":{\"base_octave\":-3,\"octave_count\":3,\"base_amplitude\":0.8},\"scale\":0.25,\"threshold\":-0.1,\"high_chance\":0.4,\"default_state\":\"minecraft:glass\",\"low_states\":[\"minecraft:stone\",\"minecraft:dirt\"],\"high_states\":[\"minecraft:gold_block\",\"minecraft:iron_block\",\"minecraft:diamond_block\"]}",
            "{\"type\":\"minecraft:dual_noise\",\"seed\":7,\"noise\":{\"base_octave\":-2,\"base_amplitude\":1.0},\"scale\":1.0,\"slow_noise\":{\"base_octave\":-6,\"base_amplitude\":1.0},\"slow_scale\":0.7,\"variety\":[2,5],\"states\":[\"minecraft:red_wool\",\"minecraft:blue_wool\",\"minecraft:green_wool\",\"minecraft:white_wool\",\"minecraft:black_wool\"]}"
        ));
        for (String text : json) {
            var decoded = net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider.CODEC
                .parse(ops, JsonParser.parseString(text)).result();
            for (long seed : SEEDS) {
                for (int[] origin : ORIGINS) {
                    if (decoded.isEmpty()) {
                        out.println("provider\t" + text + "\t" + seed + "\t" + origin[0] + " " + origin[1] + " " + origin[2] + "\tdecode error");
                        continue;
                    }
                    Level level = newLevel(0);
                    RandomSource random = random(seed);
                    BlockPos pos = new BlockPos(origin[0], origin[1], origin[2]);
                    StringBuilder draws = new StringBuilder();
                    try {
                        for (int i = 0; i < 4; i++) {
                            var optional = decoded.get().value().getOptionalState(level.level(), random, pos);
                            draws.append(optional == null ? "null" : Block.getId(optional)).append(';');
                            draws.append(Block.getId(decoded.get().value().getState(level.level(), random, pos))).append(';');
                        }
                    } catch (RuntimeException e) {
                        draws.append("error");
                    }
                    out.println("provider\t" + text + "\t" + seed + "\t" + origin[0] + " " + origin[1] + " " + origin[2] + "\t" + draws
                        + "\t" + random.nextLong());
                }
            }
        }
    }

    static void features(PrintWriter out) {
        var registry = world.lookupOrThrow(net.minecraft.core.registries.Registries.FEATURE);
        List<String> kinds = List.of("SimpleBlockFeature", "ScatteredOreFeature", "DiskFeature", "BlockColumnFeature");
        registry.listElements().sorted(java.util.Comparator.comparing(h -> h.key().identifier().toString())).forEach(holder -> {
            var feature = holder.value();
            String name = holder.key().identifier().toString();
            // MossyCarpetBlock.placeAt draws from the level's own random: not ported.
            if (!kinds.contains(feature.getClass().getSimpleName()) || name.contains("moss_carpet")) return;
            for (int s = 0; s < SEEDS.length; s++) {
                long seed = SEEDS[s];
                for (int[] origin : ORIGINS) {
                    Level level = newLevel(s % 2);
                    RandomSource random = random(seed);
                    String result;
                    try {
                        result = String.valueOf(feature.place(level.level(), null, random, new BlockPos(origin[0], origin[1], origin[2])));
                    } catch (RuntimeException e) {
                        result = "error";
                    }
                    out.println("feature\t" + name + "\t" + seed + "\t" + origin[0] + " " + origin[1] + " " + origin[2] + "\t" + result
                        + "\t" + String.join(";", level.log()) + "\t" + postProcessing(level) + "\t" + random.nextLong());
                }
            }
        });
    }

    /** FeatureSorter's cycle message, as `tryReducingError` names the sources. */
    static void cycles(PrintWriter out) {
        var placed = world.lookupOrThrow(net.minecraft.core.registries.Registries.PLACED_FEATURE);
        String[] names = {"minecraft:oak_checked", "minecraft:birch_checked", "minecraft:spruce_checked", "minecraft:fancy_oak_checked", "minecraft:pine_checked"};
        var f = new Holder[names.length];
        for (int i = 0; i < names.length; i++) {
            f[i] = placed.getOrThrow(net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.PLACED_FEATURE, Identifier.parse(names[i])));
        }
        // Each case: source name -> its steps, each step the indices into `names`.
        String[][] cases = {
            {"a:0,1", "b:1,0", "c:2"},
            {"a:0,1", "b:1,2", "c:2,0", "d:3"},
            {"a:0,1,0"},
            {"x:4", "a:0,1", "y:3,2", "b:1,0", "z:2"},
        };
        for (String[] sources : cases) {
            List<String> ids = new ArrayList<>();
            java.util.Map<String, List<HolderSet<net.minecraft.world.level.levelgen.placement.PlacedFeature>>> steps = new java.util.HashMap<>();
            for (String source : sources) {
                String[] parts = source.split(":");
                ids.add(parts[0]);
                List<Holder<net.minecraft.world.level.levelgen.placement.PlacedFeature>> holders = new ArrayList<>();
                for (String index : parts[1].split(",")) {
                    @SuppressWarnings("unchecked")
                    var holder = (Holder<net.minecraft.world.level.levelgen.placement.PlacedFeature>) f[Integer.parseInt(index)];
                    holders.add(holder);
                }
                steps.put(parts[0], List.of(HolderSet.direct(holders)));
            }
            String message;
            try {
                net.minecraft.world.level.biome.FeatureSorter.buildFeaturesPerStep(ids, steps::get, true);
                message = "no cycle";
            } catch (IllegalStateException e) {
                message = e.getMessage();
            }
            out.println("cycle\t" + String.join(" ", sources) + "\t" + message);
        }
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        bootstrapData();
        var access = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        access.setAccessible(true);
        unsafe = access.get(null);
        allocate = Class.forName("sun.misc.Unsafe").getMethod("allocateInstance", Class.class);
        synthStates = new BlockState[SYNTH.length];
        solidStates = new BlockState[SOLID.length];
        for (int i = 0; i < SOLID.length; i++) {
            solidStates[i] = BuiltInRegistries.BLOCK.getValue(Identifier.parse("minecraft:" + SOLID[i])).defaultBlockState();
        }
        for (int i = 0; i < SYNTH.length; i++) {
            synthStates[i] = BuiltInRegistries.BLOCK.getValue(Identifier.parse("minecraft:" + SYNTH[i])).defaultBlockState();
        }
        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            survival(out);
            cycles(out);
            providers(out);
            features(out);
        }
    }
}
