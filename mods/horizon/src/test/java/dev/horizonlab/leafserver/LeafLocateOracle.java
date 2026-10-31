// Writes 26.3's locating answers for horizon-server's tests (crates/horizon-server/src/worldgen/locate/
// oracle_tests.rs). Run with the server owner's own 26.3 jar and its libraries on the class path (Java's
// single-file source launcher): `java -cp <jar:libs> LeafLocateOracle.java <out.txt>`; the file is then given to
// the ignored test `locate_matches_java` (HORIZON_LOCATE_ORACLE, with HORIZON_VANILLA_JAR,
// HORIZON_SERVER_BLOCKS_JSON, HORIZON_SERVER_BLOCK_STATES and HORIZON_BIOME_REPORTS). Original Horizon code.
//
// ChunkGenerator.findNearestMapStructure needs a ServerLevel (Paper's events, its config, the chunk system's
// synchronous load), which a jar without a server cannot make. The oracle therefore runs the pieces that are
// vanilla's own classes (ChunkGeneratorStructureState with its placements and its ring positions,
// RandomSpreadStructurePlacement.getPotentialStructureChunk, the real StructureCheck over a level with no
// saved chunks, Structure.generate, StructureStart, LocateCommand.showLocateResult, BiomeSource.
// findClosestBiome3d) and writes the search's loops as ChunkGenerator.findNearestMapStructure has them, with
// the chunk load of syncLoadNonFull(STRUCTURE_STARTS) as ChunkGenerator.createStructures does it. The jar
// is vanilla's: Paper's structure seeds (spigot.yml) and its world border check are not in it, so the Rust
// side runs with its defaults. skipExistingChunks (createReference) is false and
// there is no world border. For the overworld, the Nether and the End of the normal world preset, two seeds
// and three origins:
// Rows (tab separated):
//   structure <dimension> <seed> <x> <y> <z> <query> <x y z name | none>   every structure id and structure tag
//   biome <dimension> <seed> <x> <y> <z> <query> <x y z name | none>        a list of biomes and biome tags
//   feedback <kind> <query> <x y z> <found x y z> <found name> <result> <text>   LocateCommand.showLocateResult
//   error <key> <printable> <text>                                          the not-found messages

package dev.horizonlab.leafserver;

import com.mojang.brigadier.StringReader;
import com.mojang.datafixers.util.Pair;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import net.minecraft.SharedConstants;
import net.minecraft.commands.CommandBuildContext;
import net.minecraft.commands.arguments.ResourceOrTagArgument;
import net.minecraft.commands.arguments.ResourceOrTagKeyArgument;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderSet;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.commands.LocateCommand;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.datafix.DataFixers;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.dimension.LevelStem;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureCheck;
import net.minecraft.world.level.levelgen.structure.StructureCheckResult;
import net.minecraft.world.level.levelgen.structure.StructureSet;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.levelgen.structure.placement.ConcentricRingsStructurePlacement;
import net.minecraft.world.level.levelgen.structure.placement.RandomSpreadStructurePlacement;
import net.minecraft.world.level.levelgen.structure.placement.StructurePlacement;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;
import net.minecraft.world.level.storage.LevelStorageSource;

public final class LeafLocateOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    private static final int[][] ORIGINS = {{0, 64, 0}, {-1500, 70, 2300}, {50000, -20, -30000}};
    private static final String[] BIOMES = {
        "minecraft:plains", "minecraft:desert", "minecraft:deep_ocean", "minecraft:mushroom_fields", "minecraft:cherry_grove",
        "minecraft:deep_dark", "minecraft:jagged_peaks", "minecraft:badlands", "minecraft:swamp", "minecraft:nether_wastes",
        "minecraft:soul_sand_valley", "minecraft:warped_forest", "minecraft:small_end_islands", "minecraft:end_highlands",
        "minecraft:the_void",
        "#minecraft:is_ocean", "#minecraft:is_forest", "#minecraft:is_jungle", "#minecraft:is_nether", "#minecraft:is_end",
        "#minecraft:has_structure/stronghold",
    };

    static RegistryAccess.Frozen all;

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

    /** One level's locating state. */
    static final class Locator {
        final ChunkGeneratorStructureState state;
        final NoiseBasedChunkGenerator generator;
        final BiomeSource source;
        final RandomState randomState;
        final StructureTemplateManager templates;
        final long seed;
        final LevelHeightAccessor heights;
        final PalettedContainerFactory factory;
        final StructureCheck check;
        final Map<Long, ChunkAccess> chunks = new HashMap<>();

        Locator(ChunkGeneratorStructureState state, NoiseBasedChunkGenerator generator, RandomState randomState,
                StructureTemplateManager templates, long seed, LevelHeightAccessor heights, PalettedContainerFactory factory) {
            this.state = state;
            this.generator = generator;
            this.source = generator.getBiomeSource();
            this.randomState = randomState;
            this.templates = templates;
            this.seed = seed;
            this.heights = heights;
            this.factory = factory;
            this.check = new StructureCheck(
                (net.minecraft.world.level.chunk.storage.ChunkScanAccess) Proxy.newProxyInstance(
                    LeafLocateOracle.class.getClassLoader(), new Class<?>[] {net.minecraft.world.level.chunk.storage.ChunkScanAccess.class},
                    (proxy, method, args) -> method.getName().equals("scanChunk") ? CompletableFuture.completedFuture(null) : null),
                all, templates, Level.OVERWORLD, generator, randomState, heights, source, seed, DataFixers.getDataFixer());
        }

        /** ChunkGenerator.tryGenerateStructure, without CraftBukkit's event. */
        boolean tryGenerate(StructureSet.StructureSelectionEntry selected, ChunkAccess chunk, ChunkPos pos,
                            Climate.Sampler sampler) {
            Structure structure = selected.structure().value();
            StructureStart previous = chunk.getStartForStructure(structure);
            int references = previous != null ? previous.getReferences() : 0;
            StructureStart start = structure.generate(selected.structure(), Level.OVERWORLD, all, generator, source, sampler,
                randomState, templates, seed, pos, references, heights, structure.biomes()::contains);
            if (start.isValid()) {
                chunk.setStartForStructure(structure, start);
                return true;
            }
            return false;
        }

        /** syncLoadNonFull(STRUCTURE_STARTS) of a chunk not in storage: ChunkGenerator.createStructures. */
        ChunkAccess load(ChunkPos pos) {
            ChunkAccess known = chunks.get(pos.pack());
            if (known != null) return known;
            ProtoChunk chunk = new ProtoChunk(pos, UpgradeData.EMPTY, heights, factory, null);
            Climate.Sampler sampler = randomState.createClimateSampler(
                net.minecraft.world.level.levelgen.densityfunction.SamplerContext.builder().enableCaches().build());
            for (Holder<StructureSet> set : state.possibleStructureSets()) {
                StructurePlacement placement = set.value().placement();
                List<StructureSet.StructureSelectionEntry> structures = set.value().structures();
                boolean existing = false;
                for (StructureSet.StructureSelectionEntry entry : structures) {
                    StructureStart start = chunk.getStartForStructure(entry.structure().value());
                    if (start != null && start.isValid()) existing = true;
                }
                if (existing) continue;
                if (!placement.isStructureChunk(state, pos.x(), pos.z())) continue;
                if (structures.size() == 1) {
                    tryGenerate(structures.get(0), chunk, pos, sampler);
                } else {
                    List<StructureSet.StructureSelectionEntry> options = new ArrayList<>(structures);
                    WorldgenRandom random = new WorldgenRandom(new LegacyRandomSource(0L));
                    random.setLargeFeatureSeed(seed, pos.x(), pos.z());
                    int total = 0;
                    for (StructureSet.StructureSelectionEntry option : options) total += option.weight();
                    while (!options.isEmpty()) {
                        int choice = random.nextInt(total);
                        int index = 0;
                        for (StructureSet.StructureSelectionEntry option : options) {
                            choice -= option.weight();
                            if (choice < 0) break;
                            index++;
                        }
                        StructureSet.StructureSelectionEntry selected = options.get(index);
                        if (tryGenerate(selected, chunk, pos, sampler)) break;
                        options.remove(index);
                        total -= selected.weight();
                    }
                }
            }
            check.onStructureLoad(pos, chunk.getAllStarts());
            chunks.put(pos.pack(), chunk);
            return chunk;
        }

        Pair<BlockPos, Holder<Structure>> generatingAt(List<Holder<Structure>> structures, StructurePlacement config, ChunkPos target) {
            for (Holder<Structure> structure : structures) {
                StructureCheckResult fast = check.checkStart(target, structure.value(), config, false);
                if (fast != StructureCheckResult.START_NOT_PRESENT) {
                    if (fast == StructureCheckResult.START_PRESENT) {
                        return Pair.of(config.getLocatePos(target), structure);
                    }
                    ChunkAccess chunk = load(target);
                    StructureStart start = chunk.getStartForStructure(structure.value());
                    if (start != null && start.isValid()) {
                        return Pair.of(config.getLocatePos(start.getChunkPos()), structure);
                    }
                }
            }
            return null;
        }

        /** findNearestMapStructure(level, wanted, pos, 100, false). */
        Pair<BlockPos, Holder<Structure>> findNearest(List<Holder<Structure>> wanted, BlockPos pos, int maxRadius) {
            Map<StructurePlacement, List<Holder<Structure>>> scans = new LinkedHashMap<>();
            for (Holder<Structure> structure : wanted) {
                for (StructurePlacement placement : state.getPlacementsForStructure(structure)) {
                    List<Holder<Structure>> list = scans.computeIfAbsent(placement, p -> new ArrayList<>());
                    if (!list.contains(structure)) list.add(structure);
                }
            }
            if (scans.isEmpty()) return null;
            Pair<BlockPos, Holder<Structure>> nearest = null;
            double distanceSqr = Double.MAX_VALUE;
            List<Map.Entry<StructurePlacement, List<Holder<Structure>>>> spreads = new ArrayList<>();
            for (Map.Entry<StructurePlacement, List<Holder<Structure>>> entry : scans.entrySet()) {
                if (entry.getKey() instanceof ConcentricRingsStructurePlacement rings) {
                    List<ChunkPos> positions = state.getRingPositionsFor(rings);
                    Pair<BlockPos, Holder<Structure>> closestPos = null;
                    double closest = Double.MAX_VALUE;
                    for (ChunkPos chunkPos : positions) {
                        BlockPos structurePos = new BlockPos(SectionPos.sectionToBlockCoord(chunkPos.x(), 8), 32, SectionPos.sectionToBlockCoord(chunkPos.z(), 8));
                        double distSqr = structurePos.distSqr(pos);
                        if (closestPos == null || distSqr < closest) {
                            Pair<BlockPos, Holder<Structure>> generating = generatingAt(entry.getValue(), rings, chunkPos);
                            if (generating != null) {
                                closestPos = generating;
                                closest = distSqr;
                            }
                        }
                    }
                    if (closestPos != null) {
                        double d = pos.distSqr(closestPos.getFirst());
                        if (d < distanceSqr) {
                            distanceSqr = d;
                            nearest = closestPos;
                        }
                    }
                } else if (entry.getKey() instanceof RandomSpreadStructurePlacement) {
                    spreads.add(entry);
                }
            }
            if (!spreads.isEmpty()) {
                int originX = SectionPos.blockToSectionCoord(pos.getX());
                int originZ = SectionPos.blockToSectionCoord(pos.getZ());
                for (int radius = 0; radius <= maxRadius; radius++) {
                    boolean found = false;
                    for (Map.Entry<StructurePlacement, List<Holder<Structure>>> entry : spreads) {
                        RandomSpreadStructurePlacement config = (RandomSpreadStructurePlacement) entry.getKey();
                        Pair<BlockPos, Holder<Structure>> result = null;
                        int spacing = config.spacing();
                        search:
                        for (int x = -radius; x <= radius; x++) {
                            boolean xEdge = x == -radius || x == radius;
                            for (int z = -radius; z <= radius; z += xEdge ? 1 : radius * 2) {
                                ChunkPos target = config.getPotentialStructureChunk(state.getLevelSeed(), originX + spacing * x, originZ + spacing * z);
                                result = generatingAt(entry.getValue(), config, target);
                                if (result != null) break search;
                            }
                        }
                        if (result != null) {
                            found = true;
                            double d = pos.distSqr(result.getFirst());
                            if (d < distanceSqr) {
                                distanceSqr = d;
                                nearest = result;
                            }
                        }
                    }
                    if (found) return nearest;
                }
            }
            return nearest;
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
        staticTags.forEach(Registry.PendingTags::apply);
        var lookups = TagLoader.buildUpdatedLookups(initial.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        all = initial.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();

        Path scratch = Files.createTempDirectory("locate-oracle");
        LevelStorageSource.LevelStorageAccess storage = LevelStorageSource.createDefault(scratch).createAccess("world");
        StructureTemplateManager templates = new StructureTemplateManager(resources, storage, DataFixers.getDataFixer(),
            all.lookupOrThrow(Registries.BLOCK));
        PalettedContainerFactory factory = PalettedContainerFactory.create(world);

        var noises = world.lookupOrThrow(Registries.NOISE);
        var settingsRegistry = world.lookupOrThrow(Registries.NOISE_SETTINGS);
        var preset = world.lookupOrThrow(Registries.WORLD_PRESET).listElements()
            .filter(h -> h.key().identifier().toString().equals("minecraft:normal")).findFirst().orElseThrow().value();
        var dimensions = preset.createWorldDimensions().dimensions();
        var structures = world.lookupOrThrow(Registries.STRUCTURE);
        var sets = world.lookupOrThrow(Registries.STRUCTURE_SET);
        var biomes = world.lookupOrThrow(Registries.BIOME);

        List<String> structureQueries = new ArrayList<>();
        structures.listElements().forEach(h -> structureQueries.add(h.key().identifier().toString()));
        structures.listTagIds().forEach(t -> structureQueries.add("#" + t.location()));

        StringBuilder out = new StringBuilder();
        String[][] stems = {
            {"minecraft:overworld", "minecraft:overworld", "-64", "384"},
            {"minecraft:the_nether", "minecraft:nether", "0", "256"},
            {"minecraft:the_end", "minecraft:end", "0", "256"},
        };
        for (String[] row : stems) {
            LevelStem stem = dimensions.entrySet().stream()
                .filter(e -> e.getKey().identifier().toString().equals(row[0])).findFirst().orElseThrow().getValue();
            BiomeSource source = ((NoiseBasedChunkGenerator) stem.generator()).getBiomeSource();
            var settingsHolder = settingsRegistry.listElements()
                .filter(h -> h.key().identifier().toString().equals(row[1])).findFirst().orElseThrow();
            NoiseGeneratorSettings settings = settingsHolder.value();
            int minY = Integer.parseInt(row[2]), height = Integer.parseInt(row[3]);
            LevelHeightAccessor heights = LevelHeightAccessor.create(minY, height);
            var generator = new NoiseBasedChunkGenerator(source, settingsHolder);
            for (long seed : SEEDS) {
                RandomState randomState = RandomState.create(noises, seed, settings);
                var state = ChunkGeneratorStructureState.createForNormal(randomState, seed, new ChunkPos(0, 0), source, sets);
                Locator locator = new Locator(state, generator, randomState, templates, seed, heights, factory);
                for (int[] origin : ORIGINS) {
                    BlockPos pos = new BlockPos(origin[0], origin[1], origin[2]);
                    String prefix = row[0] + "\t" + seed + "\t" + origin[0] + "\t" + origin[1] + "\t" + origin[2] + "\t";
                    for (String query : structureQueries) {
                        HolderSet<Structure> target = query.startsWith("#")
                            ? structures.get(TagKey.create(Registries.STRUCTURE, Identifier.parse(query.substring(1)))).orElseThrow()
                            : HolderSet.direct(structures.get(ResourceKey.create(Registries.STRUCTURE, Identifier.parse(query))).orElseThrow());
                        resetSearch(source);
                        var found = locator.findNearest(target.stream().toList(), pos, 100);
                        out.append("structure\t").append(prefix).append(query).append('\t');
                        if (found == null) out.append("none");
                        else out.append(found.getFirst().getX()).append(' ').append(found.getFirst().getY()).append(' ')
                            .append(found.getFirst().getZ()).append(' ').append(found.getSecond().getRegisteredName());
                        out.append('\n');
                    }
                    for (String query : BIOMES) {
                        java.util.function.Predicate<Holder<Biome>> test = query.startsWith("#")
                            ? h -> h.is(TagKey.create(Registries.BIOME, Identifier.parse(query.substring(1))))
                            : h -> h.is(Identifier.parse(query));
                        LevelHeightAccessor reader = heights;
                        var level = (net.minecraft.world.level.LevelReader) Proxy.newProxyInstance(
                            LeafLocateOracle.class.getClassLoader(), new Class<?>[] {net.minecraft.world.level.LevelReader.class},
                            (proxy, method, a) -> switch (method.getName()) {
                                case "getMinY" -> reader.getMinY();
                                case "getHeight" -> reader.getHeight();
                                case "getMaxY" -> reader.getMaxY();
                                default -> throw new UnsupportedOperationException(method.toString());
                            });
                        resetSearch(source);
                        var found = source.findClosestBiome3d(pos, 6400, 32, 64, test, randomState, level);
                        out.append("biome\t").append(prefix).append(query).append('\t');
                        if (found == null) out.append("none");
                        else out.append(found.getFirst().getX()).append(' ').append(found.getFirst().getY()).append(' ')
                            .append(found.getFirst().getZ()).append(' ').append(found.getSecond().getRegisteredName());
                        out.append('\n');
                    }
                    System.err.println("done " + row[0] + " " + seed + " " + java.util.Arrays.toString(origin));
                }
            }
        }

        // The command's feedback and errors, from LocateCommand itself.
        CommandBuildContext context = CommandBuildContext.simple(all, FeatureFlags.VANILLA_SET);
        Object unsafe = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        ((Field) unsafe).setAccessible(true);
        unsafe = ((Field) unsafe).get(null);
        Method allocate = Class.forName("sun.misc.Unsafe").getMethod("allocateInstance", Class.class);
        List<Component> captured = new ArrayList<>();
        var sourceStack = (net.minecraft.commands.CommandSourceStack) allocate.invoke(unsafe, net.minecraft.commands.CommandSourceStack.class);
        Field sourceField = net.minecraft.commands.CommandSourceStack.class.getDeclaredField("source");
        sourceField.setAccessible(true);
        sourceField.set(sourceStack, Proxy.newProxyInstance(LeafLocateOracle.class.getClassLoader(),
            new Class<?>[] {net.minecraft.commands.CommandSource.class}, (proxy, method, a) -> switch (method.getName()) {
                case "sendSystemMessage" -> { captured.add((Component) a[0]); yield null; }
                case "acceptsSuccess" -> true;
                case "acceptsFailure", "shouldInformAdmins" -> false;
                default -> throw new UnsupportedOperationException(method.toString());
            }));
        var poiTypes = all.lookupOrThrow(Registries.POINT_OF_INTEREST_TYPE);
        Object[][] cases = {
            {"structure", "minecraft:village_plains", new int[] {10, 64, -20}, new int[] {1234, 0, -567}, "minecraft:village_plains"},
            {"structure", "#minecraft:village", new int[] {-5, 90, 5}, new int[] {-3000, 0, 1500}, "minecraft:village_desert"},
            {"structure", "#minecraft:on_ocean_explorer_maps", new int[] {0, 64, 0}, new int[] {0, 0, 0}, "minecraft:monument"},
            {"biome", "minecraft:plains", new int[] {10, 64, -20}, new int[] {1234, 70, -567}, "minecraft:plains"},
            {"biome", "#minecraft:is_forest", new int[] {-5, 90, 5}, new int[] {-3008, -60, 1504}, "minecraft:birch_forest"},
            {"biome", "minecraft:deep_dark", new int[] {100, -40, 100}, new int[] {100, 40, 100}, "minecraft:deep_dark"},
            {"poi", "minecraft:nether_portal", new int[] {7, 70, 7}, new int[] {20, 64, -9}, "minecraft:nether_portal"},
            {"poi", "#minecraft:village", new int[] {0, 70, 0}, new int[] {-3, 65, 4}, "minecraft:farmer"},
        };
        for (Object[] c : cases) {
            String kind = (String) c[0], query = (String) c[1];
            int[] o = (int[]) c[2], f = (int[]) c[3];
            BlockPos sourcePos = new BlockPos(o[0], o[1], o[2]);
            BlockPos foundPos = new BlockPos(f[0], f[1], f[2]);
            captured.clear();
            int result;
            if (kind.equals("structure")) {
                var arg = ResourceOrTagKeyArgument.resourceOrTagKey(Registries.STRUCTURE).parse(new StringReader(query));
                Holder<Structure> holder = structures.getOrThrow(ResourceKey.create(Registries.STRUCTURE, Identifier.parse((String) c[4])));
                result = LocateCommand.showLocateResult(sourceStack, arg, sourcePos, Pair.of(foundPos, holder),
                    "commands.locate.structure.success", false, Duration.ZERO);
            } else if (kind.equals("biome")) {
                var arg = ResourceOrTagArgument.resourceOrTag(context, Registries.BIOME).parse(new StringReader(query));
                Holder<Biome> holder = biomes.getOrThrow(ResourceKey.create(Registries.BIOME, Identifier.parse((String) c[4])));
                result = LocateCommand.showLocateResult(sourceStack, arg, sourcePos, Pair.of(foundPos, holder),
                    "commands.locate.biome.success", true, Duration.ZERO);
            } else {
                var arg = ResourceOrTagArgument.resourceOrTag(context, Registries.POINT_OF_INTEREST_TYPE).parse(new StringReader(query));
                var holder = poiTypes.getOrThrow(ResourceKey.create(Registries.POINT_OF_INTEREST_TYPE, Identifier.parse((String) c[4])));
                result = LocateCommand.showLocateResult(sourceStack, arg, sourcePos, Pair.of(foundPos, holder),
                    "commands.locate.poi.success", false, Duration.ZERO);
            }
            out.append("feedback\t").append(kind).append('\t').append(query).append('\t').append(o[0]).append(' ').append(o[1]).append(' ')
                .append(o[2]).append('\t').append(f[0]).append(' ').append(f[1]).append(' ').append(f[2]).append('\t').append(c[4])
                .append('\t').append(result).append('\t').append(captured.get(0).getString()).append('\n');
        }
        for (String[] error : new String[][] {
            {"commands.locate.structure.not_found", "minecraft:village_plains"},
            {"commands.locate.structure.not_found", "#minecraft:village"},
            {"commands.locate.structure.invalid", "minecraft:nope"},
            {"commands.locate.biome.not_found", "minecraft:plains"},
            {"commands.locate.biome.not_found", "#minecraft:is_forest"},
            {"commands.locate.poi.not_found", "minecraft:farmer"},
            {"commands.locate.structure.not_found", "100%s \\ 'x'"},
        }) {
            out.append("error\t").append(error[0]).append('\t').append(error[1]).append('\t')
                .append(Component.translatableEscape(error[0], error[1]).getString()).append('\n');
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
