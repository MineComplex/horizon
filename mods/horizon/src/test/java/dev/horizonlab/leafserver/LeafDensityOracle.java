// Writes 26.3's density values for horizon-server's world generation tests
// (crates/horizon-server/src/worldgen/density/oracle_tests.rs). Run with the server owner's own 26.3
// jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafDensityOracle.java <out.txt>`; the file is then given to the ignored test
// `density_matches_java` (HORIZON_DENSITY_ORACLE, with HORIZON_VANILLA_JAR). Original Horizon code.
// The world registries are loaded as WorldLoader does from the vanilla pack; for every noise settings
// element and two seeds, RandomState.create compiles the router (and the aquifer roots), and the rows
// record the samplers' values, all as float bits:
//   value <settings> <seed> <root> <x> <y> <z> <bits>          sampleValue, uncached context
//   volume <settings> <seed> <root> <sx> <sy> <sz> <mx> <my> <mz> <dx> <dy> <dz> <bits...>
//                                                              sampleVolume, a cached context, then
//   cached <settings> <seed> <root> <x> <y> <z> <bits>         sampleValue with that same context
// Roots: the eight router fields by name, aquifer_<field>, and noise:<id> for every noise registry
// element as {"type":"minecraft:noise","noise":<id>,"xz_scale":1,"y_scale":1}.

package dev.horizonlab.leafserver;

import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
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
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.densityfunction.DensityBuffer;
import net.minecraft.world.level.levelgen.densityfunction.DensityFunction;
import net.minecraft.world.level.levelgen.densityfunction.DensitySampler;
import net.minecraft.world.level.levelgen.densityfunction.DensityVolume;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;

public final class LeafDensityOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    private static final int[] COLUMNS = {0, 0, 1, -1, 263, -517, 4099, 12345, -98765, 7, 299_999, -1_000_003, 29_999_983, -29_999_984};
    private static final int[] HEIGHTS = {-64, -37, -1, 0, 31, 63, 100, 127, 255, 319};

    private static String bits(float value) {
        return Integer.toHexString(Float.floatToRawIntBits(value));
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
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        var noises = world.lookupOrThrow(Registries.NOISE);
        var ops = RegistryOps.create(JsonOps.INSTANCE, world);
        Map<String, DensityFunction> noiseRoots = new TreeMap<>();
        noises.listElementIds().forEach(key -> {
            String id = key.identifier().toString();
            var json = JsonParser.parseString("{\"type\":\"minecraft:noise\",\"noise\":\"" + id + "\",\"xz_scale\":1,\"y_scale\":1}");
            noiseRoots.put("noise:" + id, DensityFunction.CODEC.parse(ops, json).getOrThrow());
        });

        StringBuilder out = new StringBuilder();
        var settingsRegistry = world.lookupOrThrow(Registries.NOISE_SETTINGS);
        for (var holder : settingsRegistry.listElements().sorted((a, b) -> a.key().identifier().toString().compareTo(b.key().identifier().toString())).toList()) {
            String key = holder.key().identifier().toString();
            NoiseGeneratorSettings settings = holder.value();
            boolean main = key.equals("minecraft:overworld") || key.equals("minecraft:nether") || key.equals("minecraft:end");
            var router = settings.noiseRouter();
            Map<String, DensityFunction> roots = new LinkedHashMap<>();
            roots.put("temperature", router.temperature());
            roots.put("vegetation", router.vegetation());
            roots.put("continents", router.continents());
            roots.put("erosion", router.erosion());
            roots.put("depth", router.depth());
            roots.put("ridges", router.ridges());
            roots.put("chunk_surface_level", router.chunkSurfaceLevel());
            roots.put("final_density", router.finalDensity());
            settings.aquifers().ifPresent(aquifers -> {
                roots.put("aquifer_barrier", aquifers.barrierNoise());
                roots.put("aquifer_fluid_level_floodedness", aquifers.fluidLevelFloodednessNoise());
                roots.put("aquifer_fluid_level_spread", aquifers.fluidLevelSpreadNoise());
                roots.put("aquifer_lava", aquifers.lavaNoise());
                roots.put("aquifer_exclusion", aquifers.exclusion());
                roots.put("aquifer_surface_level", aquifers.surfaceLevel());
            });
            boolean withNoises = key.equals("minecraft:overworld") || key.equals("minecraft:nether");
            if (withNoises) roots.putAll(noiseRoots);
            int minY = settings.noiseSettings().minY();
            int height = settings.noiseSettings().height();
            for (long seed : SEEDS) {
                RandomState state = RandomState.create(noises, seed, settings);
                for (var root : roots.entrySet()) {
                    DensitySampler sampler = state.getSampler(root.getValue());
                    String prefix = key + " " + seed + " " + root.getKey() + " ";
                    for (int i = 0; i + 1 < COLUMNS.length; i++) {
                        for (int y : HEIGHTS) {
                            int x = COLUMNS[i], z = COLUMNS[COLUMNS.length - 1 - i];
                            out.append("value ").append(prefix).append(x).append(' ').append(y).append(' ').append(z).append(' ')
                                .append(bits(sampler.sampleValue(SamplerContext.EMPTY_UNCACHED, x, y, z))).append('\n');
                        }
                    }
                    List<DensityVolume> volumes = new ArrayList<>();
                    volumes.add(new DensityVolume(5, 5, 5, -20, minY, 36, 4, 4, 4));
                    volumes.add(new DensityVolume(3, 4, 2, 4099, 7, -517, 8, 3, 5));
                    if (root.getKey().equals("chunk_surface_level")) {
                        volumes.add(new DensityVolume(16, 1, 16, 16 * 7, 0, -16 * 3, 1, 1, 1));
                    }
                    if (main && root.getKey().equals("final_density")) {
                        // A NoiseChunk's fill: the whole chunk at block resolution.
                        volumes.add(new DensityVolume(16, height, 16, 16 * 7, minY, -16 * 3, 1, 1, 1));
                        volumes.add(new DensityVolume(16, height, 16, 16 * 1000, minY, 16 * 2000, 1, 1, 1));
                    }
                    for (DensityVolume volume : volumes) {
                        SamplerContext context = SamplerContext.builder().enableCaches().build();
                        DensityBuffer buffer = DensityBuffer.createUnpooled(volume.size());
                        sampler.sampleVolume(context, buffer, volume);
                        out.append("volume ").append(prefix)
                            .append(volume.sizeX()).append(' ').append(volume.sizeY()).append(' ').append(volume.sizeZ()).append(' ')
                            .append(volume.minBlockX()).append(' ').append(volume.minBlockY()).append(' ').append(volume.minBlockZ()).append(' ')
                            .append(volume.stepBlockX()).append(' ').append(volume.stepBlockY()).append(' ').append(volume.stepBlockZ());
                        for (int i = 0; i < volume.size(); i++) out.append(' ').append(bits(buffer.get(i)));
                        out.append('\n');
                        // The same context again, point by point: lattice hits and misses.
                        for (int[] p : new int[][] {
                            {volume.minBlockX(), volume.minBlockY(), volume.minBlockZ()},
                            {volume.minBlockX() + volume.stepBlockX(), volume.minBlockY() + 2 * volume.stepBlockY(), volume.minBlockZ() + volume.stepBlockZ()},
                            {volume.minBlockX() + 1, volume.minBlockY() + 1, volume.minBlockZ() + 3},
                        }) {
                            out.append("cached ").append(prefix).append(p[0]).append(' ').append(p[1]).append(' ').append(p[2]).append(' ')
                                .append(bits(sampler.sampleValue(context, p[0], p[1], p[2]))).append('\n');
                        }
                    }
                }
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
