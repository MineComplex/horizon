// Writes 26.3's FEATURES step for horizon-server's world generation tests
// (crates/horizon-server/src/worldgen/feature/decorate_oracle_tests.rs). Run with the server owner's
// own 26.3 jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafDecorationOracle.java <out.txt>`; the file is then given to the ignored
// test `decoration_matches_java` (HORIZON_DECORATION_ORACLE, with HORIZON_VANILLA_JAR,
// HORIZON_SERVER_BLOCKS_JSON, HORIZON_SERVER_BLOCK_STATES and HORIZON_BIOME_REPORTS). Original Horizon
// code, adapted from LeafCarverOracle.
// For the overworld noise settings (and large biomes) and two seeds, the 3 x 3 chunks around each centre
// chunk get their biomes (the BIOMES step, through the chunk resolvers), are filled (doFill) and surfaced
// (buildSurface, through the stored biomes), get the status of the chunks FEATURES sees (`terrain`; no
// carvers are run), and the centre is then decorated as ChunkGenerator.addVanillaDecorations does: the
// generator's own FeatureSorter result, the possible biomes of the 3 x 3 chunks, setDecorationSeed /
// setFeatureSeed and FeaturePlacer.placeWithBiomeCheck, over a WorldGenLevel that is a proxy on those chunks
// (WorldGenRegion needs a ServerLevel). The loop is repeated here because addVanillaDecorations reads the
// Paper world config. No structures are placed, and the features whose type horizon-server has not ported
// yet (UNPORTED below, the complement of PORTED) are skipped, which leaves the others unchanged: each feature
// has its own seeded random. `updateShape` after a tree runs the real blocks' here; horizon-server answers it
// for plants and vines only (feature/shape.rs), the rest is the block behaviour port's (WP6), so a chunk
// where another block reacts to a tree's edge would differ.
// Rows (block ids are Block.getId, runs are id*count in section order y, z, x):
//   stem <settings> <world preset> <stem> <seed> <biome names...>      possibleBiomes() in the generator's order
//   chunk <settings> <world preset> <stem> <seed> <cx> <cz> <minY> <height> <placed> <skipped> <note>
//   sec <dx> <dz> <index> <runs...>                                   the 3 x 3 chunks after, dz then dx
//   count <feature name> <times placed>                               per chunk, features that placed

package dev.horizonlab.leafserver;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
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
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.FeatureSorter;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.feature.Feature;
import net.minecraft.world.level.levelgen.feature.FallenTreeFeature;
import net.minecraft.world.level.levelgen.feature.TreeFeature;
import net.minecraft.world.level.levelgen.feature.treedecorators.PaleMossDecorator;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;
import net.minecraft.world.level.levelgen.placement.FeaturePlacer;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;

public final class LeafDecorationOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    private static final int[][] CHUNKS = {{0, 0}, {5, 5}, {-30, 15}, {20, 20}, {50, -60}, {12, -90}, {64, 64}, {-200, 150}};
    private static final int EXTRA = 8;

    /** The feature classes horizon-server implements (the Rust `fully_ported` repeats it). */
    static final Set<String> PORTED = Set.of("SimpleBlockFeature", "ScatteredOreFeature", "OreFeature", "DiskFeature",
        "BlockColumnFeature", "TreeFeature", "FallenTreeFeature", "RandomSelectorFeature", "SimpleRandomSelectorFeature",
        "RandomBooleanSelectorFeature", "WeightedRandomSelectorFeature", "SequenceFeature", "OverlayFeature",
        "SpringFeature", "BlockPileFeature", "VinesFeature", "UnderwaterMagmaFeature", "BambooFeature",
        "RandomNeighborSpreadFeature", "VegetationPatchFeature", "WaterloggedVegetationPatchFeature", "SnowAndFreezeFeature",
        "LakeFeature", "BlueIceFeature", "IcebergFeature", "BlockBlobFeature", "HugeBrownMushroomFeature", "HugeRedMushroomFeature",
        "MultifaceGrowthFeature", "GeodeFeature", "MonsterRoomFeature", "LargeDripstoneFeature", "SpeleothemFeature",
        "SpeleothemClusterFeature", "RootSystemFeature", "SculkPatchFeature", "SpikeFeature");

    static boolean unported(PlacedFeature placed) {
        return placed.getFeatures().anyMatch(LeafDecorationOracle::unported);
    }

    static boolean unported(Holder<Feature> holder) {
        Feature feature = holder.value();
        if (!PORTED.contains(feature.getClass().getSimpleName())) return true;
        // The pale moss decorator places a vegetation patch, which is not ported.
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

    private static void resetSearch(BiomeSource source) throws Exception {
        if (!(source instanceof MultiNoiseBiomeSource multiNoise)) return;
        Method parameters = MultiNoiseBiomeSource.class.getDeclaredMethod("parameters");
        parameters.setAccessible(true);
        Field index = Climate.ParameterList.class.getDeclaredField("index");
        index.setAccessible(true);
        Object tree = index.get(parameters.invoke(multiNoise));
        Field last = Class.forName("net.minecraft.world.level.biome.Climate$RTree").getDeclaredField("lastResult");
        last.setAccessible(true);
        ((ThreadLocal<?>) last.get(tree)).remove();
    }

    static long key(int x, int z) {
        return (long) x & 0xFFFFFFFFL | ((long) z & 0xFFFFFFFFL) << 32;
    }

    /** The region: the nine chunks, all writable (the FEATURES step's write radius is 1). */
    static WorldGenLevel region(Map<Long, ProtoChunk> chunks, ChunkPos center, long seed, BiomeManager biomes,
                                RegistryAccess registries) {
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
                        return chunks.get(key(x >> 4, z >> 4)).getHeight((net.minecraft.world.level.levelgen.Heightmap.Types) values[0], x, z) + 1;
                    }
                    case "getMinY" -> {
                        return -64;
                    }
                    case "getSeaLevel" -> {
                        return 63;
                    }
                    case "getChunk" -> {
                        if (values.length == 1) return chunkAt.apply((BlockPos) values[0]);
                        return chunks.get(key((Integer) values[0], (Integer) values[1]));
                    }
                    case "ensureCanWrite" -> {
                        BlockPos at = (BlockPos) values[0];
                        return Math.abs((at.getX() >> 4) - center.x()) <= 1 && Math.abs((at.getZ() >> 4) - center.z()) <= 1;
                    }
                    case "getBiome" -> {
                        return biomes.getBiome((BlockPos) values[0]);
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
        var noises = world.lookupOrThrow(Registries.NOISE);
        var worldPresets = world.lookupOrThrow(Registries.WORLD_PRESET);
        PalettedContainerFactory factory = PalettedContainerFactory.create(world);
        Method pickerMethod = NoiseBasedChunkGenerator.class.getDeclaredMethod("createFluidPicker", NoiseGeneratorSettings.class);
        pickerMethod.setAccessible(true);
        Method fill = NoiseBasedChunkGenerator.class.getDeclaredMethod("doFill", NoiseChunk.class, ChunkAccess.class);
        fill.setAccessible(true);
        Method surface = NoiseBasedChunkGenerator.class.getDeclaredMethod("buildSurface", ChunkAccess.class, NoiseChunk.class,
            RandomState.class, BiomeManager.class, java.util.Set.class, MaterialRule.class);
        surface.setAccessible(true);
        Field featuresPerStepField = ChunkGenerator.class.getDeclaredField("featuresPerStep");
        featuresPerStepField.setAccessible(true);

        StringBuilder out = new StringBuilder();
        String[][] settingsStems = {
            {"minecraft:overworld", "minecraft:normal", "minecraft:overworld"},
            {"minecraft:large_biomes", "minecraft:large_biomes", "minecraft:overworld"},
        };
        var settingsRegistry = world.lookupOrThrow(Registries.NOISE_SETTINGS);
        int rowIndex = 0;
        for (String[] row : settingsStems) {
            var holder = settingsRegistry.listElements().filter(h -> h.key().identifier().toString().equals(row[0])).findFirst().orElseThrow();
            NoiseGeneratorSettings settings = holder.value();
            var worldPreset = worldPresets.listElements().filter(h -> h.key().identifier().toString().equals(row[1])).findFirst().orElseThrow().value();
            LevelStem stem = worldPreset.createWorldDimensions().dimensions().entrySet().stream()
                .filter(e -> e.getKey().identifier().toString().equals(row[2])).findFirst().orElseThrow().getValue();
            BiomeSource source = ((NoiseBasedChunkGenerator) stem.generator()).getBiomeSource();
            int minY = settings.noiseSettings().minY();
            int height = settings.noiseSettings().height();
            var access = LevelHeightAccessor.create(minY, height);
            var generator = new NoiseBasedChunkGenerator(source, holder);
            @SuppressWarnings("unchecked")
            List<FeatureSorter.StepFeatureData> featuresPerStep = ((Supplier<List<FeatureSorter.StepFeatureData>>) featuresPerStepField.get(generator)).get();
            var picker = (Aquifer.FluidPicker) pickerMethod.invoke(null, settings);
            MaterialRule rule = settings.materialRule().value();
            for (long seed : rowIndex == 0 ? SEEDS : new long[] {SEEDS[0]}) {
                RandomState state = RandomState.create(noises, seed, settings);
                out.append("stem ").append(row[0]).append(' ').append(row[1]).append(' ').append(row[2]).append(' ').append(seed);
                for (Holder<Biome> biome : List.copyOf(source.possibleBiomes())) out.append(' ').append(biome.unwrapKey().orElseThrow().identifier());
                out.append('\n');
                List<int[]> centers = new ArrayList<>(List.of(CHUNKS));
                List<String> notes = new ArrayList<>();
                for (int i = 0; i < CHUNKS.length; i++) notes.add("listed");
                long walk = 0x2545F4914F6CDD1DL ^ seed;
                for (int i = 0; i < EXTRA; i++) {
                    walk = walk * 6364136223846793005L + 1442695040888963407L;
                    centers.add(new int[] {(int) (walk >> 40) % 400, (int) (walk >> 8) % 400});
                    notes.add("spread");
                }
                int quartMinY = minY >> 2;
                int quartMaxY = quartMinY + (height >> 2) - 1;
                for (int n = 0; n < centers.size(); n++) {
                    int[] c = centers.get(n);
                    resetSearch(source);
                    Map<Long, ProtoChunk> all = new HashMap<>();
                    Map<Long, ProtoChunk> chunks = new HashMap<>();
                    // The BIOMES step of each chunk of the 5 x 5 (the surface of the outer 3 x 3 reads the
                    // biomes around it), in the order of LeafCarverOracle (the Rust side replays it).
                    for (int dz = -2; dz <= 2; dz++) {
                        for (int dx = -2; dx <= 2; dx++) {
                            int cx = c[0] + dx, cz = c[1] + dz;
                            var chunk = new ProtoChunk(new ChunkPos(cx, cz), UpgradeData.EMPTY, access, factory, null);
                            Climate.Sampler cached = state.createClimateSampler(SamplerContext.builder().enableCaches().build());
                            BiomeResolver resolver = source.createResolverForChunk(cached, cx << 2, quartMinY, cz << 2, 4, height >> 2, 4);
                            chunk.fillBiomesFromNoise(resolver);
                            chunk.setPersistedStatus(ChunkStatus.BIOMES);
                            all.put(key(cx, cz), chunk);
                            if (Math.abs(dx) <= 1 && Math.abs(dz) <= 1) chunks.put(key(cx, cz), chunk);
                        }
                    }
                    BiomeResolver stored = (qx, qy, qz) -> all.get(key(qx >> 2, qz >> 2)).getNoiseBiome(qx, qy, qz);
                    BiomeManager manager = new BiomeManager(stored, BiomeManager.obfuscateSeed(seed));
                    for (var chunk : chunks.values()) {
                        var volume = new DensityVolume(16, height, 16, chunk.getPos().x() << 4, minY, chunk.getPos().z() << 4);
                        try (var noise = new NoiseChunk(state, null, settings, picker, Blender.empty(), volume)) {
                            fill.invoke(generator, noise, chunk);
                            surface.invoke(generator, chunk, noise, state, manager, null, rule);
                        }
                        chunk.setPersistedStatus(ChunkStatus.TERRAIN);
                    }
                    ChunkPos centerPos = new ChunkPos(c[0], c[1]);
                    WorldGenLevel level = region(chunks, centerPos, seed, manager, world);
                    BlockPos origin = new BlockPos(c[0] << 4, minY, c[1] << 4);
                    WorldgenRandom random = new WorldgenRandom(new XoroshiroRandomSource(0L));
                    long decorationSeed = random.setDecorationSeed(seed, origin.getX(), origin.getZ());
                    Set<Holder<Biome>> possible = new HashSet<>();
                    for (var chunk : chunks.values()) {
                        for (var section : chunk.getSections()) section.getBiomes().getAll(possible::add);
                    }
                    possible.retainAll(source.possibleBiomes());
                    FeaturePlacer placer = new FeaturePlacer(level, generator);
                    int placedCount = 0, skipped = 0;
                    Map<String, Integer> counts = new java.util.TreeMap<>();
                    int generationSteps = Math.max(11, featuresPerStep.size());
                    for (int step = 0; step < generationSteps; step++) {
                        if (step >= featuresPerStep.size()) continue;
                        it.unimi.dsi.fastutil.ints.IntSet wanted = new it.unimi.dsi.fastutil.ints.IntArraySet();
                        for (Holder<Biome> biome : possible) {
                            List<HolderSet<PlacedFeature>> inBiome = biome.value().getGenerationSettings().features();
                            if (step < inBiome.size()) {
                                FeatureSorter.StepFeatureData data = featuresPerStep.get(step);
                                inBiome.get(step).stream().map(Holder::value).forEach(f -> wanted.add(data.indexMapping().applyAsInt(f)));
                            }
                        }
                        int[] indices = wanted.toIntArray();
                        java.util.Arrays.sort(indices);
                        FeatureSorter.StepFeatureData data = featuresPerStep.get(step);
                        for (int index : indices) {
                            PlacedFeature feature = data.features().get(index);
                            random.setFeatureSeed(decorationSeed, index, step);
                            if (unported(feature)) {
                                skipped++;
                                continue;
                            }
                            boolean placed = placer.placeWithBiomeCheck(feature, random, origin);
                            placedCount++;
                            if (placed) {
                                String name = world.lookupOrThrow(Registries.PLACED_FEATURE).getResourceKey(feature)
                                    .map(k -> k.identifier().toString()).orElse("?");
                                counts.merge(name, 1, Integer::sum);
                            }
                        }
                    }
                    out.append("chunk ").append(row[0]).append(' ').append(row[1]).append(' ').append(row[2]).append(' ').append(seed)
                        .append(' ').append(c[0]).append(' ').append(c[1]).append(' ').append(minY).append(' ').append(height)
                        .append(' ').append(placedCount).append(' ').append(skipped).append(' ').append(notes.get(n)).append('\n');
                    for (var entry : counts.entrySet()) out.append("count ").append(entry.getKey()).append(' ').append(entry.getValue()).append('\n');
                    int[] ids = new int[4096];
                    for (int dz = -1; dz <= 1; dz++) {
                        for (int dx = -1; dx <= 1; dx++) {
                            var chunk = chunks.get(key(c[0] + dx, c[1] + dz));
                            for (int s = 0; s < height / 16; s++) {
                                var section = chunk.getSection(s);
                                for (int y = 0; y < 16; y++) for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) {
                                    ids[y * 256 + z * 16 + x] = Block.getId(section.getBlockState(x, y, z));
                                }
                                out.append("sec ").append(dx).append(' ').append(dz).append(' ').append(s);
                                runs(out, ids, 4096);
                                out.append('\n');
                            }
                        }
                    }
                }
            }
            rowIndex++;
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
