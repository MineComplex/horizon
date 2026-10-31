// Writes 26.3's CARVERS step for horizon-server's world generation tests
// (crates/horizon-server/src/worldgen/carver/oracle_tests.rs). Run with the server owner's own 26.3
// jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafCarverOracle.java <out.txt>`; the file is then given to the ignored test
// `carver_matches_java` (HORIZON_CARVER_ORACLE, with HORIZON_VANILLA_JAR, HORIZON_SERVER_BLOCKS_JSON,
// HORIZON_SERVER_BLOCK_STATES and HORIZON_BIOME_REPORTS). Original Horizon code, adapted from
// LeafCarverOracle.
// For the overworld, large biomes, amplified and Nether noise settings and two seeds, each chunk of a list
// (LeafTerrainOracle's, plus a spread of pseudo-random ones) is filled (doFill), has its surface built
// (buildSurface) with the biomes of the stem's real biome source through the chunk resolvers the BIOMES
// step uses, and is then carved as NoiseBasedChunkGenerator.generateCarvers does: the 17 x 17 source
// chunks' carver lists from getBiomeGenerationSettingsForCarver over the uncached resolver, each carver's
// setLargeFeatureSeed(seed + index) and isStartChunk/carve into a CarvingMask, then applyCarvingMask with a
// BiomeManager over the uncached resolver. generateCarvers itself needs a WorldGenRegion, so its body is
// repeated here around the generator's own private methods. The jar is vanilla's (no Leaf FastRNG, no
// blending filter).
// Rows (block ids are Block.getId, runs are id*count in section order y, z, x):
//   motion <block name>...                      blocks the OCEAN_FLOOR_WG heightmap counts
//   chunk <settings> <world preset> <stem> <seed> <cx> <cz> <minY> <height> <note> <x z bottomY topY>...
//                                               the carving mask's visited runs follow the note
//   sec <index> <runs...>                       every section, after the carvers
//   hm <TYPE> <256 first-available values>      OCEAN_FLOOR_WG, WORLD_SURFACE_WG (z, x order)
//   post <section> <offsets...>                 the fluids marked for post-processing, in order

package dev.horizonlab.leafserver;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
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
import net.minecraft.util.Mth;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeGenerationSettings;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.chunk.CarvingMask;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldGenerationContext;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.carver.WorldCarver;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;

public final class LeafCarverOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    // LeafTerrainOracle's chunks.
    private static final int[][] CHUNKS = {
        {0, 0}, {1, 0}, {0, 1}, {-1, -1}, {5, 5}, {6, 5}, {7, -3}, {20, 20}, {-30, 15}, {50, -60},
        {-200, 150}, {300, -250}, {1000, 2000}, {-777, 431}, {12, -90}, {64, 64},
    };
    // Pseudo-random chunk positions, on top of CHUNKS: caves, canyons and the aquifers' water and lava all turn up.
    private static final int EXTRA = 12;

    private static void runs(StringBuilder out, int[] ids, int count) {
        int i = 0;
        while (i < count) {
            int j = i;
            while (j < count && ids[j] == ids[i]) j++;
            out.append(' ').append(ids[i]).append('*').append(j - i);
            i = j;
        }
    }

    /** Forgets the climate tree's last result on this thread (vanilla keeps one per tree). */
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
        var biomes = world.lookupOrThrow(Registries.BIOME);
        var worldPresets = world.lookupOrThrow(Registries.WORLD_PRESET);
        PalettedContainerFactory factory = PalettedContainerFactory.create(world);
        Method pickerMethod = NoiseBasedChunkGenerator.class.getDeclaredMethod("createFluidPicker", NoiseGeneratorSettings.class);
        pickerMethod.setAccessible(true);
        Method fill = NoiseBasedChunkGenerator.class.getDeclaredMethod("doFill", NoiseChunk.class, ChunkAccess.class);
        fill.setAccessible(true);
        Method surface = NoiseBasedChunkGenerator.class.getDeclaredMethod("buildSurface", ChunkAccess.class, NoiseChunk.class,
            RandomState.class, BiomeManager.class, java.util.Set.class, MaterialRule.class);
        surface.setAccessible(true);
        Method carversFor = NoiseBasedChunkGenerator.class.getDeclaredMethod("getBiomeGenerationSettingsForCarver", BiomeResolver.class, ChunkPos.class);
        carversFor.setAccessible(true);
        Method applyMask = NoiseBasedChunkGenerator.class.getDeclaredMethod("applyCarvingMask", ChunkAccess.class, CarvingMask.class,
            RandomState.class, MaterialRule.class, WorldGenerationContext.class, NoiseChunk.class, java.util.function.Function.class,
            CarvingMask.Filter.class);
        applyMask.setAccessible(true);

        StringBuilder out = new StringBuilder("motion");
        for (Block block : BuiltInRegistries.BLOCK) {
            if (Heightmap.Types.OCEAN_FLOOR_WG.isOpaque().test(block.defaultBlockState())) {
                out.append(' ').append(BuiltInRegistries.BLOCK.getKey(block));
            }
        }
        out.append('\n');

        String[][] settingsStems = {
            {"minecraft:overworld", "minecraft:normal", "minecraft:overworld"},
            {"minecraft:large_biomes", "minecraft:large_biomes", "minecraft:overworld"},
            {"minecraft:amplified", "minecraft:amplified", "minecraft:overworld"},
            {"minecraft:nether", "minecraft:normal", "minecraft:the_nether"},
        };
        var settingsRegistry = world.lookupOrThrow(Registries.NOISE_SETTINGS);
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
            var picker = (Aquifer.FluidPicker) pickerMethod.invoke(null, settings);
            MaterialRule rule = settings.materialRule().value();
            for (long seed : SEEDS) {
                RandomState state = RandomState.create(noises, seed, settings);
                List<int[]> chunks = new ArrayList<>(List.of(CHUNKS));
                List<String> notes = new ArrayList<>();
                for (int i = 0; i < CHUNKS.length; i++) notes.add("listed");
                long walk = 0x2545F4914F6CDD1DL ^ seed;
                for (int i = 0; i < EXTRA; i++) {
                    walk = walk * 6364136223846793005L + 1442695040888963407L;
                    chunks.add(new int[] {(int) (walk >> 40) % 400, (int) (walk >> 8) % 400});
                    notes.add("spread");
                }
                int quartMinY = minY >> 2;
                int quartMaxY = quartMinY + (height >> 2) - 1;
                int sections = height >> 4;
                for (int n = 0; n < chunks.size(); n++) {
                    int[] c = chunks.get(n);
                    // The biomes of the chunk and its neighbours, as their BIOMES steps store them:
                    // createResolverForChunk with a caching context, queried in Paper's section fill order
                    // (the corner first, then y, z, x). The climate tree remembers its last result per
                    // thread, so it is reset first and the Rust side replays the same order.
                    resetSearch(source);
                    Map<Long, Holder<Biome>[]> tables = new HashMap<>();
                    for (int dz = -1; dz <= 1; dz++) {
                        for (int dx = -1; dx <= 1; dx++) {
                            int qx = (c[0] + dx) << 2, qz = (c[1] + dz) << 2;
                            Climate.Sampler cached = state.createClimateSampler(SamplerContext.builder().enableCaches().build());
                            BiomeResolver resolver = source.createResolverForChunk(cached, qx, quartMinY, qz, 4, height >> 2, 4);
                            @SuppressWarnings("unchecked")
                            Holder<Biome>[] table = (Holder<Biome>[]) new Holder[sections * 64];
                            for (int section = 0; section < sections; section++) {
                                int qy = quartMinY + section * 4;
                                table[section * 64] = resolver.getNoiseBiome(qx, qy, qz);
                                for (int y = 0; y < 4; y++) for (int z = 0; z < 4; z++) for (int x = y == 0 && z == 0 ? 1 : 0; x < 4; x++) {
                                    table[section * 64 + y * 16 + z * 4 + x] = resolver.getNoiseBiome(qx + x, qy + y, qz + z);
                                }
                            }
                            tables.put((long) (c[0] + dx) & 0xFFFFFFFFL | ((long) (c[1] + dz) & 0xFFFFFFFFL) << 32, table);
                        }
                    }
                    BiomeResolver chunkBiomes = (qx, qy, qz) -> {
                        Holder<Biome>[] table = tables.get((long) (qx >> 2) & 0xFFFFFFFFL | ((long) (qz >> 2) & 0xFFFFFFFFL) << 32);
                        int clamped = Mth.clamp(qy, quartMinY, quartMaxY);
                        int section = (clamped - quartMinY) >> 2;
                        return table[section * 64 + (clamped & 3) * 16 + (qz & 3) * 4 + (qx & 3)];
                    };
                    BiomeManager manager = new BiomeManager(chunkBiomes, BiomeManager.obfuscateSeed(seed));
                    CarvingMask mask;
                    var chunk = new ProtoChunk(new ChunkPos(c[0], c[1]), UpgradeData.EMPTY, access, factory, null);
                    var volume = new DensityVolume(16, height, 16, c[0] << 4, minY, c[1] << 4);
                    try (var noise = new NoiseChunk(state, null, settings, picker, Blender.empty(), volume)) {
                        fill.invoke(generator, noise, chunk);
                        surface.invoke(generator, chunk, noise, state, manager, null, rule);
                        // generateCarvers, around the generator's own private methods.
                        BiomeResolver uncached = source.createUncachedResolver(state);
                        WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
                        WorldGenerationContext context = new WorldGenerationContext(generator, chunk.getHeightAccessorForGeneration());
                        int maxY = context.getMinGenY() + context.getGenDepth() - 1 - 7;
                        mask = new CarvingMask(context.getMinGenY() + 1, maxY);
                        for (int dx = -8; dx <= 8; dx++) {
                            for (int dz = -8; dz <= 8; dz++) {
                                ChunkPos sourcePos = new ChunkPos(c[0] + dx, c[1] + dz);
                                BiomeGenerationSettings carverSettings = (BiomeGenerationSettings) carversFor.invoke(generator, uncached, sourcePos);
                                int index = 0;
                                for (Holder<WorldCarver> carverHolder : carverSettings.getCarvers()) {
                                    WorldCarver carver = carverHolder.value();
                                    random.setLargeFeatureSeed(state.seed() + index, sourcePos.x(), sourcePos.z());
                                    if (carver.isStartChunk(random)) {
                                        carver.carve(context, random, chunk.getPos(), sourcePos, mask);
                                    }
                                    index++;
                                }
                            }
                        }
                        if (!mask.isEmpty()) {
                            BiomeManager carvingBiomes = new BiomeManager(uncached, BiomeManager.obfuscateSeed(seed));
                            java.util.function.Function<BlockPos, Holder<Biome>> biomeGetter = carvingBiomes::getBiome;
                            applyMask.invoke(generator, chunk, mask, state, rule, context, noise, biomeGetter, null);
                        }
                    }
                    out.append("chunk ").append(row[0]).append(' ').append(row[1]).append(' ').append(row[2]).append(' ').append(seed)
                        .append(' ').append(c[0]).append(' ').append(c[1]).append(' ').append(minY).append(' ').append(height)
                        .append(' ').append(notes.get(n));
                    StringBuilder maskRuns = out;
                    mask.visit((x, z, bottomY, topY) -> maskRuns.append(' ').append(x).append(' ').append(z).append(' ').append(bottomY).append(' ').append(topY));
                    out.append('\n');
                    int[] ids = new int[4096];
                    for (int s = 0; s < height / 16; s++) {
                        var section = chunk.getSection(s);
                        for (int y = 0; y < 16; y++) for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) {
                            ids[y * 256 + z * 16 + x] = Block.getId(section.getBlockState(x, y, z));
                        }
                        out.append("sec ").append(s);
                        runs(out, ids, 4096);
                        out.append('\n');
                    }
                    for (var type : new Heightmap.Types[] {Heightmap.Types.OCEAN_FLOOR_WG, Heightmap.Types.WORLD_SURFACE_WG}) {
                        var map = chunk.getOrCreateHeightmapUnprimed(type);
                        out.append("hm ").append(type.getSerializationKey());
                        for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) out.append(' ').append(map.getFirstAvailable(x, z));
                        out.append('\n');
                    }
                    var post = chunk.getPostProcessing();
                    for (int s = 0; s < post.length; s++) {
                        if (post[s] == null || post[s].isEmpty()) continue;
                        out.append("post ").append(s);
                        for (short offset : post[s]) out.append(' ').append(Short.toUnsignedInt(offset));
                        out.append('\n');
                    }
                }
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
