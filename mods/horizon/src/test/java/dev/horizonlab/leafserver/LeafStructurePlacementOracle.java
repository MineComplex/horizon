// Writes 26.3's structure placement answers for horizon-server's tests
// (crates/horizon-server/src/worldgen/structure/oracle_tests.rs). Run with the server owner's own 26.3
// jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafStructurePlacementOracle.java <out.txt>`; the file is then given to the
// ignored test `structure_placement_matches_java` (HORIZON_STRUCTURE_ORACLE, with HORIZON_VANILLA_JAR).
// Original Horizon code. The world registries are loaded as WorldLoader does from the vanilla pack.
// Rows:
//   spread <seed> <set> <x> <z> <potentialX> <potentialZ> <isStructureChunk>
//       every random-spread set, chunks around centres near the origin, negative, far and at the int
//       limits (getPotentialStructureChunk, then StructurePlacement.isStructureChunk with frequency
//       and exclusion zone)
//   reducer <method> <seed> <salt> <x> <z> <probabilityBits> <shouldGenerate>
//   ring <seed> <set> <index> <x> <z>
//       concentric-ring positions of a level whose biome source is a fixed checkerboard of
//       minecraft:plains and minecraft:deep_ocean (CheckerBiomeSource; the Rust test mirrors it), so
//       ring geometry, the forked search randoms and findBiomeHorizontal run as in vanilla
//   realring <seed> <set> <index> <x> <z>
//       the same on the normal world preset's overworld biome source (multi-noise), searched with the
//       level's own random state
//   hashset <keys...> | <order...>     fastutil LongOpenHashSet(long[]) iteration order
//   start <input snbt> | <createTag snbt or null> | <bounding box or none>
//       StructureStart.loadStaticStart then createTag in chunk (3, -7)

package dev.horizonlab.leafserver;

import com.mojang.serialization.MapCodec;
import it.unimi.dsi.fastutil.longs.LongOpenHashSet;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Random;
import java.util.stream.Stream;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.TagParser;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeResolver;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.placement.AbstractSpreadingStructurePlacement;
import net.minecraft.world.level.levelgen.structure.placement.ConcentricRingsStructurePlacement;
import net.minecraft.world.level.levelgen.structure.placement.RandomSpreadStructurePlacement;

public final class LeafStructurePlacementOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L, 0L};
    private static final int[][] CENTRES = {
        {0, 0}, {-13, -13}, {-1000, 777}, {1874990, -1875000}, {Integer.MAX_VALUE - 40, Integer.MIN_VALUE + 3}, {-75000, -250000},
    };
    private static final int SIZE = 16;

    /** minecraft:plains on rare quarts, minecraft:deep_ocean elsewhere. */
    static boolean preferred(int qx, int qz) {
        int h = qx * 73428767 ^ qz * 912931;
        return Math.floorMod(h, 97) < 2 && Math.floorMod(qx >> 5, 3) != 0;
    }

    static final class CheckerBiomeSource extends BiomeSource {
        final Holder<Biome> plains, ocean;

        CheckerBiomeSource(Holder<Biome> plains, Holder<Biome> ocean) {
            this.plains = plains;
            this.ocean = ocean;
        }

        @Override
        protected MapCodec<? extends BiomeSource> codec() {
            return null;
        }

        @Override
        protected Stream<Holder<Biome>> collectPossibleBiomes() {
            return Stream.of(plains, ocean);
        }

        @Override
        public BiomeResolver createResolver(Climate.Sampler sampler) {
            return (x, y, z) -> preferred(x, z) ? plains : ocean;
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
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        var sets = world.lookupOrThrow(Registries.STRUCTURE_SET);
        var biomes = world.lookupOrThrow(Registries.BIOME);
        var source = new CheckerBiomeSource(
            biomes.getOrThrow(ResourceKey.create(Registries.BIOME, Identifier.parse("minecraft:plains"))),
            biomes.getOrThrow(ResourceKey.create(Registries.BIOME, Identifier.parse("minecraft:deep_ocean"))));
        NoiseGeneratorSettings overworld = world.lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(NoiseGeneratorSettings.OVERWORLD).value();
        var worldPresets = world.lookupOrThrow(Registries.WORLD_PRESET);
        var normal = worldPresets.listElements().filter(h -> h.key().identifier().toString().equals("minecraft:normal"))
            .findFirst().orElseThrow().value();
        BiomeSource realSource = ((net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator) normal.createWorldDimensions()
            .dimensions().entrySet().stream().filter(e -> e.getKey().identifier().toString().equals("minecraft:overworld"))
            .findFirst().orElseThrow().getValue().generator()).getBiomeSource();
        StringBuilder out = new StringBuilder();
        for (long seed : SEEDS) {
            RandomState randomState = RandomState.create(world.lookupOrThrow(Registries.NOISE), seed, overworld);
            var state = ChunkGeneratorStructureState.createForNormal(randomState, seed, new ChunkPos(0, 0), source, sets);
            for (var holder : sets.listElements().toList()) {
                String id = holder.key().identifier().toString();
                if (holder.value().placement() instanceof RandomSpreadStructurePlacement spread) {
                    for (int[] centre : CENTRES) {
                        for (int dx = 0; dx < SIZE; dx++) {
                            for (int dz = 0; dz < SIZE; dz++) {
                                int x = centre[0] + dx, z = centre[1] + dz;
                                ChunkPos potential = spread.getPotentialStructureChunk(seed, x, z);
                                out.append("spread ").append(seed).append(' ').append(id).append(' ').append(x).append(' ').append(z)
                                    .append(' ').append(potential.x()).append(' ').append(potential.z())
                                    .append(' ').append(spread.isStructureChunk(state, x, z)).append('\n');
                            }
                        }
                    }
                } else if (holder.value().placement() instanceof ConcentricRingsStructurePlacement rings) {
                    List<ChunkPos> positions = state.getRingPositionsFor(rings);
                    for (int i = 0; positions != null && i < positions.size(); i++) {
                        out.append("ring ").append(seed).append(' ').append(id).append(' ').append(i)
                            .append(' ').append(positions.get(i).x()).append(' ').append(positions.get(i).z()).append('\n');
                    }
                }
            }
            var realState = ChunkGeneratorStructureState.createForNormal(randomState, seed, new ChunkPos(0, 0), realSource, sets);
            for (var holder : sets.listElements().toList()) {
                if (holder.value().placement() instanceof ConcentricRingsStructurePlacement rings) {
                    List<ChunkPos> positions = realState.getRingPositionsFor(rings);
                    for (int i = 0; positions != null && i < positions.size(); i++) {
                        out.append("realring ").append(seed).append(' ').append(holder.key().identifier()).append(' ').append(i)
                            .append(' ').append(positions.get(i).x()).append(' ').append(positions.get(i).z()).append('\n');
                    }
                }
            }
            Random random = new Random(seed);
            for (var method : AbstractSpreadingStructurePlacement.FrequencyReductionMethod.values()) {
                for (int i = 0; i < 400; i++) {
                    int salt = i % 3 == 0 ? random.nextInt() & Integer.MAX_VALUE : random.nextInt(1000);
                    int x = i % 5 == 0 ? random.nextInt() : random.nextInt(4001) - 2000;
                    int z = i % 7 == 0 ? random.nextInt() : random.nextInt(4001) - 2000;
                    float probability = i % 11 == 0 ? 1.0F / (1 + random.nextInt(300)) : random.nextFloat();
                    out.append("reducer ").append(method.getSerializedName()).append(' ').append(seed).append(' ').append(salt)
                        .append(' ').append(x).append(' ').append(z).append(' ').append(Float.floatToRawIntBits(probability))
                        .append(' ').append(method.shouldGenerate(seed, salt, x, z, probability)).append('\n');
                }
            }
        }
        Random random = new Random(99);
        for (int n = 0; n <= 60; n++) {
            long[] keys = new long[n];
            for (int i = 0; i < n; i++) {
                int pick = random.nextInt(10);
                keys[i] = pick == 0 ? 0L : pick == 1 && i > 0 ? keys[random.nextInt(i)]
                    : ChunkPos.pack(random.nextInt(17) - 8 + 100, random.nextInt(17) - 8 - 3);
            }
            out.append("hashset");
            for (long key : keys) out.append(' ').append(key);
            out.append(" |");
            for (long key : new LongOpenHashSet(keys)) out.append(' ').append(key);
            out.append('\n');
        }
        var context = new StructurePieceSerializationContext(resources, world, null);
        String piece = "{id:\"minecraft:btp\",BB:[I;57,-20,-103,57,-20,-103],O:-1,GD:0}";
        String other = "{id:\"minecraft:btp\",BB:[I;70,64,-90,40,90,-120],O:2,GD:3}";
        String[] starts = {
            "{id:\"INVALID\"}",
            "{id:\"minecraft:buried_treasure\",ChunkX:3,ChunkZ:-7,references:0,Children:[" + piece + "]}",
            "{id:\"buried_treasure\",ChunkX:5L,ChunkZ:-1.5d,references:2b,Children:[" + piece + "," + other + "]}",
            "{id:\"minecraft:stronghold\",ChunkX:1,ChunkZ:2,references:1,Children:[" + other + "]}",
            "{id:\"minecraft:trail_ruins\",ChunkX:1,ChunkZ:2,Children:[]}",
            "{id:\"minecraft:no_such_structure\",ChunkX:1,ChunkZ:2,Children:[" + piece + "]}",
            "{id:\"minecraft:mineshaft\",ChunkX:9,ChunkZ:9,references:0}",
        };
        for (String text : starts) {
            CompoundTag tag = TagParser.parseCompoundFully(text);
            StructureStart start = StructureStart.loadStaticStart(context, tag, 12345L);
            out.append("start ").append(text).append(" | ");
            out.append(start == null ? "null" : start.createTag(context, new ChunkPos(3, -7)).toString()).append(" | ");
            if (start != null && start.isValid()) {
                var box = start.getBoundingBox();
                out.append(box.minX()).append(' ').append(box.minY()).append(' ').append(box.minZ()).append(' ')
                    .append(box.maxX()).append(' ').append(box.maxY()).append(' ').append(box.maxZ());
            } else {
                out.append("none");
            }
            out.append('\n');
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
