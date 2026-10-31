// Writes whole chunks of 26.3's generation with structures on, for horizon-server's tests
// (crates/horizon-server/src/worldgen/generator/structure_chunks_oracle_tests.rs). Run with the server owner's
// own 26.3 jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafStructureChunksOracle.java <out.txt>`; the file is then given to the ignored test
// `structure_chunks_match_java` (HORIZON_STRUCTURE_CHUNKS_ORACLE, with HORIZON_VANILLA_JAR,
// HORIZON_SERVER_BLOCKS_JSON, HORIZON_SERVER_BLOCK_STATES and HORIZON_BIOME_REPORTS). Original Horizon code,
// built from LeafChunkGenOracle and LeafDecorationOracle.
//
// For the normal preset's overworld and two seeds, a scan of the chunks around the origin finds the first start
// of every structure (createStructures), the sparse ones the scan misses (FAR) are searched through their set's
// potential chunks, and the stronghold nearest the origin is taken from the ring positions.
// Each case decorates the 2 x 2 chunks A whose lowest corner is the start's chunk, running the pyramid's steps as
// ChunkStatusTasks runs them, one chunk at a time on this thread in this order (the Rust side replays it; the
// climate tree's last result is forgotten first):
//   STRUCTURE_STARTS (generate-structures on) for the chunks within 9 of A's ring T (T = A and its neighbours),
//   STRUCTURE_REFERENCES for T, BIOMES for T and its neighbours, TERRAIN for T (buildTerrain's body: the noise
//   with the Beardifier of the chunk's references, surface, carvers, final heightmaps), then FEATURES for A, z then
//   x: addVanillaDecorations' loop over a WorldGenLevel that is a proxy on these chunks (WorldGenRegion needs a
//   ServerLevel): the 3 x 3 chunks writable, its random WorldGenRegion's (worldgen_region_random at the centre),
//   the structures of each step placed as Paper's placeInChunk does (the pieces meeting the writable box picked
//   first), and the features horizon-server has not ported skipped (LeafDecorationOracle's PORTED).
// Like Leaf's chunks (Object2ObjectArrayMap, LongArraySet), the chunks keep their starts and references in
// insertion order. As in the other structure oracles, template pieces place with knownShape (block behaviour's
// updateShape is not ported). Entities are not ported: the fake ServerLevel enables no feature, so EntityType.create
// makes none (and no finalizeSpawn draws from the region's random; were one made, its draws would not come from it).
// Rows (block ids are Block.getId, runs are id*count in section order y, z, x):
//   case <seed> <structure> <start x> <start z> <starts placed in A>
//   sec <cx> <cz> <index> <runs...>            every section of the chunks of T after A's decoration
//   struct <cx> <cz> <structures tag, canonical>   chunks with starts or references (packStructureData)

package dev.horizonlab.leafserver;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.EnumSet;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.function.Function;
import java.util.function.Supplier;
import java.util.stream.Collectors;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.LongArrayTag;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.Identifier;
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
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.FeatureSorter;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.feature.FallenTreeFeature;
import net.minecraft.world.level.levelgen.feature.Feature;
import net.minecraft.world.level.levelgen.feature.TreeFeature;
import net.minecraft.world.level.levelgen.feature.treedecorators.PaleMossDecorator;
import net.minecraft.world.level.levelgen.material.rule.MaterialRule;
import net.minecraft.world.level.levelgen.placement.FeaturePlacer;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.pieces.PiecesContainer;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.placement.ConcentricRingsStructurePlacement;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;

public final class LeafStructureChunksOracle {
    static final long[] SEEDS = {12345L, -6066136939236227428L};
    /** The scan for starts: chunks out to this distance from the origin. */
    static final int SCAN = 96;
    /** Structures searched further through their set's placement when the scan has none: set, structure. */
    static final String[][] FAR = {
        {"minecraft:desert_pyramids", "minecraft:desert_pyramid"},
        {"minecraft:swamp_huts", "minecraft:swamp_hut"},
        {"minecraft:jungle_temples", "minecraft:jungle_pyramid"},
        {"minecraft:ocean_monuments", "minecraft:monument"},
        {"minecraft:woodland_mansions", "minecraft:mansion"},
    };
    static final int MIN_Y = -64;
    static final int HEIGHT = 384;

    /** LeafDecorationOracle.PORTED (the Rust `fully_ported`). */
    static final Set<String> PORTED = Set.of("SimpleBlockFeature", "ScatteredOreFeature", "OreFeature", "DiskFeature",
        "BlockColumnFeature", "TreeFeature", "FallenTreeFeature", "RandomSelectorFeature", "SimpleRandomSelectorFeature",
        "RandomBooleanSelectorFeature", "WeightedRandomSelectorFeature", "SequenceFeature", "OverlayFeature",
        "SpringFeature", "BlockPileFeature", "VinesFeature", "UnderwaterMagmaFeature", "BambooFeature",
        "RandomNeighborSpreadFeature", "VegetationPatchFeature", "WaterloggedVegetationPatchFeature", "SnowAndFreezeFeature",
        "LakeFeature", "BlueIceFeature", "IcebergFeature", "BlockBlobFeature", "HugeBrownMushroomFeature", "HugeRedMushroomFeature",
        "MultifaceGrowthFeature", "GeodeFeature", "MonsterRoomFeature", "LargeDripstoneFeature", "SpeleothemFeature",
        "SpeleothemClusterFeature", "RootSystemFeature", "SculkPatchFeature", "SpikeFeature", "FossilFeature");

    static boolean unported(PlacedFeature placed) {
        return placed.getFeatures().anyMatch(LeafStructureChunksOracle::unported);
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

    static void runs(StringBuilder out, int[] ids, int count) {
        int i = 0;
        while (i < count) {
            int j = i;
            while (j < count && ids[j] == ids[i]) j++;
            out.append(' ').append(ids[i]).append('*').append(j - i);
            i = j;
        }
    }

    static void resetSearch(BiomeSource source) throws Exception {
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

    /** NBT with sorted keys and typed numbers (LeafTemplateStructuresOracle's form). */
    static void canonical(Tag tag, StringBuilder out) {
        switch (tag) {
            case CompoundTag compound -> {
                out.append('{');
                boolean first = true;
                for (String k : new java.util.TreeSet<>(compound.keySet())) {
                    if (!first) out.append(',');
                    first = false;
                    out.append(k).append(':');
                    canonical(compound.get(k), out);
                }
                out.append('}');
            }
            case ListTag list -> {
                out.append('[');
                for (int i = 0; i < list.size(); i++) {
                    if (i > 0) out.append(',');
                    canonical(list.get(i), out);
                }
                out.append(']');
            }
            case net.minecraft.nbt.StringTag(String value) -> out.append('"').append(value).append('"');
            case net.minecraft.nbt.ByteTag(byte value) -> out.append(value).append('b');
            case net.minecraft.nbt.ShortTag(short value) -> out.append(value).append('s');
            case net.minecraft.nbt.IntTag(int value) -> out.append(value);
            case net.minecraft.nbt.LongTag(long value) -> out.append(value).append('L');
            case net.minecraft.nbt.FloatTag(float value) -> out.append('f').append(Integer.toHexString(Float.floatToRawIntBits(value)));
            case net.minecraft.nbt.DoubleTag(double value) -> out.append('d').append(Long.toHexString(Double.doubleToRawLongBits(value)));
            case net.minecraft.nbt.IntArrayTag array -> {
                out.append("[I;");
                int[] values = array.getAsIntArray();
                for (int i = 0; i < values.length; i++) out.append(i > 0 ? "," : "").append(values[i]);
                out.append(']');
            }
            case LongArrayTag array -> {
                out.append("[L;");
                long[] values = array.getAsLongArray();
                for (int i = 0; i < values.length; i++) out.append(i > 0 ? "," : "").append(values[i]);
                out.append(']');
            }
            case net.minecraft.nbt.ByteArrayTag array -> {
                out.append("[B;");
                byte[] values = array.getAsByteArray();
                for (int i = 0; i < values.length; i++) out.append(i > 0 ? "," : "").append(values[i]);
                out.append(']');
            }
            default -> out.append("?").append(tag);
        }
    }

    /** A ServerLevel made without its constructor (Unsafe), with what creating entities asks of it. */
    static final class OracleLevel extends net.minecraft.server.level.ServerLevel {
        OracleLevel() {
            super(null, null, null, null, null, null, false, 0L, List.of(), false);
        }

        @Override
        public int getNextEntityId() {
            return 1;
        }

        /** No feature enabled: EntityType.create makes no entity (entities are not ported). */
        @Override
        public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() {
            return net.minecraft.world.flag.FeatureFlagSet.of();
        }

        @Override
        public int getSeaLevel() {
            return 63;
        }

        @Override
        public RegistryAccess registryAccess() {
            return all;
        }

        @Override
        public net.minecraft.world.Difficulty getDifficulty() {
            return net.minecraft.world.Difficulty.NORMAL;
        }
    }

    static RegistryAccess.Frozen all;
    static StructureTemplateManager templateManager;
    static Object unsafe;
    static Method allocate;

    static net.minecraft.server.level.ServerLevel seedLevel(long seed) throws Exception {
        var level = (net.minecraft.server.level.ServerLevel) allocate.invoke(unsafe, OracleLevel.class);
        var server = allocate.invoke(unsafe, net.minecraft.server.dedicated.DedicatedServer.class);
        var settings = net.minecraft.server.MinecraftServer.class.getDeclaredField("worldGenSettings");
        settings.setAccessible(true);
        settings.set(server, new net.minecraft.world.level.levelgen.WorldGenSettings(new WorldOptions(seed, true, false), null));
        // Fossils read their templates through the server.
        var templateField = net.minecraft.server.MinecraftServer.class.getDeclaredField("structureTemplateManager");
        templateField.setAccessible(true);
        templateField.set(server, templateManager);
        var owner = net.minecraft.server.level.ServerLevel.class.getDeclaredField("server");
        owner.setAccessible(true);
        owner.set(level, server);
        var worldData = net.minecraft.server.MinecraftServer.class.getDeclaredField("worldData");
        worldData.setAccessible(true);
        worldData.set(server, Proxy.newProxyInstance(net.minecraft.world.level.storage.WorldData.class.getClassLoader(),
            new Class<?>[] {net.minecraft.world.level.storage.WorldData.class}, (proxy, method, values) -> {
                if (method.getName().equals("enabledFeatures")) return net.minecraft.world.flag.FeatureFlags.VANILLA_SET;
                if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, values);
                throw new UnsupportedOperationException(method.toString());
            }));
        var levelData = net.minecraft.server.level.ServerLevel.class.getDeclaredField("serverLevelData");
        levelData.setAccessible(true);
        levelData.set(level, Proxy.newProxyInstance(net.minecraft.world.level.storage.ServerLevelData.class.getClassLoader(),
            new Class<?>[] {net.minecraft.world.level.storage.ServerLevelData.class}, (proxy, method, values) -> {
                if (method.getName().equals("getDifficulty")) return net.minecraft.world.Difficulty.NORMAL;
                if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, values);
                throw new UnsupportedOperationException(method.toString());
            }));
        return level;
    }

    static Field startsField;
    static Field referencesField;

    /** A proto chunk keeping its starts and references in insertion order, as Leaf's do. */
    static ProtoChunk newChunk(int x, int z, LevelHeightAccessor access, PalettedContainerFactory factory) throws Exception {
        var chunk = new ProtoChunk(new ChunkPos(x, z), UpgradeData.EMPTY, access, factory, null);
        startsField.set(chunk, new LinkedHashMap<Structure, StructureStart>());
        referencesField.set(chunk, new LinkedHashMap<Structure, it.unimi.dsi.fastutil.longs.LongSet>() {
            @Override
            public it.unimi.dsi.fastutil.longs.LongSet computeIfAbsent(Structure k,
                    Function<? super Structure, ? extends it.unimi.dsi.fastutil.longs.LongSet> f) {
                return super.computeIfAbsent(k, s -> new it.unimi.dsi.fastutil.longs.LongLinkedOpenHashSet());
            }
        });
        return chunk;
    }

    /** Everything a case keeps: the chunks and the block entities pieces and features ask for. */
    static final class World {
        final Map<Long, ProtoChunk> chunks = new HashMap<>();
        final Map<BlockPos, BlockEntity> blockEntities = new HashMap<>();
        net.minecraft.server.level.ServerLevel server;
        long seed;

        ProtoChunk at(BlockPos pos) {
            return chunks.get(key(pos.getX() >> 4, pos.getZ() >> 4));
        }

        BlockState state(BlockPos pos) {
            ProtoChunk chunk = at(pos);
            return chunk == null ? Blocks.VOID_AIR.defaultBlockState() : chunk.getBlockState(pos);
        }
    }

    /** A WorldGenLevel on the case's chunks: `center` null for the whole map (reads only). */
    static WorldGenLevel level(World w, ChunkPos center, BiomeManager biomes, net.minecraft.util.RandomSource regionRandom) {
        net.minecraft.util.RandomSource entityRandom = new XoroshiroRandomSource(0x5eed);
        return (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(), new Class<?>[] {WorldGenLevel.class},
            (proxy, method, values) -> {
                switch (method.getName()) {
                    case "getBlockState" -> {
                        return w.state((BlockPos) values[0]);
                    }
                    case "getFluidState" -> {
                        return w.state((BlockPos) values[0]).getFluidState();
                    }
                    case "isStateAtPosition" -> {
                        @SuppressWarnings("unchecked") var predicate = (java.util.function.Predicate<BlockState>) values[1];
                        return predicate.test(w.state((BlockPos) values[0]));
                    }
                    case "isFluidAtPosition" -> {
                        @SuppressWarnings("unchecked") var predicate = (java.util.function.Predicate<net.minecraft.world.level.material.FluidState>) values[1];
                        return predicate.test(w.state((BlockPos) values[0]).getFluidState());
                    }
                    case "ensureCanWrite" -> {
                        BlockPos at = (BlockPos) values[0];
                        return center != null && Math.abs((at.getX() >> 4) - center.x()) <= 1 && Math.abs((at.getZ() >> 4) - center.z()) <= 1;
                    }
                    case "setBlock" -> {
                        BlockPos at = ((BlockPos) values[0]).immutable();
                        if (center == null || Math.abs((at.getX() >> 4) - center.x()) > 1 || Math.abs((at.getZ() >> 4) - center.z()) > 1) {
                            return false;
                        }
                        ProtoChunk chunk = w.at(at);
                        BlockState state = (BlockState) values[1];
                        BlockState old = chunk.setBlockState(at, state, (Integer) values[2]);
                        if (old != null && old.getBlock() != state.getBlock()) w.blockEntities.remove(at);
                        return true;
                    }
                    case "getHeight" -> {
                        if (values == null || values.length == 0) return HEIGHT;
                        int x = (Integer) values[1], z = (Integer) values[2];
                        return w.chunks.get(key(x >> 4, z >> 4)).getHeight((Heightmap.Types) values[0], x & 15, z & 15) + 1;
                    }
                    case "getMinY" -> {
                        return MIN_Y;
                    }
                    case "getSeaLevel" -> {
                        return 63;
                    }
                    case "getChunk" -> {
                        if (values.length == 1) return w.at((BlockPos) values[0]);
                        return w.chunks.get(key((Integer) values[0], (Integer) values[1]));
                    }
                    case "hasChunk" -> {
                        return w.chunks.containsKey(key((Integer) values[0], (Integer) values[1]));
                    }
                    case "getBiome" -> {
                        return biomes.getBiome((BlockPos) values[0]);
                    }
                    case "getSeed" -> {
                        return w.seed;
                    }
                    case "getRandom" -> {
                        // What a spawned mob's finalizeSpawn draws (entities are not ported) does not come from the
                        // region's random.
                        boolean entity = StackWalker.getInstance().walk(s -> s.anyMatch(f -> f.getMethodName().equals("finalizeSpawn")));
                        return entity ? entityRandom : regionRandom;
                    }
                    case "registryAccess" -> {
                        return all;
                    }
                    case "holderLookup" -> {
                        return all.lookupOrThrow((net.minecraft.resources.ResourceKey<? extends Registry<?>>) values[0]);
                    }
                    case "enabledFeatures" -> {
                        return net.minecraft.world.flag.FeatureFlags.VANILLA_SET;
                    }
                    case "scheduleTick", "playSound", "levelEvent", "gameEvent", "addParticle", "blockUpdated",
                         "updateNeighborsAt", "setCurrentlyGenerating" -> {
                        return null;
                    }
                    case "getRawBrightness", "getBrightness" -> {
                        return 0;
                    }
                    case "getBlockEntity" -> {
                        if (values.length == 2) return InvocationHandler.invokeDefault(proxy, method, values);
                        BlockPos at = ((BlockPos) values[0]).immutable();
                        BlockState there = w.state(at);
                        if (!there.hasBlockEntity()) return null;
                        // Block entities needing item components (vaults) cannot be made without a server.
                        return w.blockEntities.computeIfAbsent(at, k -> {
                            try {
                                return ((net.minecraft.world.level.block.EntityBlock) there.getBlock()).newBlockEntity(k, there);
                            } catch (Throwable e) {
                                return null;
                            }
                        });
                    }
                    case "getCurrentDifficultyAt" -> {
                        return new net.minecraft.world.DifficultyInstance(net.minecraft.world.Difficulty.NORMAL, 0L, 0L, 0.0f);
                    }
                    case "getDifficulty" -> {
                        return net.minecraft.world.Difficulty.NORMAL;
                    }
                    case "addFreshEntity", "addFreshEntityWithPassengers" -> {
                        return true;
                    }
                    case "getLevel" -> {
                        return w.server;
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
                        return "oracle region";
                    }
                    default -> {
                        if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, values);
                        throw new UnsupportedOperationException(method.toString());
                    }
                }
            });
    }

    /** Block behaviour (updateShape) is not ported: template pieces place with knownShape, as in the other oracles. */
    static void knownShape(net.minecraft.world.level.levelgen.structure.TemplateStructurePiece piece) throws Exception {
        Field field = net.minecraft.world.level.levelgen.structure.TemplateStructurePiece.class.getDeclaredField("placeSettings");
        field.setAccessible(true);
        ((net.minecraft.world.level.levelgen.structure.templatesystem.StructurePlaceSettings) field.get(piece)).setKnownShape(true);
    }

    static CompoundTag structuresTag(StructurePieceSerializationContext context, ProtoChunk chunk) {
        var registry = all.lookupOrThrow(Registries.STRUCTURE);
        CompoundTag tag = new CompoundTag();
        CompoundTag starts = new CompoundTag();
        for (var entry : chunk.getAllStarts().entrySet()) {
            starts.put(registry.getKey(entry.getKey()).toString(), entry.getValue().createTag(context, chunk.getPos()));
        }
        tag.put("starts", starts);
        CompoundTag references = new CompoundTag();
        for (var entry : chunk.getAllReferences().entrySet()) {
            if (!entry.getValue().isEmpty()) {
                references.put(registry.getKey(entry.getKey()).toString(), new LongArrayTag(entry.getValue().toLongArray()));
            }
        }
        tag.put("References", references);
        return tag;
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var access0 = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        access0.setAccessible(true);
        unsafe = access0.get(null);
        allocate = Class.forName("sun.misc.Unsafe").getMethod("allocateInstance", Class.class);
        startsField = ChunkAccess.class.getDeclaredField("structureStarts");
        startsField.setAccessible(true);
        referencesField = ChunkAccess.class.getDeclaredField("structureReferences");
        referencesField.setAccessible(true);

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        List<PackResources> packs = new ArrayList<>(repository.openAllSelected());
        MultiPackResourceManager resources = new MultiPackResourceManager(PackType.SERVER_DATA, packs);
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags = TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        staticTags.forEach(Registry.PendingTags::apply);
        var lookups = TagLoader.buildUpdatedLookups(initial.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        all = initial.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
        BuiltInRegistries_init();

        Path scratch = Files.createTempDirectory("structure-chunks-oracle");
        var storage = net.minecraft.world.level.storage.LevelStorageSource.createDefault(scratch).createAccess("world");
        StructureTemplateManager templates = new StructureTemplateManager(resources, storage, net.minecraft.util.datafix.DataFixers.getDataFixer(),
            all.lookupOrThrow(Registries.BLOCK));
        templateManager = templates;
        StructurePieceSerializationContext serialization = new StructurePieceSerializationContext(resources, all, templates);

        var noises = world.lookupOrThrow(Registries.NOISE);
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
        Method createBiomes = ChunkGenerator.class.getDeclaredMethod("doCreateBiomes", Blender.class, RandomState.class, ChunkAccess.class);
        createBiomes.setAccessible(true);
        Field featuresPerStepField = ChunkGenerator.class.getDeclaredField("featuresPerStep");
        featuresPerStepField.setAccessible(true);

        var settingsHolder = world.lookupOrThrow(Registries.NOISE_SETTINGS).listElements()
            .filter(h -> h.key().identifier().toString().equals("minecraft:overworld")).findFirst().orElseThrow();
        NoiseGeneratorSettings settings = settingsHolder.value();
        var preset = world.lookupOrThrow(Registries.WORLD_PRESET).listElements()
            .filter(h -> h.key().identifier().toString().equals("minecraft:normal")).findFirst().orElseThrow().value();
        LevelStem stem = preset.createWorldDimensions().dimensions().entrySet().stream()
            .filter(e -> e.getKey().identifier().toString().equals("minecraft:overworld")).findFirst().orElseThrow().getValue();
        BiomeSource source = ((NoiseBasedChunkGenerator) stem.generator()).getBiomeSource();
        var generator = new NoiseBasedChunkGenerator(source, settingsHolder);
        @SuppressWarnings("unchecked")
        List<FeatureSorter.StepFeatureData> featuresPerStep = ((Supplier<List<FeatureSorter.StepFeatureData>>) featuresPerStepField.get(generator)).get();
        MaterialRule rule = settings.materialRule().value();
        var heights = LevelHeightAccessor.create(MIN_Y, HEIGHT);
        var structureRegistry = all.lookupOrThrow(Registries.STRUCTURE);
        Map<Integer, List<Structure>> structuresByStep = structureRegistry.stream()
            .collect(Collectors.groupingBy(structure -> structure.step().ordinal()));

        StringBuilder out = new StringBuilder();
        for (long seed : SEEDS) {
            RandomState state = RandomState.create(noises, seed, settings);
            ChunkGeneratorStructureState structureState = generator.createState(world.lookupOrThrow(Registries.STRUCTURE_SET), state, seed);
            structureState.ensureStructuresGenerated();
            // The scan: the first start of each structure, by distance from the origin.
            Map<String, ChunkPos> first = new TreeMap<>();
            StructureManager scanManager = new StructureManager(null, new WorldOptions(seed, true, false), null);
            for (int r = 0; r <= SCAN; r++) {
                for (int dz = -r; dz <= r; dz++) {
                    for (int dx = -r; dx <= r; dx++) {
                        if (Math.max(Math.abs(dx), Math.abs(dz)) != r) continue;
                        var chunk = newChunk(dx, dz, heights, factory);
                        generator.createStructures(all, structureState, scanManager, chunk, templates, Level.OVERWORLD);
                        for (var entry : chunk.getAllStarts().entrySet()) {
                            first.putIfAbsent(structureRegistry.getKey(entry.getKey()).toString(), new ChunkPos(dx, dz));
                        }
                    }
                }
            }
            var strongholds = world.lookupOrThrow(Registries.STRUCTURE_SET).getOrThrow(
                net.minecraft.resources.ResourceKey.create(Registries.STRUCTURE_SET, Identifier.withDefaultNamespace("strongholds")));
            List<ChunkPos> rings = structureState.getRingPositionsFor((ConcentricRingsStructurePlacement) strongholds.value().placement());
            rings.stream().min(java.util.Comparator.comparingLong(p -> (long) p.x() * p.x() + (long) p.z() * p.z()))
                .ifPresent(p -> first.putIfAbsent("minecraft:stronghold", p));
            // Sparse structures the scan may miss: their sets' potential chunks, cell by cell outwards.
            for (String[] wanted : FAR) {
                if (first.containsKey(wanted[1])) continue;
                var set = world.lookupOrThrow(Registries.STRUCTURE_SET).getOrThrow(
                    net.minecraft.resources.ResourceKey.create(Registries.STRUCTURE_SET, Identifier.parse(wanted[0])));
                var spread = (net.minecraft.world.level.levelgen.structure.placement.RandomSpreadStructurePlacement) set.value().placement();
                search:
                for (int r = 0; r <= 24; r++) {
                    for (int cz = -r; cz <= r; cz++) {
                        for (int cx = -r; cx <= r; cx++) {
                            if (Math.max(Math.abs(cx), Math.abs(cz)) != r) continue;
                            ChunkPos potential = spread.getPotentialStructureChunk(seed, cx * spread.spacing(), cz * spread.spacing());
                            var chunk = newChunk(potential.x(), potential.z(), heights, factory);
                            generator.createStructures(all, structureState, scanManager, chunk, templates, Level.OVERWORLD);
                            for (var entry : chunk.getAllStarts().entrySet()) {
                                if (structureRegistry.getKey(entry.getKey()).toString().equals(wanted[1])) {
                                    first.put(wanted[1], potential);
                                    break search;
                                }
                            }
                        }
                    }
                }
            }

            for (var target : first.entrySet()) {
                ChunkPos start = target.getValue();
                int x0 = start.x(), z0 = start.z();
                resetSearch(source);
                World w = new World();
                w.seed = seed;
                w.server = seedLevel(seed);
                BiomeManager biomeManager = new BiomeManager((qx, qy, qz) -> w.chunks.get(key(qx >> 2, qz >> 2)).getNoiseBiome(qx, qy, qz),
                    BiomeManager.obfuscateSeed(seed));
                WorldGenLevel global = level(w, null, biomeManager, null);
                StructureManager manager = new StructureManager(global, new WorldOptions(seed, true, false), null);
                // STRUCTURE_STARTS: T (x0 - 1 .. x0 + 2) and 8 more.
                for (int z = z0 - 9; z <= z0 + 10; z++) {
                    for (int x = x0 - 9; x <= x0 + 10; x++) {
                        var chunk = newChunk(x, z, heights, factory);
                        generator.createStructures(all, structureState, manager, chunk, templates, Level.OVERWORLD);
                        chunk.setPersistedStatus(ChunkStatus.STRUCTURE_STARTS);
                        w.chunks.put(key(x, z), chunk);
                    }
                }
                for (int z = z0 - 1; z <= z0 + 2; z++) {
                    for (int x = x0 - 1; x <= x0 + 2; x++) {
                        var chunk = w.chunks.get(key(x, z));
                        generator.createReferences(global, manager, chunk);
                        chunk.setPersistedStatus(ChunkStatus.STRUCTURE_REFERENCES);
                    }
                }
                for (int z = z0 - 2; z <= z0 + 3; z++) {
                    for (int x = x0 - 2; x <= x0 + 3; x++) {
                        var chunk = w.chunks.get(key(x, z));
                        createBiomes.invoke(generator, Blender.empty(), state, chunk);
                        chunk.setPersistedStatus(ChunkStatus.BIOMES);
                    }
                }
                for (int z = z0 - 1; z <= z0 + 2; z++) {
                    for (int x = x0 - 1; x <= x0 + 2; x++) {
                        var chunk = w.chunks.get(key(x, z));
                        Set<Holder<Biome>> possible = new HashSet<>();
                        for (int dz = -1; dz <= 1; dz++) {
                            for (int dx = -1; dx <= 1; dx++) w.chunks.get(key(x + dx, z + dz)).collectBiomesInPalette(possible);
                        }
                        NoiseSettings noiseSettings = settings.noiseSettings().clampToHeightAccessor(chunk.getHeightAccessorForGeneration());
                        try (NoiseChunk noise = (NoiseChunk) createNoiseChunk.invoke(generator, chunk, manager, Blender.empty(), state, noiseSettings)) {
                            fill.invoke(generator, noise, chunk);
                            surface.invoke(generator, chunk, noise, state, biomeManager, possible, rule);
                            carvers.invoke(generator, chunk, Blender.empty(), noise, state, biomeManager, null, rule);
                        }
                        Heightmap.primeHeightmaps(chunk, EnumSet.of(Heightmap.Types.MOTION_BLOCKING, Heightmap.Types.MOTION_BLOCKING_NO_LEAVES,
                            Heightmap.Types.OCEAN_FLOOR, Heightmap.Types.WORLD_SURFACE));
                        chunk.setPersistedStatus(ChunkStatus.TERRAIN);
                    }
                }
                int placedStarts = 0;
                var regionFactory = state.getOrCreateRandomFactory(Identifier.withDefaultNamespace("worldgen_region_random"));
                for (int z = z0; z <= z0 + 1; z++) {
                    for (int x = x0; x <= x0 + 1; x++) {
                        ChunkPos centerPos = new ChunkPos(x, z);
                        ProtoChunk chunk = w.chunks.get(key(x, z));
                        WorldGenLevel level = level(w, centerPos, biomeManager, regionFactory.at(centerPos.getWorldPosition()));
                        BlockPos origin = new BlockPos(x << 4, MIN_Y, z << 4);
                        WorldgenRandom random = new WorldgenRandom(new XoroshiroRandomSource(0L));
                        long decorationSeed = random.setDecorationSeed(seed, origin.getX(), origin.getZ());
                        Set<Holder<Biome>> possible = new HashSet<>();
                        for (int dz = -1; dz <= 1; dz++) {
                            for (int dx = -1; dx <= 1; dx++) {
                                for (var section : w.chunks.get(key(x + dx, z + dz)).getSections()) section.getBiomes().getAll(possible::add);
                            }
                        }
                        possible.retainAll(source.possibleBiomes());
                        FeaturePlacer placer = new FeaturePlacer(level, generator);
                        BoundingBox chunkBox = new BoundingBox(x << 4, MIN_Y + 1, z << 4, (x << 4) + 15, MIN_Y + HEIGHT - 1, (z << 4) + 15);
                        int generationSteps = Math.max(11, featuresPerStep.size());
                        for (int step = 0; step < generationSteps; step++) {
                            int index = 0;
                            for (Structure structure : structuresByStep.getOrDefault(step, Collections.emptyList())) {
                                random.setFeatureSeed(decorationSeed, index, step);
                                for (StructureStart s : manager.startsForStructure(x, z, structure)) {
                                    List<StructurePiece> pieces = s.getPieces();
                                    if (pieces.isEmpty()) continue;
                                    placedStarts++;
                                    BoundingBox centerBox = pieces.get(0).getBoundingBox();
                                    BlockPos c = centerBox.getCenter();
                                    BlockPos reference = new BlockPos(c.getX(), centerBox.minY(), c.getZ());
                                    List<StructurePiece> meeting = pieces.stream().filter(p -> p.getBoundingBox().intersects(chunkBox)).toList();
                                    for (StructurePiece piece : meeting) {
                                        if (piece instanceof net.minecraft.world.level.levelgen.structure.TemplateStructurePiece t) knownShape(t);
                                        piece.postProcess(level, manager, generator, random, chunkBox, centerPos, reference);
                                    }
                                    s.getStructure().afterPlace(level, manager, generator, random, chunkBox, centerPos, new PiecesContainer(pieces));
                                }
                                index++;
                            }
                            if (step >= featuresPerStep.size()) continue;
                            it.unimi.dsi.fastutil.ints.IntSet wanted = new it.unimi.dsi.fastutil.ints.IntArraySet();
                            FeatureSorter.StepFeatureData data = featuresPerStep.get(step);
                            for (Holder<Biome> biome : possible) {
                                List<HolderSet<PlacedFeature>> inBiome = biome.value().getGenerationSettings().features();
                                if (step < inBiome.size()) {
                                    inBiome.get(step).stream().map(Holder::value).forEach(f -> wanted.add(data.indexMapping().applyAsInt(f)));
                                }
                            }
                            int[] indices = wanted.toIntArray();
                            java.util.Arrays.sort(indices);
                            for (int i : indices) {
                                PlacedFeature feature = data.features().get(i);
                                random.setFeatureSeed(decorationSeed, i, step);
                                if (unported(feature)) continue;
                                placer.placeWithBiomeCheck(feature, random, origin);
                            }
                        }
                        chunk.setPersistedStatus(ChunkStatus.FEATURES);
                    }
                }
                out.append("case ").append(seed).append(' ').append(target.getKey()).append(' ').append(x0).append(' ').append(z0)
                    .append(' ').append(placedStarts).append('\n');
                int[] ids = new int[4096];
                for (int z = z0 - 1; z <= z0 + 2; z++) {
                    for (int x = x0 - 1; x <= x0 + 2; x++) {
                        var chunk = w.chunks.get(key(x, z));
                        for (int s = 0; s < HEIGHT / 16; s++) {
                            var section = chunk.getSection(s);
                            for (int y = 0; y < 16; y++) for (int zz = 0; zz < 16; zz++) for (int xx = 0; xx < 16; xx++) {
                                ids[y * 256 + zz * 16 + xx] = Block.getId(section.getBlockState(xx, y, zz));
                            }
                            out.append("sec ").append(x).append(' ').append(z).append(' ').append(s);
                            runs(out, ids, 4096);
                            out.append('\n');
                        }
                    }
                }
                for (int z = z0 - 9; z <= z0 + 10; z++) {
                    for (int x = x0 - 9; x <= x0 + 10; x++) {
                        var chunk = w.chunks.get(key(x, z));
                        if (chunk.getAllStarts().isEmpty() && chunk.getAllReferences().isEmpty()) continue;
                        StringBuilder tag = new StringBuilder();
                        canonical(structuresTag(serialization, chunk), tag);
                        out.append("struct ").append(x).append(' ').append(z).append(' ').append(tag).append('\n');
                    }
                }
                System.err.println("case " + seed + " " + target.getKey() + " " + x0 + " " + z0 + ": " + placedStarts + " starts placed");
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }

    /** Items get their default components when a server loads its resources (mobs equip items). */
    static void BuiltInRegistries_init() {
        net.minecraft.core.registries.BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(all)
            .forEach(net.minecraft.core.component.DataComponentInitializers.PendingComponents::apply);
    }
}
