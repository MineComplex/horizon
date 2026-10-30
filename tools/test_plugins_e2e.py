#!/usr/bin/env python3
"""horizon-server's plugin API end to end: a real horizon-server built with the example plugin
(crates/horizon-plugin-example), and a real horizon-client on it through horizon-ctl.

Checks, from the client's side:
  enable    the server log enables the plugin before it is Done
  join      PlayerJoinEvent: the plugin's join message and its greeting reach the player
  command   /hello and its alias /hi, a plugin's command with arguments
  plugins   /plugins lists the plugin
  chat      AsyncPlayerChatEvent: `!hey` comes back in the plugin's shout format
  break     BlockBreakEvent: bedrock the player breaks stays, and the player is told
  drop      PlayerDropItemEvent: a diamond the player throws stays in the hand
  mode      PlayerGameModeChangeEvent: the player hears of their new game mode
  damage    EntityDamageEvent: a survival player's fall hurts nothing, and the player is told
  teleport  PlayerTeleportEvent: /tp tells the player its cause
  spawn     CreatureSpawnEvent: a summoned phantom never comes, a pig does
  death     EntityDeathEvent: a killed pig is announced
  explode   EntityExplodeEvent: TNT breaks no blocks
  loaded    ServerLoadEvent: the plugin's line in the log once the server started
  hunger    FoodLevelChangeEvent: a survival player under Hunger 255 keeps 20 food
  weather   WeatherChangeEvent: /weather rain is taken back, and everyone is told
  time      TimeSkipEvent: /time add 1000 leaves the time as it was
  sponge    SpongeAbsorbEvent: a sponge set in water stays dry, the water stays
  tnt       TNTPrimeEvent: a redstone block next to TNT lights nothing
  effect    EntityPotionEffectEvent: Speed is refused, and the player is told
  furnace   FurnaceSmeltEvent: raw iron smelts into a gold ingot
  falling   EntityChangeBlockEvent: falling sand lands as nothing
  quick     FurnaceStartSmeltEvent: the furnace's cook time is 20 ticks, not 200
  unknown   UnknownCommandEvent: the plugin's own line for a command that doesn't exist
  minecart  VehicleCreateEvent: a summoned minecart never comes
  disable   the server log disables the plugin as it stops

    python3 tools/test_plugins_e2e.py [--bin-dir target/fast]
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402

ROOT = lab.ROOT


def build():
    for args in (['-p', 'horizon-server', '--features', 'example-plugin'],
                 ['-p', 'horizon-client', '--bin', 'horizon-client', '--bin', 'horizon-ctl']):
        subprocess.run(['cargo', 'build', '--locked', '--profile', 'fast', *args], cwd=ROOT, check=True)
    meta = json.loads(subprocess.run(['cargo', 'metadata', '--format-version', '1', '--no-deps'], cwd=ROOT,
                                     capture_output=True, text=True, check=True).stdout)
    return Path(meta['target_directory']) / 'fast'


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin-dir', help='prebuilt horizon-server (with example-plugin), horizon-client, horizon-ctl')
    args = parser.parse_args()
    bins = Path(args.bin_dir).resolve() if args.bin_dir else build()
    home = Path(tempfile.mkdtemp(prefix='hzplug-'))
    env = lab.game_env(HORIZON_HOME=home)
    socket = home / 'c.sock'

    def ctl(*words, check=True):
        r = subprocess.run([str(bins / 'horizon-ctl'), '--socket', str(socket), *map(str, words)], cwd=home,
                           env=env, capture_output=True, text=True, timeout=600)
        out = r.stdout.strip().splitlines()
        reply = json.loads(out[-1]) if out else {'error': r.stderr.strip()}
        if check and (r.returncode or 'error' in reply):
            raise AssertionError(f'horizon-ctl {" ".join(map(str, words))}: {reply}')
        return reply

    def chat_until(text, ticks=100):
        for _ in range(ticks // 5):
            lines = ctl('chat-log', 30)['chat']
            if any(text in line for line in lines):
                return lines
            ctl('wait', 5)
        raise AssertionError(f'{text!r} never showed in the chat: {lines}')

    server_log = socket.with_name(socket.name + '.server.log')
    results = {}

    def check(name, fn):
        try:
            results[name] = fn() or True
            print(f'[plugins] {name}: PASS')
        except AssertionError as e:
            results[name] = False
            print(f'[plugins] {name}: FAIL {e}')

    try:
        ctl('launch', '--resources', lab.RESOURCES.resolve(), '--world', 'Plugins', '--seed', 5, '--creative',
            '--render-distance', 4, '--offscreen', '--no-pause', '--name', 'Player', '--wait')

        def enable():
            log = server_log.read_text(errors='replace')
            assert '[Example] Enabling Example v1.0' in log, log[-2000:]
            assert log.index('Enabling Example') < log.index('Done ('), 'enabled after Done'
        check('enable', enable)
        check('join', lambda: chat_until('+ Player') and chat_until('Welcome to the server!'))

        def command():
            ctl('chat', '/hello')
            chat_until('Hello, Player!')
            ctl('chat', '/hi Bob')
            chat_until('Hello, Bob!')
        check('command', command)

        def plugins():
            ctl('chat', '/plugins')
            chat_until('Plugins (1): Example')
        check('plugins', plugins)

        def chat():
            ctl('chat', '!hey there')
            chat_until('[Shout] Player: hey there')
        check('chat', chat)

        def bedrock():
            x, y, z = ctl('state')['player']['block_pos']
            target = (x + 2, y, z)
            ctl('command', f'setblock {target[0]} {target[1]} {target[2]} minecraft:bedrock')
            ctl('wait', 10)
            ctl('look-at-block', *target)
            ctl('break', *target, '--timeout', 5, check=False)
            ctl('wait', 10)
            chat_until('Bedrock stays.')
            block = ctl('block', *target)['block']
            assert block == 'minecraft:bedrock', f'the bedrock is {block}'
        check('break', bedrock)

        def held_until(count, ticks=200):
            # The server's inventory packets come when it gets to them.
            for _ in range(ticks // 5):
                held = ctl('state')['held']
                if held and held['item'] == 'minecraft:diamond' and held['count'] == count:
                    return held
                ctl('wait', 5)
            raise AssertionError(f'never held {count} diamonds: {held}')

        def drop():
            ctl('give', 'minecraft:diamond', 3)
            for _ in range(40):
                if ctl('select', 'minecraft:diamond', check=False).get('ok'):
                    break
                ctl('wait', 5)
            held_until(3)
            ctl('drop')
            chat_until('Diamonds stay in your hand.')
            held_until(3)
        check('drop', drop)

        def mode():
            ctl('chat', '/gamemode survival')
            chat_until('Game mode 0.')
        check('mode', mode)

        def damage():
            before = ctl('state')['player']['health']
            ctl('command', 'tp @s ~ ~12 ~')
            chat_until('Soft landing (', ticks=200)
            ctl('wait', 10)
            after = ctl('state')['player']['health']
            assert after == before, f'the fall hurt: {before} -> {after}'
        check('damage', damage)

        def teleport():
            ctl('command', 'tp @s ~ ~ ~')
            chat_until('Teleported (Command).')
        check('teleport', teleport)

        def mobs_near():
            return [m.get('kind', m.get('type', '')) for m in ctl('mobs', 32)['near']]

        def spawn():
            ctl('command', 'kill @e[type=!player]', check=False)
            ctl('command', 'summon minecraft:phantom ~ ~3 ~', check=False)
            ctl('command', 'summon minecraft:pig ~2 ~ ~')
            ctl('wait', 10)
            kinds = mobs_near()
            assert not any('phantom' in k for k in kinds), kinds
            assert any('pig' in k for k in kinds), kinds
        check('spawn', spawn)

        def death():
            ctl('command', 'kill @e[type=minecraft:pig]')
            chat_until('minecraft:pig died.')
        check('death', death)

        def explode():
            x, y, z = ctl('state')['player']['block_pos']
            bx = x + 8
            ctl('command', f'fill {bx - 1} {y - 1} {z - 1} {bx + 1} {y - 1} {z + 1} minecraft:stone')
            ctl('command', f'summon minecraft:tnt {bx} {y} {z} {{fuse:20}}')
            ctl('wait', 60)
            for dx in (-1, 0, 1):
                for dz in (-1, 0, 1):
                    block = ctl('block', bx + dx, y - 1, z + dz)['block']
                    assert block == 'minecraft:stone', f'({bx + dx}, {y - 1}, {z + dz}) is {block}'
        check('explode', explode)

        def loaded():
            log = server_log.read_text(errors='replace')
            assert '[Example] Server loaded.' in log, log[-2000:]
        check('loaded', loaded)

        def hunger():
            ctl('command', 'gamemode survival')
            ctl('command', 'effect give @s minecraft:hunger 10 255')
            ctl('wait', 80)
            food = ctl('state')['player']['food']
            assert food == 20, f'food went down to {food}'
            ctl('command', 'effect clear @s')
        check('hunger', hunger)

        def weather():
            ctl('command', 'weather rain')
            chat_until('Rain stays away.')
        check('weather', weather)

        def daytime():
            return ctl('status')['day_time']

        def time_skip():
            before = daytime()
            ctl('command', 'time add 1000', check=False)
            ctl('wait', 2)
            after = daytime()
            assert abs(after - before) < 100, f'the time moved: {before} -> {after}'
        check('time', time_skip)

        x, y, z = ctl('state')['player']['block_pos']

        def sponge():
            sx = x - 8
            ctl('command', f'fill {sx - 2} {y} {z - 2} {sx + 2} {y} {z + 2} minecraft:water')
            ctl('wait', 5)
            ctl('command', f'setblock {sx} {y} {z} minecraft:sponge')
            chat_until('The sponge stays dry (')
            ctl('wait', 5)
            assert ctl('block', sx, y, z)['block'] == 'minecraft:sponge'
            assert ctl('block', sx + 1, y, z)['block'].startswith('minecraft:water')
            ctl('command', f'fill {sx - 2} {y} {z - 2} {sx + 2} {y} {z + 2} minecraft:air')
        check('sponge', sponge)

        def tnt():
            tz = z + 8
            ctl('command', f'setblock {x} {y} {tz} minecraft:tnt')
            ctl('command', f'setblock {x + 1} {y} {tz} minecraft:redstone_block')
            ctl('wait', 20)
            assert ctl('block', x, y, tz)['block'] == 'minecraft:tnt', 'the TNT went'
            ctl('command', f'fill {x} {y} {tz} {x + 1} {y} {tz} minecraft:air')
        check('tnt', tnt)

        def effect():
            ctl('command', 'effect give @s minecraft:speed 30')
            chat_until('No speed here.')
        check('effect', effect)

        def furnace():
            fz = z - 8
            items = '[{Slot:0b,id:"minecraft:raw_iron",count:1},{Slot:1b,id:"minecraft:coal",count:1}]'
            ctl('command', f'setblock {x} {y} {fz} minecraft:furnace{{Items:{items}}}')
            chat = []
            for _ in range(30):
                ctl('wait', 20)
                chat = ctl('command', f'data get block {x} {y} {fz} Items', check=False).get('chat', [])
                if any('gold_ingot' in line for line in chat):
                    return
            raise AssertionError(f'no gold ingot: {chat}')
        check('furnace', furnace)

        def falling():
            fx = x + 4
            ctl('command', f'setblock {fx} {y - 1} {z} minecraft:stone')
            ctl('command', f'setblock {fx} {y} {z} minecraft:air')
            ctl('command', f'summon minecraft:falling_block {fx} {y + 5} {z} {{BlockState:{{Name:"minecraft:sand"}}}}')
            ctl('wait', 60)
            block = ctl('block', fx, y, z)['block']
            assert block != 'minecraft:sand', 'the sand landed'
        check('falling', falling)

        def quick():
            qz = z - 12
            items = '[{Slot:0b,id:"minecraft:raw_iron",count:1},{Slot:1b,id:"minecraft:coal",count:1}]'
            ctl('command', f'setblock {x} {y} {qz} minecraft:furnace{{Items:{items}}}')
            ctl('wait', 10)
            chat = ctl('command', f'data get block {x} {y} {qz} cooking_total_time', check=False).get('chat', [])
            assert any(line.rstrip().endswith(': 20') for line in chat), chat
        check('quick', quick)

        def unknown():
            ctl('chat', '/nosuchcommand here')
            chat_until('No such command: /nosuchcommand')
        check('unknown', unknown)

        def minecart():
            before = len(ctl('status').get('carts', []))
            ctl('command', f'summon minecraft:minecart {x + 2} {y} {z + 2}', check=False)
            ctl('wait', 10)
            after = len(ctl('status').get('carts', []))
            assert after == before, f'carts {before} -> {after}'
        check('minecart', minecart)
    finally:
        ctl('quit', check=False)
        deadline = time.time() + 60
        while time.time() < deadline and 'Disabling Example' not in server_log.read_text(errors='replace'):
            time.sleep(0.5)
    log = server_log.read_text(errors='replace')
    results['disable'] = '[Example] Disabling Example v1.0' in log
    print(f'[plugins] disable: {"PASS" if results["disable"] else "FAIL"}')
    if not all(results.values()):
        print(log[-3000:])
    print(json.dumps(results))
    sys.exit(0 if all(results.values()) else 1)


if __name__ == '__main__':
    main()
