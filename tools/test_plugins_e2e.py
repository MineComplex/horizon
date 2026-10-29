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
