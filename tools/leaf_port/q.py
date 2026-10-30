#!/usr/bin/env python3
"""Look up how a Java type is ported (tools/leaf_port/gen.py's index).

  q.py net.minecraft.world.entity.Entity        type: Rust path, trait, super, fields, methods (Java -> Rust names)
  q.py Entity                                   every type with that simple name
  q.py Entity.hurt                              the methods named hurt (inherited ones too) with their Rust names
  q.py --file net/minecraft/world/entity/Entity.java   the Rust module of a Java file
"""
import json, os, sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
IDX = json.load(open(os.path.join(ROOT, '.local/leaf-port/index.json')))


def find(name):
    if name in IDX:
        return [name]
    return [k for k in IDX if k.endswith('.' + name) or k == name]


def chain(fq):
    out = []
    while fq in IDX and fq not in out:
        out.append(fq)
        fq = IDX[fq]['super']
    return out


def show(fq, meth=None):
    t = IDX[fq]
    print(f"{fq}  [{t['kind']}{' poly' if t['poly'] else ''}{' value' if t['value'] else ''}]  {t['file']}:{t['line']}")
    print(f"  rust:  {t['rust']}" + (f"   trait: {t['trait']}" if t['trait'] else ''))
    if t['craft_api']:
        print(f"  CraftBukkit wrapper of API {t['craft_api']}: implemented on the NMS type")
    if t['super']:
        print(f"  extends {t['super']}")
    if t['interfaces']:
        print(f"  implements {', '.join(t['interfaces'])}")
    seen = set()
    for c in ([fq] + chain(fq)[1:] if meth else [fq]):
        if c not in IDX:
            continue
        for m in IDX[c]['methods']:
            if meth and m['java'] != meth:
                continue
            key = (m['java'], m['rust'], len(m['params']))
            if key in seen:
                continue
            seen.add(key)
            st = 'static ' if m['static'] else ''
            ab = 'abstract ' if m['abstract'] else ''
            owner = '' if c == fq else f'   (declared in {c})'
            print(f"  {st}{ab}{m['ret']} {m['java']}({', '.join(m['params'])}) -> {m['rust']}{owner}")
    if not meth:
        for f in t['fields']:
            print(f"  {'static ' if f['static'] else ''}field {f['java']} -> {f['rust']}")


def main():
    a = sys.argv[1:]
    if not a:
        print(__doc__)
        return
    if a[0] == '--file':
        for k, t in IDX.items():
            if t['file'].endswith(a[1]):
                print(k, '->', t['rust'])
        return
    for q in a:
        meth = None
        hits = find(q)
        if not hits and '.' in q:
            q2, meth = q.rsplit('.', 1)
            hits = find(q2)
        for h in hits[:20]:
            show(h, meth)
        if not hits:
            print('no type', q)


main()
