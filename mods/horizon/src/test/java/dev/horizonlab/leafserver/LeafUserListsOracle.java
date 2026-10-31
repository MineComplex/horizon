// Writes what horizon-server's user lists (crates/horizon-server/src/server/user_lists) must reproduce
// from the real classes: the 26.3 jar's StoredUserList family (vanilla: a HashMap, no Paper changes),
// the JDK's ConcurrentHashMap (what Paper keeps the lists in) and UUID. Run with the server owner's own
// 26.3 jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafUserListsOracle.java <out.jsonl>`; the file is given to the ignored test
// `user_lists_match_java` (HORIZON_USER_LISTS_ORACLE). Original Horizon code.
// One JSON object per line, "kind" first:
//   chm     script (p:<key> put, r:<key> remove, c clear), keys: keySet() order after it
//   format  list, input, output: load(input) then save(), single entry (vanilla's order is a HashMap's)
//   read    list, input, entries sorted by key, or error: the exception's class
//   uuid    text, uuid or null (UUID.fromString)
// The ban lists are not here: MineComplex's cleanup removed them from the server.
// Not in the vanilla jar and so derived from sources: Paper's ConcurrentHashMap keeping (checked on
// the JDK class), the .backup file, Spigot's messages.whitelist and Adventure's legacy section
// serializer (not among the jar's libraries).

package dev.horizonlab.leafserver;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.File;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.List;
import java.util.Random;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import net.minecraft.server.notifications.EmptyNotificationService;
import net.minecraft.server.players.NameAndId;
import net.minecraft.server.players.ServerOpList;
import net.minecraft.server.players.ServerOpListEntry;
import net.minecraft.server.players.StoredUserEntry;
import net.minecraft.server.players.StoredUserList;
import net.minecraft.server.players.UserWhiteList;
import net.minecraft.server.players.UserWhiteListEntry;

public final class LeafUserListsOracle {
    private static final Gson GSON = new Gson();
    private static PrintWriter out;
    private static Path scratch;

    private static void row(JsonObject o) {
        out.println(GSON.toJson(o));
    }

    private static JsonObject kind(String kind) {
        JsonObject o = new JsonObject();
        o.addProperty("kind", kind);
        return o;
    }

    public static void main(String[] args) throws Exception {
        scratch = Files.createTempDirectory("user-lists-oracle");
        try (PrintWriter writer = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            out = writer;
            chm();
            format();
            read();
            uuids();
        }
    }

    // Random UUID keys put, removed and cleared on a real ConcurrentHashMap.
    private static void chm() {
        int[] sizes = {1, 5, 11, 12, 13, 20, 23, 24, 25, 40, 48, 49, 90, 100, 200};
        for (int seed = 0; seed < 60; seed++) {
            Random random = new Random(seed);
            int puts = sizes[seed % sizes.length] + random.nextInt(4);
            ConcurrentHashMap<String, Integer> map = new ConcurrentHashMap<>();
            List<String> live = new ArrayList<>();
            JsonArray script = new JsonArray();
            for (int i = 0; i < puts; i++) {
                String key = new UUID(random.nextLong(), random.nextLong()).toString();
                if (seed % 5 == 1 && !live.isEmpty() && random.nextInt(4) == 0) {
                    String gone = live.remove(random.nextInt(live.size()));
                    map.remove(gone);
                    script.add("r:" + gone);
                } else if (seed % 5 == 2 && i == puts / 2) {
                    map.clear();
                    live.clear();
                    script.add("c");
                } else if (seed % 5 == 3 && !live.isEmpty() && random.nextInt(3) == 0) {
                    key = live.get(random.nextInt(live.size())); // put over an existing key
                    map.put(key, i);
                    script.add("p:" + key);
                } else {
                    map.put(key, i);
                    live.add(key);
                    script.add("p:" + key);
                }
            }
            JsonObject o = kind("chm");
            o.add("script", script);
            JsonArray keys = new JsonArray();
            map.keySet().forEach(keys::add);
            o.add("keys", keys);
            row(o);
        }
        // Other keys.
        ConcurrentHashMap<String, Integer> map = new ConcurrentHashMap<>();
        JsonArray script = new JsonArray();
        for (int i = 0; i < 40; i++) {
            String key = "10.0." + (i % 7) + "." + (i * 13 % 251);
            map.put(key, i);
            script.add("p:" + key);
        }
        JsonObject o = kind("chm");
        o.add("script", script);
        JsonArray keys = new JsonArray();
        map.keySet().forEach(keys::add);
        o.add("keys", keys);
        row(o);
    }

    private static final String U1 = "069a79f4-44e9-4726-a5be-fca90e38aaf5";

    private static StoredUserList<?, ?> list(String name, File file) {
        EmptyNotificationService n = new EmptyNotificationService();
        return switch (name) {
            case "whitelist" -> new UserWhiteList(file, n);
            case "ops" -> new ServerOpList(file, n);
            default -> throw new IllegalArgumentException(name);
        };
    }

    // One entry in, the file Java writes out.
    private static void format() throws Exception {
        String[][] cases = {
            {"whitelist", "[{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\"}]"},
            {"whitelist", "[]"},
            {"whitelist", "[{\"uuid\":\"" + U1 + "\",\"name\":\"a<b>&c='d'\\\"e\\\\f\\t\\n\\u2028 \\u00e9\\u0001\"}]"},
            {"ops", "[{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\",\"level\":4,\"bypassesPlayerLimit\":true}]"},
            {"ops", "[{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\"}]"},
            {"ops", "[{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\",\"level\":9}]"},
            {"ops", "[{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\",\"level\":-3,\"bypassesPlayerLimit\":\"TRUE\"}]"},
            {"ops", "[{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\",\"level\":\"2\"}]"},
            {"ops", "[{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\",\"level\":2.7}]"},
            {"ops", "[{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\",\"level\":5000000000}]"},
        };
        for (String[] c : cases) {
            File file = scratch.resolve("format.json").toFile();
            Files.writeString(file.toPath(), c[1]);
            StoredUserList<?, ?> list = list(c[0], file);
            list.load();
            list.save();
            JsonObject o = kind("format");
            o.addProperty("list", c[0]);
            o.addProperty("input", c[1]);
            o.addProperty("output", Files.readString(file.toPath()));
            row(o);
        }
    }

    // Entries read from JSON, or the exception that stopped it.
    private static void read() throws Exception {
        String ok = "{\"uuid\":\"" + U1 + "\",\"name\":\"Notch\"";
        String[][] cases = {
            {"whitelist", "[" + ok + "},{\"uuid\":\"61699b2e-d327-4a01-9f1e-0ea8c3f06bc6\",\"name\":\"jeb_\"}]"},
            {"whitelist", ""},
            {"whitelist", "   \n"},
            {"whitelist", "null"},
            {"whitelist", "{}"},
            {"whitelist", "[1]"},
            {"whitelist", "[{\"uuid\":\"" + U1 + "\"}]"},
            {"whitelist", "[{\"name\":\"a\"}]"},
            {"whitelist", "[{\"uuid\":\"not-a-uuid\",\"name\":\"a\"}]"},
            {"whitelist", "[{\"uuid\":\"1-2-3-4-5\",\"name\":\"a\"}]"},
            {"whitelist", "[{\"uuid\":\"069A79F4-44E9-4726-A5BE-FCA90E38AAF5\",\"name\":\"a\"}]"},
            {"whitelist", "[{\"uuid\":null,\"name\":\"a\"}]"},
            {"whitelist", "[{\"uuid\":12,\"name\":\"a\"}]"},
            {"whitelist", "[{\"uuid\":\"" + U1 + "\",\"name\":7}]"},
            {"whitelist", "[" + ok + "},{\"uuid\":\"" + U1 + "\",\"name\":\"Other\"}]"},
            {"whitelist", "[" + ok + "}"},
            {"ops", "[" + ok + ",\"level\":3,\"bypassesPlayerLimit\":true}]"},
            {"ops", "[" + ok + ",\"level\":\"x\"}]"},
            {"ops", "[" + ok + ",\"level\":true}]"},
            {"ops", "[" + ok + ",\"level\":null}]"},
            {"ops", "[" + ok + ",\"level\":1.5e1}]"},
            {"ops", "[" + ok + ",\"bypassesPlayerLimit\":1}]"},
            {"ops", "[" + ok + ",\"bypassesPlayerLimit\":\"true\"}]"},
            {"ops", "[" + ok + ",\"bypassesPlayerLimit\":null}]"},
        };
        for (String[] c : cases) {
            File file = scratch.resolve("read.json").toFile();
            Files.writeString(file.toPath(), c[1]);
            StoredUserList<?, ?> list = list(c[0], file);
            JsonObject o = kind("read");
            o.addProperty("list", c[0]);
            o.addProperty("input", c[1]);
            try {
                list.load();
                JsonArray entries = new JsonArray();
                List<String> rows = new ArrayList<>();
                for (Object e : (Collection<?>) list.getEntries()) {
                    StoredUserEntry<?> entry = (StoredUserEntry<?>) e;
                    NameAndId user = (NameAndId) entry.getUser();
                    String row = user.id() + " " + user.name();
                    if (e instanceof ServerOpListEntry op) {
                        row += " " + op.permissions().level().id() + " " + op.getBypassesPlayerLimit();
                    }
                    rows.add(row);
                }
                rows.sort(Comparator.naturalOrder());
                rows.forEach(entries::add);
                o.add("entries", entries);
            } catch (Throwable t) {
                o.addProperty("error", t.getClass().getName());
            }
            row(o);
        }
    }

    private static void uuids() {
        String[] texts = {
            U1, "069A79F4-44E9-4726-A5BE-FCA90E38AAF5", "1-2-3-4-5", "0-0-0-0-0", "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "fffffffff-ffff-ffff-ffff-ffffffffffff", "+1-+2-+3-+4-+5", "069a79f4-44e9-4726-a5be-fca90e38aafg", "",
            "069a79f444e947264726a5befca90e38aaf5", "069a79f4-44e9-4726-a5be", "069a79f4-44e9-4726-a5be-fca90e38aaf5-1",
            "069a79f4-44e9-4726-a5be-fca90e38aaf55", "-1-2-3-4-5", "1--3-4-5", "+69a79f4-44e9-4726-a5be-fca90e38aaf5",
            "069a79f4-44e9-4726-a5be-+ca90e38aaf5", "069a79f4-44e9-4726-a5be-fca90e38aaf 5", " 1-2-3-4-5",
            "10000000000000000-1-1-1-1", "1-10000-1-1-1", "123456789-1-1-1-1", "1-1-1-1-1000000000000",
        };
        for (String text : texts) {
            JsonObject o = kind("uuid");
            o.addProperty("text", text);
            try {
                o.addProperty("uuid", UUID.fromString(text).toString());
            } catch (IllegalArgumentException e) {
                o.add("uuid", null);
            }
            row(o);
        }
    }
}
