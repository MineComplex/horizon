// Writes 26.3's flat and debug chunk generators for horizon-server's world generation tests
// (crates/horizon-server/src/worldgen/generator/fixed_tests.rs). Run with the server owner's own 26.3 jar
// and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafFlatDebugOracle.java <out.txt>`; the file is then given to the ignored tests
// `flat_generator_matches_java` and `debug_generator_matches_java` (HORIZON_FLAT_DEBUG_ORACLE, with
// HORIZON_VANILLA_JAR, HORIZON_SERVER_BLOCKS_JSON, HORIZON_SERVER_BLOCK_STATES). Original Horizon code,
// adapted from LeafDecorationOracle.
//
// Flat: several `generator-settings` strings go through FlatLevelGeneratorSettings.CODEC (the parse, its
// layers, biome, flags and structure overrides), then a FlatLevelSource is made of each and asked what the
// generation pipeline asks of it: spawn height, base heights and column, the layers after
// adjustGenerationSettings (non-opaque ones become fill-layer features) and the generator's feature order
// (FeatureSorter over its one biome). Two chunks per setting then go through BIOMES (a fixed biome),
// buildTerrain, the final heightmaps and the decoration of the centre as ChunkGenerator.addVanillaDecorations
// does it (no structures; the loop is repeated here because the original reads the Paper world config), over
// a WorldGenLevel proxy on the 3 x 3 chunks, as LeafDecorationOracle does. Features whose type horizon-server
// has not ported yet are skipped on both sides.
// Debug: DebugLevelSource.getBlockStateFor over the grid, and three chunks through applyBiomeDecoration.
// Rows (block ids are Block.getId, runs are id*count in section order y, z, x):
//   flat <index> <settings json>                       or `flat <index> error`
//   layers <block*height...>                           the parsed layers
//   info <biome> <features> <lakes> <overrides|none>   overrides as ids joined by commas
//   adjusted <runs>                                    getLayers() after the adjustment, -1 for null
//   facts <spawnHeight> <WORLD_SURFACE_WG> <OCEAN_FLOOR_WG> <MOTION_BLOCKING> <column runs>
//   plan <step> <names...>                             the feature order; inline fill layers `fill:<h>:<id>`
//   chunk <index> <cx> <cz> <placed> <skipped>         then sec <dx> <dz> <index> <runs...> (3 x 3), hm
//   hm <dx> <dz> <TYPE> <256 first-available values>   the four final heightmaps of the 3 x 3
//   debug <states> <width> <height>                    then dbgrow <z> <ids at x = 1, 3, 5...> and dbgprobe <x> <z> <id>
//   dbgchunk <cx> <cz>                                 then dbglayer <y> <256 ids (z, x)> for y 60 and 70

package dev.horizonlab.leafserver;

import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.EnumSet;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Supplier;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
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
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.biome.FeatureSorter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.DebugLevelSource;
import net.minecraft.world.level.levelgen.FlatLevelSource;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.feature.Feature;
import net.minecraft.world.level.levelgen.feature.FallenTreeFeature;
import net.minecraft.world.level.levelgen.feature.FillLayerFeature;
import net.minecraft.world.level.levelgen.feature.TreeFeature;
import net.minecraft.world.level.levelgen.feature.treedecorators.PaleMossDecorator;
import net.minecraft.world.level.levelgen.flat.FlatLayerInfo;
import net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorSettings;
import net.minecraft.world.level.levelgen.placement.FeaturePlacer;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;

public final class LeafFlatDebugOracle {
    private static final String[] SETTINGS = {
        // The classic preset with features on (desert instead of plains).
        "{\"biome\":\"minecraft:desert\",\"features\":true,\"lakes\":false,\"layers\":[{\"block\":\"minecraft:bedrock\",\"height\":1},"
            + "{\"block\":\"minecraft:stone\",\"height\":3},{\"block\":\"minecraft:grass_block\",\"height\":1}]}",
        // Lakes and features, villages only.
        "{\"biome\":\"minecraft:plains\",\"features\":true,\"lakes\":true,\"structure_overrides\":[\"minecraft:villages\"],"
            + "\"layers\":[{\"block\":\"minecraft:bedrock\",\"height\":1},{\"block\":\"minecraft:dirt\",\"height\":2},"
            + "{\"block\":\"minecraft:grass_block\",\"height\":1}]}",
        // Water, snow, a carpet and air layers (the non-opaque ones become fill-layer features), features off.
        "{\"biome\":\"minecraft:snowy_plains\",\"features\":false,\"lakes\":false,\"layers\":[{\"block\":\"minecraft:bedrock\",\"height\":1},"
            + "{\"block\":\"minecraft:stone\",\"height\":5},{\"block\":\"minecraft:water\",\"height\":3},"
            + "{\"block\":\"minecraft:white_carpet\",\"height\":1},{\"block\":\"minecraft:air\",\"height\":2},"
            + "{\"block\":\"minecraft:snow\",\"height\":1}]}",
        // The same with features on.
        "{\"biome\":\"minecraft:snowy_plains\",\"features\":true,\"lakes\":false,\"layers\":[{\"block\":\"minecraft:bedrock\",\"height\":1},"
            + "{\"block\":\"minecraft:stone\",\"height\":5},{\"block\":\"minecraft:water\",\"height\":3},"
            + "{\"block\":\"minecraft:white_carpet\",\"height\":1},{\"block\":\"minecraft:air\",\"height\":2},"
            + "{\"block\":\"minecraft:snow\",\"height\":1}]}",
        // An empty layer list.
        "{\"biome\":\"minecraft:plains\",\"features\":true,\"layers\":[]}",
        // A void world (air only) in the void biome keeps its features.
        "{\"biome\":\"minecraft:the_void\",\"features\":true,\"layers\":[{\"block\":\"minecraft:air\",\"height\":1}]}",
        // An air-only world in plains has none.
        "{\"biome\":\"minecraft:plains\",\"features\":true,\"layers\":[{\"block\":\"minecraft:air\",\"height\":2}]}",
        // Only a layer list: plains, no features.
        "{\"layers\":[{\"block\":\"minecraft:stone\",\"height\":10}]}",
        // Taller than a chunk's height.
        "{\"layers\":[{\"block\":\"minecraft:bedrock\",\"height\":1},{\"block\":\"minecraft:stone\",\"height\":400}]}",
        // A biome that does not exist falls back to plains.
        "{\"biome\":\"minecraft:nowhere\",\"layers\":[{\"block\":\"minecraft:dirt\",\"height\":2}]}",
        // Failing parses.
        "{\"layers\":[{\"block\":\"minecraft:nothing\",\"height\":1}]}",
        "{}",
        "{\"layers\":[{\"block\":\"minecraft:stone\",\"height\":4000},{\"block\":\"minecraft:dirt\",\"height\":100}]}",
    };
    private static final int[][] CHUNKS = {{0, 0}, {7, -3}};
    private static final long SEED = 12345L;

    /** The feature classes horizon-server implements (the Rust `fully_ported` repeats it); fill layers are placed natively, and the void platform
     * (the void biome's feature) is ported with the End features. */
    static final Set<String> PORTED = Set.of("SimpleBlockFeature", "ScatteredOreFeature", "OreFeature", "DiskFeature",
        "BlockColumnFeature", "TreeFeature", "FallenTreeFeature", "RandomSelectorFeature", "SimpleRandomSelectorFeature",
        "RandomBooleanSelectorFeature", "WeightedRandomSelectorFeature", "SequenceFeature", "OverlayFeature",
        "SpringFeature", "BlockPileFeature", "VinesFeature", "UnderwaterMagmaFeature", "BambooFeature",
        "RandomNeighborSpreadFeature", "VegetationPatchFeature", "WaterloggedVegetationPatchFeature", "SnowAndFreezeFeature",
        "LakeFeature", "BlueIceFeature", "IcebergFeature", "BlockBlobFeature", "HugeBrownMushroomFeature", "HugeRedMushroomFeature",
        "MultifaceGrowthFeature", "GeodeFeature", "MonsterRoomFeature", "LargeDripstoneFeature", "SpeleothemFeature",
        "SpeleothemClusterFeature", "RootSystemFeature", "SculkPatchFeature", "SpikeFeature", "FillLayerFeature",
        "VoidStartPlatformFeature");

    static boolean unported(PlacedFeature placed) {
        return placed.getFeatures().anyMatch(LeafFlatDebugOracle::unported);
    }

    static boolean unported(Holder<Feature> holder) {
        Feature feature = holder.value();
        if (!PORTED.contains(feature.getClass().getSimpleName())) return true;
        if (feature instanceof TreeFeature tree) return tree.decorators().stream().anyMatch(d -> d instanceof PaleMossDecorator);
        if (feature instanceof FallenTreeFeature fallen) {
            return fallen.stumpDecorators().stream().anyMatch(d -> d instanceof PaleMossDecorator)
                || fallen.logDecorators().stream().anyMatch(d -> d instanceof PaleMossDecorator);
        }
        return false;
    }

    private static void runs(StringBuilder out, int[] ids, int count) {
        int i = 0;
        while (i < count) {
            int j = i;
            while (j < count && ids[j] == ids[i]) j++;
            out.append(' ').append(ids[i]).append('*').append(j - i);
            i = j;
        }
    }

    static long key(int x, int z) {
        return (long) x & 0xFFFFFFFFL | ((long) z & 0xFFFFFFFFL) << 32;
    }

    /** The region: the chunks given, writable within `radius` of the centre. */
    static WorldGenLevel region(Map<Long, ProtoChunk> chunks, ChunkPos center, int radius, long seed, int seaLevel,
                                java.util.function.Function<BlockPos, Holder<Biome>> biomeAt, RegistryAccess registries) {
        java.util.function.Function<BlockPos, ProtoChunk> chunkAt = pos -> chunks.get(key(pos.getX() >> 4, pos.getZ() >> 4));
        Map<BlockPos, net.minecraft.world.level.block.entity.BlockEntity> entities = new HashMap<>();
        return (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(), new Class<?>[] {WorldGenLevel.class},
            (proxy, method, values) -> {
                switch (method.getName()) {
                    case "getBlockState" -> {
                        BlockPos at = (BlockPos) values[0];
                        ProtoChunk chunk = chunkAt.apply(at);
                        if (chunk == null) return net.minecraft.world.level.block.Blocks.VOID_AIR.defaultBlockState();
                        return chunk.getBlockState(at);
                    }
                    case "getFluidState" -> {
                        BlockPos at = (BlockPos) values[0];
                        ProtoChunk chunk = chunkAt.apply(at);
                        if (chunk == null) return net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
                        return chunk.getFluidState(at);
                    }
                    case "isStateAtPosition" -> {
                        BlockPos at = (BlockPos) values[0];
                        ProtoChunk chunk = chunkAt.apply(at);
                        BlockState state = chunk == null ? net.minecraft.world.level.block.Blocks.VOID_AIR.defaultBlockState() : chunk.getBlockState(at);
                        @SuppressWarnings("unchecked") var predicate = (java.util.function.Predicate<BlockState>) values[1];
                        return predicate.test(state);
                    }
                    case "isFluidAtPosition" -> {
                        BlockPos at = (BlockPos) values[0];
                        ProtoChunk chunk = chunkAt.apply(at);
                        var state = chunk == null ? net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState() : chunk.getFluidState(at);
                        @SuppressWarnings("unchecked") var predicate = (java.util.function.Predicate<net.minecraft.world.level.material.FluidState>) values[1];
                        return predicate.test(state);
                    }
                    case "setBlock" -> {
                        BlockPos at = (BlockPos) values[0];
                        ProtoChunk chunk = chunkAt.apply(at);
                        if (chunk == null) return false;
                        chunk.setBlockState(at, (BlockState) values[1], (Integer) values[2]);
                        return true;
                    }
                    case "getHeight" -> {
                        if (values == null || values.length == 0) return 384;
                        int x = (Integer) values[1], z = (Integer) values[2];
                        return chunks.get(key(x >> 4, z >> 4)).getHeight((Heightmap.Types) values[0], x, z) + 1;
                    }
                    case "getMinY" -> {
                        return -64;
                    }
                    case "getSeaLevel" -> {
                        return seaLevel;
                    }
                    case "getChunk" -> {
                        if (values.length == 1) return chunkAt.apply((BlockPos) values[0]);
                        return chunks.get(key((Integer) values[0], (Integer) values[1]));
                    }
                    case "ensureCanWrite" -> {
                        BlockPos at = (BlockPos) values[0];
                        return Math.abs((at.getX() >> 4) - center.x()) <= radius && Math.abs((at.getZ() >> 4) - center.z()) <= radius;
                    }
                    case "getBiome" -> {
                        return biomeAt.apply((BlockPos) values[0]);
                    }
                    case "getSeed" -> {
                        return seed;
                    }
                    case "getRawBrightness", "getBrightness" -> {
                        return 0;
                    }
                    case "getRandom" -> {
                        return new XoroshiroRandomSource(0L);
                    }
                    case "registryAccess" -> {
                        return registries;
                    }
                    case "scheduleTick", "playSound", "levelEvent", "gameEvent" -> {
                        return null;
                    }
                    case "getBlockEntity" -> {
                        if (values.length == 2) return InvocationHandler.invokeDefault(proxy, method, values);
                        BlockPos at = ((BlockPos) values[0]).immutable();
                        ProtoChunk chunk = chunkAt.apply(at);
                        BlockState there = chunk.getBlockState(at);
                        if (!there.hasBlockEntity()) return null;
                        return entities.computeIfAbsent(at, k ->
                            ((net.minecraft.world.level.block.EntityBlock) there.getBlock()).newBlockEntity(k, there));
                    }
                    default -> {
                        if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, values);
                        throw new UnsupportedOperationException(method.toString());
                    }
                }
            });
    }

    static String featureName(Registry<PlacedFeature> registry, PlacedFeature feature) {
        if (feature.feature().value() instanceof FillLayerFeature fill) {
            return "fill:" + fill.height() + ":" + Block.getId(fill.state());
        }
        return registry.getResourceKey(feature).map(k -> k.identifier().toString()).orElse("?");
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        List<PackResources> packs = new ArrayList<>(repository.openAllSelected());
        MultiPackResourceManager resources = new MultiPackResourceManager(PackType.SERVER_DATA, packs);
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags = TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        var lookups = TagLoader.buildUpdatedLookups(initial.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        staticTags.forEach(Registry.PendingTags::apply);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        PalettedContainerFactory factory = PalettedContainerFactory.create(world);
        var placedRegistry = world.lookupOrThrow(Registries.PLACED_FEATURE);
        var biomeRegistry = world.lookupOrThrow(Registries.BIOME);
        Field featuresPerStepField = ChunkGenerator.class.getDeclaredField("featuresPerStep");
        featuresPerStepField.setAccessible(true);
        var ops = RegistryOps.create(JsonOps.INSTANCE, world);
        var access = LevelHeightAccessor.create(-64, 384);
        EnumSet<Heightmap.Types> finalMaps = EnumSet.of(Heightmap.Types.MOTION_BLOCKING, Heightmap.Types.MOTION_BLOCKING_NO_LEAVES,
            Heightmap.Types.OCEAN_FLOOR, Heightmap.Types.WORLD_SURFACE);

        StringBuilder out = new StringBuilder();
        for (int index = 0; index < SETTINGS.length; index++) {
            var parsed = FlatLevelGeneratorSettings.CODEC.parse(ops, JsonParser.parseString(SETTINGS[index]));
            var result = parsed.result();
            if (result.isEmpty()) {
                out.append("flat ").append(index).append(" error ").append(SETTINGS[index]).append('\n');
                continue;
            }
            FlatLevelGeneratorSettings settings = result.get();
            out.append("flat ").append(index).append(' ').append(SETTINGS[index]).append('\n');
            out.append("layers");
            for (FlatLayerInfo layer : settings.getLayersInfo()) {
                out.append(' ').append(Block.getId(layer.getBlockState())).append('*').append(layer.getHeight());
            }
            out.append('\n');
            Holder<Biome> biome = settings.getBiome();
            out.append("info ").append(biome.unwrapKey().orElseThrow().identifier());
            // The flags are private; they are read back through the codec's own output.
            var encoded = FlatLevelGeneratorSettings.CODEC.encodeStart(ops, settings).getOrThrow().getAsJsonObject();
            out.append(' ').append(encoded.get("features").getAsBoolean() ? 1 : 0).append(' ').append(encoded.get("lakes").getAsBoolean() ? 1 : 0).append(' ');
            var overrides = settings.structureOverrides();
            if (overrides.isEmpty()) {
                out.append("none");
            } else {
                var names = new ArrayList<String>();
                overrides.get().forEach(h -> names.add(h.unwrapKey().orElseThrow().identifier().toString()));
                out.append(String.join(",", names));
            }
            out.append('\n');
            var generator = new FlatLevelSource(settings);
            @SuppressWarnings("unchecked")
            List<FeatureSorter.StepFeatureData> featuresPerStep = ((Supplier<List<FeatureSorter.StepFeatureData>>) featuresPerStepField.get(generator)).get();
            // The adjustment has run now: the layers are the ones the generation uses.
            List<BlockState> layers = settings.getLayers();
            int[] ids = new int[layers.size()];
            for (int i = 0; i < ids.length; i++) ids[i] = layers.get(i) == null ? -1 : Block.getId(layers.get(i));
            out.append("adjusted");
            runs(out, ids, ids.length);
            out.append('\n');
            out.append("facts ").append(generator.getSpawnHeight(access));
            for (var type : List.of(Heightmap.Types.WORLD_SURFACE_WG, Heightmap.Types.OCEAN_FLOOR_WG, Heightmap.Types.MOTION_BLOCKING)) {
                out.append(' ').append(generator.getBaseHeight(0, 0, type, access, null));
            }
            var column = generator.getBaseColumn(0, 0, access, null);
            int[] columnIds = new int[384];
            for (int y = 0; y < 384; y++) columnIds[y] = Block.getId(column.getBlock(-64 + y));
            runs(out, columnIds, 384);
            out.append('\n');
            for (int step = 0; step < featuresPerStep.size(); step++) {
                out.append("plan ").append(step);
                for (PlacedFeature feature : featuresPerStep.get(step).features()) out.append(' ').append(featureName(placedRegistry, feature));
                out.append('\n');
            }
            for (int[] c : CHUNKS) {
                Map<Long, ProtoChunk> chunks = new HashMap<>();
                for (int dz = -1; dz <= 1; dz++) {
                    for (int dx = -1; dx <= 1; dx++) {
                        var chunk = new ProtoChunk(new ChunkPos(c[0] + dx, c[1] + dz), UpgradeData.EMPTY, access, factory, null);
                        chunk.fillBiomesFromNoise((qx, qy, qz) -> biome);
                        chunk.setPersistedStatus(ChunkStatus.BIOMES);
                        generator.buildTerrain(chunk, Blender.empty(), null, null, null, null, Set.of()).join();
                        Heightmap.primeHeightmaps(chunk, finalMaps);
                        chunk.setPersistedStatus(ChunkStatus.TERRAIN);
                        chunks.put(key(c[0] + dx, c[1] + dz), chunk);
                    }
                }
                ChunkPos centerPos = new ChunkPos(c[0], c[1]);
                WorldGenLevel level = region(chunks, centerPos, 1, SEED, generator.getSeaLevel(), pos -> biome, world);
                BlockPos origin = new BlockPos(c[0] << 4, -64, c[1] << 4);
                WorldgenRandom random = new WorldgenRandom(new XoroshiroRandomSource(0L));
                long decorationSeed = random.setDecorationSeed(SEED, origin.getX(), origin.getZ());
                Set<Holder<Biome>> possible = new HashSet<>();
                for (var chunk : chunks.values()) {
                    for (var section : chunk.getSections()) section.getBiomes().getAll(possible::add);
                }
                possible.retainAll(generator.getBiomeSource().possibleBiomes());
                FeaturePlacer placer = new FeaturePlacer(level, generator);
                int placedCount = 0, skipped = 0;
                int generationSteps = Math.max(11, featuresPerStep.size());
                for (int step = 0; step < generationSteps; step++) {
                    if (step >= featuresPerStep.size()) continue;
                    it.unimi.dsi.fastutil.ints.IntSet wanted = new it.unimi.dsi.fastutil.ints.IntArraySet();
                    FeatureSorter.StepFeatureData data = featuresPerStep.get(step);
                    for (Holder<Biome> b : possible) {
                        List<HolderSet<PlacedFeature>> inBiome = generator.getBiomeGenerationSettings(b).features();
                        if (step < inBiome.size()) {
                            inBiome.get(step).stream().map(Holder::value).forEach(f -> wanted.add(data.indexMapping().applyAsInt(f)));
                        }
                    }
                    int[] indices = wanted.toIntArray();
                    java.util.Arrays.sort(indices);
                    for (int featureIndex : indices) {
                        PlacedFeature feature = data.features().get(featureIndex);
                        random.setFeatureSeed(decorationSeed, featureIndex, step);
                        if (unported(feature)) {
                            skipped++;
                            continue;
                        }
                        placer.placeWithBiomeCheck(feature, random, origin);
                        placedCount++;
                    }
                }
                out.append("chunk ").append(index).append(' ').append(c[0]).append(' ').append(c[1]).append(' ')
                    .append(placedCount).append(' ').append(skipped).append('\n');
                int[] blockIds = new int[4096];
                for (int dz = -1; dz <= 1; dz++) {
                    for (int dx = -1; dx <= 1; dx++) {
                        var chunk = chunks.get(key(c[0] + dx, c[1] + dz));
                        for (int s = 0; s < 24; s++) {
                            var section = chunk.getSection(s);
                            for (int y = 0; y < 16; y++) for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) {
                                blockIds[y * 256 + z * 16 + x] = Block.getId(section.getBlockState(x, y, z));
                            }
                            out.append("sec ").append(dx).append(' ').append(dz).append(' ').append(s);
                            runs(out, blockIds, 4096);
                            out.append('\n');
                        }
                        for (Heightmap.Types type : finalMaps) {
                            out.append("hm ").append(dx).append(' ').append(dz).append(' ').append(type.getSerializationKey());
                            for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) out.append(' ').append(chunk.getHeight(type, x, z) + 1);
                            out.append('\n');
                        }
                    }
                }
            }
        }

        // The debug generator: the grid and three decorated chunks.
        var plains = biomeRegistry.get(Biomes.PLAINS).orElseThrow();
        var debug = new DebugLevelSource(plains);
        int states = 0;
        for (Block block : net.minecraft.core.registries.BuiltInRegistries.BLOCK) states += block.getStateDefinition().getPossibleStates().size();
        int width = net.minecraft.util.Mth.ceil(net.minecraft.util.Mth.sqrt(states));
        int height = net.minecraft.util.Mth.ceil((float) states / width);
        out.append("debug ").append(states).append(' ').append(width).append(' ').append(height).append('\n');
        for (int z = 1; z <= 2 * height + 3; z += 2) {
            out.append("dbgrow ").append(z);
            for (int x = 1; x <= 2 * width + 3; x += 2) out.append(' ').append(Block.getId(DebugLevelSource.getBlockStateFor(x, z)));
            out.append('\n');
        }
        int[][] probes = {{0, 0}, {1, 0}, {0, 1}, {-1, 1}, {1, -1}, {2, 2}, {2, 3}, {3, 2}, {-3, -3}, {1, 1}, {5, 7}, {2 * width + 1, 1},
            {2 * width + 3, 1}, {2 * width + 5, 1}, {1, 2 * height + 1}, {1, 2 * height + 5}, {100000, 100000}, {-100000, 1}};
        for (int[] p : probes) {
            out.append("dbgprobe ").append(p[0]).append(' ').append(p[1]).append(' ').append(Block.getId(DebugLevelSource.getBlockStateFor(p[0], p[1]))).append('\n');
        }
        for (int[] c : new int[][] {{0, 0}, {1, 1}, {4, 2}}) {
            Map<Long, ProtoChunk> chunks = new HashMap<>();
            var chunk = new ProtoChunk(new ChunkPos(c[0], c[1]), UpgradeData.EMPTY, access, factory, null);
            chunk.setPersistedStatus(ChunkStatus.TERRAIN);
            chunks.put(key(c[0], c[1]), chunk);
            WorldGenLevel level = region(chunks, new ChunkPos(c[0], c[1]), 0, SEED, debug.getSeaLevel(), pos -> plains, world);
            debug.applyBiomeDecoration(level, chunk, null);
            out.append("dbgchunk ").append(c[0]).append(' ').append(c[1]).append('\n');
            for (int y : new int[] {60, 70}) {
                out.append("dbglayer ").append(y);
                for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) out.append(' ').append(Block.getId(chunk.getBlockState(new BlockPos(x + c[0] * 16, y, z + c[1] * 16))));
                out.append('\n');
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
