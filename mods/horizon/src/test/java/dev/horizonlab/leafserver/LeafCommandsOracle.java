// Writes what horizon-server's commands (crates/horizon-server/src/server/commands) must reproduce
// from the real classes: the 26.3 jar's Commands dispatcher (the full vanilla tree for a dedicated
// server) restricted to the ported commands. Run with the server owner's own 26.3 jar and its
// libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafCommandsOracle.java <out.jsonl>`; the file is given to the ignored test
// `commands_match_java` (HORIZON_COMMANDS_ORACLE). Original Horizon code.
// One JSON object per line, "kind" first:
//   parse   level, input, then error (key, args, cursor, context) or ok (arguments: name -> text)
//   usage   level, command, restricted, lines (Brigadier's getAllUsage)
//   packet  level, hex (ClientboundCommandsPacket of the vanilla tree for that permission level,
//           restricted to the ported commands)
//   render  key, args ([type, text]), text (Component.translatable(...).getString())
//   unknown input, text (what a source is sent for a line that does not parse: the two
//           sendFailure messages of the jar's finishParsing, joined by a new line as Leaf joins them)
// Not in the vanilla jar and so derived from sources: Paper's "minecraft:" aliases, Leaf's single
// unknown-command message, the console's "System chat: " log line, every command's body (they need
// a MinecraftServer), Spigot/Paper permission nodes. The tree is restricted to what is ported:
// "time of <clock>" and the timeline queries are removed from /time before it is compared.

package dev.horizonlab.leafserver;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.ParseResults;
import com.mojang.brigadier.context.CommandContextBuilder;
import com.mojang.brigadier.context.ParsedArgument;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.tree.CommandNode;
import com.mojang.brigadier.tree.RootCommandNode;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.SharedConstants;
import net.minecraft.commands.CommandBuildContext;
import net.minecraft.commands.CommandSource;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.commands.arguments.GameProfileArgument;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.ComponentContents;
import net.minecraft.network.chat.contents.TranslatableContents;
import net.minecraft.network.protocol.game.ClientboundCommandsPacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.permissions.PermissionLevel;
import net.minecraft.world.phys.Vec2;
import net.minecraft.world.phys.Vec3;

public final class LeafCommandsOracle {
    private static final Gson GSON = new Gson();
    private static PrintWriter out;
    private static CommandDispatcher<CommandSourceStack> dispatcher;

    /** The ported commands, in the order Commands registers them. */
    private static final List<String> PORTED = List.of(
        "difficulty", "list", "locate", "seed", "tick", "time", "weather",
        "deop", "op", "save-all", "save-off", "save-on", "stop", "whitelist");

    private static final String[] INPUTS = {
        "", "frobnicate", "list", "list uuids", "list nope", "list uuids x", "seed", "seed x",
        "difficulty", "difficulty hard", "difficulty HARD", "difficulty 1", "difficulty peaceful ",
        "tick", "tick query", "tick rate", "tick rate 0", "tick rate 20", "tick rate 10001",
        "tick rate 20.5", "tick rate abc", "tick step", "tick step 5", "tick step 0", "tick step 1s",
        "tick step 2d", "tick step stop", "tick step -1", "tick sprint", "tick sprint 60s",
        "tick sprint stop", "tick sprint 0t", "tick freeze", "tick unfreeze", "tick nope",
        "time", "time set 1000", "time set day", "time set 1x", "time set minecraft:night",
        "time set Bad", "time set -5", "time add -5", "time add 2d", "time add 1.5s", "time pause",
        "time resume", "time rate 0", "time rate 1000", "time rate 1001", "time rate 0.00001",
        "time rate 2.5", "time query time", "time query gametime", "time query day",
        "time query", "weather", "weather clear", "weather clear 0", "weather clear 1", "weather rain 100",
        "weather thunder 5s", "weather snow", "weather rain 1x",
        "op", "op Steve", "op @a", "op @p", "op @r", "op @e", "op @s", "op @n", "op @x", "op @",
        "op Steve Alex", "op 0123", "deop", "deop Steve", "deop dd12be42-52a9-4a91-a8a1-11c01849e498",
        "save-all", "save-all flush", "save-all x", "save-off", "save-on", "save-on now", "stop",
        "stop now", "whitelist", "whitelist on", "whitelist off", "whitelist add", "whitelist add Steve",
        "whitelist add @a", "whitelist remove Steve", "whitelist remove", "whitelist reload",
        "whitelist list", "whitelist nonsense",
        "a very long unknown command line to cut", "whitelist add Steve extra words here that go on",
        "tést üïö unknown line",
        "locate", "locate structure", "locate structure minecraft:village_plains", "locate structure village_plains",
        "locate structure #minecraft:village", "locate structure #", "locate structure Bad", "locate structure a:b:c",
        "locate structure #minecraft:on_ocean_explorer_maps extra", "locate biome", "locate biome minecraft:plains",
        "locate biome plains", "locate biome #minecraft:is_forest", "locate biome #", "locate poi",
        "locate poi minecraft:farmer", "locate poi #minecraft:village", "locate nothing", "locate structure  x"
    };

    private static final Object[][] RENDERS = {
        {"commands.save.saving"}, {"commands.save.success"}, {"commands.save.failed"},
        {"commands.save.alreadyOff"}, {"commands.save.alreadyOn"}, {"commands.save.disabled"},
        {"commands.save.enabled"}, {"commands.stop.stopping"},
        {"commands.seed.success", "s", "[1234]"},
        {"commands.list.players", "i", 3, "i", 20, "s", "a, b, c"},
        {"commands.list.nameAndId", "s", "Steve", "s", "dd12be42-52a9-4a91-a8a1-11c01849e498"},
        {"commands.whitelist.alreadyOn"}, {"commands.whitelist.alreadyOff"},
        {"commands.whitelist.add.failed"}, {"commands.whitelist.remove.failed"},
        {"commands.whitelist.reloaded"}, {"commands.whitelist.enabled"}, {"commands.whitelist.disabled"},
        {"commands.whitelist.none"},
        {"commands.whitelist.list", "i", 2, "s", "Alex, Steve"},
        {"commands.whitelist.add.success", "s", "Steve"}, {"commands.whitelist.remove.success", "s", "Steve"},
        {"commands.op.success", "s", "Steve"}, {"commands.op.failed"},
        {"commands.deop.success", "s", "Steve"}, {"commands.deop.failed"},
        {"commands.difficulty.query", "c", "options.difficulty.hard"},
        {"commands.difficulty.success", "c", "options.difficulty.peaceful"},
        {"commands.difficulty.failure", "c", "options.difficulty.easy"},
        {"commands.time.query.gametime", "l", 123456789012L},
        {"commands.time.query.absolute", "s", "minecraft:overworld", "l", 6000L},
        {"commands.time.set.absolute", "s", "minecraft:overworld", "l", 6000L},
        {"commands.time.set.already_at_time", "s", "minecraft:overworld", "l", 6000L},
        {"commands.time.set.time_marker", "s", "minecraft:overworld", "s", "minecraft:day"},
        {"commands.time.set.already_at_time_marker", "s", "minecraft:overworld", "s", "minecraft:day"},
        {"commands.time.no_time_marker_found", "s", "minecraft:day", "s", "minecraft:overworld"},
        {"commands.time.no_default_clock", "s", "minecraft:the_nether"},
        {"commands.time.pause", "s", "minecraft:overworld"}, {"commands.time.resume", "s", "minecraft:overworld"},
        {"commands.time.pause.already_paused", "s", "minecraft:overworld"},
        {"commands.time.pause.already_running", "s", "minecraft:overworld"},
        {"commands.time.rate", "s", "minecraft:overworld", "f", 2.5f},
        {"commands.time.rate.already_same", "s", "minecraft:overworld", "f", 1.0f},
        {"commands.weather.set.clear"}, {"commands.weather.set.rain"}, {"commands.weather.set.thunder"},
        {"commands.tick.rate.success", "s", "40.0"}, {"commands.tick.status.sprinting"},
        {"commands.tick.status.frozen"}, {"commands.tick.status.running"}, {"commands.tick.status.lagging"},
        {"commands.tick.query.rate.sprinting", "s", "20.0", "s", "1.5"},
        {"commands.tick.query.rate.running", "s", "20.0", "s", "1.5", "s", "50.0"},
        {"commands.tick.query.percentiles", "s", "1.0", "s", "2.0", "s", "3.0", "i", 100},
        {"commands.tick.step.success", "i", 5}, {"commands.tick.step.fail"},
        {"commands.tick.step.stop.success"}, {"commands.tick.step.stop.fail"},
        {"commands.tick.sprint.stop.success"}, {"commands.tick.sprint.stop.fail"},
        {"commands.tick.sprint.report", "i", 250, "s", "4.00"},
        {"chat.type.admin", "s", "Server", "s", "Saved the game"},
        {"command.unknown.command"}, {"command.unknown.argument"}, {"command.context.here"},
        {"command.forkLimit", "i", 65536},
        {"argument.player.unknown"}, {"argument.entity.notfound.player"}, {"argument.player.entities"},
        {"argument.entity.selector.not_allowed"}, {"argument.id.invalid"},
        {"chat.square_brackets", "s", "1234"},
    };

    private static CommandSourceStack source(int level, CommandSource target) {
        return new CommandSourceStack(
            target, Vec3.ZERO, Vec2.ZERO, null,
            LevelBasedPermissionSet.forLevel(PermissionLevel.byId(level)), Component.literal("x"), null);
    }

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
        CommandBuildContext context = Commands.createValidationContext(VanillaRegistries.createWorldLookup());
        Commands commands = new Commands(Commands.CommandSelection.DEDICATED, context);
        dispatcher = commands.getDispatcher();
        restrictTime();
        try (PrintWriter writer = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            out = writer;
            parses();
            usages();
            packets();
            renders();
            unknowns(commands);
        }
    }

    // "time of <clock> ..." and "time query <timeline> ..." are not ported: removed from the tree.
    @SuppressWarnings("unchecked")
    private static void restrictTime() throws Exception {
        CommandNode<CommandSourceStack> time = dispatcher.getRoot().getChild("time");
        removeChild(time, "of");
        removeChild(time.getChild("query"), "timeline");
    }

    @SuppressWarnings("unchecked")
    private static void removeChild(CommandNode<CommandSourceStack> node, String name) throws Exception {
        for (String field : new String[] {"children", "literals", "arguments"}) {
            Field f = CommandNode.class.getDeclaredField(field);
            f.setAccessible(true);
            ((Map<String, ?>) f.get(node)).remove(name);
        }
    }

    private static String text(Object arg) {
        return arg instanceof Component c ? c.getString() : String.valueOf(arg);
    }

    private static void parses() {
        for (int level : new int[] {0, 2, 3, 4}) {
            for (String input : INPUTS) {
                ParseResults<CommandSourceStack> parse = dispatcher.parse(input, source(level, CommandSource.NULL));
                JsonObject o = kind("parse");
                o.addProperty("level", level);
                o.addProperty("input", input);
                CommandSyntaxException error = Commands.getParseException(parse);
                if (error == null) {
                    try {
                        com.mojang.brigadier.context.ContextChain.tryFlatten(parse.getContext().build(input))
                            .orElseThrow(() -> CommandSyntaxException.BUILT_IN_EXCEPTIONS.dispatcherUnknownCommand().createWithContext(parse.getReader()));
                    } catch (CommandSyntaxException e) {
                        error = e;
                    }
                }
                if (error != null) {
                    o.add("error", errorRow(error));
                } else {
                    JsonObject arguments = new JsonObject();
                    CommandContextBuilder<CommandSourceStack> last = parse.getContext().getLastChild();
                    for (Map.Entry<String, ParsedArgument<CommandSourceStack, ?>> e : last.getArguments().entrySet()) {
                        Object value = e.getValue().getResult();
                        arguments.addProperty(e.getKey(), value instanceof GameProfileArgument.Result ? "<profile>"
                            : value instanceof net.minecraft.commands.arguments.ResourceOrTagKeyArgument.Result<?> key ? key.asPrintable()
                            : value instanceof net.minecraft.commands.arguments.ResourceOrTagArgument.Result<?> tag ? tag.asPrintable()
                            : String.valueOf(value));
                    }
                    o.add("ok", arguments);
                }
                row(o);
            }
        }
    }

    private static JsonObject errorRow(CommandSyntaxException e) {
        JsonObject err = new JsonObject();
        ComponentContents contents = ((Component) e.getRawMessage()).getContents();
        TranslatableContents tc = (TranslatableContents) contents;
        err.addProperty("key", tc.getKey());
        JsonArray args = new JsonArray();
        for (Object a : tc.getArgs()) {
            args.add(text(a));
        }
        err.add("args", args);
        err.addProperty("cursor", e.getCursor());
        err.addProperty("context", e.getContext());
        return err;
    }

    private static void usages() {
        for (int level : new int[] {0, 2, 3, 4}) {
            CommandSourceStack source = source(level, CommandSource.NULL);
            for (String name : PORTED) {
                CommandNode<CommandSourceStack> node = dispatcher.getRoot().getChild(name);
                if (!node.canUse(source)) {
                    continue;
                }
                for (boolean restricted : new boolean[] {false, true}) {
                    JsonObject o = kind("usage");
                    o.addProperty("level", level);
                    o.addProperty("command", name);
                    o.addProperty("restricted", restricted);
                    JsonArray lines = new JsonArray();
                    for (String line : dispatcher.getAllUsage(node, source, restricted)) {
                        lines.add(line);
                    }
                    o.add("lines", lines);
                    row(o);
                }
            }
        }
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static void packets() throws Exception {
        Method fill = Commands.class.getDeclaredMethod("fillUsableCommands", CommandNode.class, CommandNode.class, Object.class, Map.class);
        fill.setAccessible(true);
        Field inspectorField = Commands.class.getDeclaredField("COMMAND_NODE_INSPECTOR");
        inspectorField.setAccessible(true);
        ClientboundCommandsPacket.NodeInspector inspector = (ClientboundCommandsPacket.NodeInspector) inspectorField.get(null);
        for (int level = 0; level <= 4; level++) {
            RootCommandNode<CommandSourceStack> subset = new RootCommandNode<>();
            for (String name : PORTED) {
                subset.addChild(dispatcher.getRoot().getChild(name));
            }
            RootCommandNode<CommandSourceStack> result = new RootCommandNode<>();
            Map<CommandNode<CommandSourceStack>, CommandNode<CommandSourceStack>> map = new HashMap<>();
            map.put(subset, result);
            fill.invoke(null, subset, result, source(level, CommandSource.NULL), map);
            ClientboundCommandsPacket packet = new ClientboundCommandsPacket(result, inspector);
            ByteBuf buffer = Unpooled.buffer();
            ClientboundCommandsPacket.STREAM_CODEC.encode(new FriendlyByteBuf(buffer), packet);
            byte[] bytes = new byte[buffer.readableBytes()];
            buffer.readBytes(bytes);
            StringBuilder hex = new StringBuilder();
            for (byte b : bytes) {
                hex.append(String.format("%02x", b));
            }
            JsonObject o = kind("packet");
            o.addProperty("level", level);
            o.addProperty("hex", hex.toString());
            row(o);
        }
    }

    private static void renders() {
        for (Object[] r : RENDERS) {
            JsonObject o = kind("render");
            o.addProperty("key", (String) r[0]);
            JsonArray args = new JsonArray();
            List<Object> values = new ArrayList<>();
            for (int i = 1; i < r.length; i += 2) {
                String type = (String) r[i];
                Object value = r[i + 1];
                JsonArray pair = new JsonArray();
                pair.add(type);
                pair.add(String.valueOf(value));
                args.add(pair);
                values.add(type.equals("c") ? Component.translatable((String) value) : value);
            }
            o.add("args", args);
            o.addProperty("text", Component.translatable((String) r[0], values.toArray()).getString());
            row(o);
        }
    }

    // The jar's finishParsing sends the error message, then the context; Leaf sends them as one
    // message with a new line between.
    private static void unknowns(Commands commands) {
        for (String input : INPUTS) {
            List<String> messages = new ArrayList<>();
            CommandSource capture = new CommandSource() {
                @Override
                public void sendSystemMessage(Component message) {
                    messages.add(message.getString());
                }

                @Override
                public boolean acceptsSuccess() {
                    return true;
                }

                @Override
                public boolean acceptsFailure() {
                    return true;
                }

                @Override
                public boolean shouldInformAdmins() {
                    return false;
                }
            };
            ParseResults<CommandSourceStack> parse = dispatcher.parse(input, source(4, capture));
            if (Commands.getParseException(parse) == null) {
                try {
                    if (com.mojang.brigadier.context.ContextChain.tryFlatten(parse.getContext().build(input)).isPresent()) {
                        continue;
                    }
                } catch (RuntimeException e) {
                    continue;
                }
            }
            commands.performPrefixedCommand(source(4, capture), input);
            JsonObject o = kind("unknown");
            o.addProperty("input", input);
            o.addProperty("text", String.join("\n", messages));
            row(o);
        }
    }
}
