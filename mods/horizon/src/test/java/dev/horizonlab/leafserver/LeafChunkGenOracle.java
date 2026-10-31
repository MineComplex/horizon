// Writes 26.3's chunk status pipeline up to TERRAIN (noise, surface and carvers in one step) for
// horizon-server's world generation tests (crates/horizon-server/src/worldgen/generator/oracle_tests.rs).
// Run with the server owner's own 26.3 jar and its libraries on the class path (Java's single-file
// source launcher): `java -cp <jar:libs> LeafChunkGenOracle.java <out.txt>`; the file is then given to
// the ignored test `chunk_generation_matches_java` (HORIZON_CHUNKGEN_ORACLE, with HORIZON_VANILLA_JAR,
// HORIZON_SERVER_BLOCKS_JSON, HORIZON_SERVER_BLOCK_STATES and HORIZON_BIOME_REPORTS). Original Horizon code.
//
// The jar is vanilla's: its ChunkPyramid, ChunkLevel and the generator's own steps are run; Moonrise's
// scheduler and Paper's changes are not in it. For the overworld, the Nether and the End of the normal
// world preset and two seeds, each chunk of a list goes through the generation pyramid's steps as
// ChunkStatusTasks runs them, on this thread (vanilla runs them on worker threads; one thread keeps the
// climate tree's last result in a known order, which the Rust side replays):
// STRUCTURE_STARTS and STRUCTURE_REFERENCES with generate-structures off (no starts, so no references),
// BIOMES for the chunk and its 8 neighbours (doCreateBiomes, as createBiomes runs it), then TERRAIN for
// the chunk: buildTerrain's body (createNoiseChunk with a StructureManager over these chunks, doFill,
// buildSurface with the region's BiomeManager and the 3 x 3 chunks' palette biomes, generateCarvers,
// then the four final heightmaps primed as ChunkStatusTasks.buildTerrain does).
// Rows:
//   step <status> <writeRadius> <accumulated dependencies...>   ChunkPyramid.GENERATION_PYRAMID
//   level <ticket level> <generationStatus or none>              ChunkLevel.generationStatus
//   masks <runs>                 per block state id, the heightmap types (bit = ordinal) it is opaque for
//   biomes <id...>               the biome registry's ids in id order
//   chunk <settings> <stem> <seed> <cx> <cz> <minY> <height> <status> <starts> <references>
//   sec <index> <runs...>        every section's block state ids (y, z, x)
//   bio <index> <64 biome ids>   every section's biomes (y, z, x)
//   hm <TYPE> <256 first-available values>   every heightmap the chunk has (z, x order)
//   post <section> <offsets...>  the fluids marked for post-processing, in order

package dev.horizonlab.leafserver;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
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
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.level.ChunkLevel;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkPyramid;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;

public final class LeafChunkGenOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    private static final int[][] CHUNKS = {
        {0, 0}, {1, 0}, {-1, -1}, {5, 5}, {20, 20}, {-30, 15}, {50, -60}, {-200, 150}, {300, -250}, {1000, 2000},
    };
    private static final int EXTRA = 6;

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

    private static long key(int x, int z) {
        return (long) x & 0xFFFFFFFFL | ((long) z & 0xFFFFFFFFL) << 32;
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
        var biomeRegistry = world.lookupOrThrow(Registries.BIOME);
        var worldPresets = world.lookupOrThrow(Registries.WORLD_PRESET);
        PalettedContainerFactory factory = PalettedContainerFactory.create(world);

        Method createNoiseChunk = NoiseBasedChunkGenerator.class.getDeclaredMethod("createNoiseChunk", ChunkAccess.class,
            StructureManager.class, Blender.class, RandomState.class, NoiseSettings.class);
        createNoiseChunk.setAccessible(true);
        Method fill = NoiseBasedChunkGenerator.class.getDeclaredMethod("doFill", NoiseChunk.class, ChunkAccess.class);
        fill.setAccessible(true);
        Method surface = NoiseBasedChunkGenerator.class.getDeclaredMethod("buildSurface", ChunkAccess.class, NoiseChunk.class,
            RandomState.class, BiomeManager.class, Set.class, MaterialRule.class);
        surface.setAccessible(true);
        Method carvers = NoiseBasedChunkGenerator.class.getDeclaredMethod("generateCarvers", ChunkAccess.class, Blender.class,
            NoiseChunk.class, RandomState.class, BiomeManager.class, net.minecraft.server.level.WorldGenRegion.class, MaterialRule.class);
        carvers.setAccessible(true);
        Method createBiomes = net.minecraft.world.level.chunk.ChunkGenerator.class.getDeclaredMethod("doCreateBiomes", Blender.class,
            RandomState.class, ChunkAccess.class);
        createBiomes.setAccessible(true);

        StringBuilder out = new StringBuilder();
        for (ChunkStatus status : ChunkStatus.getStatusList()) {
            var step = ChunkPyramid.GENERATION_PYRAMID.getStepTo(status);
            out.append("step ").append(status.getName()).append(' ').append(step.blockStateWriteRadius());
            for (ChunkStatus dependency : step.accumulatedDependencies().asList()) out.append(' ').append(dependency.getName());
            out.append('\n');
        }
        for (int level = 28; level <= 46; level++) {
            ChunkStatus status = ChunkLevel.generationStatus(level);
            out.append("level ").append(level).append(' ').append(status == null ? "none" : status.getName()).append('\n');
        }
        int stateCount = Block.BLOCK_STATE_REGISTRY.size();
        int[] masks = new int[stateCount];
        Heightmap.Types[] types = Heightmap.Types.values();
        for (int id = 0; id < stateCount; id++) {
            BlockState state = Block.BLOCK_STATE_REGISTRY.byId(id);
            for (int t = 0; t < types.length; t++) {
                if (types[t].isOpaque().test(state)) masks[id] |= 1 << t;
            }
        }
        out.append("masks");
        runs(out, masks, stateCount);
        out.append('\n');
        out.append("biomes");
        Map<Biome, Integer> biomeIds = new HashMap<>();
        var biomeList = biomeRegistry.listElements().toList();
        for (var holder : biomeList) {
            out.append(' ').append(holder.key().identifier());
        }
        out.append('\n');
        var biomeRegistryValue = world.lookupOrThrow(Registries.BIOME);

        String[][] stems = {
            {"minecraft:overworld", "minecraft:overworld"},
            {"minecraft:nether", "minecraft:the_nether"},
            {"minecraft:end", "minecraft:the_end"},
        };
        var settingsRegistry = world.lookupOrThrow(Registries.NOISE_SETTINGS);
        var preset = worldPresets.listElements().filter(h -> h.key().identifier().toString().equals("minecraft:normal")).findFirst().orElseThrow().value();
        for (String[] row : stems) {
            var holder = settingsRegistry.listElements().filter(h -> h.key().identifier().toString().equals(row[0])).findFirst().orElseThrow();
            NoiseGeneratorSettings settings = holder.value();
            LevelStem stem = preset.createWorldDimensions().dimensions().entrySet().stream()
                .filter(e -> e.getKey().identifier().toString().equals(row[1])).findFirst().orElseThrow().getValue();
            BiomeSource source = ((NoiseBasedChunkGenerator) stem.generator()).getBiomeSource();
            int minY = settings.noiseSettings().minY();
            int height = settings.noiseSettings().height();
            var generator = new NoiseBasedChunkGenerator(source, holder);
            MaterialRule rule = settings.materialRule().value();
            for (long seed : SEEDS) {
                RandomState state = RandomState.create(noises, seed, settings);
                List<int[]> chunks = new ArrayList<>(List.of(CHUNKS));
                long walk = 0x2545F4914F6CDD1DL ^ seed;
                for (int i = 0; i < EXTRA; i++) {
                    walk = walk * 6364136223846793005L + 1442695040888963407L;
                    chunks.add(new int[] {(int) (walk >> 40) % 400, (int) (walk >> 8) % 400});
                }
                for (int[] c : chunks) {
                    resetSearch(source);
                    Map<Long, ProtoChunk> region = new HashMap<>();
                    var access = net.minecraft.world.level.LevelHeightAccessor.create(minY, height);
                    // STRUCTURE_STARTS (generate-structures off), STRUCTURE_REFERENCES (nothing to
                    // reference), BIOMES: the 3 x 3 chunks, z then x.
                    for (int dz = -1; dz <= 1; dz++) {
                        for (int dx = -1; dx <= 1; dx++) {
                            var chunk = new ProtoChunk(new ChunkPos(c[0] + dx, c[1] + dz), UpgradeData.EMPTY, access, factory, null);
                            chunk.setPersistedStatus(ChunkStatus.STRUCTURE_REFERENCES);
                            createBiomes.invoke(generator, Blender.empty(), state, chunk);
                            chunk.setPersistedStatus(ChunkStatus.BIOMES);
                            region.put(key(c[0] + dx, c[1] + dz), chunk);
                        }
                    }
                    ProtoChunk chunk = region.get(key(c[0], c[1]));
                    LevelAccessor level = (LevelAccessor) Proxy.newProxyInstance(LevelAccessor.class.getClassLoader(),
                        new Class<?>[] {LevelAccessor.class}, (proxy, method, margs) -> {
                            if (method.getName().equals("getChunk") && margs != null && margs.length >= 2
                                && margs[0] instanceof Integer x && margs[1] instanceof Integer z) {
                                return region.get(key(x, z));
                            }
                            throw new UnsupportedOperationException(method.toString());
                        });
                    StructureManager structures = new StructureManager(level, new WorldOptions(seed, false, false), null);
                    // TERRAIN: buildTerrain's body.
                    BiomeManager biomeManager = new BiomeManager((qx, qy, qz) -> region.get(key(qx >> 2, qz >> 2)).getNoiseBiome(qx, qy, qz),
                        BiomeManager.obfuscateSeed(seed));
                    Set<Holder<Biome>> possible = new HashSet<>();
                    for (int dz = -1; dz <= 1; dz++) {
                        for (int dx = -1; dx <= 1; dx++) {
                            region.get(key(c[0] + dx, c[1] + dz)).collectBiomesInPalette(possible);
                        }
                    }
                    NoiseSettings noiseSettings = settings.noiseSettings().clampToHeightAccessor(chunk.getHeightAccessorForGeneration());
                    if (noiseSettings.height() > 0) {
                        try (NoiseChunk noise = (NoiseChunk) createNoiseChunk.invoke(generator, chunk, structures, Blender.empty(), state, noiseSettings)) {
                            fill.invoke(generator, noise, chunk);
                            surface.invoke(generator, chunk, noise, state, biomeManager, possible, rule);
                            carvers.invoke(generator, chunk, Blender.empty(), noise, state, biomeManager, null, rule);
                        }
                    }
                    Heightmap.primeHeightmaps(chunk, EnumSet.of(Heightmap.Types.MOTION_BLOCKING, Heightmap.Types.MOTION_BLOCKING_NO_LEAVES,
                        Heightmap.Types.OCEAN_FLOOR, Heightmap.Types.WORLD_SURFACE));
                    chunk.setPersistedStatus(ChunkStatus.TERRAIN);

                    out.append("chunk ").append(row[0]).append(' ').append(row[1]).append(' ').append(seed)
                        .append(' ').append(c[0]).append(' ').append(c[1]).append(' ').append(minY).append(' ').append(height)
                        .append(' ').append(chunk.getPersistedStatus().getName())
                        .append(' ').append(chunk.getAllStarts().size()).append(' ').append(chunk.getAllReferences().size()).append('\n');
                    int[] ids = new int[4096];
                    for (int s = 0; s < height / 16; s++) {
                        var section = chunk.getSection(s);
                        for (int y = 0; y < 16; y++) for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) {
                            ids[y * 256 + z * 16 + x] = Block.getId(section.getBlockState(x, y, z));
                        }
                        out.append("sec ").append(s);
                        runs(out, ids, 4096);
                        out.append('\n');
                        out.append("bio ").append(s);
                        for (int y = 0; y < 4; y++) for (int z = 0; z < 4; z++) for (int x = 0; x < 4; x++) {
                            out.append(' ').append(biomeRegistryValue.getOrThrow(section.getNoiseBiome(x, y, z).unwrapKey().orElseThrow())
                                .key().identifier());
                        }
                        out.append('\n');
                    }
                    for (var entry : chunk.getHeightmaps()) {
                        var map = entry.getValue();
                        out.append("hm ").append(entry.getKey().getSerializationKey());
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
