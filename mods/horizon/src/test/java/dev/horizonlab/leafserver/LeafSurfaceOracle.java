// Writes 26.3's surface building for horizon-server's world generation tests
// (crates/horizon-server/src/worldgen/surface/oracle_tests.rs). Run with the server owner's own 26.3
// jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafSurfaceOracle.java <out.txt>`; the file is then given to the ignored test
// `surface_matches_java` (HORIZON_SURFACE_ORACLE, with HORIZON_VANILLA_JAR, HORIZON_SERVER_BLOCKS_JSON,
// HORIZON_SERVER_BLOCK_STATES and HORIZON_BIOME_REPORTS). Original Horizon code, adapted from
// LeafTerrainOracle and LeafBiomeOracle.
// For every noise settings element (overworld, large_biomes, amplified, caves, floating_islands, nether,
// end) and two seeds, each chunk of LeafTerrainOracle's list (plus, for the overworld, chunks whose centre
// biome is one the surface rules or the extensions treat specially, and 16 of the eroded badlands) is filled (doFill) and then built
// (NoiseBasedChunkGenerator.buildSurface, which the TERRAIN step runs next), with the biomes of the real
// biome source of the stem named in the row, through the chunk resolvers the BIOMES step uses (a
// BiomeManager over the chunk and its neighbours, as the TERRAIN step reads them). The jar is vanilla's:
// its bedrock rules are the plain vertical gradients, which Paper's optionally flat condition equals
// while environment.generate-flat-bedrock is off; flat bedrock cannot be checked here.
// Rows (block ids are Block.getId, runs are id*count in section order y, z, x):
//   motion <block name>...                      blocks the OCEAN_FLOOR_WG heightmap counts
//   chunk <settings> <world preset> <stem> <seed> <cx> <cz> <minY> <height> <note>
//   sec <index> <runs...>                       every section, after the surface
//   hm <TYPE> <256 first-available values>      OCEAN_FLOOR_WG, WORLD_SURFACE_WG (z, x order)
//   post <section> <offsets...>                 the fluids marked for post-processing, in order
//   temp <biome> <seaLevel> <x> <y> <z> <float bits of getTemperature> <coldEnoughToSnow> <meltsIceberg>

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
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Block;
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
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;

public final class LeafSurfaceOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    // LeafTerrainOracle's chunks.
    private static final int[][] CHUNKS = {
        {0, 0}, {1, 0}, {0, 1}, {-1, -1}, {5, 5}, {6, 5}, {7, -3}, {20, 20}, {-30, 15}, {50, -60},
        {-200, 150}, {300, -250}, {1000, 2000}, {-777, 431}, {12, -90}, {64, 64},
    };
    // Biomes the surface rules and extensions single out, found by scanning the overworld.
    private static final String[] WANTED = {
        "minecraft:eroded_badlands", "minecraft:badlands", "minecraft:wooded_badlands", "minecraft:frozen_ocean",
        "minecraft:deep_frozen_ocean", "minecraft:ice_spikes", "minecraft:snowy_plains", "minecraft:snowy_beach",
        "minecraft:swamp", "minecraft:mangrove_swamp", "minecraft:desert", "minecraft:beach", "minecraft:stony_shore",
        "minecraft:windswept_gravelly_hills", "minecraft:old_growth_pine_taiga", "minecraft:frozen_peaks",
        "minecraft:jagged_peaks", "minecraft:stony_peaks", "minecraft:mushroom_fields", "minecraft:meadow",
        "minecraft:grove", "minecraft:snowy_slopes", "minecraft:warm_ocean", "minecraft:deep_ocean",
        "minecraft:dripstone_caves", "minecraft:lush_caves", "minecraft:deep_dark",
    };
    private static final int MAX_EXTRA = 28;

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
        Method temperature = Biome.class.getDeclaredMethod("getTemperature", BlockPos.class, int.class);
        temperature.setAccessible(true);

        StringBuilder out = new StringBuilder("motion");
        for (Block block : BuiltInRegistries.BLOCK) {
            if (Heightmap.Types.OCEAN_FLOOR_WG.isOpaque().test(block.defaultBlockState())) {
                out.append(' ').append(BuiltInRegistries.BLOCK.getKey(block));
            }
        }
        out.append('\n');

        // Temperatures of every biome at spread positions, around and above the snow line.
        long walk = 0x2545F4914F6CDD1DL;
        List<int[]> spots = new ArrayList<>();
        for (int i = 0; i < 60; i++) {
            walk = walk * 6364136223846793005L + 1442695040888963407L;
            spots.add(new int[] {(int) (walk >> 40) % 5000, 40 + (int) ((walk >>> 20) & 0xFF) % 280, (int) (walk >> 8) % 5000});
        }
        spots.add(new int[] {0, 63, 0});
        spots.add(new int[] {-1, 80, -1});
        for (var holder : biomes.listElements().sorted((a, b) -> a.key().identifier().toString().compareTo(b.key().identifier().toString())).toList()) {
            Biome biome = holder.value();
            // Vanilla's per-thread temperature cache is keyed by position only (Moonrise removes it), so
            // a position is only asked with one sea level; the sea level varies between positions.
            int[] seas = {63, -64, 120};
            for (int i = 0; i < spots.size(); i++) {
                int[] spot = spots.get(i);
                int sea = seas[i % seas.length];
                BlockPos pos = new BlockPos(spot[0], spot[1], spot[2]);
                float value = (Float) temperature.invoke(biome, pos, sea);
                out.append("temp ").append(holder.key().identifier()).append(' ').append(sea).append(' ').append(spot[0]).append(' ')
                    .append(spot[1]).append(' ').append(spot[2]).append(' ').append(Float.floatToIntBits(value)).append(' ')
                    .append(biome.coldEnoughToSnow(pos, sea)).append(' ').append(biome.shouldMeltFrozenOceanIcebergSlightly(pos, sea)).append('\n');
            }
        }

        String[][] settingsStems = {
            {"minecraft:overworld", "minecraft:normal", "minecraft:overworld"},
            {"minecraft:large_biomes", "minecraft:large_biomes", "minecraft:overworld"},
            {"minecraft:amplified", "minecraft:amplified", "minecraft:overworld"},
            {"minecraft:caves", "minecraft:normal", "minecraft:overworld"},
            {"minecraft:floating_islands", "minecraft:normal", "minecraft:overworld"},
            {"minecraft:nether", "minecraft:normal", "minecraft:the_nether"},
            {"minecraft:end", "minecraft:normal", "minecraft:the_end"},
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
                for (int i = 0; i < CHUNKS.length; i++) notes.add("terrain");
                if (row[0].equals("minecraft:overworld")) {
                    // Scan for the wanted biomes at the chunk centres, then fill up with any other biome.
                    BiomeResolver scan = source.createResolver(state.createClimateSampler(SamplerContext.EMPTY_UNCACHED));
                    Map<String, int[]> found = new LinkedHashMap<>();
                    for (int cz = -256; cz <= 256; cz += 8) {
                        for (int cx = -256; cx <= 256; cx += 8) {
                            String name = scan.getNoiseBiome(cx * 4 + 2, 16, cz * 4 + 2).unwrapKey().orElseThrow().identifier().toString();
                            found.putIfAbsent(name, new int[] {cx, cz});
                        }
                    }
                    List<String> picked = new ArrayList<>();
                    for (String name : WANTED) if (found.containsKey(name)) picked.add(name);
                    for (String name : found.keySet()) if (!picked.contains(name) && picked.size() < MAX_EXTRA) picked.add(name);
                    for (String name : picked) {
                        chunks.add(found.get(name));
                        notes.add(name);
                    }
                    // The eroded badlands' pillars need low ground inside the biome: more of its chunks.
                    int more = 0;
                    for (int cz = -1024; cz <= 1024 && more < 16; cz += 4) {
                        for (int cx = -1024; cx <= 1024 && more < 16; cx += 4) {
                            String name = scan.getNoiseBiome(cx * 4 + 2, 16, cz * 4 + 2).unwrapKey().orElseThrow().identifier().toString();
                            if (name.equals("minecraft:eroded_badlands")) {
                                chunks.add(new int[] {cx, cz});
                                notes.add("eroded_badlands+");
                                more++;
                            }
                        }
                    }
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
                    var chunk = new ProtoChunk(new ChunkPos(c[0], c[1]), UpgradeData.EMPTY, access, factory, null);
                    var volume = new DensityVolume(16, height, 16, c[0] << 4, minY, c[1] << 4);
                    try (var noise = new NoiseChunk(state, null, settings, picker, Blender.empty(), volume)) {
                        fill.invoke(generator, noise, chunk);
                        surface.invoke(generator, chunk, noise, state, manager, null, rule);
                    }
                    out.append("chunk ").append(row[0]).append(' ').append(row[1]).append(' ').append(row[2]).append(' ').append(seed)
                        .append(' ').append(c[0]).append(' ').append(c[1]).append(' ').append(minY).append(' ').append(height)
                        .append(' ').append(notes.get(n)).append('\n');
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
