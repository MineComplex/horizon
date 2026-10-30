// Writes what the 26.3 server sends in the configuration state, for horizon-server's registry and
// configuration tests (crates/horizon-server/src/network/configuration/tests.rs). Run with the server
// owner's own 26.3 jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafConfigurationOracle.java <out.tsv>`; the TSV is then given to the ignored
// test `real_registries_match_java` (HORIZON_ORACLE_TSV). Original Horizon code.
// Registries are loaded as WorldLoader does (static tags, then the world registries) from the vanilla
// pack plus Paper's built-in `paper` pack (its one chat type, written to a temporary folder here).
// Rows: select_known_packs, registry_known (with the known packs), registry_full (without),
// raw (JsonOps.convertTo(NbtOps) of every synchronized registry file), tags/tag (per registry).

package dev.horizonlab.leafserver;

import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.RegistrySynchronization;
import net.minecraft.nbt.NbtOps;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.configuration.ClientboundRegistryDataPacket;
import net.minecraft.network.protocol.configuration.ClientboundSelectKnownPacks;
import net.minecraft.resources.FileToIdConverter;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackLocationInfo;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.PathPackResources;
import net.minecraft.server.packs.repository.KnownPack;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.PackSource;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.tags.TagNetworkSerialization;

public final class LeafConfigurationOracle {
    private static final String PAPER_RAW = """
        {"chat": {"parameters": ["content"], "translation_key": "%s"},
         "narration": {"parameters": ["content"], "translation_key": "%s"}}""";

    private static String hex(ByteBuf buf) {
        StringBuilder out = new StringBuilder();
        while (buf.isReadable()) {
            out.append(String.format("%02x", buf.readUnsignedByte()));
        }
        return out.toString();
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Path paper = Files.createTempDirectory("paper-pack");
        Files.createDirectories(paper.resolve("data/paper/chat_type"));
        Files.writeString(paper.resolve("data/paper/chat_type/raw.json"), PAPER_RAW);

        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        List<PackResources> packs = new ArrayList<>(repository.openAllSelected());
        packs.add(new PathPackResources(new PackLocationInfo("paper", Component.literal("paper"), PackSource.BUILT_IN, Optional.empty()), paper));
        MultiPackResourceManager resources = new MultiPackResourceManager(PackType.SERVER_DATA, packs);

        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags = TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        var lookups = TagLoader.buildUpdatedLookups(initial.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
        staticTags.forEach(Registry.PendingTags::apply);

        StringBuilder out = new StringBuilder();
        List<KnownPack> known = resources.listPacks().flatMap(p -> p.location().knownPackInfo().stream()).toList();
        FriendlyByteBuf buf = new FriendlyByteBuf(Unpooled.buffer());
        ClientboundSelectKnownPacks.STREAM_CODEC.encode(buf, new ClientboundSelectKnownPacks(known));
        out.append("select_known_packs\t").append(hex(buf)).append('\n');

        var ops = layers.compositeAccess().createSerializationContext(NbtOps.INSTANCE);
        for (Set<KnownPack> negotiated : List.of(Set.copyOf(known), Set.<KnownPack>of())) {
            String row = negotiated.isEmpty() ? "registry_full\t" : "registry_known\t";
            RegistrySynchronization.packRegistries(ops, layers.getAccessFrom(RegistryLayer.WORLD), negotiated, (key, entries) -> {
                FriendlyByteBuf packet = new FriendlyByteBuf(Unpooled.buffer());
                ClientboundRegistryDataPacket.STREAM_CODEC.encode(packet, new ClientboundRegistryDataPacket(key, entries));
                out.append(row).append(key.identifier()).append('\t').append(hex(packet)).append('\n');
            });
        }
        for (var data : RegistryDataLoader.SYNCHRONIZED_REGISTRIES) {
            var files = FileToIdConverter.registry(data.key()).listMatchingResources(resources);
            for (var file : new TreeMap<>(files).entrySet()) {
                var nbt = JsonOps.INSTANCE.convertTo(NbtOps.INSTANCE, JsonParser.parseReader(file.getValue().openAsReader()));
                FriendlyByteBuf packet = new FriendlyByteBuf(Unpooled.buffer());
                packet.writeNbt(nbt);
                out.append("raw\t").append(file.getKey()).append('\t').append(hex(packet)).append('\n');
            }
        }
        for (var registry : TagNetworkSerialization.serializeTagsToNetwork(layers).entrySet()) {
            FriendlyByteBuf payload = new FriendlyByteBuf(Unpooled.buffer());
            TagNetworkSerialization.NetworkPayload.STREAM_CODEC.encode(payload, registry.getValue());
            String name = registry.getKey().identifier().toString();
            out.append("tags\t").append(name).append('\t').append(hex(payload)).append('\n');
            Map<String, List<Integer>> sorted = new TreeMap<>();
            registry.getValue().tags().forEach((id, ids) -> sorted.put(id.toString(), new ArrayList<>(ids)));
            sorted.forEach((id, ids) -> out.append("tag\t").append(name).append('\t').append(id).append('\t').append(ids).append('\n'));
        }
        Files.writeString(Path.of(args[0]), out.toString());
        System.exit(0);
    }
}
