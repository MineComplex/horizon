#!/usr/bin/env python3
"""Plugins end to end (docs/plugins.md): a real dedicated server with the
essentials example in plugins/, scripted clients (horizon-bots' plugin_client
example) and the server console.

    cargo build --profile fast -p horizon-client --features mods --bins
    cargo build -p horizon-bots --example plugin_client
    (cd crates/horizon-mod-sdk/examples/essentials && cargo horizon-mod build)
    python3 tools/test_plugins_e2e.py [--keep]

Every run uses fresh temporary folders for the server and the game's data.
"""
import argparse
import os
import queue
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Server:
    def __init__(self, game, resources, folder, port, env):
        self.lines = []
        self.queue = queue.Queue()
        self.proc = subprocess.Popen(
            [game, "--resources", resources, "--dedicated", "--nogui", "--port", str(port)],
            cwd=folder,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            env=env,
            text=True,
            bufsize=1,
        )
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.proc.stdout:
            line = line.rstrip("\n")
            self.lines.append(line)
            self.queue.put(line)

    def wait_for(self, pattern, timeout=180):
        deadline = time.time() + timeout
        rx = re.compile(pattern)
        for line in self.lines:
            if rx.search(line):
                return line
        while time.time() < deadline:
            try:
                line = self.queue.get(timeout=0.5)
            except queue.Empty:
                if self.proc.poll() is not None:
                    raise RuntimeError("the server exited:\n" + "\n".join(self.lines[-40:]))
                continue
            if rx.search(line):
                return line
        raise RuntimeError(f"no {pattern!r} in the log:\n" + "\n".join(self.lines[-40:]))

    def console(self, line):
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()

    def stop(self):
        if self.proc.poll() is None:
            try:
                self.console("stop")
                self.proc.wait(timeout=60)
            except Exception:
                self.proc.kill()


def client(binary, port, name, steps, timeout=60):
    args = [binary, "--server", f"127.0.0.1:{port}", "--name", name]
    for s in steps:
        args += ["--step", s]
    out = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    return out.stdout.splitlines()


class Checks:
    def __init__(self):
        self.failed = []
        self.passed = 0

    def check(self, what, ok, detail=""):
        if ok:
            self.passed += 1
            print(f"ok   {what}")
        else:
            self.failed.append(what)
            print(f"FAIL {what}\n     {detail}")


def received(lines, kind):
    prefix = f"RECV {kind}: "
    return [l[len(prefix):] for l in lines if l.startswith(prefix)]


def after(lines, step):
    """The lines after the client's `STEP <step>` line, up to its next step
    that isn't a wait (the answers arrive while the client waits)."""
    out, on = [], False
    for l in lines:
        if l.startswith("STEP "):
            if l == f"STEP {step}":
                on = True
            elif not l.startswith("STEP wait"):
                on = False
            continue
        if on:
            out.append(l)
    return out


def block_events(o, c, server, port):
    """Pistons, fluids and growth fire their block events while a player
    keeps the spawn's chunks ticking; a cancelled piston stays put."""
    out = []
    run = threading.Thread(
        target=lambda: out.extend(client(o.client, port, "Builder", ["wait 9000"], timeout=60))
    )
    run.start()
    server.wait_for(r"EVENT player-join Builder", timeout=30)
    for line in (
        "setblock 8 80 8 minecraft:piston[facing=east]",
        "setblock 9 80 8 minecraft:stone",
        "setblock 8 80 7 minecraft:redstone_block",
    ):
        server.console(line)
    time.sleep(1.0)
    server.console("setblock 8 80 7 minecraft:air")
    server.console("eventlog cancel block-piston-extend")
    for line in (
        "setblock 8 80 4 minecraft:piston[facing=east]",
        "setblock 9 80 4 minecraft:stone",
        "setblock 8 80 3 minecraft:redstone_block",
    ):
        server.console(line)
    time.sleep(1.0)
    server.console("execute if block 9 80 4 minecraft:stone")
    server.console("eventlog allow block-piston-extend")
    # Water poured in the air flows down.
    server.console("setblock 8 90 12 minecraft:water")
    # Wheat about to ripen, with the random ticks sped up.
    server.console("fill 12 79 0 20 79 8 minecraft:farmland")
    server.console("fill 12 80 0 20 80 8 minecraft:wheat[age=6]")
    server.console("gamerule random_tick_speed 300")
    time.sleep(3.0)
    server.console("gamerule random_tick_speed 3")
    run.join()
    log = "\n".join(server.lines)

    def saw(what, pattern):
        c.check(f"the server fires {what}", re.search(pattern, log) is not None, pattern)

    saw("block-piston-extend", r"EVENT block-piston-extend 8 80 8 east 1 blocks(?! \(cancelled\))")
    saw("block-piston-retract", r"EVENT block-piston-retract 8 80 8 east 0 blocks")
    saw("block-piston-extend cancelled", r"EVENT block-piston-extend 8 80 4 east 1 blocks \(cancelled\)")
    c.check(
        "a cancelled piston leaves the block where it was",
        re.search(r"Test passed", log) is not None,
        [x for x in server.lines if "Test " in x],
    )
    saw("block-from-to for water flowing down", r"EVENT block-from-to minecraft:water\S* 8 90 12 -> 8 89 12 down")
    saw("block-grow for ripening wheat", r"EVENT block-grow minecraft:wheat\[age=6\] -> minecraft:wheat\[age=7\]")


def events(o, c, server, port):
    """The event-log plugin logs every event it hears (`EVENT <kind> ...`):
    one player does what fires each hook the client can reach."""
    out = []
    run = threading.Thread(
        target=lambda: out.extend(
            client(o.client, port, "Tester", [
                "wait 2500",
                "slot 3",
                "wait 300",
                "sneak on",
                "wait 300",
                "sneak off",
                "sprint on",
                "wait 300",
                "move 1.5 64 1.5",
                "wait 400",
                "use ~ ~-1 ~ 1",
                "wait 400",
                "chat first words",
                "wait 400",
                "cmd eventlog rewrite on",
                "wait 300",
                "chat second words",
                "wait 400",
                "cmd eventlog cancel player-chat",
                "wait 300",
                "chat third words",
                "wait 400",
                "cmd menu",
                "wait 500",
                "click 0",
                "wait 300",
                "close",
                "wait 300",
                "slot 0",
                "wait 300",
                "drop",
                "wait 300",
                "swap",
                "wait 4000",
                "respawn",
                "wait 1500",
            ], timeout=90)
        )
    )
    run.start()
    server.wait_for(r"EVENT player-join Tester", timeout=30)
    # Opped once on (an offline server's `op` of a name it hasn't seen is
    # the lower-case name's player, as vanilla's).
    server.console("op Tester")
    server.console("give Tester minecraft:dirt 16")
    server.wait_for(r"EVENT player-swap-hands Tester", timeout=60)
    # While a player keeps the spawn's chunks ticking.
    server.console("summon minecraft:zombie 0 70 0")
    # Burning: the surroundings hurt it once a second (entity-damage on_fire).
    server.console("summon minecraft:pig 2 70 0 {Fire:200s}")
    # And the player standing in fire.
    server.console("execute at Tester run setblock ~ ~-1 ~ minecraft:netherrack")
    server.console("execute at Tester run setblock ~ ~ ~ minecraft:fire")
    server.console("summon minecraft:tnt 0 70 6 {fuse:1}")
    time.sleep(1.5)
    server.console("kill Tester")
    run.join()
    log = "\n".join(server.lines)

    def saw(what, pattern):
        c.check(f"the server fires {what}", re.search(pattern, log) is not None, pattern)

    saw("plugin-state for each plugin", r"EVENT plugin-state event_log true")
    saw("player-login", r"EVENT player-login Tester")
    saw("player-join", r"EVENT player-join Tester first=true")
    saw("server-command for console lines", r"EVENT server-command give Tester")
    saw("player-item-held", r"EVENT player-item-held Tester 0 -> 3")
    saw("player-toggle-sneak", r"EVENT player-toggle-sneak Tester true")
    saw("player-toggle-sprint", r"EVENT player-toggle-sprint Tester true")
    saw("player-move", r"EVENT player-move Tester 1\.5 64\.0 1\.5")
    saw("player-interact on a block", r"EVENT player-interact Tester \S*RightClickBlock")
    saw("player-chat", r"EVENT player-chat Tester: first words")
    saw("player-chat changed by a plugin", r"EVENT player-chat Tester: SECOND WORDS")
    saw("player-chat cancelled", r"EVENT player-chat Tester: THIRD WORDS \(cancelled\)")
    saw("player-command", r"EVENT player-command Tester: /menu")
    saw("inventory-click", r"EVENT inventory-click Tester minecraft:generic_9x3 slot 0")
    saw("inventory-close", r"EVENT inventory-close Tester minecraft:generic_9x3")
    saw("player-drop-item", r"EVENT player-drop-item Tester minecraft:dirt")
    saw("player-swap-hands", r"EVENT player-swap-hands Tester")
    saw("player-death", r"EVENT player-death Tester")
    saw("entity-death for a player", r"EVENT entity-death minecraft:player")
    saw("player-respawn", r"EVENT player-respawn Tester")
    chat = [x for x in received(out, "system") if "words" in x.lower()]
    c.check(
        "the client got the changed chat and not the cancelled one",
        any("SECOND WORDS" in x for x in chat) and not any("THIRD" in x for x in chat),
        chat,
    )
    saw("entity-spawn for /summon", r"EVENT entity-spawn minecraft:zombie")
    saw("entity-damage for a burning mob", r"EVENT entity-damage minecraft:pig minecraft:on_fire 1\.0 by -")
    saw("entity-damage for a player in fire", r"EVENT entity-damage minecraft:player minecraft:in_fire 1\.0 by -")
    # A hit lands every 10 ticks at most (LivingEntity.hurtServer), so about
    # three in the second and a half in the fire, not one a tick.
    c.check(
        "no entity-damage for the surroundings inside the hurt cooldown",
        len(re.findall(r"EVENT entity-damage minecraft:player minecraft:in_fire", log)) <= 6,
        re.findall(r"EVENT entity-damage minecraft:player minecraft:in_fire", log),
    )
    saw("entity-explode", r"EVENT entity-explode \d+ blocks")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--game", default=os.path.join(ROOT, "target/fast/horizon-client"))
    p.add_argument("--client", default=os.path.join(ROOT, "target/debug/examples/plugin_client"))
    p.add_argument("--plugin", default=os.path.join(ROOT, "target/horizon-mod/essentials.hmod"))
    p.add_argument("--event-log", default=os.path.join(ROOT, "target/horizon-mod/event_log.hmod"))
    p.add_argument("--resources", default=os.path.join(ROOT, ".local/horizon-client/26.3"))
    p.add_argument("--keep", action="store_true", help="keep the server folder")
    o = p.parse_args()
    for f in (o.game, o.client, o.plugin, o.event_log):
        if not os.path.exists(f):
            sys.exit(f"missing {f}: build it first (see the docstring)")
    folder = tempfile.mkdtemp(prefix="horizon-plugins-e2e-")
    home = tempfile.mkdtemp(prefix="horizon-plugins-home-")
    port = free_port()
    with open(os.path.join(folder, "eula.txt"), "w") as f:
        f.write("eula=true\n")
    with open(os.path.join(folder, "server.properties"), "w") as f:
        f.write(
            "online-mode=false\nwhite-list=false\nenforce-secure-profile=false\n"
            f"server-port={port}\nlevel-seed=12345\nview-distance=4\nsimulation-distance=4\n"
            "spawn-protection=0\ngamemode=survival\nmotd=Plugins test\n"
        )
    os.makedirs(os.path.join(folder, "plugins"))
    shutil.copy(o.plugin, os.path.join(folder, "plugins"))
    shutil.copy(o.event_log, os.path.join(folder, "plugins"))
    env = dict(os.environ)
    for k in ("HORIZON_HOME", "HORIZON_LAUNCHER_HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME"):
        env[k] = tempfile.mkdtemp(prefix="horizon-plugins-" + k.lower() + "-", dir=home)
    env["HORIZON_NO_AUDIO"] = "1"
    env["HORIZON_LAUNCHER_NO_SELF_UPDATE"] = "1"
    c = Checks()
    server = Server(o.game, o.resources, folder, port, env)
    try:
        server.wait_for(r"Done \(")
        # The plugins are enabled on the first tick after the world is up.
        line = server.wait_for(r"Essentials enabled", timeout=60)
        c.check("the plugin is enabled on the server", "Essentials enabled" in line, line)
        server.console("plugins")
        line = server.wait_for(r"Plugins \(2\)")
        c.check(
            "/plugins lists both from the console",
            "Essentials" in line and "Event Log" in line,
            line,
        )

        steve = client(o.client, port, "Steve", [
            "wait 2500",
            "move 2.5 64 2.5",
            "wait 300",
            "cmd spawn",
            "wait 600",
            "cmd heal",
            "wait 600",
            "complete gamemode cr",
            "wait 600",
            "chat hello from steve",
            "wait 800",
            "cmd menu",
            "wait 800",
            "click 0",
            "wait 600",
            "move 2.5 64 2.5",
            "wait 300",
            "click 11",
            "wait 800",
            "cmd tps",
            "wait 800",
        ])
        joined = after(steve, "wait 2500")
        c.check("the client joined", "JOINED Steve" in steve, steve[:3])
        commands = received(steve, "commands")
        c.check(
            "plugin commands are in the command tree",
            any("spawn" in x.split(",") and "menu" in x.split(",") and "gm" in x.split(",") for x in commands),
            commands[:1],
        )
        c.check(
            "the join message is the plugin's",
            "+ Steve" in received(joined, "system"),
            received(joined, "system"),
        )
        c.check("the welcome title shows", "Welcome" in received(joined, "title"), received(joined, "title"))
        c.check(
            "the welcome boss bar shows",
            "Welcome to Horizon" in received(joined, "bossbar add"),
            received(joined, "bossbar add"),
        )
        c.check(
            "the sidebar shows",
            any("Horizon" in x for x in received(steve, "objective")),
            received(steve, "objective")[:3],
        )
        spawn = after(steve, "cmd spawn")
        c.check(
            "/spawn answers and teleports",
            "Teleported to the spawn." in received(spawn, "system") and received(spawn, "teleport"),
            spawn,
        )
        heal = after(steve, "cmd heal")
        c.check(
            "/heal is refused without the permission",
            any("do not have permission" in x for x in received(heal, "system")),
            heal,
        )
        complete = after(steve, "complete gamemode cr")
        c.check(
            "/gamemode offers nothing without its permission",
            received(complete, "suggestions") == [""],
            complete,
        )
        chat = after(steve, "chat hello from steve")
        c.check(
            "chat goes out in the plugin's format",
            "Steve: hello from steve" in received(chat, "system"),
            chat,
        )
        menu = after(steve, "cmd menu")
        c.check(
            "/menu opens a 3-row chest titled by the plugin",
            any(x.endswith("Server menu") for x in received(menu, "open"))
            and any("slots 63" in x and "filled 27" in x for x in received(menu, "content")),
            menu,
        )
        c.check(
            "the menu's items carry their names, lore and glint to the client",
            any(re.search(r"named 27 lore 1 glint 1", x) for x in received(menu, "content")),
            received(menu, "content"),
        )
        filler = after(steve, "click 0")
        c.check(
            "a click on the menu is cancelled and the menu sent again",
            any("filled 27" in x for x in received(filler, "content")),
            filler,
        )
        action = after(steve, "click 11")
        c.check(
            "the menu's spawn button closes it and acts",
            "Welcome back to the spawn." in received(action, "system") and received(action, "teleport"),
            action,
        )
        tps = after(steve, "cmd tps")
        c.check(
            "/tps reads the server's tick rate",
            any(x.startswith("TPS from last 1m, 5m, 15m:") for x in received(tps, "system")),
            tps,
        )
        line = server.wait_for(r"\[Not Secure\] Steve: hello from steve")
        c.check("the console logs the formatted chat", True, line)

        # The console: op, a plugin command as the console, broadcast.
        server.console("op Steve")
        server.wait_for(r"Made Steve a server operator|Opped|op", timeout=20)
        server.console("bc Restart soon")
        line = server.wait_for(r"\[Broadcast\] Restart soon", timeout=20)
        c.check("a plugin command runs from the console", "Restart soon" in line, line)
        server.console("spawn")
        line = server.wait_for(r"Only players can do that", timeout=20)
        c.check("a player-only command answers the console", True, line)

        op = client(o.client, port, "Steve", [
            "wait 2500",
            "cmd heal",
            "wait 600",
            "complete gamemode cr",
            "wait 600",
            "cmd fly",
            "wait 600",
            "cmd gm creative",
            "wait 800",
        ])
        heal = after(op, "cmd heal")
        c.check("an operator may /heal", "You were healed." in received(heal, "system"), heal)
        complete = after(op, "complete gamemode cr")
        c.check(
            "/gamemode completes from the plugin",
            received(complete, "suggestions") == ["creative"],
            complete,
        )
        gm = after(op, "cmd gm creative")
        c.check(
            "/gm creative changes the game mode",
            "1" in received(gm, "gamemode") and any("game mode" in x for x in received(gm, "system")),
            gm,
        )
        fly = after(op, "cmd fly")
        c.check("/fly answers", "You can fly." in received(fly, "system"), fly)

        # Alex is new: the plugin sends a newcomer to the spawn. In creative
        # a left click breaks at once, so the break event decides.
        alex_run = threading.Thread(
            target=lambda: alex_out.extend(
                client(o.client, port, "Alex", ["wait 4000", "dig 0 63 0", "wait 1000"])
            )
        )
        alex_out = []
        alex_run.start()
        server.wait_for(r"Alex joined the game", timeout=30)
        time.sleep(1.0)
        server.console("setblock 0 63 0 minecraft:stone")
        server.console("gamemode creative Alex")
        alex_run.join()
        alex = alex_out
        dig = after(alex, "dig 0 63 0")
        stone_back = [b for b in received(dig, "block") if b.startswith("0 63 0 ") and not b.endswith(" 0")]
        c.check(
            "breaking at the spawn is refused and the block sent back",
            "The spawn is protected." in received(dig, "actionbar") and stone_back,
            dig,
        )
        line = server.wait_for(r"Alex", timeout=10)
        c.check("the server logged Alex", True, line)
        events(o, c, server, port)
        block_events(o, c, server, port)
        server.console("stop")
        server.wait_for(r"Disabling Essentials", timeout=60)
        c.check("the plugin is disabled when the server stops", True)
        server.proc.wait(timeout=60)
        homes = os.path.join(folder, "plugins", "essentials", "config.yml")
        c.check("the default config was written to plugins/essentials/", os.path.isfile(homes), homes)
    except Exception as e:  # noqa: BLE001 - the report says what broke
        c.check("the run finished", False, str(e))
    finally:
        server.stop()
        log = os.path.join(folder, "logs", "latest.log")
        if c.failed and os.path.exists(log):
            print("--- server log tail ---")
            with open(log) as f:
                print("".join(f.readlines()[-60:]))
        if not o.keep:
            shutil.rmtree(folder, ignore_errors=True)
            shutil.rmtree(home, ignore_errors=True)
        else:
            print(f"kept {folder}")
    print(f"{c.passed} passed, {len(c.failed)} failed")
    sys.exit(1 if c.failed else 0)


if __name__ == "__main__":
    main()
