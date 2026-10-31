// Writes what horizon-server's per-level data (crates/horizon-server/src/server/level_data.rs) must
// reproduce from the real classes: the 26.3 jar's WorldBorder (its Settings codec, applying the
// settings, ticking a moving border, ClientboundInitializeBorderPacket), ClockState's codec and
// LevelSettings.DifficultySettings' codec. Run with the server owner's own 26.3 jar and its
// libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafLevelDataOracle.java <out.jsonl>`; the file is given to the ignored test
// `level_data_matches_java` (HORIZON_LEVEL_DATA_ORACLE). Original Horizon code.
// One JSON object per line, "kind" first:
//   border      settings (the nine fields), ticks: ok, size, lerp_time, lerp_target, saved (Settings
//               after the ticks, the nine fields), packet (hex) after applyInitialSettings + ticks
//   clock       input (fields present), ok, output (the fields the codec writes back)
//   difficulty  input, ok
// Not in the vanilla jar and so derived from sources: Paper's PaperLevelOverrides (its codec is the
// record of the fields tested here), the border packet's centre scaling by the dimension type's
// coordinate scale, ServerClockManager (a clock per registry entry, Paper's per-world time).

package dev.horizonlab.leafserver;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import io.netty.buffer.Unpooled;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Optional;
import java.util.Random;
import net.minecraft.SharedConstants;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundInitializeBorderPacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.clock.ClockState;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.border.WorldBorder;

public final class LeafLevelDataOracle {
    private static final Gson GSON = new Gson();
    private static PrintWriter out;

    private static void row(JsonObject o) {
        out.println(GSON.toJson(o));
    }

    private static JsonObject kind(String kind) {
        JsonObject o = new JsonObject();
        o.addProperty("kind", kind);
        return o;
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        try (PrintWriter writer = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            out = writer;
            borders();
            clocks();
            difficulties();
        }
    }

    private static JsonObject settingsJson(WorldBorder.Settings s) {
        JsonObject o = new JsonObject();
        o.addProperty("center_x", s.centerX());
        o.addProperty("center_z", s.centerZ());
        o.addProperty("damage_per_block", s.damagePerBlock());
        o.addProperty("safe_zone", s.safeZone());
        o.addProperty("warning_blocks", s.warningBlocks());
        o.addProperty("warning_time", s.warningTime());
        o.addProperty("size", s.size());
        o.addProperty("lerp_time", s.lerpTime());
        o.addProperty("lerp_target", s.lerpTarget());
        return o;
    }

    private static String hex(byte[] bytes) {
        StringBuilder sb = new StringBuilder();
        for (byte b : bytes) {
            sb.append(String.format("%02x", b));
        }
        return sb.toString();
    }

    private static void borders() {
        Random random = new Random(26);
        for (int i = 0; i < 80; i++) {
            double size = i % 7 == 0 ? 5.999997E7F : 16 + random.nextInt(100000);
            long lerpTime = i % 3 == 0 ? 0 : 1 + random.nextInt(60);
            double target = i % 5 == 0 ? size : 1 + random.nextInt(100000);
            WorldBorder.Settings settings = new WorldBorder.Settings(
                random.nextInt(2000) - 1000 + (i % 4 == 0 ? 0.5 : 0.0),
                random.nextInt(2000) - 1000,
                0.1 * random.nextInt(10),
                random.nextInt(20),
                random.nextInt(30),
                15 + random.nextInt(300),
                size,
                lerpTime,
                target
            );
            int ticks = random.nextInt(80);
            CompoundTag tag = (CompoundTag) WorldBorder.Settings.CODEC.encodeStart(NbtOps.INSTANCE, settings).getOrThrow();
            WorldBorder.Settings read = WorldBorder.Settings.CODEC.parse(NbtOps.INSTANCE, tag).getOrThrow();
            WorldBorder border = new WorldBorder(read);
            border.applyInitialSettings(0L);
            for (int t = 0; t < ticks; t++) {
                border.tick();
            }
            FriendlyByteBuf buf = new FriendlyByteBuf(Unpooled.buffer());
            ClientboundInitializeBorderPacket.STREAM_CODEC.encode(buf, new ClientboundInitializeBorderPacket(border));
            byte[] packet = new byte[buf.readableBytes()];
            buf.readBytes(packet);
            JsonObject o = kind("border");
            o.add("settings", settingsJson(settings));
            o.addProperty("ticks", ticks);
            o.addProperty("size", border.getSize());
            o.addProperty("lerp_time", border.getLerpTime());
            o.addProperty("lerp_target", border.getLerpTarget());
            o.add("saved", settingsJson(new WorldBorder.Settings(border)));
            o.addProperty("packet", hex(packet));
            row(o);
        }
        // A centre outside +-2.9999984E7 does not decode.
        CompoundTag far = (CompoundTag) WorldBorder.Settings.CODEC.encodeStart(NbtOps.INSTANCE, WorldBorder.Settings.DEFAULT).getOrThrow();
        far.putDouble("center_x", 3.0E7);
        JsonObject o = kind("border_far");
        o.addProperty("ok", WorldBorder.Settings.CODEC.parse(NbtOps.INSTANCE, far).result().isPresent());
        row(o);
    }

    private static void clocks() {
        Object[][] cases = {
            {1000L, null, null, null},
            {0L, 0.25F, null, null},
            {5L, null, 2.0F, true},
            {5L, null, 0.0F, null},
            {5L, null, -1.0F, null},
            {7L, 0.5F, 1.0F, false},
            {-4L, null, null, null},
        };
        for (Object[] c : cases) {
            CompoundTag tag = new CompoundTag();
            JsonObject input = new JsonObject();
            tag.putLong("total_ticks", (Long) c[0]);
            input.addProperty("total_ticks", (Long) c[0]);
            if (c[1] != null) {
                tag.putFloat("partial_tick", (Float) c[1]);
                input.addProperty("partial_tick", (Float) c[1]);
            }
            if (c[2] != null) {
                tag.putFloat("rate", (Float) c[2]);
                input.addProperty("rate", (Float) c[2]);
            }
            if (c[3] != null) {
                tag.putBoolean("paused", (Boolean) c[3]);
                input.addProperty("paused", (Boolean) c[3]);
            }
            Optional<ClockState> state = ClockState.CODEC.parse(NbtOps.INSTANCE, tag).result();
            JsonObject o = kind("clock");
            o.add("input", input);
            o.addProperty("ok", state.isPresent());
            if (state.isPresent()) {
                CompoundTag back = (CompoundTag) ClockState.CODEC.encodeStart(NbtOps.INSTANCE, state.get()).getOrThrow();
                JsonObject output = new JsonObject();
                output.addProperty("total_ticks", back.getLongOr("total_ticks", -1L));
                for (String key : new String[] {"partial_tick", "rate"}) {
                    if (back.contains(key)) {
                        output.addProperty(key, back.getFloatOr(key, 0.0F));
                    }
                }
                if (back.contains("paused")) {
                    output.addProperty("paused", back.getBooleanOr("paused", false));
                }
                o.add("output", output);
            }
            row(o);
        }
    }

    private static void difficulties() {
        for (String difficulty : new String[] {"peaceful", "easy", "normal", "hard", "impossible", "HARD", ""}) {
            CompoundTag tag = new CompoundTag();
            tag.putString("difficulty", difficulty);
            tag.putBoolean("hardcore", difficulty.length() % 2 == 0);
            tag.putBoolean("locked", true);
            Optional<LevelSettings.DifficultySettings> parsed = LevelSettings.DifficultySettings.CODEC.parse(NbtOps.INSTANCE, (Tag) tag).result();
            JsonObject o = kind("difficulty");
            o.addProperty("input", difficulty);
            o.addProperty("ok", parsed.isPresent());
            row(o);
        }
        // A missing field fails the whole record.
        CompoundTag missing = new CompoundTag();
        missing.putString("difficulty", "easy");
        missing.putBoolean("hardcore", false);
        JsonObject o = kind("difficulty_missing_locked");
        o.addProperty("ok", LevelSettings.DifficultySettings.CODEC.parse(NbtOps.INSTANCE, (Tag) missing).result().isPresent());
        row(o);
    }
}
