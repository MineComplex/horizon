// Writes 26.3's piece-based ("legacy") structure generation for horizon-server's tests
// (crates/horizon-server/src/worldgen/structure/legacy_oracle_tests.rs). Run with the server owner's own
// 26.3 jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafLegacyStructuresOracle.java <out.txt>`; the file is then given to the ignored
// test `legacy_structures_match_java` (HORIZON_LEGACY_STRUCTURES_ORACLE, with HORIZON_VANILLA_JAR,
// HORIZON_SERVER_BLOCKS_JSON, HORIZON_SERVER_BLOCK_STATES and HORIZON_BIOME_REPORTS). Original Horizon code.
//
// For the desert pyramid, jungle temple, swamp hut, buried treasure, both mineshafts, the stronghold, the
// nether fortress and the ocean monument, two seeds and a deterministic walk of chunks (until three valid
// starts or the attempt limit), Structure.generate runs as tryGenerateStructure calls it. Rows are tab
// separated:
//   case <structure> <seed> <cx> <cz> <pieces (0: INVALID)>
//   piece <StructurePiece.createTag, canonical>
//   nbt <StructureStart.createTag, canonical>
//   place <structure> <seed> <cx> <cz> <chunk x> <chunk z> <setBlock calls "x,y,z,state,flags;"> <block
//         entities "x,y,z,type,loot table,seed,spawner entity;" sorted> <entities "type,x,y,z;"> <the
//         structure random's next long> <the level random's next long>
// A place row is the first valid start of each structure and seed placed in the chunks its pieces meet
// (the start's own and up to three more), one after the other with the same pieces, as placeInChunk does
// (pieces meeting the chunk's writable box, then Structure.afterPlace), into the synthetic world of
// LeafTemplateOracle.

package dev.horizonlab.leafserver;

import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.ByteArrayTag;
import net.minecraft.nbt.ByteTag;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.DoubleTag;
import net.minecraft.nbt.FloatTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.IntTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.LongArrayTag;
import net.minecraft.nbt.LongTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.ShortTag;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.PoolElementStructurePiece;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructurePiece;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;
import net.minecraft.world.level.levelgen.structure.pools.JigsawJunction;
import net.minecraft.world.level.levelgen.structure.pools.StructurePoolElement;
import net.minecraft.world.level.levelgen.structure.structures.JigsawStructure;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;

public final class LeafLegacyStructuresOracle {
    static final long[] SEEDS = {12345L, -6066136939236227428L};
    static final int VALID = 3;
    static final java.util.Set<String> NAMES = java.util.Set.of("minecraft:desert_pyramid", "minecraft:jungle_pyramid",
        "minecraft:swamp_hut", "minecraft:buried_treasure", "minecraft:mineshaft", "minecraft:mineshaft_mesa",
        "minecraft:stronghold", "minecraft:fortress", "minecraft:monument");
    static final int ATTEMPTS = 4000;

    static int mix(int h) {
        h ^= h >>> 7;
        h *= 0x2545F491;
        h ^= h >>> 13;
        return h;
    }

    static final String[] DEEP = {"stone", "deepslate", "lava", "gravel", "stone", "water", "andesite"};
    static final String[] TOP = {"dirt", "grass_block", "sand", "dirt"};
    static BlockState[] deep;
    static BlockState[] top;

    static int ground(int x, int z) {
        return 58 + Math.floorMod(mix(x * 31 ^ z * 17 ^ 0x5bd1), 8);
    }

    static BlockState synth(int x, int y, int z) {
        int g = ground(x, z);
        int hash = mix(x * 73856093 ^ y * 19349663 ^ z * 83492791);
        if (y < g - 3) return deep[Math.floorMod(hash, deep.length)];
        if (y <= g) return top[Math.floorMod(hash, top.length)];
        if (y <= 62) return Blocks.WATER.defaultBlockState();
        return Blocks.AIR.defaultBlockState();
    }

    static int height(String type, int x, int z) {
        int g = ground(x, z);
        return (type.startsWith("OCEAN_FLOOR") ? g : Math.max(g, 62)) + 1;
    }

    /** NBT with sorted keys and typed numbers (the Rust test writes the same form). */
    static void canonical(Tag tag, StringBuilder out) {
        switch (tag) {
            case CompoundTag compound -> {
                out.append('{');
                boolean first = true;
                for (String key : new java.util.TreeSet<>(compound.keySet())) {
                    if (!first) out.append(',');
                    first = false;
                    out.append(key).append(':');
                    canonical(compound.get(key), out);
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
            case StringTag(String value) -> out.append('"').append(value).append('"');
            case ByteTag(byte value) -> out.append(value).append('b');
            case ShortTag(short value) -> out.append(value).append('s');
            case IntTag(int value) -> out.append(value);
            case LongTag(long value) -> out.append(value).append('L');
            case FloatTag(float value) -> out.append('f').append(Integer.toHexString(Float.floatToRawIntBits(value)));
            case DoubleTag(double value) -> out.append('d').append(Long.toHexString(Double.doubleToRawLongBits(value)));
            case IntArrayTag array -> {
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
            case ByteArrayTag array -> {
                out.append("[B;");
                byte[] values = array.getAsByteArray();
                for (int i = 0; i < values.length; i++) out.append(i > 0 ? "," : "").append(values[i]);
                out.append(']');
            }
            default -> out.append("?").append(tag);
        }
    }

    static String canonical(Tag tag) {
        StringBuilder out = new StringBuilder();
        canonical(tag, out);
        return out.toString();
    }

    /** The element's short key: type letter and template or feature; a list's children in brackets. */
    static String key(CompoundTag element) {
        String type = element.getStringOr("element_type", "");
        return switch (type) {
            case "minecraft:single_pool_element" -> "S:" + element.getStringOr("location", "");
            case "minecraft:legacy_single_pool_element" -> "L:" + element.getStringOr("location", "");
            case "minecraft:feature_pool_element" -> "F:" + element.getStringOr("feature", "");
            case "minecraft:list_pool_element" -> {
                StringBuilder out = new StringBuilder("[");
                ListTag children = element.getListOrEmpty("elements");
                for (int i = 0; i < children.size(); i++) {
                    if (i > 0) out.append(',');
                    out.append(key(children.getCompoundOrEmpty(i)));
                }
                yield out.append(']').toString();
            }
            default -> "E";
        };
    }

    static String box(BoundingBox b) {
        return b.minX() + "," + b.minY() + "," + b.minZ() + "," + b.maxX() + "," + b.maxY() + "," + b.maxZ();
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

    static RegistryAccess.Frozen all;
    static Object unsafe;
    static Method allocate;

    static net.minecraft.server.level.ServerLevel seedLevel(long seed) throws Exception {
        var level = (net.minecraft.server.level.ServerLevel) allocate.invoke(unsafe, net.minecraft.server.level.ServerLevel.class);
        var server = allocate.invoke(unsafe, net.minecraft.server.dedicated.DedicatedServer.class);
        var settings = net.minecraft.server.MinecraftServer.class.getDeclaredField("worldGenSettings");
        settings.setAccessible(true);
        settings.set(server, new net.minecraft.world.level.levelgen.WorldGenSettings(
            new net.minecraft.world.level.levelgen.WorldOptions(seed, false, false), null));
        var owner = net.minecraft.server.level.ServerLevel.class.getDeclaredField("server");
        owner.setAccessible(true);
        owner.set(level, server);
        return level;
    }

    record SynthLevel(WorldGenLevel level, List<String> sets, Map<BlockPos, BlockEntity> entities, List<String> spawned,
                      net.minecraft.util.RandomSource levelRandom) {
    }

    static SynthLevel newLevel(long seed, long salt) throws Exception {
        var writes = new java.util.HashMap<BlockPos, BlockState>();
        var sets = new ArrayList<String>();
        var entities = new LinkedHashMap<BlockPos, BlockEntity>();
        var spawned = new ArrayList<String>();
        var levelRandom = new net.minecraft.world.level.levelgen.LegacyRandomSource(seed ^ salt);
        var server = seedLevel(seed);
        java.util.function.Function<BlockPos, BlockState> at = pos -> {
            BlockState written = writes.get(pos);
            return written != null ? written : synth(pos.getX(), pos.getY(), pos.getZ());
        };
        WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),
            new Class<?>[] {WorldGenLevel.class}, (proxy, method, values) -> {
                switch (method.getName()) {
                    case "getBlockState" -> {
                        return at.apply(((BlockPos) values[0]).immutable());
                    }
                    case "getFluidState" -> {
                        return at.apply(((BlockPos) values[0]).immutable()).getFluidState();
                    }
                    case "getBlockEntity" -> {
                        if (values.length == 2) return InvocationHandler.invokeDefault(proxy, method, values);
                        BlockPos pos = ((BlockPos) values[0]).immutable();
                        BlockState there = at.apply(pos);
                        if (!there.hasBlockEntity()) return null;
                        // Block entities needing item components (vaults) cannot be made without a
                        // server; only loot seeds are compared, so they are left out.
                        return entities.computeIfAbsent(pos, k -> {
                            try {
                                return ((net.minecraft.world.level.block.EntityBlock) there.getBlock()).newBlockEntity(k, there);
                            } catch (Throwable e) {
                                return null;
                            }
                        });
                    }
                    case "setBlock", "setBlockAndUpdate" -> {
                        BlockPos pos = ((BlockPos) values[0]).immutable();
                        BlockState state = (BlockState) values[1];
                        int flags = values.length > 2 ? (Integer) values[2] : 3;
                        BlockState old = at.apply(pos);
                        if (old.hasBlockEntity() && old.getBlock() != state.getBlock()) entities.remove(pos);
                        writes.put(pos, state);
                        sets.add(pos.getX() + "," + pos.getY() + "," + pos.getZ() + "," + Block.getId(state) + "," + flags);
                        return true;
                    }
                    case "scheduleTick", "addParticle", "playSound", "levelEvent", "gameEvent" -> {
                        return null;
                    }
                    case "getHeight" -> {
                        if (values != null && values.length == 3) {
                            return height(((net.minecraft.world.level.levelgen.Heightmap.Types) values[0]).name(),
                                (Integer) values[1], (Integer) values[2]);
                        }
                        return 384;
                    }
                    case "getMinY" -> {
                        return -64;
                    }
                    case "getMaxY" -> {
                        return 319;
                    }
                    case "getChunk" -> {
                        // StructurePiece.placeBlock marks shape-checked blocks on the chunk; a bare LevelChunk
                        // only logs that this is not supported.
                        return allocate.invoke(unsafe, net.minecraft.world.level.chunk.LevelChunk.class);
                    }
                    case "getBiome" -> {
                        return all.lookupOrThrow(Registries.BIOME).getOrThrow(net.minecraft.world.level.biome.Biomes.PLAINS);
                    }
                    case "getSeaLevel" -> {
                        return 63;
                    }
                    case "getRandom" -> {
                        return levelRandom;
                    }
                    case "addFreshEntity", "addFreshEntityWithPassengers" -> {
                        net.minecraft.world.entity.Entity entity = (net.minecraft.world.entity.Entity) values[0];
                        spawned.add(BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType()) + "," + entity.getX() + "," + entity.getY() + "," + entity.getZ());
                        return true;
                    }
                    case "getSeed" -> {
                        return seed;
                    }
                    case "getLevel" -> {
                        return server;
                    }
                    case "registryAccess" -> {
                        return all;
                    }
                    case "holderLookup" -> {
                        return all.lookupOrThrow((net.minecraft.resources.ResourceKey<? extends Registry<?>>) values[0]);
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
                        return "oracle level";
                    }
                    default -> {
                        if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, values);
                        throw new UnsupportedOperationException(method.toString());
                    }
                }
            });
        return new SynthLevel(level, sets, entities, spawned, levelRandom);
    }

    static boolean hasFeature(CompoundTag element) {
        String type = element.getStringOr("element_type", "");
        if (type.equals("minecraft:feature_pool_element")) return true;
        ListTag children = element.getListOrEmpty("elements");
        for (int i = 0; i < children.size(); i++) {
            if (hasFeature(children.getCompoundOrEmpty(i))) return true;
        }
        return false;
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var access = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        access.setAccessible(true);
        unsafe = access.get(null);
        allocate = Class.forName("sun.misc.Unsafe").getMethod("allocateInstance", Class.class);
        deep = java.util.Arrays.stream(DEEP).map(n -> BuiltInRegistries.BLOCK.getValue(net.minecraft.resources.Identifier.withDefaultNamespace(n)).defaultBlockState()).toArray(BlockState[]::new);
        top = java.util.Arrays.stream(TOP).map(n -> BuiltInRegistries.BLOCK.getValue(net.minecraft.resources.Identifier.withDefaultNamespace(n)).defaultBlockState()).toArray(BlockState[]::new);

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

        Path scratch = Files.createTempDirectory("jigsaw-oracle");
        LevelStorageSource.LevelStorageAccess storage = LevelStorageSource.createDefault(scratch).createAccess("world");
        StructureTemplateManager templates = new StructureTemplateManager(resources, storage, DataFixers.getDataFixer(),
            all.lookupOrThrow(Registries.BLOCK));
        StructurePieceSerializationContext serialization = new StructurePieceSerializationContext(resources, all, templates);
        var ops = all.createSerializationContext(NbtOps.INSTANCE);

        var noises = world.lookupOrThrow(Registries.NOISE);
        var settingsRegistry = world.lookupOrThrow(Registries.NOISE_SETTINGS);
        var preset = world.lookupOrThrow(Registries.WORLD_PRESET).listElements()
            .filter(h -> h.key().identifier().toString().equals("minecraft:normal")).findFirst().orElseThrow().value();
        var dimensions = preset.createWorldDimensions().dimensions();
        var structures = world.lookupOrThrow(Registries.STRUCTURE);

        StringBuilder out = new StringBuilder();
        for (var holder : structures.listElements().toList()) {
            String name = holder.key().identifier().toString();
            if (!NAMES.contains(name)) continue;
            Structure structure = holder.value();
            boolean nether = name.equals("minecraft:fortress");
            String stemKey = nether ? "minecraft:the_nether" : "minecraft:overworld";
            String settingsKey = nether ? "minecraft:nether" : "minecraft:overworld";
            LevelStem stem = dimensions.entrySet().stream()
                .filter(e -> e.getKey().identifier().toString().equals(stemKey)).findFirst().orElseThrow().getValue();
            BiomeSource source = ((NoiseBasedChunkGenerator) stem.generator()).getBiomeSource();
            var settingsHolder = settingsRegistry.listElements()
                .filter(h -> h.key().identifier().toString().equals(settingsKey)).findFirst().orElseThrow();
            NoiseGeneratorSettings settings = settingsHolder.value();
            var generator = new NoiseBasedChunkGenerator(source, settingsHolder);
            int minY = nether ? 0 : -64;
            int height = nether ? 256 : 384;
            LevelHeightAccessor heights = LevelHeightAccessor.create(minY, height);
            for (long seed : SEEDS) {
                RandomState state = RandomState.create(noises, seed, settings);
                long walk = 0x9E3779B97F4A7C15L ^ seed ^ name.hashCode();
                int valid = 0;
                boolean placed = false;
                for (int attempt = 0; attempt < ATTEMPTS && valid < VALID; attempt++) {
                    walk = walk * 6364136223846793005L + 1442695040888963407L;
                    int cx = (int) (walk >> 33) % 1500;
                    int cz = (int) (walk >> 13) % 1500;
                    resetSearch(source);
                    ChunkPos chunk = new ChunkPos(cx, cz);
                    StructureStart start = structure.generate(holder, Level.OVERWORLD, all, generator, source,
                        state.createClimateSampler(net.minecraft.world.level.levelgen.densityfunction.SamplerContext.builder().enableCaches().build()), state, templates, seed, chunk, 0, heights, structure.biomes()::contains);
                    List<StructurePiece> pieces = start.getPieces();
                    out.append("case\t").append(name).append('\t').append(seed).append('\t').append(cx).append('\t').append(cz)
                        .append('\t').append(pieces.size()).append('\n');
                    if (!start.isValid()) continue;
                    valid++;
                    for (StructurePiece piece : pieces) {
                        out.append("piece\t").append(canonical(piece.createTag(serialization))).append('\n');
                    }
                    out.append("nbt\t").append(canonical(start.createTag(serialization, chunk))).append('\n');
                    if (placed) continue;
                    placed = true;
                    List<ChunkPos> chunks = new ArrayList<>(List.of(chunk));
                    for (StructurePiece piece : pieces) {
                        BoundingBox pb = piece.getBoundingBox();
                        ChunkPos other = new ChunkPos(pb.minX() >> 4, pb.minZ() >> 4);
                        if (!chunks.contains(other) && chunks.size() < 4) chunks.add(other);
                    }
                    for (ChunkPos target : chunks) {
                        SynthLevel synth = newLevel(seed, target.pack());
                        WorldgenRandom random = new WorldgenRandom(new XoroshiroRandomSource(seed ^ target.pack()));
                        BoundingBox chunkBox = new BoundingBox(target.getMinBlockX(), minY + 1, target.getMinBlockZ(),
                            target.getMinBlockX() + 15, minY + height - 1, target.getMinBlockZ() + 15);
                        BoundingBox centerBox = pieces.get(0).getBoundingBox();
                        BlockPos center = centerBox.getCenter();
                        BlockPos reference = new BlockPos(center.getX(), centerBox.minY(), center.getZ());
                        for (StructurePiece piece : pieces) {
                            if (!piece.getBoundingBox().intersects(chunkBox)) continue;
                            try {
                                piece.postProcess(synth.level(), null, generator, random, chunkBox, target, reference);
                            } catch (RuntimeException e) {
                                System.err.println("postProcess failed: " + name + " " + e);
                                e.printStackTrace();
                            }
                        }
                        try {
                            structure.afterPlace(synth.level(), null, generator, random, chunkBox, target,
                                new net.minecraft.world.level.levelgen.structure.pieces.PiecesContainer(pieces));
                        } catch (RuntimeException e) {
                            System.err.println("afterPlace failed: " + name + " " + e);
                        }
                        out.append("place\t").append(name).append('\t').append(seed).append('\t').append(cx).append('\t').append(cz)
                            .append('\t').append(target.x()).append('\t').append(target.z()).append('\t')
                            .append(String.join(";", synth.sets())).append('\t');
                        TreeMap<String, String> blockEntities = new TreeMap<>();
                        for (var entry : synth.entities().entrySet()) {
                            BlockEntity entity = entry.getValue();
                            CompoundTag tag = entity.saveWithoutMetadata(all);
                            BlockPos pos = entry.getKey();
                            String spawner = tag.getCompoundOrEmpty("SpawnData").getCompoundOrEmpty("entity").getStringOr("id", "");
                            blockEntities.put(pos.getX() + "," + pos.getY() + "," + pos.getZ(),
                                BuiltInRegistries.BLOCK_ENTITY_TYPE.getKey(entity.getType()) + ","
                                    + tag.getStringOr("LootTable", "") + "," + tag.getLongOr("LootTableSeed", 0L) + "," + spawner);
                        }
                        for (var entry : blockEntities.entrySet()) out.append(entry.getKey()).append(',').append(entry.getValue()).append(';');
                        out.append('\t');
                        for (String spawned : synth.spawned()) out.append(spawned).append(';');
                        out.append('\t').append(random.nextLong()).append('\t').append(synth.levelRandom().nextLong()).append('\n');
                    }
                }
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
