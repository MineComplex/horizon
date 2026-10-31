// Writes 26.3's noise fill for horizon-server's world generation tests
// (crates/horizon-server/src/worldgen/terrain/oracle_tests.rs). Run with the server owner's own 26.3
// jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafTerrainOracle.java <out.txt>`; the file is then given to the ignored test
// `terrain_matches_java` (HORIZON_TERRAIN_ORACLE, with HORIZON_VANILLA_JAR, HORIZON_SERVER_BLOCKS_JSON and
// HORIZON_SERVER_BLOCK_STATES). Original Horizon code.
// For every noise settings element (overworld, large_biomes, amplified, caves, floating_islands, nether,
// end) and two seeds, a NoiseChunk (no structures, no blending) is made for each of several chunks and
// NoiseBasedChunkGenerator.doFill runs on a ProtoChunk, as the TERRAIN step does before surface rules.
// Rows (block ids are Block.getId, runs are id*count in section order y, z, x):
//   motion <block name>...                      blocks the OCEAN_FLOOR_WG heightmap counts
//   chunk <settings> <seed> <cx> <cz> <minY> <height> <schedule of the last block>
//   sec <index> <runs...>                       every section
//   hm <TYPE> <256 first-available values>      OCEAN_FLOOR_WG, WORLD_SURFACE_WG (z, x order)
//   post <section> <offsets...>                 the fluids marked for post-processing, in order
//   column <settings> <seed> <x> <z> <minY> <runs...>   getBaseColumn (bottom up)
//   height <settings> <seed> <x> <z> <value>            getBaseHeight(OCEAN_FLOOR_WG)

package dev.horizonlab.leafserver;

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
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
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.biome.FixedBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.block.state.BlockState;

public final class LeafTerrainOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    private static final int[][] CHUNKS = {
        {0, 0}, {1, 0}, {0, 1}, {-1, -1}, {5, 5}, {6, 5}, {7, -3}, {20, 20}, {-30, 15}, {50, -60},
        {-200, 150}, {300, -250}, {1000, 2000}, {-777, 431}, {12, -90}, {64, 64},
    };
    private static final int[][] COLUMNS = {{0, 0}, {5, -9}, {-123, 456}, {4099, -517}, {-32000, 17}, {100, 100}};

    private static void runs(StringBuilder out, int[] ids, int count) {
        int i = 0;
        while (i < count) {
            int j = i;
            while (j < count && ids[j] == ids[i]) j++;
            out.append(' ').append(ids[i]).append('*').append(j - i);
            i = j;
        }
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
        var biome = world.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
        PalettedContainerFactory factory = PalettedContainerFactory.create(world);
        Method pickerMethod = NoiseBasedChunkGenerator.class.getDeclaredMethod("createFluidPicker", NoiseGeneratorSettings.class);
        pickerMethod.setAccessible(true);
        Method fill = NoiseBasedChunkGenerator.class.getDeclaredMethod("doFill", NoiseChunk.class, ChunkAccess.class);
        fill.setAccessible(true);

        StringBuilder out = new StringBuilder("motion");
        for (Block block : BuiltInRegistries.BLOCK) {
            if (Heightmap.Types.OCEAN_FLOOR_WG.isOpaque().test(block.defaultBlockState())) {
                out.append(' ').append(BuiltInRegistries.BLOCK.getKey(block));
            }
        }
        out.append('\n');

        var settingsRegistry = world.lookupOrThrow(Registries.NOISE_SETTINGS);
        for (var holder : settingsRegistry.listElements().sorted((a, b) -> a.key().identifier().toString().compareTo(b.key().identifier().toString())).toList()) {
            String key = holder.key().identifier().toString();
            NoiseGeneratorSettings settings = holder.value();
            int minY = settings.noiseSettings().minY();
            int height = settings.noiseSettings().height();
            var access = LevelHeightAccessor.create(minY, height);
            var generator = new NoiseBasedChunkGenerator(new FixedBiomeSource(biome), holder);
            var picker = (Aquifer.FluidPicker) pickerMethod.invoke(null, settings);
            for (long seed : SEEDS) {
                RandomState state = RandomState.create(noises, seed, settings);
                for (int[] c : CHUNKS) {
                    var chunk = new ProtoChunk(new ChunkPos(c[0], c[1]), UpgradeData.EMPTY, access, factory, null);
                    var volume = new DensityVolume(16, height, 16, c[0] << 4, minY, c[1] << 4);
                    try (var noise = new NoiseChunk(state, null, settings, picker, Blender.empty(), volume)) {
                        fill.invoke(generator, noise, chunk);
                        out.append("chunk ").append(key).append(' ').append(seed).append(' ').append(c[0]).append(' ').append(c[1])
                            .append(' ').append(minY).append(' ').append(height).append(' ').append(noise.aquifer().shouldScheduleFluidUpdate()).append('\n');
                    }
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
                for (int[] column : COLUMNS) {
                    var noiseColumn = generator.getBaseColumn(column[0], column[1], access, state);
                    int[] ids = new int[height];
                    for (int y = 0; y < height; y++) ids[y] = Block.getId(noiseColumn.getBlock(minY + y));
                    out.append("column ").append(key).append(' ').append(seed).append(' ').append(column[0]).append(' ').append(column[1]).append(' ').append(minY);
                    runs(out, ids, height);
                    out.append('\n');
                    out.append("height ").append(key).append(' ').append(seed).append(' ').append(column[0]).append(' ').append(column[1]).append(' ')
                        .append(generator.getBaseHeight(column[0], column[1], Heightmap.Types.OCEAN_FLOOR_WG, access, state)).append('\n');
                }
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
