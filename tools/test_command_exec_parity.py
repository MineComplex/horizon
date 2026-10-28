#!/usr/bin/env python3
"""Compare Horizon's command execution with vanilla's real command implementations.

Java (VanillaCommandExecOracle) starts a stand-in integrated server: a real
MinecraftServer with a generated world, Commands.CommandSelection.INTEGRATED
and one single-player owner named Player. Rust runs the same script through
Horizon's integrated server with a connected headless client (HORIZON_COMMAND_SCRIPT). Both run each command as
the player through Commands.performPrefixedCommand and record every system or
chat message the player receives (text and resolved style: colour, bold,
italic, underlined, strikethrough, obfuscated) together with the results the
command reported to the player's CommandResultCallback. World, entity and item
changes are observed through probe commands whose feedback reports them
(execute if, clear with a zero count, data get, scoreboard queries...).
Messages that depend on time or a random UUID are compared after masking.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import socket
import subprocess
import sys
import time
import uuid

try:
    from . import lab, test_rust_parity as base
    from .integration_env import run_game
    from . import command_exec_corpus as corpus_module
except ImportError:
    import lab
    from integration_env import run_game
    import test_rust_parity as base
    import command_exec_corpus as corpus_module

SEED = 1


def run_rust(server_bin, client_bin, rust_dir, script_path, build):
    """The script on horizon-server (HORIZON_COMMAND_SCRIPT), as the vanilla oracle's dedicated server runs it,
    with a headless horizon-client as its player (HORIZON_STATIONARY_CLIENT): it stays where commands put it."""
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        port = s.getsockname()[1]
    (rust_dir / 'eula.txt').write_text('eula=true\n')
    (rust_dir / 'server.properties').write_text(
        f'server-port={port}\nlevel-name=harness\nlevel-seed={SEED}\ngamemode=survival\ndifficulty=normal\n'
        'online-mode=false\nwhite-list=false\nenforce-secure-profile=false\nspawn-protection=0\n'
        'allow-flight=true\nview-distance=4\nsimulation-distance=4\npause-when-empty-seconds=0\n')
    (rust_dir / 'ops.json').write_text(json.dumps([{
        'uuid': str(uuid.UUID(bytes=offline_uuid('Player'))), 'name': 'Player', 'level': 4, 'bypassesPlayerLimit': False}]))
    server_env = lab.game_env(HORIZON_COMMAND_SCRIPT=script_path, HORIZON_COMMAND_OUTPUT=build / 'rust.jsonl')
    client_env = lab.game_env(HORIZON_STATIONARY_CLIENT=1, HORIZON_PLAYER_NAME='Player')
    with (build / 'rust.log').open('w', encoding='utf-8') as log, (build / 'rust-client.log').open('w', encoding='utf-8') as client_log:
        server = subprocess.Popen([str(server_bin), '--resources', str(lab.RESOURCES), '--nogui'], cwd=rust_dir,
                                  env=server_env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        client = None
        try:
            deadline = time.time() + 600
            while 'Done (' not in (build / 'rust.log').read_text(errors='replace'):
                if server.poll() is not None or time.time() > deadline:
                    sys.exit('horizon-server did not start:\n' + (build / 'rust.log').read_text(errors='replace')[-3000:])
                time.sleep(0.2)
            client = subprocess.Popen([str(client_bin), '--resources', str(lab.RESOURCES), '--server', f'127.0.0.1:{port}',
                                       '--name', 'Player', '--render-distance', '4'], cwd=rust_dir / 'client', env=client_env,
                                      stdin=subprocess.DEVNULL, stdout=client_log, stderr=subprocess.STDOUT)
            code = server.wait(timeout=int(os.environ.get('HORIZON_HARNESS_TIMEOUT', '600')) + 120)
        finally:
            for p in (server, client):
                if p and p.poll() is None:
                    p.kill()
    if code:
        sys.exit(f'horizon-server failed ({code}):\n' + (build / 'rust.log').read_text(errors='replace')[-3000:])


def offline_uuid(name):
    """UUIDUtil.createOfflinePlayerUUID: a name-based (version 3) MD5 UUID."""
    digest = bytearray(hashlib.md5(f'OfflinePlayer:{name}'.encode()).digest())
    digest[6] = digest[6] & 0x0f | 0x30
    digest[8] = digest[8] & 0x3f | 0x80
    return bytes(digest)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--limit', type=int, default=40, help='print at most this many mismatches')
    parser.add_argument('--allow-gaps', action='store_true', help='report mismatches without failing')
    parser.add_argument('--only', help='run only corpus groups whose name contains this')
    args = parser.parse_args()
    build = lab.LOCAL / 'command-exec-parity'
    build.mkdir(parents=True, exist_ok=True)
    report_path = build / 'report.json'
    report_path.unlink(missing_ok=True)
    if args.prepare:
        lab.prepare()
    lock, jars, libraries = base.verify_artifacts()
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaCommandExecOracle.java'
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    dependency_path = os.pathsep.join(map(str, libraries))

    def run(command, name, **kw):
        with (build / (name + '.log')).open('w', encoding='utf-8') as log:
            runner = subprocess.run
            result = runner(list(map(str, command)), stdout=log, stderr=subprocess.STDOUT, **kw)
        if result.returncode:
            tail = (build / (name + '.log')).read_text(errors='replace')[-3000:]
            sys.exit(f'{name} failed ({result.returncode}):\n{tail}')

    groups = corpus_module.groups()
    if args.only:
        groups = [g for g in groups if args.only in g[0] or g[0] == 'setup']
    script = []
    for _, lines in groups:
        script.extend(lines)
    script_path = build / 'script.jsonl'
    script_path.write_text(''.join(json.dumps(op, ensure_ascii=False) + '\n' for op in script), encoding='utf-8')

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', str(jars['server']) + os.pathsep + dependency_path, '-d', classes, oracle], 'compile-java')
    vanilla_dir = build / 'vanilla'
    shutil.rmtree(vanilla_dir, ignore_errors=True)
    vanilla_dir.mkdir()
    run(['java', '-Xmx3G', '-Duser.timezone=UTC', '-cp', os.pathsep.join([str(classes), str(jars['server']), dependency_path]),
         'dev.horizonlab.parity.VanillaCommandExecOracle', vanilla_dir / 'universe', script_path, build / 'vanilla.jsonl', SEED], 'java', cwd=vanilla_dir)

    run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-server', '--bin', 'horizon-server',
         '-p', 'horizon-client', '--bin', 'horizon-client'], 'compile-rust', cwd=lab.ROOT)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True))
    exe = '.exe' if os.name == 'nt' else ''
    target = Path(metadata['target_directory']) / 'fast'
    rust_dir = build / 'rust'
    shutil.rmtree(rust_dir, ignore_errors=True)
    (rust_dir / 'client').mkdir(parents=True)
    run_rust(target / f'horizon-server{exe}', target / f'horizon-client{exe}', rust_dir, script_path, build)

    vanilla = [json.loads(l) for l in (build / 'vanilla.jsonl').open(encoding='utf-8')]
    rust = [json.loads(l) for l in (build / 'rust.jsonl').open(encoding='utf-8')]
    if len(vanilla) != len(rust):
        sys.exit(f'record count differs: vanilla {len(vanilla)} rust {len(rust)}')
    commands_by_group = []
    index = 0
    for name, lines in groups:
        n = sum(1 for op in lines if (op['op'] == 'cmd' and op.get('record', True)) or (op['op'] == 'tick' and op.get('record')))
        commands_by_group.append((name, index, index + n))
        index += n
    mismatches = []
    for v, r in zip(vanilla, rust):
        assert v['cmd'] == r['cmd'], (v['cmd'], r['cmd'])
        kind = compare(v, r)
        if kind:
            mismatches.append({'kind': kind, 'cmd': v['cmd'], 'vanilla': v, 'rust': r})
    (build / 'mismatches.jsonl').write_text(''.join(json.dumps(m, ensure_ascii=False) + '\n' for m in mismatches), encoding='utf-8')
    summary = []
    bad = {m['cmd'] for m in mismatches}
    for name, lo, hi in commands_by_group:
        failed = sum(1 for rec in vanilla[lo:hi] if rec['cmd'] in bad)
        summary.append({'group': name, 'commands': hi - lo, 'mismatches': failed})
        print(f'{name}: {hi - lo - failed}/{hi - lo} commands match', flush=True)
    for m in mismatches[:args.limit]:
        print(m['kind'], repr(m['cmd']))
        print('  vanilla', render(m['vanilla'])[:400])
        print('  rust   ', render(m['rust'])[:400])
    total = len(vanilla)
    print(f'total: {total - len(mismatches)}/{total} commands match', flush=True)
    report = {'schema': 1, 'minecraft': lock['minecraft']['version'], 'platform': platform.platform(), 'seed': SEED,
              'commands': total, 'mismatches': len(mismatches), 'groups': summary,
              'scope': 'Feedback and failure messages (text and resolved style), chat lines, and CommandResultCallback results of commands run by the single-player owner of an integrated server; world, entity and item effects through probe commands.'}
    lab.write_json(report_path, report)
    if mismatches and not args.allow_gaps:
        sys.exit(f'{len(mismatches)} command execution mismatches; see {build}')
    print(f'Command execution parity report: {report_path}')


MASKS = [
    (re.compile(r'debug-trace-\d{4}-\d\d-\d\d_\d\d\.\d\d\.\d\d\.txt'), 'debug-trace-<time>.txt'),
    (re.compile(r'after [0-9.]+ second\(s\) and \d+ tick\(s\) \([0-9.,NaInfity]+ tick\(s\) per second\)'), 'after <time>'),
    (re.compile(r'Average time per tick: [0-9.]+ms'), 'Average time per tick: <ms>'),
    (re.compile(r'P50: [0-9.]+ms P95: [0-9.]+ms P99: [0-9.]+ms'), 'P50: <ms> P95: <ms> P99: <ms>'),
    (re.compile(r'debug/client-[0-9_.-]+\.(jfr|json)'), 'debug/client-<time>'),
]


GAMERULES = re.compile(r'/gamerule \(([^)]*)\)')


def masked(record):
    text = json.dumps(record['messages'], ensure_ascii=False)
    # GameRuleCommand registers the rules in identity-hash order, different
    # in every JVM: compare /help's gamerule alternatives as a set.
    text = GAMERULES.sub(lambda m: '/gamerule (' + '|'.join(sorted(m.group(1).split('|'))) + ')', text)
    for pattern, replacement in MASKS:
        text = pattern.sub(replacement, text)
    return text


def compare(v, r):
    if masked(v) != masked(r):
        return 'messages'
    if v['results'] != r['results']:
        if v['cmd'].startswith('/debug stop'):
            return None
        return 'results'
    return None


def render(record):
    lines = [''.join(span[0] for span in m) for m in record['messages']]
    return json.dumps({'messages': lines, 'styles': [[span[1] for span in m] for m in record['messages']], 'results': record['results']}, ensure_ascii=False)


if __name__ == '__main__':
    main()
