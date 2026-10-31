// Writes 26.3's placement results for horizon-server's world generation tests
// (crates/horizon-server/src/worldgen/placement/oracle_tests.rs). Run with the server owner's own 26.3
// jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafPlacementOracle.java <out.txt>`; the file is then given to the ignored test
// `placement_matches_java` (HORIZON_PLACEMENT_ORACLE, with HORIZON_VANILLA_JAR, HORIZON_SERVER_BLOCKS_JSON and
// HORIZON_SERVER_BLOCK_STATES). Original Horizon code. Rows are tab separated:
//   int     <random> <seed> <samples|error>              <IntProvider json>     (or "decode error")
//   height  <profile> <random> <seed> <samples|error>    <HeightProvider json>
//   set     <name> <biome names>                          the biomes of a feature sorter input, in order
//   sort    <set> <step> <placed feature names>          FeatureSorter.buildFeaturesPerStep, or "cycle"
//   place   <profile> <random> <seed> <ox,oy,oz> <biome|vary> <feature name | json modifier list>
//           <positions "x y z;..."|error> <placedAny> <next long>
// A placed feature's modifier chain is run as FeaturePlacer runs it (its loop is copied here), with the
// feature itself replaced by one draw of random.nextInt(3), over a synthetic world given by formulas that
// the Rust test repeats. would_survive predicates are replaced by true (their answer is block behaviour,
// not placement); the biome filter still checks the original placed feature. A world has one of three
// profiles: A level -64..320 and the overworld generator, B the same level and the nether generator, C level
// 0..256 and the overworld generator.

package dev.horizonlab.leafserver;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.stream.Stream;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
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
import net.minecraft.util.valueproviders.IntProvider;
import net.minecraft.util.valueproviders.IntProviders;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeSource;
import net.minecraft.world.level.biome.FeatureSorter;
import net.minecraft.world.level.biome.FixedBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists;
import net.minecraft.world.level.biome.TheEndBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.WorldGenerationContext;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.heightproviders.HeightProvider;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;
import net.minecraft.world.level.levelgen.placement.PlacementContext;
import net.minecraft.world.level.levelgen.placement.PlacementModifier;

public final class LeafPlacementOracle {
    private static final long[] SEEDS = {12345L, -6066136939236227428L};
    private static final int[][] ORIGINS = {{0, 0}, {16, -32}, {-48, 64}, {1024, -2048}};
    /** Level minimum y, level height, generator key (true: nether) per profile. */
    private static final int[][] PROFILES = {{-64, 384, 0}, {-64, 384, 1}, {0, 256, 0}};
    private static final String[] PROFILE_NAMES = {"A", "B", "C"};
    private static Block[] PALETTE;
    private static final String[] INT_PROVIDERS = {
        "5",
        "{\"type\":\"minecraft:constant\",\"value\":-3}",
        "{\"type\":\"minecraft:uniform\",\"min_inclusive\":-4,\"max_inclusive\":9}",
        "{\"type\":\"minecraft:biased_to_bottom\",\"min_inclusive\":0,\"max_inclusive\":10}",
        "{\"type\":\"minecraft:very_biased_to_bottom\",\"min_inclusive\":2,\"max_inclusive\":20}",
        "{\"type\":\"minecraft:trapezoid\",\"min\":-8,\"max\":8,\"plateau\":0}",
        "{\"type\":\"minecraft:trapezoid\",\"min\":2,\"max\":30,\"plateau\":4}",
        "{\"type\":\"minecraft:trapezoid\",\"min\":0,\"max\":10,\"plateau\":10}",
        "{\"type\":\"minecraft:clamped\",\"source\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":-10,\"max_inclusive\":30},\"min_inclusive\":0,\"max_inclusive\":12}",
        "{\"type\":\"minecraft:clamped_normal\",\"mean\":4.0,\"deviation\":3.0,\"min_inclusive\":0,\"max_inclusive\":10}",
        "{\"type\":\"minecraft:weighted_list\",\"distribution\":[{\"data\":1,\"weight\":3},{\"data\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":5,\"max_inclusive\":8},\"weight\":2},{\"data\":0,\"weight\":0},{\"data\":-7,\"weight\":1}]}",
        "{\"type\":\"minecraft:uniform\",\"min_inclusive\":5,\"max_inclusive\":1}",
        "{\"type\":\"minecraft:weighted_list\",\"distribution\":[]}",
    };
    private static final String[] HEIGHT_PROVIDERS = {
        "{\"absolute\":10}",
        "{\"above_bottom\":8}",
        "{\"below_top\":5}",
        "{\"relative_to_sea_level\":-12}",
        "{\"type\":\"minecraft:constant\",\"value\":{\"absolute\":-3}}",
        "{\"type\":\"minecraft:uniform\",\"min_inclusive\":{\"above_bottom\":0},\"max_inclusive\":{\"absolute\":50}}",
        "{\"type\":\"minecraft:uniform\",\"min_inclusive\":{\"absolute\":40},\"max_inclusive\":{\"absolute\":10}}",
        "{\"type\":\"minecraft:biased_to_bottom\",\"min_inclusive\":{\"absolute\":-20},\"max_inclusive\":{\"below_top\":10},\"inner\":3}",
        "{\"type\":\"minecraft:biased_to_bottom\",\"min_inclusive\":{\"absolute\":10},\"max_inclusive\":{\"absolute\":11},\"inner\":5}",
        "{\"type\":\"minecraft:very_biased_to_bottom\",\"min_inclusive\":{\"above_bottom\":4},\"max_inclusive\":{\"absolute\":120},\"inner\":2}",
        "{\"type\":\"minecraft:very_biased_to_bottom\",\"min_inclusive\":{\"absolute\":4},\"max_inclusive\":{\"absolute\":5}}",
        "{\"type\":\"minecraft:trapezoid\",\"min_inclusive\":{\"absolute\":-30},\"max_inclusive\":{\"relative_to_sea_level\":60},\"plateau\":6}",
        "{\"type\":\"minecraft:trapezoid\",\"min_inclusive\":{\"above_bottom\":0},\"max_inclusive\":{\"below_top\":0}}",
        "{\"type\":\"minecraft:trapezoid\",\"min_inclusive\":{\"absolute\":50},\"max_inclusive\":{\"absolute\":10}}",
        "{\"type\":\"minecraft:weighted_list\",\"distribution\":[{\"data\":{\"absolute\":3},\"weight\":1},{\"data\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":{\"absolute\":0},\"max_inclusive\":{\"absolute\":9}},\"weight\":4}]}",
        "{\"type\":\"minecraft:uniform\",\"min_inclusive\":{\"absolute\":2},\"max_inclusive\":{\"above_bottom\":3000}}",
    };
    private static final String IN_SQUARE = "{\"type\":\"minecraft:in_square\"}";
    private static final String ANYWHERE = "{\"type\":\"minecraft:height_range\",\"height\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":{\"absolute\":-20},\"max_inclusive\":{\"absolute\":150}}}";
    private static final List<String> CUSTOM = new ArrayList<>();

    private static void custom(String... modifiers) {
        CUSTOM.add("[" + String.join(",", modifiers) + "]");
    }

    private static void predicate(String predicate) {
        custom(IN_SQUARE, ANYWHERE, "{\"type\":\"minecraft:block_predicate_filter\",\"predicate\":" + predicate + "}");
    }

    private static void scan(String direction, String target, String allowed, int steps) {
        custom(IN_SQUARE, ANYWHERE, "{\"type\":\"minecraft:environment_scan\",\"direction_of_search\":\"" + direction
            + "\",\"target_condition\":" + target + (allowed == null ? "" : ",\"allowed_search_condition\":" + allowed)
            + ",\"max_steps\":" + steps + "}");
    }

    static {
        custom(IN_SQUARE);
        custom("{\"type\":\"minecraft:count\",\"count\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":0,\"max_inclusive\":5}}", IN_SQUARE,
            "{\"type\":\"minecraft:heightmap\",\"heightmap\":\"WORLD_SURFACE_WG\"}");
        for (String heightmap : new String[] {"WORLD_SURFACE_WG", "WORLD_SURFACE", "OCEAN_FLOOR_WG", "OCEAN_FLOOR", "MOTION_BLOCKING", "MOTION_BLOCKING_NO_LEAVES"}) {
            custom(IN_SQUARE, "{\"type\":\"minecraft:heightmap\",\"heightmap\":\"" + heightmap + "\"}");
        }
        for (String edges : new String[] {"true", "false"}) {
            for (String interior : new String[] {"true", "false"}) {
                custom("{\"type\":\"minecraft:cuboid\",\"xz_size\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":1,\"max_inclusive\":3},\"y_size\":2,\"include_edges\":"
                    + edges + ",\"include_interior\":" + interior + "}");
            }
        }
        custom("{\"type\":\"minecraft:cuboid\",\"xz_size\":2,\"y_size\":1}");
        custom(IN_SQUARE, "{\"type\":\"minecraft:random_chance\",\"chance\":0.3}");
        custom(IN_SQUARE, "{\"type\":\"minecraft:rarity_filter\",\"chance\":3}");
        custom("{\"type\":\"minecraft:count\",\"count\":8}", "{\"type\":\"minecraft:randomly_selected\",\"placements\":[" + IN_SQUARE
            + ",{\"type\":\"minecraft:offset\",\"x\":3,\"y\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":-2,\"max_inclusive\":2},\"z\":-1},{\"type\":\"minecraft:rarity_filter\",\"chance\":2}]}");
        custom("{\"type\":\"minecraft:count\",\"count\":3}", "{\"type\":\"minecraft:offset\",\"x\":{\"type\":\"minecraft:trapezoid\",\"min\":-6,\"max\":6,\"plateau\":0},\"y\":0,\"z\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":-16,\"max_inclusive\":16}}");
        custom("{\"type\":\"minecraft:fixed_placement\",\"positions\":[[0,70,0],[3,64,5],[16,70,0],[-1,10,-1],[1030,5,-2040],[-40,2,70]]}");
        custom(IN_SQUARE, ANYWHERE, "{\"type\":\"minecraft:surface_relative_threshold_filter\",\"heightmap\":\"WORLD_SURFACE\",\"min_inclusive\":-10,\"max_inclusive\":10}");
        custom(IN_SQUARE, ANYWHERE, "{\"type\":\"minecraft:surface_relative_threshold_filter\",\"heightmap\":\"OCEAN_FLOOR\",\"min_inclusive\":-30}");
        custom(IN_SQUARE, ANYWHERE, "{\"type\":\"minecraft:surface_relative_threshold_filter\",\"heightmap\":\"MOTION_BLOCKING\",\"max_inclusive\":2}");
        for (int depth : new int[] {-1, 0, 3}) {
            custom(IN_SQUARE, "{\"type\":\"minecraft:surface_water_depth_filter\",\"max_water_depth\":" + depth + "}");
        }
        custom("{\"type\":\"minecraft:count\",\"count\":6}", IN_SQUARE,
            "{\"type\":\"minecraft:noise_based_count\",\"noise_to_count_ratio\":10,\"noise_factor\":80.0,\"noise_offset\":0.3}");
        custom("{\"type\":\"minecraft:count\",\"count\":6}", IN_SQUARE,
            "{\"type\":\"minecraft:noise_threshold_count\",\"noise_level\":-0.2,\"below_noise\":2,\"above_noise\":7}");
        custom("{\"type\":\"minecraft:count_on_every_layer\",\"count\":3}");
        custom(IN_SQUARE, "{\"type\":\"minecraft:count_on_every_layer\",\"count\":{\"type\":\"minecraft:uniform\",\"min_inclusive\":1,\"max_inclusive\":4}}");
        for (String height : HEIGHT_PROVIDERS) {
            custom("{\"type\":\"minecraft:height_range\",\"height\":" + height + "}");
        }
        custom("{\"type\":\"minecraft:biome\"}");
        // Predicates, as the filter and as the scan's target and allowed conditions.
        String air = "{\"type\":\"minecraft:matching_blocks\",\"blocks\":[\"minecraft:air\",\"minecraft:cave_air\",\"minecraft:void_air\"]}";
        predicate("{\"type\":\"minecraft:true\"}");
        predicate("{\"type\":\"minecraft:solid\"}");
        predicate("{\"type\":\"minecraft:solid\",\"offset\":[0,-1,0]}");
        predicate("{\"type\":\"minecraft:replaceable\",\"offset\":[1,0,-3]}");
        predicate("{\"type\":\"minecraft:unobstructed\",\"offset\":[0,2,0]}");
        predicate("{\"type\":\"minecraft:inside_world_bounds\",\"offset\":[0,-16,0]}");
        predicate("{\"type\":\"minecraft:has_sturdy_face\",\"direction\":\"up\",\"offset\":[0,-1,0]}");
        predicate("{\"type\":\"minecraft:has_sturdy_face\",\"direction\":\"north\"}");
        predicate(air);
        predicate("{\"type\":\"minecraft:matching_blocks\",\"blocks\":\"minecraft:sand\",\"offset\":[0,-1,0]}");
        predicate("{\"type\":\"minecraft:matching_block_tag\",\"tag\":\"minecraft:dirt\",\"offset\":[0,-1,0]}");
        predicate("{\"type\":\"minecraft:matching_block_tag\",\"tag\":\"minecraft:leaves\"}");
        predicate("{\"type\":\"minecraft:matching_block_tag\",\"tag\":\"minecraft:no_such_tag\"}");
        predicate("{\"type\":\"minecraft:matching_blocks\",\"blocks\":\"#minecraft:base_stone_overworld\"}");
        predicate("{\"type\":\"minecraft:matching_fluids\",\"fluids\":\"minecraft:water\"}");
        predicate("{\"type\":\"minecraft:matching_fluids\",\"fluids\":[\"minecraft:lava\",\"minecraft:flowing_lava\"],\"offset\":[0,1,0]}");
        predicate("{\"type\":\"minecraft:matching_fluids\",\"fluids\":\"#minecraft:water\"}");
        predicate("{\"type\":\"minecraft:matching_biomes\",\"biomes\":[\"minecraft:plains\",\"minecraft:desert\",\"minecraft:badlands\"]}");
        predicate("{\"type\":\"minecraft:not\",\"predicate\":" + air + "}");
        predicate("{\"type\":\"minecraft:all_of\",\"predicates\":[" + air + ",{\"type\":\"minecraft:solid\",\"offset\":[0,-1,0]}]}");
        predicate("{\"type\":\"minecraft:any_of\",\"predicates\":[{\"type\":\"minecraft:solid\",\"offset\":[0,1,0]},{\"type\":\"minecraft:matching_fluids\",\"fluids\":\"minecraft:water\"}]}");
        predicate("{\"type\":\"minecraft:all_of\",\"predicates\":[]}");
        predicate("{\"type\":\"minecraft:any_of\",\"predicates\":[]}");
        predicate("{\"type\":\"minecraft:volume_match\",\"min\":[-1,0,-1],\"max\":[1,1,1],\"match\":" + air + "}");
        scan("down", "{\"type\":\"minecraft:solid\"}", air, 12);
        scan("up", "{\"type\":\"minecraft:solid\"}", air, 12);
        scan("down", "{\"type\":\"minecraft:matching_blocks\",\"blocks\":\"minecraft:grass_block\"}", null, 32);
        scan("up", "{\"type\":\"minecraft:matching_fluids\",\"fluids\":\"minecraft:lava\"}", "{\"type\":\"minecraft:not\",\"predicate\":{\"type\":\"minecraft:solid\"}}", 5);
        scan("down", "{\"type\":\"minecraft:true\"}", null, 1);
        // Decode errors.
        custom("{\"type\":\"minecraft:count\",\"count\":5000}");
        custom("{\"type\":\"minecraft:rarity_filter\",\"chance\":0}");
        custom("{\"type\":\"minecraft:no_such_modifier\"}");
        custom("{\"type\":\"minecraft:offset\",\"x\":17,\"y\":0,\"z\":0}");
        custom("{\"type\":\"minecraft:environment_scan\",\"direction_of_search\":\"north\",\"target_condition\":{\"type\":\"minecraft:true\"},\"max_steps\":3}");
        custom("{\"type\":\"minecraft:environment_scan\",\"direction_of_search\":\"up\",\"target_condition\":{\"type\":\"minecraft:true\"},\"max_steps\":33}");
        custom("{\"type\":\"minecraft:random_chance\",\"chance\":1.5}");
        custom("{\"type\":\"minecraft:randomly_selected\",\"placements\":[]}");
        custom("{\"type\":\"minecraft:heightmap\",\"heightmap\":\"NO_SUCH\"}");
        predicate("{\"type\":\"minecraft:solid\",\"offset\":[0,17,0]}");
        predicate("{\"type\":\"minecraft:volume_match\",\"min\":[1,0,0],\"max\":[0,0,0],\"match\":{\"type\":\"minecraft:true\"}}");
        predicate("{\"type\":\"minecraft:no_such_predicate\"}");
    }

    /** The synthetic world, repeated in oracle_tests.rs. */
    private static final class World {
        final int levelMin;
        final int levelHeight;
        final List<? extends Holder<Biome>> biomes;
        final Holder<Biome> fixed;

        World(int levelMin, int levelHeight, List<? extends Holder<Biome>> biomes, Holder<Biome> fixed) {
            this.levelMin = levelMin;
            this.levelHeight = levelHeight;
            this.biomes = biomes;
            this.fixed = fixed;
        }

        int height(Heightmap.Types kind, int x, int z) {
            return this.levelMin + 24 + Math.floorMod(x * 31 + z * 17 + kind.ordinal() * 7, 97);
        }

        Block block(int x, int y, int z) {
            if (y <= this.levelMin) {
                return Blocks.BEDROCK;
            }
            if (y >= this.height(Heightmap.Types.MOTION_BLOCKING, x, z) + 4) {
                return Blocks.AIR;
            }
            return PALETTE[Math.floorMod(x * 31 + y * 17 + z * 13 + (x >> 2) * (y >> 1), PALETTE.length)];
        }

        Holder<Biome> biome(int x, int y, int z) {
            if (this.fixed != null) {
                return this.fixed;
            }
            return this.biomes.get(Math.floorMod((x >> 2) * 3 + (z >> 2) * 5 + (y >> 2) * 7, this.biomes.size()));
        }

        WorldGenLevel level() {
            return (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(), new Class<?>[] {WorldGenLevel.class},
                (InvocationHandler) (proxy, method, args) -> switch (method.getName()) {
                    case "getMinY" -> this.levelMin;
                    case "getHeight" -> {
                        if (args == null || args.length == 0) {
                            yield this.levelHeight;
                        }
                        yield this.height((Heightmap.Types) args[0], (Integer) args[1], (Integer) args[2]);
                    }
                    case "getSeaLevel" -> 63;
                    case "getLevel" -> null;
                    case "getEntities" -> List.of();
                    case "getBlockState" -> {
                        BlockPos p = (BlockPos) args[0];
                        yield this.block(p.getX(), p.getY(), p.getZ()).defaultBlockState();
                    }
                    case "getFluidState" -> {
                        BlockPos p = (BlockPos) args[0];
                        yield this.block(p.getX(), p.getY(), p.getZ()).defaultBlockState().getFluidState();
                    }
                    // Paper's WorldGenRegion: no light yet during features.
                    case "getRawBrightness" -> 0;
                    case "getBiome" -> {
                        BlockPos p = (BlockPos) args[0];
                        yield this.biome(p.getX(), p.getY(), p.getZ());
                    }
                    case "getMaxY", "isInsideBuildHeight", "isOutsideBuildHeight", "isUnobstructed" -> InvocationHandler.invokeDefault(proxy, method, args);
                    default -> throw new AssertionError("unexpected world method " + method);
                });
        }
    }

    private static String describe(RandomSource random) {
        return Long.toString(random.nextLong());
    }

    private static RandomSource random(boolean legacy, long seed) {
        return legacy ? new LegacyRandomSource(seed) : new XoroshiroRandomSource(seed);
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PALETTE = new Block[] {
            Blocks.AIR, Blocks.STONE, Blocks.DIRT, Blocks.WATER, Blocks.LAVA, Blocks.GRASS_BLOCK, Blocks.CAVE_AIR, Blocks.SAND,
            Blocks.VOID_AIR, Blocks.OAK_LEAVES, Blocks.SNOW, Blocks.GRAVEL, Blocks.SHORT_GRASS, Blocks.PODZOL, Blocks.DEEPSLATE,
            Blocks.COBBLESTONE
        };
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
        HolderLookup.Provider provider = HolderLookup.Provider.create(
            Stream.concat(lookups.stream(), world.registries().map(entry -> (HolderLookup.RegistryLookup<?>) entry.value())));
        RegistryOps<JsonElement> ops = RegistryOps.create(JsonOps.INSTANCE, provider);

        Registry<Biome> biomeRegistry = world.lookupOrThrow(Registries.BIOME);
        Registry<PlacedFeature> placedRegistry = world.lookupOrThrow(Registries.PLACED_FEATURE);
        Registry<NoiseGeneratorSettings> settingsRegistry = world.lookupOrThrow(Registries.NOISE_SETTINGS);
        List<Holder.Reference<Biome>> biomes = new ArrayList<>(biomeRegistry.listElements().sorted((a, b) -> a.key().identifier().toString().compareTo(b.key().identifier().toString())).toList());
        Map<String, Holder.Reference<Biome>> biomeByName = new TreeMap<>();
        biomes.forEach(h -> biomeByName.put(h.key().identifier().toString(), h));
        List<Holder.Reference<PlacedFeature>> placed = placedRegistry.listElements().sorted((a, b) -> a.key().identifier().toString().compareTo(b.key().identifier().toString())).toList();
        @SuppressWarnings("unchecked")
        Holder<NoiseGeneratorSettings>[] generatorSettings = (Holder<NoiseGeneratorSettings>[]) new Holder[] {
            settingsRegistry.getOrThrow(NoiseGeneratorSettings.OVERWORLD), settingsRegistry.getOrThrow(NoiseGeneratorSettings.NETHER)
        };
        Holder.Reference<Biome> plains = biomeByName.get("minecraft:plains");

        StringBuilder out = new StringBuilder();
        providers(out, generatorSettings, plains);
        sorting(out, world, biomeRegistry, placedRegistry);

        // Placed features' own chains, and the custom ones.
        for (int profile = 0; profile < PROFILES.length; profile++) {
            for (String chain : CUSTOM) {
                List<PlacementModifier> modifiers = decodeChain(chain, ops);
                for (int[] origin : ORIGINS) {
                    for (int s = 0; s < 3; s++) {
                        long seed = s == 2 ? 424242L : SEEDS[s];
                        place(out, profile, s == 2, seed, origin, "vary", chain, modifiers, null, biomes, null, generatorSettings, plains);
                    }
                }
            }
        }
        for (Holder.Reference<PlacedFeature> holder : placed) {
            String name = holder.key().identifier().toString();
            Holder.Reference<Biome> owner = biomes.stream().filter(b -> b.value().getGenerationSettings().hasFeature(holder.value())).findFirst().orElse(plains);
            JsonElement json = JsonParser.parseString(new String(resources.getResourceOrThrow(Identifier.parse("minecraft:worldgen/placed_feature/" + holder.key().identifier().getPath() + ".json")).open().readAllBytes()));
            String chain = json.getAsJsonObject().get("placement").toString();
            List<PlacementModifier> modifiers = decodeChain(chain, ops);
            for (int profile = 0; profile < 2; profile++) {
                for (int o = 0; o < ORIGINS.length; o++) {
                    if (profile == 1 && o > 0) {
                        break;
                    }
                    for (int s = 0; s < 2; s++) {
                        if (profile == 1 && s > 0) {
                            break;
                        }
                        for (String mode : new String[] {"vary", owner.key().identifier().toString()}) {
                            place(out, profile, false, SEEDS[s], ORIGINS[o], mode, name, modifiers, holder.value(), biomes,
                                mode.equals("vary") ? null : biomeByName.get(mode), generatorSettings, plains);
                        }
                    }
                }
            }
        }
        Files.writeString(Path.of(args[0]), out.toString());
    }

    private static List<PlacementModifier> decodeChain(String chain, RegistryOps<JsonElement> ops) {
        try {
            return PlacementModifier.CODEC.listOf().parse(ops, JsonParser.parseString(chain)).getOrThrow();
        } catch (RuntimeException e) {
            return null;
        }
    }

    private static void place(StringBuilder out, int profile, boolean legacy, long seed, int[] origin, String mode, String name,
        List<PlacementModifier> modifiers, PlacedFeature top, List<? extends Holder<Biome>> biomes, Holder<Biome> fixed,
        Holder<NoiseGeneratorSettings>[] generatorSettings, Holder<Biome> plains) {
        int[] p = PROFILES[profile];
        String head = "place\t" + PROFILE_NAMES[profile] + "\t" + (legacy ? "legacy" : "xoroshiro") + "\t" + seed + "\t" + origin[0] + "," + p[0] + "," + origin[1]
            + "\t" + mode + "\t" + name + "\t";
        if (modifiers == null) {
            out.append(head).append("decode error\t\t\n");
            return;
        }
        World world = new World(p[0], p[1], biomes, fixed);
        WorldGenLevel level = world.level();
        ChunkGenerator generator = new NoiseBasedChunkGenerator(new FixedBiomeSource(plains), generatorSettings[p[2]]);
        PlacementContext context = new PlacementContext(level, generator, Optional.ofNullable(top));
        RandomSource random = random(legacy, seed);
        List<BlockPos> positions = new ArrayList<>();
        List<BlockPos> pending = new ArrayList<>();
        List<Integer> indices = new ArrayList<>();
        List<BlockPos> modified = new ArrayList<>();
        boolean placedAny = false;
        String result;
        try {
            if (modifiers.isEmpty()) {
                positions.add(new BlockPos(origin[0], p[0], origin[1]));
                placedAny = random.nextInt(3) == 0;
            } else {
                pending.add(new BlockPos(origin[0], p[0], origin[1]));
                indices.add(0);
                while (!pending.isEmpty()) {
                    BlockPos pos = pending.removeLast();
                    int index = indices.removeLast();
                    modifiers.get(index).modify(context, random, pos, modified::add);
                    int next = index + 1;
                    if (next < modifiers.size()) {
                        for (int i = modified.size() - 1; i >= 0; i--) {
                            pending.add(modified.get(i));
                            indices.add(next);
                        }
                    } else {
                        for (BlockPos position : modified) {
                            positions.add(position);
                            placedAny |= random.nextInt(3) == 0;
                        }
                    }
                    modified.clear();
                }
            }
            StringBuilder list = new StringBuilder();
            for (BlockPos position : positions) {
                if (list.length() > 0) {
                    list.append(';');
                }
                list.append(position.getX()).append(' ').append(position.getY()).append(' ').append(position.getZ());
            }
            result = list.toString();
        } catch (RuntimeException e) {
            result = "error";
            placedAny = false;
        }
        out.append(head).append(result).append('\t').append(placedAny).append('\t').append(result.equals("error") ? "" : describe(random)).append('\n');
    }

    private static void providers(StringBuilder out, Holder<NoiseGeneratorSettings>[] generatorSettings, Holder<Biome> plains) {
        for (String json : INT_PROVIDERS) {
            for (int s = 0; s < 4; s++) {
                boolean legacy = s >= 2;
                long seed = SEEDS[s % 2];
                String samples;
                try {
                    IntProvider provider = IntProviders.CODEC.parse(JsonOps.INSTANCE, JsonParser.parseString(json)).getOrThrow();
                    RandomSource random = random(legacy, seed);
                    StringBuilder values = new StringBuilder();
                    try {
                        for (int i = 0; i < 24; i++) {
                            values.append(i > 0 ? "," : "").append(provider.sample(random));
                        }
                        samples = values.toString();
                    } catch (IllegalArgumentException e) {
                        samples = "error";
                    }
                } catch (RuntimeException e) {
                    samples = "decode error";
                }
                out.append("int\t").append(legacy ? "legacy" : "xoroshiro").append('\t').append(seed).append('\t').append(samples).append('\t').append(json).append('\n');
            }
        }
        for (int profile = 0; profile < PROFILES.length; profile++) {
            int[] p = PROFILES[profile];
            ChunkGenerator generator = new NoiseBasedChunkGenerator(new FixedBiomeSource(plains), generatorSettings[p[2]]);
            WorldGenerationContext context = new WorldGenerationContext(generator, LevelHeightAccessor.create(p[0], p[1]));
            for (String json : HEIGHT_PROVIDERS) {
                for (int s = 0; s < 4; s++) {
                    boolean legacy = s >= 2;
                    long seed = SEEDS[s % 2];
                    String samples;
                    try {
                        HeightProvider provider = HeightProvider.CODEC.parse(JsonOps.INSTANCE, JsonParser.parseString(json)).getOrThrow();
                        RandomSource random = random(legacy, seed);
                        StringBuilder values = new StringBuilder();
                        try {
                            for (int i = 0; i < 24; i++) {
                                values.append(i > 0 ? "," : "").append(provider.sample(random, context));
                            }
                            samples = values.toString();
                        } catch (IllegalArgumentException e) {
                            samples = "error";
                        }
                    } catch (RuntimeException e) {
                        samples = "decode error";
                    }
                    out.append("height\t").append(PROFILE_NAMES[profile]).append('\t').append(legacy ? "legacy" : "xoroshiro").append('\t').append(seed).append('\t')
                        .append(samples).append('\t').append(json).append('\n');
                }
            }
        }
    }

    private static void sorting(StringBuilder out, RegistryAccess.Frozen world, Registry<Biome> biomeRegistry, Registry<PlacedFeature> placedRegistry) {
        var lists = world.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST);
        BiomeSource overworld = MultiNoiseBiomeSource.createFromList(lists.getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD).value().parameters());
        BiomeSource nether = MultiNoiseBiomeSource.createFromList(lists.getOrThrow(MultiNoiseBiomeSourceParameterLists.NETHER).value().parameters());
        BiomeSource end = TheEndBiomeSource.create(biomeRegistry);
        Map<String, List<Holder<Biome>>> sets = new java.util.LinkedHashMap<>();
        sets.put("overworld", new ArrayList<>(overworld.possibleBiomes()));
        sets.put("nether", new ArrayList<>(nether.possibleBiomes()));
        sets.put("end", new ArrayList<>(end.possibleBiomes()));
        for (String name : List.of("overworld", "nether", "end")) {
            List<Holder<Biome>> reversed = new ArrayList<>(sets.get(name));
            Collections.reverse(reversed);
            sets.put(name + "_reversed", reversed);
        }
        sets.put("all_sorted", biomeRegistry.listElements().sorted((a, b) -> a.key().identifier().toString().compareTo(b.key().identifier().toString())).map(h -> (Holder<Biome>) h).toList());
        biomeRegistry.listElements().sorted((a, b) -> a.key().identifier().toString().compareTo(b.key().identifier().toString()))
            .forEach(h -> sets.put("single:" + h.key().identifier(), List.of(h)));
        for (Map.Entry<String, List<Holder<Biome>>> set : sets.entrySet()) {
            StringBuilder names = new StringBuilder();
            for (Holder<Biome> biome : set.getValue()) {
                if (names.length() > 0) {
                    names.append(' ');
                }
                names.append(((Holder.Reference<Biome>) biome).key().identifier());
            }
            out.append("set\t").append(set.getKey()).append('\t').append(names).append('\n');
            try {
                List<FeatureSorter.StepFeatureData> steps = FeatureSorter.buildFeaturesPerStep(set.getValue(), biome -> biome.value().getGenerationSettings().features(), true);
                for (int step = 0; step < steps.size(); step++) {
                    StringBuilder features = new StringBuilder();
                    for (PlacedFeature feature : steps.get(step).features()) {
                        if (features.length() > 0) {
                            features.append(' ');
                        }
                        features.append(placedRegistry.getKey(feature));
                    }
                    out.append("sort\t").append(set.getKey()).append('\t').append(step).append('\t').append(features).append('\n');
                }
            } catch (IllegalStateException e) {
                out.append("sort\t").append(set.getKey()).append("\tcycle\t\n");
            }
        }
    }
}
