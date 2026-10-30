#!/usr/bin/env python3
"""Splits the port into work units of about LIMIT Java lines, grouped by package (for parallel agents).
Writes .local/leaf-port/units.json: [{id, set, lines, files: [[java_rel, rust_rel], ...]}]."""
import json, os, collections, sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LEAF = os.path.expanduser('~/Desktop/new/Leaf')
IDX = json.load(open(os.path.join(ROOT, '.local/leaf-port/index.json')))
LIB_PREFIXES = ('leaf-server/src/minecraft/java/com/mojang/serialization', 'leaf-server/src/minecraft/java/com/mojang/datafixers',
                'leaf-server/src/minecraft/java/com/mojang/brigadier', 'leaf-server/src/minecraft/java/com/mojang/authlib')
files = {}
for fq, t in IDX.items():
    mod = t['rust'].split('::')[1:-1]
    files[t['file']] = (t['set'], 'crates/horizon-server/src/' + '/'.join(mod) + '.rs')
lines = {f: sum(1 for _ in open(os.path.join(LEAF, f), errors='ignore')) for f in files}
units = []
def limit(s):
    return 14000 if s == 'api' else 6000
bydir = collections.defaultdict(list)
for f, (s, r) in files.items():
    if f.startswith(LIB_PREFIXES):
        continue
    bydir[(s, os.path.dirname(f))].append(f)
cur = None
for (s, d) in sorted(bydir):
    fs = sorted(bydir[(s, d)])
    n = sum(lines[f] for f in fs)
    L = limit(s)
    if n > L:
        chunk, cn = [], 0
        for f in fs:
            if cn + lines[f] > L and chunk:
                units.append({'set': s, 'dirs': [d], 'files': chunk, 'lines': cn})
                chunk, cn = [], 0
            chunk.append(f); cn += lines[f]
        if chunk:
            units.append({'set': s, 'dirs': [d], 'files': chunk, 'lines': cn})
        cur = None
        continue
    if cur and cur['set'] == s and cur['lines'] + n <= L and os.path.dirname(cur['dirs'][0]).split('/')[:6] == d.split('/')[:6]:
        cur['dirs'].append(d); cur['files'] += fs; cur['lines'] += n
    else:
        cur = {'set': s, 'dirs': [d], 'files': fs, 'lines': n}
        units.append(cur)
out = []
for i, u in enumerate(units):
    out.append({'id': f'u{i:03d}', 'set': u['set'], 'lines': u['lines'], 'dirs': u['dirs'],
                'files': [[f, files[f][1]] for f in u['files']]})
json.dump(out, open(os.path.join(ROOT, '.local/leaf-port/units.json'), 'w'), indent=1)
c = collections.Counter(u['set'] for u in out)
print(len(out), dict(c), sum(u['lines'] for u in out))
