// Writes 26.3's climate and biome results for horizon-server's biome tests
// (crates/horizon-server/src/worldgen/biome/oracle_tests.rs). Run with the server owner's own 26.3 jar and
// its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafBiomeOracle.java <out.txt>`; the file is then given to the ignored test
// `biomes_match_java` (HORIZON_BIOME_ORACLE, with HORIZON_VANILLA_JAR and HORIZON_BIOME_REPORTS, the data
// generator's reports directory). Original Horizon code.
// The world registries are loaded as WorldLoader does from the vanilla pack. Rows, in the order computed (the
// biome search keeps its previous result per tree, so the Rust side replays them in this order):
//   preset <preset> <i> <biome> <t0> <t1> <h0> <h1> <c0> <c1> <e0> <e1> <d0> <d1> <w0> <w1> <offset>
//                      every entry of the two built-in multi-noise presets, quantized
//   level <world preset> <stem> <seed>            the stems below use this level (RandomState.create)
//   target <qx> <qy> <qz> <six quantized values>  Climate.Sampler.sample, an uncached context
//   biome <qx> <qy> <qz> <biome>                  createResolver(that sampler).getNoiseBiome
//   chunk <cx> <cz> <section> <64 biomes, y z x order> <PalettedContainer.write bytes, hex>
//                      createResolverForChunk with a caching context, then Paper's section fill
//                      (LevelChunkSection.fillBiomesFromNoise in Leaf: the biome at 0 0 0 seeds the
//                      container, then y, z, x order)
//   spawn <x> <z>                                 NoiseSpawnFinder.findSpawnPosition (overworld stems)
//   zoom <seed> <obfuscated> <x> <y> <z> <vanilla qx qy qz> <leaf qx qy qz>
//                      BiomeManager.getBiome's quart in the jar (vanilla) and with Leaf's patched loop
//                      (Carpet-Fixes 0087: x² + y² + z² instead of z² + y² + x²), written out here

package dev.horizonlab.leafserver;

import io.netty.buffer.Unpooled;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.LinearCongruentialGenerator;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList;
import net.minecraft.world.level.chunk.PalettedContainer;
import net.minecraft.world.level.chunk.Strategy;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseSpawnFinder;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.densityfunction.SamplerContext;
import net.minecraft.world.level.levelgen.presets.WorldPresets;

public final class LeafBiomeOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    private static final int[][] CHUNKS = {{0, 0}, {-1, 3}, {100, -37}, {-4093, 777}, {1874998, -1874999}};

    /** A deterministic spread of quart columns: near the origin, a coarse grid, far and wrapping ones. */
    private static List<int[]> columns() {
        List<int[]> columns = new ArrayList<>();
        int[] fixed = {0, 1, -1, 65, -129, 1024, 3086, -24691, 74999, -250001, 7499995, -7499996, 536870911, -536870912};
        for (int i = 0; i < fixed.length; i++) columns.add(new int[] {fixed[i], fixed[fixed.length - 1 - i]});
        for (int x = -6; x < 6; x++) for (int z = -6; z < 6; z++) columns.add(new int[] {x * 37 + 3, z * 41 - 5});
        long state = 0x5DEECE66DL;
        for (int i = 0; i < 60; i++) {
            state = state * 6364136223846793005L + 1442695040888963407L;
            columns.add(new int[] {(int) (state >> 40) % 400_000, (int) (state >> 16) % 400_000});
        }
        return columns;
    }

    private static double leafFiddle(long seed) {
        return (((int) (seed >>> 24) & 1023) - 512) * (0.9 / 1024.0);
    }

    /** Leaf's BiomeManager.getBiome corner choice (its early exits do not change the result). */
    private static int[] leafZoom(long seed, int blockX, int blockY, int blockZ) {
        int ax = blockX - 2, ay = blockY - 2, az = blockZ - 2;
        int x = ax >> 2, y = ay >> 2, z = az >> 2;
        double fx = (ax & 3) / 4.0, fy = (ay & 3) / 4.0, fz = (az & 3) / 4.0;
        int best = 0;
        double bestDistance = Double.POSITIVE_INFINITY;
        for (int i = 0; i < 8; i++) {
            boolean xe = (i & 4) == 0, ye = (i & 2) == 0, ze = (i & 1) == 0;
            int cx = xe ? x : x + 1, cy = ye ? y : y + 1, cz = ze ? z : z + 1;
            double dx = xe ? fx : fx - 1.0, dy = ye ? fy : fy - 1.0, dz = ze ? fz : fz - 1.0;
            long s = LinearCongruentialGenerator.next(seed, cx);
            s = LinearCongruentialGenerator.next(s, cy);
            s = LinearCongruentialGenerator.next(s, cz);
            s = LinearCongruentialGenerator.next(s, cx);
            s = LinearCongruentialGenerator.next(s, cy);
            s = LinearCongruentialGenerator.next(s, cz);
            double sx = dx + leafFiddle(s);
            s = LinearCongruentialGenerator.next(s, seed);
            double sy = dy + leafFiddle(s);
            s = LinearCongruentialGenerator.next(s, seed);
            double sz = dz + leafFiddle(s);
            double distance = sx * sx + sy * sy + sz * sz;
            if (bestDistance > distance) {
                bestDistance = distance;
                best = i;
            }
        }
        return new int[] {(best & 4) == 0 ? x : x + 1, (best & 2) == 0 ? y : y + 1, (best & 1) == 0 ? z : z + 1};
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
        var biomeRegistry = world.lookupOrThrow(Registries.BIOME);
        Strategy<Holder<Biome>> biomeStrategy = Strategy.createForBiomes(biomeRegistry.asHolderIdMap());
        StringBuilder out = new StringBuilder();

        for (var preset : MultiNoiseBiomeSourceParameterList.knownPresets().entrySet()) {
            int i = 0;
            for (var entry : preset.getValue().values()) {
                Climate.ParameterPoint p = entry.getFirst();
                out.append("preset ").append(preset.getKey().id()).append(' ').append(i++).append(' ').append(entry.getSecond().identifier());
                for (Climate.Parameter q : List.of(p.temperature(), p.humidity(), p.continentalness(), p.erosion(), p.depth(), p.weirdness())) {
                    out.append(' ').append(q.min()).append(' ').append(q.max());
                }
                out.append(' ').append(p.offset()).append('\n');
            }
        }

        var worldPresets = world.lookupOrThrow(Registries.WORLD_PRESET);
        String[][] stems = {
            {"minecraft:normal", "minecraft:overworld"}, {"minecraft:normal", "minecraft:the_nether"}, {"minecraft:normal", "minecraft:the_end"},
            {"minecraft:large_biomes", "minecraft:overworld"}, {"minecraft:amplified", "minecraft:overworld"},
        };
        List<int[]> columns = columns();
        int[] quartYs = {-20, -16, -9, 0, 7, 15, 16, 31, 40, 79, 100};
        for (long seed : SEEDS) {
            for (String[] stemId : stems) {
                var worldPreset = worldPresets.listElements().filter(h -> h.key().identifier().toString().equals(stemId[0])).findFirst().orElseThrow().value();
                LevelStem stem = worldPreset.createWorldDimensions().dimensions().entrySet().stream()
                    .filter(e -> e.getKey().identifier().toString().equals(stemId[1])).findFirst().orElseThrow().getValue();
                var generator = (NoiseBasedChunkGenerator) stem.generator();
                NoiseGeneratorSettings settings = generator.generatorSettings().value();
                BiomeSource source = generator.getBiomeSource();
                RandomState state = RandomState.create(noises, seed, settings);
                out.append("level ").append(stemId[0]).append(' ').append(stemId[1]).append(' ').append(seed).append('\n');
                Climate.Sampler sampler = state.createClimateSampler(SamplerContext.EMPTY_UNCACHED);
                BiomeResolver resolver = source.createResolver(sampler);
                for (int[] column : columns) {
                    for (int qy : quartYs) {
                        Climate.TargetPoint t = sampler.sample(column[0], qy, column[1]);
                        out.append("target ").append(column[0]).append(' ').append(qy).append(' ').append(column[1])
                            .append(' ').append(t.temperature()).append(' ').append(t.humidity()).append(' ').append(t.continentalness())
                            .append(' ').append(t.erosion()).append(' ').append(t.depth()).append(' ').append(t.weirdness()).append('\n');
                        out.append("biome ").append(column[0]).append(' ').append(qy).append(' ').append(column[1]).append(' ')
                            .append(resolver.getNoiseBiome(column[0], qy, column[1]).unwrapKey().orElseThrow().identifier()).append('\n');
                    }
                }
                int minY = settings.noiseSettings().minY(), height = settings.noiseSettings().height();
                for (int[] chunk : CHUNKS) {
                    Climate.Sampler cached = state.createClimateSampler(SamplerContext.builder().enableCaches().build());
                    int qx = chunk[0] << 2, qz = chunk[1] << 2, qMinY = minY >> 2;
                    BiomeResolver chunkResolver = source.createResolverForChunk(cached, qx, qMinY, qz, 4, height >> 2, 4);
                    for (int section = 0; section < height >> 4; section++) {
                        int qy = qMinY + section * 4;
                        PalettedContainer<Holder<Biome>> container = new PalettedContainer<>(chunkResolver.getNoiseBiome(qx, qy, qz), biomeStrategy);
                        for (int y = 0; y < 4; y++) {
                            for (int z = 0; z < 4; z++) {
                                for (int x = y == 0 && z == 0 ? 1 : 0; x < 4; x++) {
                                    container.set(x, y, z, chunkResolver.getNoiseBiome(qx + x, qy + y, qz + z));
                                }
                            }
                        }
                        out.append("chunk ").append(chunk[0]).append(' ').append(chunk[1]).append(' ').append(section);
                        for (int i = 0; i < 64; i++) {
                            out.append(' ').append(container.get(i & 3, i >> 4, (i >> 2) & 3).unwrapKey().orElseThrow().identifier());
                        }
                        FriendlyByteBuf buffer = new FriendlyByteBuf(Unpooled.buffer());
                        container.write(buffer);
                        byte[] bytes = new byte[buffer.readableBytes()];
                        buffer.readBytes(bytes);
                        out.append(' ').append(HexFormat.of().formatHex(bytes)).append('\n');
                    }
                }
                if (!settings.spawnTarget().isEmpty()) {
                    BlockPos spawn = NoiseSpawnFinder.findSpawnPosition(settings.spawnTarget(), state.samplersWithContext(SamplerContext.builder().enableCaches().build()));
                    out.append("spawn ").append(spawn.getX()).append(' ').append(spawn.getZ()).append('\n');
                }
            }
            long obfuscated = BiomeManager.obfuscateSeed(seed);
            int[] last = new int[3];
            BiomeManager manager = new BiomeManager((x, y, z) -> {
                last[0] = x;
                last[1] = y;
                last[2] = z;
                return null;
            }, obfuscated);
            List<int[]> blocks = new ArrayList<>();
            for (int x = -9; x < 9; x++) for (int y = -9; y < 9; y++) for (int z = -9; z < 9; z++) blocks.add(new int[] {x, y * 7, z});
            long state = seed;
            for (int i = 0; i < 20000; i++) {
                state = state * 6364136223846793005L + 1442695040888963407L;
                int x = (int) (state >> 32), z = (int) state;
                blocks.add(new int[] {i % 2 == 0 ? x : x % 30_000_000, (int) (state >>> 52) % 4096 - 2048, i % 2 == 0 ? z : z % 30_000_000});
            }
            int[] extremes = {Integer.MIN_VALUE, Integer.MIN_VALUE + 1, -2, 2, Integer.MAX_VALUE - 1, Integer.MAX_VALUE};
            for (int x : extremes) for (int y : extremes) for (int z : extremes) blocks.add(new int[] {x, y, z});
            for (int[] b : blocks) {
                manager.getBiome(b[0], b[1], b[2]);
                int[] leaf = leafZoom(obfuscated, b[0], b[1], b[2]);
                out.append("zoom ").append(seed).append(' ').append(obfuscated).append(' ').append(b[0]).append(' ').append(b[1]).append(' ').append(b[2])
                    .append(' ').append(last[0]).append(' ').append(last[1]).append(' ').append(last[2])
                    .append(' ').append(leaf[0]).append(' ').append(leaf[1]).append(' ').append(leaf[2]).append('\n');
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
