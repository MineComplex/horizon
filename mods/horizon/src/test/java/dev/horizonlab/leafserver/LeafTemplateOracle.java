// Writes 26.3's structure template decoding and placement for horizon-server's tests
// (crates/horizon-server/src/worldgen/structure/template_oracle_tests.rs). Run with the server owner's
// own 26.3 jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafTemplateOracle.java <out.txt>`; the file is then given to the ignored test
// `templates_match_java` (HORIZON_TEMPLATE_ORACLE, with HORIZON_VANILLA_JAR, HORIZON_SERVER_BLOCKS_JSON
// and HORIZON_SERVER_BLOCK_STATES). Original Horizon code. Rows are tab separated:
//   template <id> <size x,y,z> <palettes "count:hash;..."> <palette 0 jigsaws> <entities>
//   case <id> <rotation> <mirror> <position> <pivot> <spec> <shared> <waterlog> <box|-> <seed> <level seed>
//        <bounding box> <zero position> <structure blocks> <jigsaws> <placed> <setBlock calls> <ticks>
//        <block entities "x,y,z,seed|-;"> <entities "bx,by,bz,px,py,pz;" (double bits in hex)>
//        <the placement random's next long>
// A case places the template with StructureTemplate.placeInWorld into a synthetic WorldGenLevel whose
// blocks and heights are given by formulas (`synth` here and in the Rust test). The processor spec is
// StructureProcessorType.LIST_CODEC's JSON: a processor_list id, or an inline list. Shape updates are
// block behaviour, so cases run with knownShape; entities need a ServerLevel, so their rows come from
// StructureTemplate.transform as placeEntities uses it.

package dev.horizonlab.leafserver;

import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.io.PrintWriter;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.stream.Stream;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.Mirror;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.templatesystem.LiquidSettings;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructurePlaceSettings;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessor;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessorList;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureProcessorType;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate;
import net.minecraft.world.phys.Vec3;

public final class LeafTemplateOracle {
    /** Every STRIDE-th template id (sorted), at most MAX_BLOCKS blocks in palette 0. */
    static final int STRIDE = 13;
    static final int MAX_BLOCKS = 12000;

    /** Built-in processor lists the cases use besides the registry's (the same JSON in the Rust test). */
    static final String[] BUILTIN = {
        "[{\"processor_type\":\"minecraft:block_ignore\",\"blocks\":[{\"id\":\"minecraft:structure_block\"},{\"id\":\"minecraft:air\"}]}]",
        "[{\"processor_type\":\"minecraft:block_rot\",\"integrity\":0.6}]",
        "[{\"processor_type\":\"minecraft:block_rot\",\"rottable_blocks\":\"#minecraft:planks\",\"integrity\":0.3}]",
        "[{\"processor_type\":\"minecraft:gravity\",\"heightmap\":\"WORLD_SURFACE_WG\",\"offset\":-1},{\"processor_type\":\"minecraft:jigsaw_replacement\"}]",
        "[{\"processor_type\":\"minecraft:block_age\",\"mossiness\":0.4},{\"processor_type\":\"minecraft:blackstone_replace\"}]",
        "[{\"processor_type\":\"minecraft:lava_submerged_block\"},{\"processor_type\":\"minecraft:protected_blocks\",\"value\":\"#minecraft:features_cannot_replace\"}]",
        "{\"processors\":[{\"processor_type\":\"minecraft:block_age\",\"mossiness\":0.8},{\"processor_type\":\"minecraft:nop\"},{\"processor_type\":\"minecraft:gravity\",\"heightmap\":\"OCEAN_FLOOR_WG\"}]}",
        "[{\"processor_type\":\"minecraft:capped\",\"delegate\":{\"processor_type\":\"minecraft:rule\",\"rules\":[{\"input_predicate\":{\"predicate_type\":\"minecraft:always_true\"},\"location_predicate\":{\"predicate_type\":\"minecraft:always_true\"},\"output_state\":{\"id\":\"minecraft:gold_block\"}}]},\"limit\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":1,\"max_inclusive\":20}},{\"processor_type\":\"minecraft:jigsaw_replacement\"}]",
        "[{\"processor_type\":\"minecraft:rule\",\"rules\":["
            + "{\"input_predicate\":{\"predicate_type\":\"minecraft:tag_match\",\"tag\":\"minecraft:logs\"},\"location_predicate\":{\"predicate_type\":\"minecraft:always_true\"},\"position_predicate\":{\"predicate_type\":\"minecraft:linear_pos\",\"min_chance\":0.0,\"max_chance\":1.0,\"min_dist\":0,\"max_dist\":20},\"output_state\":{\"id\":\"minecraft:stripped_oak_log\",\"properties\":{\"axis\":\"x\"}}},"
            + "{\"input_predicate\":{\"predicate_type\":\"minecraft:block_match\",\"block\":\"minecraft:chest\"},\"location_predicate\":{\"predicate_type\":\"minecraft:always_true\"},\"output_state\":{\"id\":\"minecraft:barrel\"},\"block_entity_modifier\":{\"type\":\"minecraft:append_static\",\"data\":{\"LootTable\":\"minecraft:chests/simple_dungeon\",\"x\":{\"a\":1}}}},"
            + "{\"input_predicate\":{\"predicate_type\":\"minecraft:random_block_match\",\"block\":\"minecraft:cobblestone\",\"probability\":0.5},\"location_predicate\":{\"predicate_type\":\"minecraft:block_match\",\"block\":\"minecraft:water\"},\"output_state\":{\"id\":\"minecraft:mossy_cobblestone\"},\"block_entity_modifier\":{\"type\":\"minecraft:clear\"}},"
            + "{\"input_predicate\":{\"predicate_type\":\"minecraft:random_blockstate_match\",\"block_state\":{\"id\":\"minecraft:air\"},\"probability\":0.2},\"location_predicate\":{\"predicate_type\":\"minecraft:tag_match\",\"tag\":\"minecraft:base_stone_overworld\"},\"position_predicate\":{\"predicate_type\":\"minecraft:axis_aligned_linear_pos\",\"min_chance\":1.0,\"max_chance\":0.0,\"min_dist\":2,\"max_dist\":9,\"axis\":\"x\"},\"output_state\":{\"id\":\"minecraft:chest\",\"properties\":{\"facing\":\"east\"}},\"block_entity_modifier\":{\"type\":\"minecraft:append_loot\",\"loot_table\":\"minecraft:chests/igloo_chest\"}}"
            + "]}]",
    };

    static int mix(int h) {
        h ^= h >>> 7;
        h *= 0x2545F491;
        h ^= h >>> 13;
        return h;
    }

    static final String[] DEEP = {"stone", "deepslate", "lava", "gravel", "stone", "water", "andesite"};
    static final String[] TOP = {"dirt", "grass_block", "sand", "dirt"};
    static BlockState[] deep;
    static BlockState[] top;

    /** The ground's top y at a column: 58..65. */
    static int ground(int x, int z) {
        return 58 + Math.floorMod(mix(x * 31 ^ z * 17 ^ 0x5bd1), 8);
    }

    static BlockState synth(int x, int y, int z) {
        int g = ground(x, z);
        int hash = mix(x * 73856093 ^ y * 19349663 ^ z * 83492791);
        if (y < g - 3) return deep[Math.floorMod(hash, deep.length)];
        if (y <= g) return top[Math.floorMod(hash, top.length)];
        if (y <= 62) return Blocks.WATER.defaultBlockState();
        return Blocks.AIR.defaultBlockState();
    }

    /** getHeight(type, x, z): the first free y (the ocean floor below the water). */
    static int height(String type, int x, int z) {
        int g = ground(x, z);
        return (type.startsWith("OCEAN_FLOOR") ? g : Math.max(g, 62)) + 1;
    }

    static RegistryOps<JsonElement> ops;
    static RegistryAccess.Frozen world;
    static RegistryAccess.Frozen all;
    static HolderLookup.Provider provider;
    static MultiPackResourceManager resources;

    static void bootstrapData() throws Exception {
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        List<PackResources> packs = new ArrayList<>(repository.openAllSelected());
        resources = new MultiPackResourceManager(PackType.SERVER_DATA, packs);
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags = TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        staticTags.forEach(Registry.PendingTags::apply);
        var lookups = TagLoader.buildUpdatedLookups(initial.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        world = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        provider = HolderLookup.Provider.create(
            Stream.concat(lookups.stream(), world.registries().map(entry -> (HolderLookup.RegistryLookup<?>) entry.value())));
        ops = RegistryOps.create(JsonOps.INSTANCE, provider);
        all = initial.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
    }

    static Object unsafe;
    static java.lang.reflect.Method allocate;

    /** A ServerLevel with only its seed (CappedProcessor reads `getLevel().getSeed()`). */
    static net.minecraft.server.level.ServerLevel seedLevel(long seed) throws Exception {
        // Vanilla's getSeed: server.getWorldGenSettings().options().seed().
        var level = (net.minecraft.server.level.ServerLevel) allocate.invoke(unsafe, net.minecraft.server.level.ServerLevel.class);
        var server = allocate.invoke(unsafe, net.minecraft.server.dedicated.DedicatedServer.class);
        var settings = net.minecraft.server.MinecraftServer.class.getDeclaredField("worldGenSettings");
        settings.setAccessible(true);
        settings.set(server, new net.minecraft.world.level.levelgen.WorldGenSettings(
            new net.minecraft.world.level.levelgen.WorldOptions(seed, false, false), null));
        var owner = net.minecraft.server.level.ServerLevel.class.getDeclaredField("server");
        owner.setAccessible(true);
        owner.set(level, server);
        return level;
    }

    record Level(WorldGenLevel level, List<String> sets, List<String> ticks, Map<BlockPos, BlockEntity> entities) {
    }

    static Level newLevel(long seed) throws Exception {
        var writes = new java.util.HashMap<BlockPos, BlockState>();
        var sets = new ArrayList<String>();
        var ticks = new ArrayList<String>();
        var entities = new LinkedHashMap<BlockPos, BlockEntity>();
        var server = seedLevel(seed);
        java.util.function.Function<BlockPos, BlockState> at = pos -> {
            BlockState written = writes.get(pos);
            return written != null ? written : synth(pos.getX(), pos.getY(), pos.getZ());
        };
        WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),
            new Class<?>[] {WorldGenLevel.class}, (proxy, method, values) -> {
                switch (method.getName()) {
                    case "getBlockState" -> {
                        return at.apply(((BlockPos) values[0]).immutable());
                    }
                    case "getFluidState" -> {
                        return at.apply(((BlockPos) values[0]).immutable()).getFluidState();
                    }
                    case "getBlockEntity" -> {
                        if (values.length == 2) return InvocationHandler.invokeDefault(proxy, method, values);
                        BlockPos pos = ((BlockPos) values[0]).immutable();
                        BlockState there = at.apply(pos);
                        if (!there.hasBlockEntity()) return null;
                        return entities.computeIfAbsent(pos, k ->
                            ((net.minecraft.world.level.block.EntityBlock) there.getBlock()).newBlockEntity(k, there));
                    }
                    case "setBlock", "setBlockAndUpdate" -> {
                        BlockPos pos = ((BlockPos) values[0]).immutable();
                        BlockState state = (BlockState) values[1];
                        int flags = values.length > 2 ? (Integer) values[2] : 3;
                        BlockState old = at.apply(pos);
                        if (old.hasBlockEntity() && old.getBlock() != state.getBlock()) entities.remove(pos);
                        writes.put(pos, state);
                        sets.add(pos.getX() + "," + pos.getY() + "," + pos.getZ() + "," + Block.getId(state) + "," + flags);
                        return true;
                    }
                    case "scheduleTick" -> {
                        BlockPos pos = ((BlockPos) values[0]).immutable();
                        String what = values[1] instanceof net.minecraft.world.level.material.Fluid fluid
                            ? BuiltInRegistries.FLUID.getKey(fluid).toString()
                            : BuiltInRegistries.BLOCK.getKey((Block) values[1]).toString();
                        ticks.add(pos.getX() + "," + pos.getY() + "," + pos.getZ() + "," + what + "," + values[2]);
                        return null;
                    }
                    case "getHeight" -> {
                        if (values != null && values.length == 3) {
                            return height(((net.minecraft.world.level.levelgen.Heightmap.Types) values[0]).name(),
                                (Integer) values[1], (Integer) values[2]);
                        }
                        return 384;
                    }
                    case "getMinY" -> {
                        return -64;
                    }
                    case "getSeed" -> {
                        return seed;
                    }
                    case "getLevel" -> {
                        return server;
                    }
                    case "registryAccess" -> {
                        return all;
                    }
                    case "holderLookup" -> {
                        return all.lookupOrThrow((net.minecraft.resources.ResourceKey<? extends Registry<?>>) values[0]);
                    }
                    case "addParticle", "playSound", "levelEvent", "gameEvent" -> {
                        // WorldGenRegion: no effects during generation.
                        return null;
                    }
                    case "isClientSide" -> {
                        return false;
                    }
                    case "hashCode" -> {
                        return System.identityHashCode(proxy);
                    }
                    case "equals" -> {
                        return proxy == values[0];
                    }
                    case "toString" -> {
                        return "oracle level";
                    }
                    default -> {
                        if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, values);
                        throw new UnsupportedOperationException(method.toString());
                    }
                }
            });
        return new Level(level, sets, ticks, entities);
    }

    static String pos(BlockPos p) {
        return p.getX() + "," + p.getY() + "," + p.getZ();
    }

    static String box(BoundingBox b) {
        return b.minX() + "," + b.minY() + "," + b.minZ() + "," + b.maxX() + "," + b.maxY() + "," + b.maxZ();
    }

    static String jigsaws(List<StructureTemplate.JigsawBlockInfo> list) {
        StringBuilder out = new StringBuilder();
        for (var j : list) {
            out.append(pos(j.pos())).append(',').append(Block.getId(j.state())).append(',')
                .append(j.jointType().getSerializedName()).append(',').append(j.name()).append(',')
                .append(j.pool().identifier()).append(',').append(j.target()).append(',')
                .append(j.placementPriority()).append(',').append(j.selectionPriority()).append(';');
        }
        return out.toString();
    }

    static long blocksHash(List<StructureTemplate.StructureBlockInfo> blocks) {
        long h = 1;
        for (var b : blocks) {
            h = h * 31 + b.pos().getX();
            h = h * 31 + b.pos().getY();
            h = h * 31 + b.pos().getZ();
            h = h * 31 + Block.getId(b.state());
            h = h * 31 + (b.nbt() != null ? 1 : 0);
        }
        return h;
    }

    @SuppressWarnings("unchecked")
    static List<StructureTemplate.Palette> palettes(StructureTemplate template) throws Exception {
        var field = StructureTemplate.class.getDeclaredField("palettes");
        field.setAccessible(true);
        return (List<StructureTemplate.Palette>) field.get(template);
    }

    @SuppressWarnings("unchecked")
    static List<StructureTemplate.StructureEntityInfo> entities(StructureTemplate template) throws Exception {
        var field = StructureTemplate.class.getDeclaredField("entityInfoList");
        field.setAccessible(true);
        return (List<StructureTemplate.StructureEntityInfo>) field.get(template);
    }

    static String bits(double v) {
        return Long.toHexString(Double.doubleToRawLongBits(v));
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        bootstrapData();
        var access = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        access.setAccessible(true);
        unsafe = access.get(null);
        allocate = Class.forName("sun.misc.Unsafe").getMethod("allocateInstance", Class.class);
        deep = new BlockState[DEEP.length];
        top = new BlockState[TOP.length];
        for (int i = 0; i < DEEP.length; i++) deep[i] = BuiltInRegistries.BLOCK.getValue(Identifier.parse("minecraft:" + DEEP[i])).defaultBlockState();
        for (int i = 0; i < TOP.length; i++) top[i] = BuiltInRegistries.BLOCK.getValue(Identifier.parse("minecraft:" + TOP[i])).defaultBlockState();

        var blockLookup = provider.lookupOrThrow(Registries.BLOCK);
        var files = new TreeMap<String, Identifier>();
        resources.listResources("structure", id -> id.getPath().endsWith(".nbt")).keySet()
            .forEach(file -> files.put(file.toString(), file));
        List<String> specs = new ArrayList<>();
        specs.add("[]");
        world.lookupOrThrow(Registries.PROCESSOR_LIST).listElementIds()
            .map(key -> "\"" + key.identifier() + "\"").sorted().forEach(specs::add);
        specs.addAll(List.of(BUILTIN));
        int cases = 0, templates = 0;
        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            int index = 0;
            for (Identifier file : files.values()) {
                if (index++ % STRIDE != 0) continue;
                String id = file.getNamespace() + ":" + file.getPath().substring("structure/".length(), file.getPath().length() - 4);
                StructureTemplate template = new StructureTemplate();
                try (var input = resources.open(file)) {
                    template.load(blockLookup, NbtIo.readCompressed(input, NbtAccounter.unlimitedHeap()));
                }
                if (palettes(template).isEmpty() || palettes(template).get(0).blocks().size() > MAX_BLOCKS) continue;
                int ti = templates++;
                StringBuilder palettes = new StringBuilder();
                for (var palette : palettes(template)) {
                    palettes.append(palette.blocks().size()).append(':').append(blocksHash(palette.blocks())).append(';');
                }
                StringBuilder entityList = new StringBuilder();
                for (var e : entities(template)) {
                    entityList.append(pos(e.blockPos)).append(',').append(bits(e.pos.x)).append(',').append(bits(e.pos.y))
                        .append(',').append(bits(e.pos.z)).append(';');
                }
                var size = template.getSize();
                out.println(String.join("\t", "template", id, size.getX() + "," + size.getY() + "," + size.getZ(),
                    palettes.toString(), jigsaws(palettes(template).get(0).jigsaws()), entityList.toString()));
                for (int r = 0; r < 4; r++) {
                    for (int m = 0; m < 3; m++) {
                        int h = mix(ti * 7919 + r * 31 + m * 7 + 17);
                        String spec = r == 0 && m == 0 ? "[]" : specs.get(Math.floorMod(h, specs.size()));
                        BlockPos position = new BlockPos(Math.floorMod(h >>> 3, 64) - 32, 52 + Math.floorMod(h >>> 9, 14),
                            Math.floorMod(h >>> 13, 64) - 32);
                        BlockPos pivot = new BlockPos(Math.floorMod(h >>> 17, 6), 0, Math.floorMod(h >>> 20, 6));
                        boolean shared = (h >>> 25 & 1) == 1;
                        boolean waterlog = (h >>> 26 & 3) != 0;
                        BoundingBox chunkBox = (h >>> 28 & 3) == 0
                            ? new BoundingBox(position.getX() >> 4 << 4, -64, position.getZ() >> 4 << 4,
                                (position.getX() >> 4 << 4) + 15, 319, (position.getZ() >> 4 << 4) + 15)
                            : null;
                        long seed = h * 0x9E3779B97F4A7C15L ^ ti;
                        long levelSeed = 12345L ^ ti;
                        Rotation rotation = Rotation.values()[r];
                        Mirror mirror = Mirror.values()[m];
                        StringBuilder row = new StringBuilder(String.join("\t", "case", id, rotation.name(), mirror.name(),
                            pos(position), pos(pivot), spec, String.valueOf(shared), String.valueOf(waterlog),
                            chunkBox == null ? "-" : box(chunkBox), String.valueOf(seed), String.valueOf(levelSeed)));
                        try {
                            Holder<StructureProcessorList> list = StructureProcessorType.LIST_CODEC
                                .parse(ops, JsonParser.parseString(spec)).getOrThrow();
                            RandomSource random = RandomSource.create(seed);
                            StructurePlaceSettings settings = new StructurePlaceSettings().setRotation(rotation).setMirror(mirror)
                                .setRotationPivot(pivot).setKnownShape(true).setIgnoreEntities(true)
                                .setLiquidSettings(waterlog ? LiquidSettings.APPLY_WATERLOGGING : LiquidSettings.IGNORE_WATERLOGGING);
                            if (chunkBox != null) settings.setBoundingBox(chunkBox);
                            for (StructureProcessor p : list.value().list()) settings.addProcessor(p);
                            StringBuilder structureBlocks = new StringBuilder();
                            for (var info : template.filterBlocks(position, settings.copy().setRandom(null), Blocks.STRUCTURE_BLOCK)) {
                                structureBlocks.append(pos(info.pos())).append(',').append(Block.getId(info.state())).append(';');
                            }
                            if (shared) settings.setRandom(random);
                            Level level = newLevel(levelSeed);
                            boolean placed = template.placeInWorld(level.level(), position, position, settings, random, 2);
                            StringBuilder bes = new StringBuilder();
                            for (var entry : level.entities().entrySet()) {
                                CompoundTag saved = entry.getValue().saveWithoutMetadata(all);
                                bes.append(pos(entry.getKey())).append(',')
                                    .append(saved.getLong("LootTableSeed").map(String::valueOf).orElse("-")).append(';');
                            }
                            StringBuilder entities = new StringBuilder();
                            for (var e : entities(template)) {
                                BlockPos blockPos = StructureTemplate.transform(e.blockPos, mirror, rotation, pivot).offset(position);
                                if (chunkBox != null && !chunkBox.isInside(blockPos)) continue;
                                Vec3 p = StructureTemplate.transform(e.pos, mirror, rotation, pivot)
                                    .add(position.getX(), position.getY(), position.getZ());
                                entities.append(pos(blockPos)).append(',').append(bits(p.x)).append(',').append(bits(p.y))
                                    .append(',').append(bits(p.z)).append(';');
                            }
                            row.append('\t').append(box(template.getBoundingBox(position, rotation, pivot, mirror)))
                                .append('\t').append(pos(template.getZeroPositionWithTransform(position, mirror, rotation)))
                                .append('\t').append(structureBlocks)
                                .append('\t').append(jigsaws(template.getJigsaws(position, rotation)))
                                .append('\t').append(placed)
                                .append('\t').append(String.join(";", level.sets()))
                                .append('\t').append(String.join(";", level.ticks()))
                                .append('\t').append(bes)
                                .append('\t').append(entities)
                                .append('\t').append(random.nextLong());
                        } catch (Exception e) {
                            row.append("\terror ").append(e);
                            System.err.println(id + " " + spec + ": " + e);
                        }
                        out.println(row);
                        cases++;
                    }
                }
            }
        }
        System.out.println("templates " + templates + ", cases " + cases);
    }
}
