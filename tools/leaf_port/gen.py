#!/usr/bin/env python3
"""Leaf 26.3 -> Rust skeleton generator (crates/horizon-server).

Parses Leaf's applied Java sources (the patched NMS, Leaf's own code, Paper's server code without
CraftBukkit, and paper-api) with tree-sitter and writes:

  .local/leaf-port/index.json   every Java type -> its Rust path, kind, fields, methods (Rust names)
  crates/horizon-server/src/... one Rust module per Java file, mirroring the Java package tree:
                                structs, traits for class hierarchies, method signatures with todo!()
                                bodies, statics, and the generated hierarchy glue

The conventions it applies are in crates/horizon-server/PORTING.md. Needs tree-sitter and
tree-sitter-java (pip). Usage: gen.py [--leaf DIR] [--out DIR] [--index-only]
"""
import argparse, json, os, re, sys, collections
import tree_sitter as ts, tree_sitter_java as tsj

LEAF = os.path.expanduser('~/Desktop/new/Leaf')
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

JLANG = ts.Language(tsj.language())
PARSER = ts.Parser(JLANG)

RUST_KW = set('''as break const continue crate else enum extern false fn for if impl in let loop match mod move mut
pub ref return self Self static struct super trait true type unsafe use where while async await dyn abstract become box
do final macro override priv typeof unsized virtual yield try gen union'''.split())
# Type names that would shadow Rust's prelude or the runtime's names inside a module.
RESERVED_TYPES = set('''Option Some None Result Ok Err Box Vec String Send Sync Sized Copy Clone Drop Fn FnMut FnOnce
Iterator IntoIterator Default Eq PartialEq Ord PartialOrd ToString ToOwned Into From AsRef AsMut Extend Debug Hash Arc
Weak Mut JRef JObject JObjectRef LazyLock HashMap HashSet IndexMap IndexSet VecDeque BTreeMap BTreeSet Uuid Ordering
Any Unpin Self'''.split())

JAVA_LANG = set('''String Object Integer Long Double Float Short Byte Character Boolean Number Math StrictMath System
Thread Runnable Iterable Comparable CharSequence StringBuilder StringBuffer Exception RuntimeException Throwable Error
IllegalArgumentException IllegalStateException UnsupportedOperationException NullPointerException
IndexOutOfBoundsException ArrayIndexOutOfBoundsException Class Enum Record Void AutoCloseable Cloneable Override
Deprecated SuppressWarnings FunctionalInterface SafeVarargs InterruptedException ClassCastException ArithmeticException
NumberFormatException ThreadLocal InheritableThreadLocal Process Runtime StackTraceElement OutOfMemoryError
StackOverflowError CloneNotSupportedException ReflectiveOperationException ClassNotFoundException SecurityException
ProcessBuilder Appendable Readable ScopedValue NoSuchFieldException NoSuchMethodException ExceptionInInitializerError
LinkageError AssertionError VirtualMachineError InternalError IllegalAccessException InstantiationException
UnsupportedClassVersionError NegativeArraySizeException IllegalMonitorStateException StringIndexOutOfBoundsException
ClassLoader Module Package ThreadGroup ArrayStoreException EnumConstantNotPresentException TypeNotPresentException
IllegalCallerException LayerInstantiationException ModuleLayer StackWalker ProcessHandle'''.split())

PRIMS = {'int': 'i32', 'long': 'i64', 'short': 'i16', 'byte': 'i8', 'char': 'char', 'boolean': 'bool',
         'float': 'f32', 'double': 'f64', 'void': '()'}
BOXES = {'java.lang.Integer': 'i32', 'java.lang.Long': 'i64', 'java.lang.Short': 'i16', 'java.lang.Byte': 'i8',
         'java.lang.Character': 'char', 'java.lang.Boolean': 'bool', 'java.lang.Float': 'f32',
         'java.lang.Double': 'f64', 'java.lang.Void': '()', 'java.lang.Number': 'f64'}

# Value types besides records: passed by value (Clone, Copy when every field is), no `this`, no Mut fields.
VALUE_FORCE = {'net.minecraft.core.Vec3i', 'net.minecraft.core.BlockPos', 'net.minecraft.core.BlockPos.MutableBlockPos',
               'net.minecraft.core.SectionPos'}
COLLECTION_PREFIXES = ('Vec<', 'HashMap<', 'HashSet<', 'IndexMap<', 'IndexSet<', 'VecDeque<', 'BTreeMap<',
                       'BTreeSet<', 'crate::java::util::PriorityQueue<', 'crate::java::util::BitSet')


def snake(name):
    out = []
    for i, c in enumerate(name):
        if c.isupper():
            prev = name[i - 1] if i else ''
            nxt = name[i + 1] if i + 1 < len(name) else ''
            if i and (prev.islower() or prev.isdigit() or (prev.isupper() and nxt.islower())):
                out.append('_')
            out.append(c.lower())
        else:
            out.append(c)
    s = ''.join(out).replace('$', '_')
    return s + '_' if s in RUST_KW else s


def upper_snake(name):
    if name.upper() == name:
        return name
    return snake(name).rstrip('_').upper()


def safe_ident(name):
    return name + '_' if name in RUST_KW else name


def txt(n):
    return n.text.decode('utf8') if n is not None else ''


def kids(n, *types):
    return [c for c in n.children if not types or c.type in types]


def modifiers(n):
    for c in n.children:
        if c.type == 'modifiers':
            mods, ann = set(), set()
            for m in c.children:
                if m.type in ('marker_annotation', 'annotation'):
                    ann.add(txt(m.child_by_field_name('name')).split('.')[-1])
                else:
                    mods.add(txt(m))
            return mods, ann
    return set(), set()


# ---------------------------------------------------------------------------------------------- parse
class JType:
    """One Java type declaration."""
    def __init__(self, fqn, name, kind, file, outer=None):
        self.fqn, self.name, self.kind, self.file, self.outer = fqn, name, kind, file, outer
        self.mods, self.ann = set(), set()
        self.tparams = []          # [(name, bound node or None)]
        self.ext = None            # superclass type node
        self.impls = []            # interface type nodes
        self.fields, self.methods, self.ctors, self.consts, self.components = [], [], [], [], []
        self.nested = {}           # simple name -> fqn
        self.anon_subclassed = False
        self.node = None
        self.line = 0


class JFile:
    def __init__(self, path, rel, source_set):
        self.path, self.rel, self.source_set = path, rel, source_set
        self.package = ''
        self.imports = {}          # simple -> fqn
        self.wild = []             # package or type prefixes
        self.static_imports = []
        self.types = []            # top-level fqns


TYPES = {}       # fqn -> JType
FILES = []
ANON_TARGETS = collections.Counter()   # simple/dotted names of `new X() { ... }`


def parse_file(path, rel, source_set):
    src = open(path, 'rb').read()
    tree = PARSER.parse(src)
    f = JFile(path, rel, source_set)
    root = tree.root_node
    for c in root.children:
        if c.type == 'package_declaration':
            f.package = txt(kids(c, 'scoped_identifier', 'identifier')[0])
        elif c.type == 'import_declaration':
            static = any(k.type == 'static' for k in c.children)
            name = txt(kids(c, 'scoped_identifier', 'identifier')[0])
            wild = any(k.type == 'asterisk' for k in c.children)
            if static:
                f.static_imports.append(name + ('.*' if wild else ''))
            elif wild:
                f.wild.append(name)
            else:
                f.imports[name.split('.')[-1]] = name
    for c in root.children:
        if c.type in ('class_declaration', 'interface_declaration', 'enum_declaration', 'record_declaration',
                      'annotation_type_declaration'):
            t = parse_type(c, f, f.package, None)
            if t:
                f.types.append(t.fqn)
    for n in walk(root):
        if n.type == 'object_creation_expression' and n.child_by_field_name('body') is None:
            pass
        if n.type == 'object_creation_expression' and any(k.type == 'class_body' for k in n.children):
            ty = n.child_by_field_name('type')
            ANON_TARGETS[(f.path, strip_generics(txt(ty)))] += 1
    FILES.append(f)
    return f


def walk(n):
    stack = [n]
    while stack:
        x = stack.pop()
        yield x
        stack.extend(x.children)


def strip_generics(s):
    out, depth = [], 0
    for ch in s:
        if ch == '<':
            depth += 1
        elif ch == '>':
            depth -= 1
        elif depth == 0:
            out.append(ch)
    return re.sub(r'@\w+(\.\w+)*\s*', '', ''.join(out)).strip()


KIND = {'class_declaration': 'class', 'interface_declaration': 'interface', 'enum_declaration': 'enum',
        'record_declaration': 'record', 'annotation_type_declaration': 'annotation'}


def parse_type(node, f, prefix, outer):
    name = txt(node.child_by_field_name('name'))
    fqn = prefix + '.' + name if prefix else name
    t = JType(fqn, name, KIND[node.type], f, outer)
    t.node = node
    t.line = node.start_point[0] + 1
    t.mods, t.ann = modifiers(node)
    if t.kind == 'annotation':
        TYPES[fqn] = t
        return t
    tp = node.child_by_field_name('type_parameters')
    if tp is not None:
        for p in kids(tp, 'type_parameter'):
            nm = [k for k in p.children if k.type in ('type_identifier', 'identifier')]
            bound = [k for k in p.children if k.type == 'type_bound']
            t.tparams.append((txt(nm[0]) if nm else txt(p).split()[0],
                              bound[0].children[1] if bound and len(bound[0].children) > 1 else None))
    sc = node.child_by_field_name('superclass')
    if sc is not None:
        t.ext = [k for k in sc.children if k.is_named][0]
    ifs = node.child_by_field_name('interfaces')
    if ifs is None:
        for k in node.children:
            if k.type in ('extends_interfaces', 'super_interfaces'):
                ifs = k
    if ifs is not None:
        tl = [k for k in ifs.children if k.type == 'type_list']
        if tl:
            t.impls = [k for k in tl[0].children if k.is_named]
    if t.kind == 'interface':
        # `interface A extends B, C`
        for k in node.children:
            if k.type == 'extends_interfaces':
                tl = [x for x in k.children if x.type == 'type_list']
                t.impls = [x for x in tl[0].children if x.is_named] if tl else []
    if t.kind == 'record':
        ps = node.child_by_field_name('parameters')
        for p in kids(ps, 'formal_parameter', 'spread_parameter'):
            t.components.append(param_info(p))
    body = node.child_by_field_name('body')
    TYPES[fqn] = t
    if body is None:
        return t
    members = list(body.children)
    if t.kind == 'enum':
        for k in body.children:
            if k.type == 'enum_constant':
                args = k.child_by_field_name('arguments')
                t.consts.append({'name': txt(k.child_by_field_name('name')), 'args': txt(args) if args else '',
                                 'body': k.child_by_field_name('body') is not None})
            if k.type == 'enum_body_declarations':
                members = list(k.children)
    for m in members:
        if m.type in ('field_declaration', 'constant_declaration'):
            mods, ann = modifiers(m)
            if t.kind == 'interface':
                mods |= {'static', 'final', 'public'}
            ty = m.child_by_field_name('type')
            for d in kids(m, 'variable_declarator'):
                dims = d.child_by_field_name('dimensions')
                val = d.child_by_field_name('value')
                t.fields.append({'name': txt(d.child_by_field_name('name')), 'type': ty, 'dims': txt(dims).count('[') if dims else 0,
                                 'mods': mods, 'ann': ann, 'value': txt(val) if val is not None else None,
                                 'line': m.start_point[0] + 1})
        elif m.type == 'method_declaration':
            mods, ann = modifiers(m)
            body_n = m.child_by_field_name('body')
            if t.kind == 'interface':
                if body_n is None:
                    mods.add('abstract')
                if 'private' not in mods:
                    mods.add('public')
            tps = []
            tpn = m.child_by_field_name('type_parameters')
            if tpn is not None:
                for p in kids(tpn, 'type_parameter'):
                    nm = [k for k in p.children if k.type in ('type_identifier', 'identifier')]
                    bound = [k for k in p.children if k.type == 'type_bound']
                    tps.append((txt(nm[0]) if nm else '', bound[0].children[1] if bound and len(bound[0].children) > 1 else None))
            params = [param_info(p) for p in kids(m.child_by_field_name('parameters'), 'formal_parameter', 'spread_parameter')]
            throws = [k for k in m.children if k.type == 'throws']
            dims = m.child_by_field_name('dimensions')
            t.methods.append({'name': txt(m.child_by_field_name('name')), 'ret': m.child_by_field_name('type'),
                              'ret_dims': txt(dims).count('[') if dims else 0,
                              'params': params, 'mods': mods, 'ann': ann, 'tparams': tps,
                              'throws': bool(throws), 'body': body_n is not None,
                              'body_text': txt(body_n) if body_n is not None else '',
                              'line': m.start_point[0] + 1})
        elif m.type in ('constructor_declaration', 'compact_constructor_declaration'):
            mods, ann = modifiers(m)
            ps = m.child_by_field_name('parameters')
            params = [param_info(p) for p in kids(ps, 'formal_parameter', 'spread_parameter')] if ps is not None else None
            throws = [k for k in m.children if k.type == 'throws']
            t.ctors.append({'params': params, 'mods': mods, 'throws': bool(throws), 'line': m.start_point[0] + 1,
                            'compact': m.type == 'compact_constructor_declaration'})
        elif m.type in KIND:
            nt = parse_type(m, f, fqn, t)
            t.nested[nt.name] = nt.fqn
    return t


def param_info(p):
    mods, ann = modifiers(p)
    if p.type == 'spread_parameter':
        ty = [k for k in p.children if k.is_named and k.type not in ('modifiers', 'variable_declarator')][0]
        d = kids(p, 'variable_declarator')
        name = txt(d[0].child_by_field_name('name')) if d else 'args'
        return {'name': name, 'type': ty, 'dims': 1, 'ann': ann, 'final': 'final' in mods}
    dims = p.child_by_field_name('dimensions')
    return {'name': txt(p.child_by_field_name('name')), 'type': p.child_by_field_name('type'),
            'dims': txt(dims).count('[') if dims else 0, 'ann': ann, 'final': 'final' in mods}


# ---------------------------------------------------------------------------------------------- resolve
EXTERNAL = collections.Counter()      # external fqn -> uses
EXT_ARITY = {}
EXT_IMPLEMENTED = set()               # external types implemented by in-tree types (traits)
PKG_TYPES = collections.defaultdict(dict)   # package -> simple -> fqn (top-level)
SUPER_CACHE = {}


def file_of(t):
    return t.file


def resolve(name, t, tvars=()):
    """Resolve a (possibly dotted) type name used inside type `t` to an fqn, or None for a type variable."""
    if name in tvars or tvn(name) in tvars:
        return None
    parts = name.split('.')
    head = parts[0]
    base = resolve_simple(head, t)
    if base is None:
        # Fully qualified name?
        for i in range(len(parts), 0, -1):
            cand = '.'.join(parts[:i])
            if cand in TYPES:
                base = cand
                parts = [cand] + parts[i:]
                break
        else:
            if head[:1].islower():
                fq = name
                for i in range(len(parts), 0, -1):
                    if parts[i - 1][:1].isupper() and (i == 1 or not parts[i - 2][:1].isupper()):
                        return '.'.join(parts)
                return fq
            return '?' + name
        head_done = True
    for p in parts[1:]:
        tt = TYPES.get(base)
        if tt is None:
            base = base + '.' + p
            continue
        nf = find_nested(tt, p)
        base = nf if nf else base + '.' + p
    return base


def find_nested(tt, name, seen=None):
    if name in tt.nested:
        return tt.nested[name]
    seen = seen or set()
    if tt.fqn in seen:
        return None
    seen.add(tt.fqn)
    for s in supertypes(tt):
        st = TYPES.get(s)
        if st:
            r = find_nested(st, name, seen)
            if r:
                return r
    return None


def resolve_simple(name, t):
    f = t.file
    cur = t
    while cur is not None:
        if cur.name == name:
            return cur.fqn
        r = find_nested(cur, name)
        if r:
            return r
        cur = cur.outer
    if name in f.imports:
        return f.imports[name]
    if name in PKG_TYPES[f.package]:
        return PKG_TYPES[f.package][name]
    for w in f.wild:
        if w in PKG_TYPES and name in PKG_TYPES[w]:
            return PKG_TYPES[w][name]
        if w in TYPES:
            r = find_nested(TYPES[w], name)
            if r:
                return r
    if name in JAVA_LANG:
        return 'java.lang.' + name
    for w in f.wild:
        if not w in TYPES and w.split('.')[0] in ('java', 'javax', 'com', 'org', 'io', 'it', 'net'):
            if w not in PKG_TYPES:
                return w + '.' + name   # external wildcard import: best guess
    return None


def supertypes(t):
    if t.fqn in SUPER_CACHE:
        return SUPER_CACHE[t.fqn]
    SUPER_CACHE[t.fqn] = []
    out = []
    tv = [p[0] for p in t.tparams]
    ctx = t.outer if t.outer else t
    for n in ([t.ext] if t.ext is not None else []) + list(t.impls):
        nm = strip_generics(txt(n))
        r = resolve(nm, ctx if False else t, tv)
        if r:
            out.append(r)
    SUPER_CACHE[t.fqn] = out
    return out


def superclass(t):
    if t.ext is None:
        return None
    return resolve(strip_generics(txt(t.ext)), t, [p[0] for p in t.tparams])


def interfaces(t):
    tv = [p[0] for p in t.tparams]
    return [resolve(strip_generics(txt(n)), t, tv) for n in t.impls]


# ---------------------------------------------------------------------------------------------- rust naming
MODPATH = {}      # file path -> rust module path (list of segments)
RNAME = {}        # fqn -> rust type name
RPATH = {}        # fqn -> crate path of the type ("crate::a::b::Name")
TRAIT = {}        # fqn -> crate path of its trait (polymorphic classes) or itself (interfaces)
POLY = set()
VALUE = set()
COPY = set()
MUTVALUE = set()
CRAFT_MAP = {}    # craftbukkit fqn -> api fqn (wrappers)
SUBCLASSES = collections.defaultdict(list)


def pkg_segments(pkg):
    return [safe_ident(s) for s in pkg.split('.')] if pkg else []


def module_for_file(f):
    segs = pkg_segments(f.package)
    if f.source_set == 'craft':
        segs = ['api_impl'] + [safe_ident(s) for s in f.package.split('.')[3:]]
    stem = os.path.splitext(os.path.basename(f.path))[0]
    return segs, snake(stem)


def assign_names():
    # modules: resolve file-module vs subpackage collisions
    subpkgs = collections.defaultdict(set)
    for f in FILES:
        segs, _ = module_for_file(f)
        for i in range(len(segs)):
            subpkgs[tuple(segs[:i])].add(segs[i])
    for f in FILES:
        segs, m = module_for_file(f)
        if m in subpkgs[tuple(segs)] or m in ('mod', 'prelude', 'java', 'api_impl'):
            m = m + '_class' if not m.endswith('_') else m + 'class'
        MODPATH[f.path] = segs + [m]
    for f in FILES:
        used = set()
        for top in f.types:
            for t in iter_types(TYPES[top]):
                chain = []
                c = t
                while c is not None:
                    chain.append(c.name)
                    c = c.outer
                nm = ''.join(reversed(chain))
                if nm in RESERVED_TYPES:
                    nm = nm + 'J'
                while nm in used:
                    nm = nm + '_'
                used.add(nm)
                RNAME[t.fqn] = nm
                RPATH[t.fqn] = 'crate::' + '::'.join(MODPATH[f.path]) + '::' + nm


def iter_types(t):
    yield t
    for n in t.nested.values():
        yield from iter_types(TYPES[n])


def compute_kinds():
    for t in TYPES.values():
        if t.kind == 'class':
            s = superclass(t)
            if s in TYPES:
                SUBCLASSES[s].append(t.fqn)
    anon = collections.defaultdict(int)
    for (path, nm), c in ANON_TARGETS.items():
        # resolve against the first type in that file
        f = next((x for x in FILES if x.path == path), None)
        if not f or not f.types:
            continue
        r = resolve(nm, TYPES[f.types[0]])
        if r in TYPES:
            TYPES[r].anon_subclassed = True
    for t in TYPES.values():
        if t.kind == 'class' and t.file.source_set != 'craft':
            subs = [s for s in SUBCLASSES[t.fqn] if TYPES[s].file.source_set != 'craft']
            if 'abstract' in t.mods or subs or (t.anon_subclassed and 'final' not in t.mods):
                POLY.add(t.fqn)
    for v in VALUE_FORCE:
        POLY.discard(v)
    # value types
    for t in TYPES.values():
        if t.kind in ('record', 'enum'):
            VALUE.add(t.fqn)
    VALUE.update(v for v in VALUE_FORCE if v in TYPES)
    changed = True
    cand = [t for t in TYPES.values() if t.kind == 'class' and t.fqn not in POLY and t.ext is None
            and 'abstract' not in t.mods and t.file.source_set != 'craft' and not SUBCLASSES[t.fqn]]
    while changed:
        changed = False
        for t in cand:
            if t.fqn in VALUE:
                continue
            inst = [fl for fl in t.fields if 'static' not in fl['mods']]
            if not inst or not all('final' in fl['mods'] for fl in inst):
                continue
            ok = True
            for fl in inst:
                tn = fl['type']
                if fl['dims']:
                    ok = False
                    break
                if tn.type in ('integral_type', 'floating_point_type', 'boolean_type'):
                    continue
                r = resolve(strip_generics(txt(tn)), t, [p[0] for p in t.tparams])
                if r == 'java.lang.String' or r in BOXES or r in VALUE:
                    continue
                ok = False
                break
            if ok:
                VALUE.add(t.fqn)
                changed = True
    for v in list(VALUE):
        t = TYPES[v]
        if t.kind == 'enum':
            COPY.add(v)
            continue
        inst = [fl for fl in t.fields if 'static' not in fl['mods']] + [
            {'type': c['type'], 'dims': c['dims']} for c in t.components]
        if all(fl['type'] is not None and fl['type'].type in ('integral_type', 'floating_point_type', 'boolean_type')
               and not fl['dims'] for fl in inst):
            COPY.add(v)
        if any('final' not in fl.get('mods', {'final'}) for fl in inst):
            MUTVALUE.add(v)
    for t in TYPES.values():
        if t.fqn in RPATH:
            if t.kind == 'interface':
                TRAIT[t.fqn] = RPATH[t.fqn]
            elif t.fqn in POLY:
                TRAIT[t.fqn] = RPATH[t.fqn] + 'Trait'


def craft_mapping():
    """CraftBukkit wrappers map to the API interface they implement (the NMS type implements it in Rust)."""
    for t in TYPES.values():
        if t.file.source_set != 'craft':
            continue
        cur, api = t, None
        seen = set()
        while cur is not None and cur.fqn not in seen:
            seen.add(cur.fqn)
            for i in interfaces(cur):
                if i in TYPES and TYPES[i].file.source_set == 'api':
                    api = i
                    break
            if api:
                break
            s = superclass(cur)
            if s in TYPES and TYPES[s].file.source_set == 'api':
                api = s
                break
            cur = TYPES.get(s) if s else None
        if api:
            CRAFT_MAP[t.fqn] = api


# ---------------------------------------------------------------------------------------------- types
FASTUTIL_PRIM = {'Int': 'i32', 'Long': 'i64', 'Short': 'i16', 'Byte': 'i8', 'Char': 'char', 'Float': 'f32',
                 'Double': 'f64', 'Boolean': 'bool'}
JU = 'crate::java::util::'


def ext_rust(fqn, args):
    """Rust type for an external (not in-tree) Java type."""
    a = lambda i, d='JObjectRef': args[i] if i < len(args) else d
    simple = fqn.split('.')[-1]
    pkg = fqn.rsplit('.', 1)[0]
    if fqn in BOXES:
        return BOXES[fqn]
    if fqn in ('java.lang.String', 'java.lang.CharSequence', 'java.lang.StringBuilder', 'java.lang.StringBuffer'):
        return 'String'
    if fqn in ('java.lang.Object', 'java.lang.Record'):
        return 'JObjectRef'
    if fqn == 'java.lang.Iterable' or fqn in ('java.util.List', 'java.util.ArrayList', 'java.util.LinkedList',
            'java.util.Collection', 'java.util.AbstractList', 'java.util.Vector', 'java.util.Stack',
            'java.util.SequencedCollection', 'java.util.AbstractCollection', 'java.util.RandomAccess',
            'com.google.common.collect.ImmutableList', 'com.google.common.collect.ImmutableCollection',
            'it.unimi.dsi.fastutil.objects.ObjectList', 'it.unimi.dsi.fastutil.objects.ObjectArrayList',
            'it.unimi.dsi.fastutil.objects.ObjectCollection', 'it.unimi.dsi.fastutil.objects.ObjectImmutableList',
            'it.unimi.dsi.fastutil.objects.ReferenceArrayList', 'it.unimi.dsi.fastutil.objects.ReferenceList',
            'it.unimi.dsi.fastutil.objects.ObjectIterable', 'it.unimi.dsi.fastutil.objects.ReferenceCollection',
            'java.util.concurrent.CopyOnWriteArrayList'):
        return f'Vec<{a(0)}>'
    if fqn in ('java.util.Set', 'java.util.HashSet', 'java.util.AbstractSet', 'it.unimi.dsi.fastutil.objects.ObjectSet',
               'it.unimi.dsi.fastutil.objects.ObjectOpenHashSet', 'it.unimi.dsi.fastutil.objects.ReferenceSet',
               'it.unimi.dsi.fastutil.objects.ReferenceOpenHashSet', 'java.util.EnumSet',
               'it.unimi.dsi.fastutil.objects.ObjectArraySet', 'it.unimi.dsi.fastutil.objects.ReferenceArraySet',
               'it.unimi.dsi.fastutil.objects.ObjectOpenCustomHashSet', 'com.google.common.collect.Sets'):
        return f'HashSet<{a(0)}>'
    if fqn in ('java.util.LinkedHashSet', 'java.util.SequencedSet', 'com.google.common.collect.ImmutableSet',
               'it.unimi.dsi.fastutil.objects.ObjectLinkedOpenHashSet',
               'it.unimi.dsi.fastutil.objects.ReferenceLinkedOpenHashSet'):
        return f'IndexSet<{a(0)}>'
    if fqn in ('java.util.SortedSet', 'java.util.NavigableSet', 'java.util.TreeSet',
               'com.google.common.collect.ImmutableSortedSet', 'it.unimi.dsi.fastutil.objects.ObjectSortedSet',
               'it.unimi.dsi.fastutil.objects.ObjectAVLTreeSet', 'it.unimi.dsi.fastutil.objects.ObjectRBTreeSet'):
        return f'BTreeSet<{a(0)}>'
    if fqn in ('java.util.Map', 'java.util.HashMap', 'java.util.AbstractMap', 'java.util.IdentityHashMap',
               'java.util.WeakHashMap', 'java.util.EnumMap', 'com.google.common.collect.Maps',
               'it.unimi.dsi.fastutil.objects.Object2ObjectMap', 'it.unimi.dsi.fastutil.objects.Object2ObjectOpenHashMap',
               'it.unimi.dsi.fastutil.objects.Reference2ObjectMap',
               'it.unimi.dsi.fastutil.objects.Reference2ObjectOpenHashMap',
               'it.unimi.dsi.fastutil.objects.Object2ObjectArrayMap', 'it.unimi.dsi.fastutil.objects.Reference2ObjectArrayMap',
               'it.unimi.dsi.fastutil.objects.Reference2ReferenceOpenHashMap',
               'it.unimi.dsi.fastutil.objects.Reference2ReferenceMap',
               'it.unimi.dsi.fastutil.objects.Object2ObjectOpenCustomHashMap',
               'it.unimi.dsi.fastutil.objects.Reference2ReferenceArrayMap', 'com.google.common.collect.BiMap',
               'com.google.common.collect.HashBiMap', 'com.google.common.collect.ImmutableBiMap'):
        return f'HashMap<{a(0)}, {a(1)}>'
    if fqn in ('java.util.LinkedHashMap', 'java.util.SequencedMap', 'com.google.common.collect.ImmutableMap',
               'it.unimi.dsi.fastutil.objects.Object2ObjectLinkedOpenHashMap',
               'it.unimi.dsi.fastutil.objects.Reference2ObjectLinkedOpenHashMap'):
        return f'IndexMap<{a(0)}, {a(1)}>'
    if fqn in ('java.util.TreeMap', 'java.util.SortedMap', 'java.util.NavigableMap',
               'com.google.common.collect.ImmutableSortedMap', 'it.unimi.dsi.fastutil.objects.Object2ObjectSortedMap',
               'it.unimi.dsi.fastutil.objects.Object2ObjectAVLTreeMap', 'it.unimi.dsi.fastutil.objects.Object2ObjectRBTreeMap'):
        return f'BTreeMap<{a(0)}, {a(1)}>'
    if fqn in ('java.util.Map.Entry', 'it.unimi.dsi.fastutil.objects.Object2ObjectMap.Entry',
               'com.mojang.datafixers.util.Pair') and False:
        return f'({a(0)}, {a(1)})'
    if fqn == 'java.util.Map.Entry':
        return f'({a(0)}, {a(1)})'
    if fqn in ('com.google.common.collect.Multimap', 'com.google.common.collect.ListMultimap',
               'com.google.common.collect.ArrayListMultimap', 'com.google.common.collect.ImmutableMultimap',
               'com.google.common.collect.ImmutableListMultimap', 'com.google.common.collect.LinkedListMultimap',
               'com.google.common.collect.LinkedHashMultimap'):
        return f'IndexMap<{a(0)}, Vec<{a(1)}>>'
    if fqn in ('com.google.common.collect.SetMultimap', 'com.google.common.collect.HashMultimap',
               'com.google.common.collect.ImmutableSetMultimap', 'com.google.common.collect.Multimaps'):
        return f'IndexMap<{a(0)}, IndexSet<{a(1)}>>'
    if fqn in ('com.google.common.collect.Table', 'com.google.common.collect.HashBasedTable',
               'com.google.common.collect.ImmutableTable'):
        return f'HashMap<({a(0)}, {a(1)}), {a(2)}>'
    if fqn in ('java.util.Deque', 'java.util.ArrayDeque', 'java.util.Queue', 'java.util.AbstractQueue',
               'it.unimi.dsi.fastutil.objects.ObjectArrayFIFOQueue', 'com.google.common.collect.EvictingQueue'):
        return f'VecDeque<{a(0)}>'
    if fqn in ('java.util.Optional',):
        return f'Option<{a(0)}>'
    if fqn == 'java.util.OptionalInt':
        return 'Option<i32>'
    if fqn == 'java.util.OptionalLong':
        return 'Option<i64>'
    if fqn == 'java.util.OptionalDouble':
        return 'Option<f64>'
    if fqn == 'java.util.UUID':
        return 'Uuid'
    if fqn in ('java.io.File', 'java.nio.file.Path'):
        return 'std::path::PathBuf'
    if fqn == 'java.time.Duration':
        return 'std::time::Duration'
    if fqn in ('java.net.SocketAddress', 'java.net.InetSocketAddress'):
        return 'std::net::SocketAddr'
    if fqn == 'java.net.InetAddress':
        return 'std::net::IpAddr'
    if fqn in ('java.net.URI', 'java.net.URL'):
        return 'String'
    if fqn in ('java.util.regex.Pattern',):
        return 'regex::Regex'
    if fqn in ('java.util.concurrent.atomic.AtomicInteger',):
        return 'std::sync::atomic::AtomicI32'
    if fqn in ('java.util.concurrent.atomic.AtomicLong', 'java.util.concurrent.atomic.LongAdder'):
        return 'std::sync::atomic::AtomicI64'
    if fqn == 'java.util.concurrent.atomic.AtomicBoolean':
        return 'std::sync::atomic::AtomicBool'
    if fqn in ('java.util.concurrent.ConcurrentHashMap', 'java.util.concurrent.ConcurrentMap'):
        return f'dashmap::DashMap<{a(0)}, {a(1)}>'
    if fqn in ('java.lang.Throwable', 'java.lang.Exception', 'java.lang.RuntimeException', 'java.lang.Error') or (
            simple.endswith(('Exception', 'Error')) and pkg.startswith('java')):
        return 'crate::java::lang::Throwable'
    if fqn in ('org.slf4j.Logger', 'org.apache.logging.log4j.Logger'):
        return 'crate::java::Logger'
    if fqn in ('java.util.stream.IntStream',):
        return 'crate::java::util::stream::Stream<i32>'
    if fqn in ('java.util.stream.LongStream',):
        return 'crate::java::util::stream::Stream<i64>'
    if fqn in ('java.util.stream.DoubleStream',):
        return 'crate::java::util::stream::Stream<f64>'
    if fqn == 'java.lang.Class':
        return 'crate::java::lang::Class'
    m = re.match(r'it\.unimi\.dsi\.fastutil\.\w+\.(Int|Long|Short|Byte|Char|Float|Double|Boolean|Object|Reference)2'
                 r'(Int|Long|Short|Byte|Char|Float|Double|Boolean|Object|Reference)'
                 r'(Open|Linked|Array|AVLTree|RBTree|OpenCustom|LinkedOpen|Sorted|Immutable)?(Hash)?(Sorted)?(Map)$', fqn)
    if m:
        k, v = m.group(1), m.group(2)
        ai = 0
        kt = FASTUTIL_PRIM.get(k)
        if kt is None:
            kt = a(ai)
            ai += 1
        vt = FASTUTIL_PRIM.get(v)
        if vt is None:
            vt = a(ai)
        kind = (m.group(3) or '') + (m.group(5) or '')
        if 'Linked' in kind:
            return f'IndexMap<{kt}, {vt}>'
        if 'Tree' in kind or 'Sorted' in kind:
            return f'BTreeMap<{kt}, {vt}>'
        return f'HashMap<{kt}, {vt}>'
    m = re.match(r'it\.unimi\.dsi\.fastutil\.\w+\.(Int|Long|Short|Byte|Char|Float|Double|Boolean)'
                 r'(Open|Linked|Array|AVLTree|RBTree|LinkedOpen|Immutable|Big)?(Hash)?(Sorted)?'
                 r'(Set|List|Collection|Iterable|ArrayFIFOQueue|PriorityQueue|Stack|HeapPriorityQueue)$', fqn)
    if m:
        p = FASTUTIL_PRIM[m.group(1)]
        coll = m.group(5)
        mid = (m.group(2) or '') + (m.group(4) or '')
        if coll == 'Set':
            if 'Linked' in mid:
                return f'IndexSet<{p}>'
            if 'Tree' in mid or 'Sorted' in mid:
                return f'BTreeSet<{p}>'
            return f'HashSet<{p}>'
        if coll in ('ArrayFIFOQueue',):
            return f'VecDeque<{p}>'
        if 'PriorityQueue' in coll:
            return f'crate::java::util::PriorityQueue<{p}>'
        return f'Vec<{p}>'
    m = re.match(r'it\.unimi\.dsi\.fastutil\.\w+\.(Int|Long|Short|Byte|Char|Float|Double|Boolean)(Iterator|ListIterator|BidirectionalIterator)$', fqn)
    if m:
        return f'crate::java::util::JIterator<{FASTUTIL_PRIM[m.group(1)]}>'
    if fqn in ('java.util.Iterator', 'java.util.ListIterator', 'java.util.Spliterator',
               'it.unimi.dsi.fastutil.objects.ObjectIterator', 'it.unimi.dsi.fastutil.objects.ObjectListIterator',
               'it.unimi.dsi.fastutil.objects.ObjectBidirectionalIterator', 'com.google.common.collect.UnmodifiableIterator',
               'com.google.common.collect.PeekingIterator', 'com.google.common.collect.AbstractIterator'):
        return f'crate::java::util::JIterator<{a(0)}>'
    # Everything else mirrors its Java package under crate:: (implemented by the runtime and library ports).
    path = 'crate::' + '::'.join(safe_ident(s) for s in pkg.split('.')) + '::' + simple.replace('$', '')
    if pkg.split('.')[-1][:1].isupper():   # nested in an external type: Outer.Inner -> OuterInner in Outer's package
        opk = pkg.rsplit('.', 1)
        path = 'crate::' + '::'.join(safe_ident(s) for s in opk[0].split('.')) + '::' + opk[1] + simple
    if args:
        path += '<' + ', '.join(args) + '>'
    if fqn in EXT_IMPLEMENTED:
        return f'JRef<dyn {path}>'
    return path


class TCtx:
    def __init__(self, t, tvars):
        self.t, self.tvars = t, set(tvars)


def rtype(n, ctx, dims=0, nullable=False, generic_arg=False):
    s = rtype0(n, ctx, generic_arg)
    for _ in range(dims):
        s = f'Vec<{s}>'
    if nullable and not s.startswith('Option<') and s != '()':
        s = f'Option<{s}>'
    return s


def rtype0(n, ctx, generic_arg=False):
    if n is None:
        return '()'
    ty = n.type
    if ty in ('integral_type', 'floating_point_type', 'boolean_type', 'void_type'):
        return PRIMS[txt(n)]
    if ty == 'array_type':
        el = n.child_by_field_name('element')
        d = txt(n.child_by_field_name('dimensions')).count('[')
        s = rtype0(el, ctx, True)
        for _ in range(d):
            s = f'Vec<{s}>'
        return s
    if ty == 'annotated_type':
        inner = [k for k in n.children if k.is_named and k.type not in ('marker_annotation', 'annotation')]
        nullable = any(k.type in ('marker_annotation', 'annotation') and 'Nullable' in txt(k) for k in n.children)
        s = rtype0(inner[-1], ctx, generic_arg)
        return f'Option<{s}>' if nullable and not generic_arg else s
    if ty == 'wildcard':
        named = [k for k in n.children if k.is_named and k.type not in ('marker_annotation', 'annotation')]
        if named and any(k.type == 'extends' or txt(k) == 'extends' for k in n.children):
            return rtype0(named[-1], ctx, True)
        return 'JObjectRef'
    args = []
    base = n
    if ty == 'generic_type':
        base = [k for k in n.children if k.type in ('type_identifier', 'scoped_type_identifier')][0]
        ta = [k for k in n.children if k.type == 'type_arguments']
        if ta:
            args = [rtype0(k, ctx, True) for k in ta[0].children if k.is_named and k.type not in ('marker_annotation', 'annotation')]
    name = strip_generics(txt(base))
    if ty == 'scoped_type_identifier' and '<' in txt(n):
        # Outer<A>.Inner: drop the outer arguments
        name = strip_generics(txt(n))
    if tvn(name) in ctx.tvars:
        return tvn(name)
    fqn = resolve(name, ctx.t, ctx.tvars)
    if fqn is None:
        return name
    return ref_type(fqn, args)


def ref_type(fqn, args):
    if fqn in CRAFT_MAP:
        fqn = CRAFT_MAP[fqn]
    t = TYPES.get(fqn)
    if t is None or fqn not in RPATH or t.kind == 'annotation':
        if fqn.startswith('?'):
            return 'JObjectRef /* ' + fqn[1:] + ' */'
        EXTERNAL[fqn] += 1
        EXT_ARITY[fqn] = max(EXT_ARITY.get(fqn, 0), len(args))
        return ext_rust(fqn, args)
    exp = len(t.tparams)
    if len(args) != exp:
        args = (args + ['JObjectRef'] * exp)[:exp]
    g = '<' + ', '.join(args) + '>' if args else ''
    if t.kind == 'interface':
        return f'JRef<dyn {RPATH[fqn]}{g}>'
    if fqn in VALUE:
        return f'{RPATH[fqn]}{g}'
    if fqn in POLY:
        return f'JRef<dyn {RPATH[fqn]}Trait{g}>'
    return f'JRef<{RPATH[fqn]}{g}>'


def type_bound_rust(b, ctx):
    return ': JGeneric'


# ---------------------------------------------------------------------------------------------- methods
METHOD_NAMES = {}   # fqn -> {sigkey: rust name}  (instance + inherited)
OWN_RUST = {}       # fqn -> list of (method dict, rust name)


def sigkey(m, t):
    tv = set(p[0] for p in t.tparams) | set(p[0] for p in m['tparams'])
    parts = []
    for p in m['params']:
        nm = strip_generics(txt(p['type']))
        nm = nm.split('.')[-1]
        if nm in tv:
            nm = 'Object'
        parts.append(nm + '[]' * p['dims'])
    return m['name'] + '(' + ','.join(parts) + ')'


def compute_method_names():
    order, seen = [], set()

    def visit(fq):
        if fq in seen or fq not in TYPES:
            return
        seen.add(fq)
        t = TYPES[fq]
        for s in supertypes(t):
            visit(s)
        order.append(fq)
    for fq in TYPES:
        visit(fq)
    for fq in order:
        t = TYPES[fq]
        inherited = {}
        for s in supertypes(t):
            for k, v in METHOD_NAMES.get(s, {}).items():
                inherited.setdefault(k, v)
        names = dict(inherited)
        taken = set(names.values())
        own = []
        news = []
        by_name_arity = collections.defaultdict(list)
        for k, v in inherited.items():
            by_name_arity[(k.split('(')[0], k.count(',') + 1 if '()' not in k else 0)].append(v)
        for m in t.methods:
            k = sigkey(m, t)
            if 'static' in m['mods']:
                news.append((m, k))
                continue
            if k in inherited:
                own.append((m, inherited[k]))
                continue
            cands = by_name_arity.get((m['name'], len(m['params'])), [])
            if len(set(cands)) == 1:
                names[k] = cands[0]
                own.append((m, cands[0]))
                continue
            news.append((m, k))
        groups = collections.defaultdict(list)
        for m, k in news:
            groups[m['name']].append((m, k))
        for jn, lst in groups.items():
            lst.sort(key=lambda mk: (len(mk[0]['params']), mk[0]['line']))
            for i, (m, k) in enumerate(lst):
                base = snake(jn)
                nm = base
                if nm in taken or (i > 0):
                    nm = f'{base.rstrip("_")}_{len(m["params"])}' if i > 0 or nm in taken else nm
                    j = 2
                    while nm in taken:
                        nm = f'{base.rstrip("_")}_{len(m["params"])}_{j}'
                        j += 1
                taken.add(nm)
                if 'static' not in m['mods']:
                    names[k] = nm
                own.append((m, nm))
        METHOD_NAMES[fq] = names
        OWN_RUST[fq] = own


# ---------------------------------------------------------------------------------------------- emit
LIT_RE = re.compile(r"^[-+~()\s\dxXa-fA-FlLfFdD._|&^<>*/%]+$")


def java_literal(v, rt):
    v = v.strip()
    if v in ('true', 'false'):
        return v if rt == 'bool' else None
    if v.startswith('"') and v.endswith('"') and not v.startswith('"""') and rt == 'String' and '" +' not in v and '+ "' not in v:
        return re.sub(r'\\u([0-9a-fA-F]{4})', r'\\u{\1}', v)
    if re.fullmatch(r"'(\\.|[^'])'", v) or re.fullmatch(r"'\\u[0-9a-fA-F]{4}'", v):
        return re.sub(r'\\u([0-9a-fA-F]{4})', r'\\u{\1}', v) if rt == 'char' else None
    if rt in ('i32', 'i64', 'i16', 'i8', 'f32', 'f64') and LIT_RE.match(v) and re.search(r'\d', v):
        if re.search(r'[a-zA-Z_]\w*', re.sub(r'0[xX][0-9a-fA-F_]+[lL]?|\d[\d_]*\.?\d*([eE][-+]?\d+)?[fFdDlL]?', '', v)):
            return None
        if '>>>' in v:
            return None
        s = re.sub(r'(0[xX][0-9a-fA-F_]+|\d[\d_]*(\.\d+)?([eE][-+]?\d+)?)[lLfFdD]\b', r'\1', v)
        s = re.sub(r'(\d)\.(?!\d)', r'\1.0', s)
        if rt in ('f32', 'f64'):
            s = re.sub(r'(?<![\w.])(\d+)(?![\w.]|\.\d)', r'\1.0', s) if not re.search(r'0[xX]', s) else s
        if rt in ('i32', 'i16', 'i8') and re.search(r'0[xX][89a-fA-F][0-9a-fA-F]{7}\b', s):
            s = re.sub(r'(0[xX][0-9a-fA-F]{8})\b', r'(\1u32 as i32)', s)
        if rt == 'i64' and re.search(r'0[xX][89a-fA-F][0-9a-fA-F]{15}\b', s):
            s = re.sub(r'(0[xX][0-9a-fA-F]{16})\b', r'(\1u64 as i64)', s)
        return s
    return None


def const_type(rt):
    return '&str' if rt == 'String' else rt


class Emitter:
    def __init__(self, f):
        self.f = f
        self.lines = []

    def w(self, s=''):
        self.lines.append(s)


def params_rust(m, ctx, value_mut=False):
    out = []
    used = set()
    for p in m['params']:
        nm = snake(p['name']) if p['name'] else 'arg'
        if nm in used:
            nm += '_'
        used.add(nm)
        nullable = 'Nullable' in p['ann']
        rt = rtype(p['type'], ctx, p['dims'], nullable)
        if rt == 'String':
            rt = '&str'
        elif rt.startswith('Vec<') and re.search(r'\b' + re.escape(p['name']) + r'\s*\.\s*(add|addAll|remove|removeIf|clear|set|sort|push|poll|offer|put)\b', m.get('body_text', '')):
            rt = '&mut ' + rt
        elif rt.startswith(('Vec<', 'HashMap<', 'HashSet<', 'IndexMap<', 'IndexSet<', 'BTreeMap<', 'BTreeSet<', 'VecDeque<')):
            if re.search(r'\b' + re.escape(p['name']) + r'\s*\.\s*(add|addAll|remove|removeIf|clear|set|sort|push|poll|offer|put|putAll|computeIfAbsent|merge|compute)\b', m.get('body_text', '')):
                rt = '&mut ' + rt
            else:
                rt = '&' + rt
        out.append((nm, rt))
    return out


def method_tparams(m, t):
    tps = [tvn(p[0]) for p in m['tparams'] if p[0]]
    return tps


def generics_decl(names, extra=None):
    names = [n for n in names if n]
    if not names and not extra:
        return ''
    parts = [f'{n}: JGeneric' for n in names]
    if extra:
        parts = extra + parts
    return '<' + ', '.join(parts) + '>'


def ret_rust(m, ctx):
    nullable = 'Nullable' in m['ann']
    r = rtype(m['ret'], ctx, m['ret_dims'], nullable)
    if m['throws']:
        r = f'Result<{r}, crate::java::lang::Throwable>'
    return r


def sig(m, t, rname, recv, ctx, extra_generics=None):
    ps = params_rust(m, ctx)
    g = generics_decl(method_tparams(m, t), extra_generics)
    args = ([recv] if recv else []) + [f'{n}: {ty}' for n, ty in ps]
    r = ret_rust(m, ctx)
    rr = '' if r == '()' else f' -> {r}'
    return f'fn {rname}{g}({", ".join(args)}){rr}', [n for n, _ in ps]


def tvn(n):
    return n + 'T' if n in RUST_KW or n in ('Self',) else n


def type_generics(t):
    return [tvn(p[0]) for p in t.tparams]


def tg_decl(t):
    tps = type_generics(t)
    return ('<' + ', '.join(f'{p}: JGeneric' for p in tps) + '>') if tps else ''


def tg_use(t):
    tps = type_generics(t)
    return ('<' + ', '.join(tps) + '>') if tps else ''


def ancestors(t):
    """Superclass chain (in-tree classes only), nearest first."""
    out = []
    cur = t
    seen = set()
    while True:
        s = superclass(cur)
        if s in CRAFT_MAP or s not in TYPES or s in seen:
            break
        seen.add(s)
        cur = TYPES[s]
        if cur.kind != 'class':
            break
        out.append(cur)
    return out


def all_interfaces(t):
    out, seen = [], set()

    def add(i):
        if i in seen or i not in TYPES or TYPES[i].kind != 'interface':
            return
        seen.add(i)
        out.append(i)
        for s in interfaces(TYPES[i]):
            add(s)
    for c in [t] + ancestors(t):
        for i in interfaces(c):
            add(i)
    return out


def trait_methods(c):
    """Virtual methods a polymorphic class or interface adds to its trait: [(method, rustname)]."""
    out = []
    inherited = set()
    for s in supertypes(c):
        inherited.update(METHOD_NAMES.get(s, {}).values())
    for m, rn in OWN_RUST.get(c.fqn, []):
        if 'static' in m['mods'] or 'private' in m['mods']:
            continue
        if c.kind == 'class' and 'final' in m['mods']:
            continue
        if rn in inherited:
            continue
        if m['tparams'] and c.kind == 'class':
            continue   # generic methods are not dyn-compatible: they stay inherent on the class
        out.append((m, rn))
    return out


def declared_impl(c, rn):
    for m, n in OWN_RUST.get(c.fqn, []):
        if n == rn and 'static' not in m['mods'] and 'abstract' not in m['mods'] and m['body']:
            return m
    return None


def emit_file(f, out_root):
    segs = MODPATH[f.path]
    e = Emitter(f)
    src_rel = f.rel
    e.w(f'//! Port of `{f.types[0] if f.types else f.package}` ({src_rel}).')
    e.w('//! Generated by tools/leaf_port/gen.py; bodies are ported by hand from the Java file.')
    e.w('use crate::prelude::*;')
    if f.source_set in ('api', 'craft'):
        e.w('use crate::api_prelude::*;')
    e.w('')
    for top in f.types:
        for t in iter_types(TYPES[top]):
            if t.kind == 'annotation':
                continue
            if f.source_set == 'craft' and t.fqn in CRAFT_MAP:
                e.w(f'// {t.fqn} is a CraftBukkit wrapper: its API ({CRAFT_MAP[t.fqn]}) is implemented on the NMS type.')
                continue
            emit_type(e, t)
    path = os.path.join(out_root, *segs) + '.rs'
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w') as fh:
        fh.write(shorten('\n'.join(e.lines) + '\n', segs))
    return segs


PATH_RE = re.compile(r'\bcrate::(?:[a-z_][a-z0-9_]*::)+[A-Z][A-Za-z0-9_]*')


def shorten(text, segs):
    """Import each crate type once (`use`) and refer to it by its short name, as the Java file does."""
    here = 'crate::' + '::'.join(segs) + '::'
    local = set(re.findall(r'^pub (?:struct|enum|trait) (\w+)', text, re.M))
    paths = sorted(set(PATH_RE.findall(text)))
    by_short = collections.defaultdict(set)
    for p in paths:
        by_short[p.rsplit('::', 1)[1]].add(p)
    uses = []
    repl = {}
    for short, ps in by_short.items():
        if len(ps) != 1:
            continue
        p = next(iter(ps))
        if p.startswith(here) and p[len(here):] == short:
            repl[p] = short
            continue
        if short in local or short in RESERVED_TYPES:
            continue
        repl[p] = short
        uses.append(p)
    if repl:
        text = PATH_RE.sub(lambda m: repl.get(m.group(0), m.group(0)), text)
    if uses:
        head, rest = text.split('use crate::prelude::*;\n', 1)
        text = head + 'use crate::prelude::*;\n' + ''.join(f'use {u};\n' for u in sorted(uses)) + rest
    return text


def jline(t, line):
    return f'{t.file.rel}:{line}'


def emit_type(e, t):
    tv = type_generics(t)
    ctx = TCtx(t, tv)
    name = RNAME[t.fqn]
    G, GU = tg_decl(t), tg_use(t)
    static_prefix = '' if t.outer is None else upper_snake(name[len(RNAME[top_of(t).fqn]):]) + '_'
    e.w(f'// ---- {t.kind} {t.fqn} ({jline(t, t.line)})')
    if t.kind == 'interface':
        emit_interface(e, t, ctx, name, G, GU, static_prefix)
        return
    poly = t.fqn in POLY
    value = t.fqn in VALUE
    sup = superclass(t)
    sup_t = TYPES.get(sup) if sup and sup not in CRAFT_MAP and sup in RPATH else None
    if sup_t is not None and sup_t.kind != 'class':
        sup_t = None
    # ---- struct / enum
    if t.kind == 'enum':
        e.w('#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]')
        e.w(f'pub enum {name} {{')
        for c in t.consts:
            e.w(f'    {c["name"]},' + (f' // {" ".join(c["args"].split())}' if c['args'] else '') + (' // has a body' if c['body'] else ''))
        e.w('}')
        inst = [fl for fl in t.fields if 'static' not in fl['mods']]
        if inst:
            e.w(f'/// The fields each {name} constant is built with (Java: the enum constructor).')
            e.w(f'pub struct {name}Data {{')
            for fl in inst:
                e.w(f'    pub {snake(fl["name"])}: {field_type(fl, ctx, value=True)},')
            e.w('}')
    else:
        derive = []
        if value:
            derive = ['Clone'] + (['Copy', 'PartialEq', 'Eq', 'Hash', 'Debug', 'Default'] if t.fqn in COPY and not any(
                fl['type'] is not None and fl['type'].type == 'floating_point_type' for fl in t.fields if 'static' not in fl['mods']) else (['Copy', 'PartialEq', 'Debug', 'Default'] if t.fqn in COPY else []))
        if derive:
            e.w(f'#[derive({", ".join(derive)})]')
        e.w(f'pub struct {name}{G} {{')
        if sup_t is not None:
            e.w(f'    pub base: {RPATH[sup_t.fqn]}{generic_args_of(t.ext, ctx, sup_t)},')
        if not value:
            if poly:
                e.w(f'    pub this: Weak<dyn {name}Trait{GU}>,')
            else:
                e.w(f'    pub this: Weak<Self>,')
            if t.outer is not None and 'static' not in t.mods and t.outer.kind == 'class' and t.kind == 'class':
                e.w(f'    /// The enclosing instance (Java inner class).')
                e.w(f'    pub outer: Weak<{weak_target(t.outer)}>,')
        for c in t.components:
            e.w(f'    pub {snake(c["name"])}: {rtype(c["type"], ctx, c["dims"], "Nullable" in c["ann"])},')
        for fl in t.fields:
            if 'static' in fl['mods']:
                continue
            ft = field_type(fl, ctx, value=value)
            if ft is None:
                continue
            e.w(f'    pub {snake(fl["name"])}: {ft}, // {jline(t, fl["line"])}')
        if tv:
            e.w(f'    pub _phantom: std::marker::PhantomData<({", ".join(tv)},)>,')
        e.w('}')
        if sup_t is not None:
            e.w(f'impl{G} std::ops::Deref for {name}{GU} {{')
            e.w(f'    type Target = {RPATH[sup_t.fqn]}{generic_args_of(t.ext, ctx, sup_t)};')
            e.w('    fn deref(&self) -> &Self::Target { &self.base }')
            e.w('}')
            if value:
                e.w(f'impl{G} std::ops::DerefMut for {name}{GU} {{')
                e.w('    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }')
                e.w('}')
    # ---- trait for polymorphic classes
    if poly:
        supers = []
        if sup_t is not None and sup_t.fqn in POLY:
            supers.append(f'{RPATH[sup_t.fqn]}Trait{generic_args_of(t.ext, ctx, sup_t)}')
        for n in t.impls:
            r = rtype0(n, ctx)
            if r.startswith('JRef<dyn '):
                supers.append(r[len('JRef<dyn '):-1])
        if not supers:
            supers = ['JObject']
        e.w(f'pub trait {name}Trait{G}: {" + ".join(supers)} {{')
        e.w(f'    fn as_{snake(name).rstrip("_")}(&self) -> &{name}{GU};')
        for m, rn in trait_methods(t):
            mctx = TCtx(t, tv + method_tparams(m, t))
            s, _ = sig(m, t, rn, '&self', mctx)
            e.w(f'    {s}; // {jline(t, m["line"])}')
        e.w('}')
        e.w(f'impl{G} std::ops::Deref for dyn {name}Trait{GU} {{')
        e.w(f'    type Target = {name}{GU};')
        e.w(f'    fn deref(&self) -> &{name}{GU} {{ self.as_{snake(name).rstrip("_")}() }}')
        e.w('}')
    # ---- statics
    emit_statics(e, t, ctx, static_prefix)
    # ---- inherent impl
    e.w(f'impl{G} {name}{GU} {{')
    if t.kind == 'enum':
        e.w(f'    pub const VALUES: [{name}; {len(t.consts)}] = [{", ".join(name + "::" + c["name"] for c in t.consts)}];')
        e.w('    pub fn ordinal(self) -> i32 { self as i32 }')
        e.w('    pub fn name(self) -> &\'static str {')
        e.w('        match self {' + ' '.join(f'{name}::{c["name"]} => "{c["name"]}",' for c in t.consts) + ' }')
        e.w('    }')
        e.w(f'    pub fn value_of(name: &str) -> Option<{name}> {{ Self::VALUES.into_iter().find(|v| v.name() == name) }}')
        if any('static' not in fl['mods'] for fl in t.fields):
            e.w(f'    /// The constructor arguments of each constant (see the Java enum constants).')
            e.w(f'    pub fn data(self) -> &\'static {name}Data {{ todo!("{jline(t, t.line)}") }}')
    elif not value:
        if poly:
            e.w(f'    pub fn this(&self) -> JRef<dyn {name}Trait{GU}> {{ self.this.upgrade().expect("this") }}')
        else:
            e.w(f'    pub fn this(&self) -> JRef<Self> {{ self.this.upgrade().expect("this") }}')
    # constructors
    ctors = t.ctors or [{'params': [], 'mods': set(), 'throws': False, 'line': t.line, 'compact': False, 'implicit': True}]
    if t.kind == 'record':
        ctors = [c for c in ctors if not c.get('compact') and c['params'] is not None]
        canon = {'params': t.components, 'mods': {'public'}, 'throws': False, 'line': t.line, 'compact': False}
        if not any(len(c['params']) == len(t.components) for c in ctors):
            ctors = [canon] + ctors
    ctor_names = []
    if t.kind != 'enum':
        ctors = sorted(ctors, key=lambda c: (len(c['params'] or []), c['line']))
        used = set()
        for c in ctors:
            n = len(c['params'] or [])
            suffix = '' if not used else f'_{n}'
            while suffix in used:
                suffix += '_'
            used.add(suffix)
            ctor_names.append((c, suffix))
        for c, suffix in ctor_names:
            m = {'params': c['params'] or [], 'tparams': [], 'ann': set(), 'throws': c['throws'], 'ret': None,
                 'ret_dims': 0, 'body_text': ''}
            ps = params_rust(m, ctx)
            pdecl = ', '.join(f'{n}: {ty}' for n, ty in ps)
            note = f'// {jline(t, c["line"])}'
            res = lambda ty: f'Result<{ty}, crate::java::lang::Throwable>' if c['throws'] else ty
            if value:
                e.w(f'    pub fn new{suffix}({pdecl}) -> {res("Self")} {{ todo!() }} {note}')
            elif poly:
                tr = f'{name}Trait{GU}'
                e.w(f'    pub fn new_base{suffix}<S: {tr} + \'static>(this: &Weak<S>{", " if pdecl else ""}{pdecl}) -> {res("Self")} {{ todo!() }} {note}')
                if 'abstract' not in t.mods:
                    args = ', '.join(n for n, _ in ps)
                    q = '?' if c['throws'] else ''
                    body = (f'Arc::try_new_cyclic' if False else None)
                    if c['throws']:
                        e.w(f'    pub fn new{suffix}({pdecl}) -> {res("JRef<Self>")} {{ todo!() }}')
                    else:
                        e.w(f'    pub fn new{suffix}({pdecl}) -> JRef<Self> {{ Arc::new_cyclic(|this| Self::new_base{suffix}(this{", " if args else ""}{args})) }}')
            else:
                e.w(f'    pub fn new{suffix}({pdecl}) -> {res("JRef<Self>")} {{ todo!() }} {note}')
    # methods
    for m, rn in OWN_RUST.get(t.fqn, []):
        if 'abstract' in m['mods'] or not m['body']:
            continue
        mctx = TCtx(t, tv + method_tparams(m, t))
        if 'static' in m['mods']:
            recv = None
        elif value and t.fqn in MUTVALUE and re.search(r'\bthis\.\w+\s*(=[^=]|\+=|-=|\+\+|--)', m['body_text']):
            recv = '&mut self'
        elif t.kind == 'enum':
            recv = 'self'
        else:
            recv = '&self'
        s, _ = sig(m, t, rn, recv, mctx)
        e.w(f'    pub {s} {{ todo!() }} // {jline(t, m["line"])}')
    e.w('}')
    # ---- glue: hierarchy traits for concrete classes
    if t.kind in ('class', 'enum', 'record') and 'abstract' not in t.mods:
        emit_glue(e, t, ctx, name, G, GU)
    emit_jobject(e, t, name, G, GU)


def top_of(t):
    while t.outer is not None:
        t = t.outer
    return t


def weak_target(t):
    if t.fqn in POLY:
        return f'dyn {RNAME[t.fqn]}Trait{tg_use(t)}'
    return f'{RNAME[t.fqn]}{tg_use(t)}'


def generic_args_of(ext_node, ctx, sup_t):
    if not sup_t.tparams:
        return ''
    args = []
    if ext_node is not None and ext_node.type == 'generic_type':
        ta = [k for k in ext_node.children if k.type == 'type_arguments']
        if ta:
            args = [rtype0(k, ctx, True) for k in ta[0].children if k.is_named]
    args = (args + ['JObjectRef'] * len(sup_t.tparams))[:len(sup_t.tparams)]
    return '<' + ', '.join(args) + '>'


def field_type(fl, ctx, value=False):
    rt = rtype(fl['type'], ctx, fl['dims'], 'Nullable' in fl['ann'])
    if rt == 'crate::java::Logger':
        return None
    if value:
        return rt
    final = 'final' in fl['mods']
    interior = rt.startswith(('std::sync::atomic::', 'dashmap::', 'crate::java::util::concurrent::'))
    if interior:
        return rt
    mutable_value = any(rt == RPATH.get(v) for v in MUTVALUE)
    if not final or rt.startswith(COLLECTION_PREFIXES) or mutable_value or 'volatile' in fl['mods']:
        return f'Mut<{rt}>'
    return rt


def emit_statics(e, t, ctx, prefix):
    for fl in t.fields:
        if 'static' not in fl['mods']:
            continue
        rt = rtype(fl['type'], ctx, fl['dims'], 'Nullable' in fl['ann'])
        if rt == 'crate::java::Logger':
            continue
        nm = prefix + upper_snake(fl['name'])
        if nm in RUST_KW:
            nm += '_'
        note = f'// {jline(t, fl["line"])}'
        final = 'final' in fl['mods']
        lit = java_literal(fl['value'], rt) if fl['value'] is not None else None
        if final and lit is not None:
            e.w(f'pub const {nm}: {const_type(rt)} = {lit}; {note}')
        elif not final and lit is not None and rt != 'String':
            e.w(f'pub static {nm}: Mut<{rt}> = Mut::new({lit}); {note}')
        elif final:
            e.w(f'pub static {nm}: LazyLock<{rt}> = LazyLock::new(|| todo!()); {note}')
        else:
            e.w(f'pub static {nm}: LazyLock<Mut<{rt}>> = LazyLock::new(|| todo!()); {note}')


def is_sam(t):
    abstract = [(m, rn) for m, rn in trait_methods(t) if not m['body']]
    inherited_abstract = any(
        any(not mm['body'] for mm, _ in trait_methods(TYPES[s])) for s in interfaces(t) if s in TYPES)
    supers = [i for i in interfaces(t) if i]
    return len(abstract) == 1 and not inherited_abstract and not supers and not abstract[0][0]['tparams']


def emit_interface(e, t, ctx, name, G, GU, prefix):
    supers = []
    for n in t.impls:
        r = rtype0(n, ctx)
        if r.startswith('JRef<dyn '):
            supers.append(r[len('JRef<dyn '):-1])
    sam = is_sam(t)
    supers = supers or (['Send + Sync + \'static'] if sam else ['JObject'])
    e.w(f'pub trait {name}{G}: {" + ".join(supers)} {{')
    for m, rn in trait_methods(t):
        mctx = TCtx(t, type_generics(t) + method_tparams(m, t))
        s, _ = sig(m, t, rn, '&self', mctx)
        if m['tparams']:
            s += ' where Self: Sized'
        if m['body']:
            e.w(f'    {s} {{ todo!() }} // default, {jline(t, m["line"])}')
        else:
            e.w(f'    {s}; // {jline(t, m["line"])}')
    e.w('}')
    # A single-abstract-method interface takes Rust closures, as Java takes lambdas.
    if sam:
        m, rn = [(m, rn) for m, rn in trait_methods(t) if not m['body']][0]
        ps = params_rust(m, ctx)
        r = ret_rust(m, ctx)
        fn_args = ', '.join(ty for _, ty in ps)
        s, names = sig(m, t, rn, '&self', ctx)
        tps = type_generics(t)
        gdecl = ''.join(f'{p}: JGeneric, ' for p in tps)
        e.w(f'impl<{gdecl}F: Fn({fn_args}){" -> " + r if r != "()" else ""} + Send + Sync + \'static> {name}{GU} for F {{')
        e.w(f'    {s} {{ self({", ".join(names)}) }}')
        e.w('}')
    statics = [m for m, rn in OWN_RUST.get(t.fqn, []) if 'static' in m['mods']]
    emit_statics(e, t, ctx, prefix)
    fnprefix = '' if t.outer is None else snake(name[len(RNAME[top_of(t).fqn]):]).strip('_') + '_'
    for m, rn in OWN_RUST.get(t.fqn, []):
        if 'static' not in m['mods']:
            continue
        mctx = TCtx(t, method_tparams(m, t))
        s, _ = sig(m, t, fnprefix + rn, None, mctx)
        e.w(f'pub {s} {{ todo!() }} // {jline(t, m["line"])}')


def emit_glue(e, t, ctx, name, G, GU):
    chain = [t] + ancestors(t)
    polys = [c for c in chain if c.fqn in POLY]
    lines = []
    for c in polys:
        tr = RPATH[c.fqn] + 'Trait' + (generic_args_of(None, ctx, c) if c.tparams else '')
        lines.append(f'impl{G} {tr} for {name}{GU} {{')
        lines.append(f'    fn as_{snake(RNAME[c.fqn]).rstrip("_")}(&self) -> &{RPATH[c.fqn]}{generic_args_of(None, ctx, c) if c.tparams else ""} {{ self }}')
        for m, rn in trait_methods(c):
            owner = None
            for x in chain:
                if declared_impl(x, rn):
                    owner = x
                    break
                if x is c:
                    break
            mctx = TCtx(c, type_generics(c) + method_tparams(m, c))
            s, names = sig(m, c, rn, '&self', mctx)
            if owner is None:
                lines.append(f'    {s} {{ todo!("abstract {c.name}.{m["name"]}") }}')
            else:
                tp = method_tparams(m, c)
                turbofish = ('::<' + ', '.join(tp) + '>') if tp else ''
                lines.append(f'    {s} {{ {RPATH[owner.fqn]}::{rn}{turbofish}(self{"".join(", " + n for n in names)}) }}')
        lines.append('}')
    for i in all_interfaces(t):
        it = TYPES[i]
        args = ''
        # find the type arguments the class gives this interface (direct implements only)
        for c in chain:
            for n in c.impls:
                if resolve(strip_generics(txt(n)), c, type_generics(c)) == i and n.type == 'generic_type':
                    cctx = TCtx(c, type_generics(c))
                    ta = [k for k in n.children if k.type == 'type_arguments']
                    if ta:
                        args = '<' + ', '.join(rtype0(k, cctx, True) for k in ta[0].children if k.is_named) + '>'
        if it.tparams and not args:
            args = '<' + ', '.join(['JObjectRef'] * len(it.tparams)) + '>'
        elif not it.tparams:
            args = ''
        lines.append(f'impl{G} {RPATH[i]}{args} for {name}{GU} {{')
        for m, rn in trait_methods(it):
            owner = None
            for x in chain:
                if declared_impl(x, rn):
                    owner = x
                    break
            if owner is None:
                if not m['body']:
                    mctx = TCtx(it, type_generics(it) + method_tparams(m, it))
                    s, _ = sig(m, it, rn, '&self', mctx)
                    if m['tparams']:
                        s += ' where Self: Sized'
                    lines.append(f'    {s} {{ todo!("abstract {it.name}.{m["name"]}") }}')
                continue
            mctx = TCtx(it, type_generics(it) + method_tparams(m, it))
            s, names = sig(m, it, rn, '&self', mctx)
            if m['tparams']:
                s += ' where Self: Sized'
            tp = method_tparams(m, it)
            turbofish = ('::<' + ', '.join(tp) + '>') if tp else ''
            recv = 'self' if TYPES[owner.fqn].kind != 'enum' else '*self'
            lines.append(f'    {s} {{ {RPATH[owner.fqn]}::{rn}{turbofish}({recv}{"".join(", " + n for n in names)}) }}')
        lines.append('}')
    if lines:
        e.w('// ---- generated hierarchy glue (tools/leaf_port/gen.py): forwards each virtual method to the class that')
        e.w('// ---- declares it nearest to this one; keep the signatures in step with the trait.')
        for l in lines:
            e.w(l)


def emit_jobject(e, t, name, G, GU):
    if t.tparams:
        return   # generic types: JObject is implemented by hand where needed
    targets = []
    if t.kind != 'interface' and t.fqn not in VALUE and 'abstract' not in t.mods:
        for c in [t] + ancestors(t):
            if c.fqn in POLY and not c.tparams:
                targets.append(f'dyn {RPATH[c.fqn]}Trait')
        for i in all_interfaces(t):
            if not TYPES[i].tparams:
                targets.append(f'dyn {RPATH[i]}')
        e.w(f'crate::jobject!({name}{": " + ", ".join(targets) if targets else ""});')
    elif t.kind in ('record', 'enum') or t.fqn in VALUE:
        targets = [f'dyn {RPATH[i]}' for i in all_interfaces(t) if not TYPES[i].tparams]
        e.w(f'crate::jobject!({name}{": " + ", ".join(targets) if targets else ""});')


# ---------------------------------------------------------------------------------------------- driver
def source_sets(leaf):
    sets = [
        ('nms', os.path.join(leaf, 'leaf-server/src/minecraft/java'), None),
        ('leaf', os.path.join(leaf, 'leaf-server/src/main/java'), None),
        ('paper', os.path.join(leaf, 'paper-server/src/main/java'), 'org/bukkit/craftbukkit'),
        ('craft', os.path.join(leaf, 'paper-server/src/main/java/org/bukkit/craftbukkit'), None),
        ('api', os.path.join(leaf, 'paper-api/src/main/java'), None),
        ('api', os.path.join(leaf, 'leaf-api/src/main/java'), None),
    ]
    out = []
    for name, root, excl in sets:
        for dp, dn, fn in os.walk(root):
            dn[:] = [d for d in dn if not d.startswith('.')]
            rel = os.path.relpath(dp, root)
            if excl and (rel == excl or rel.startswith(excl + '/')):
                continue
            for x in fn:
                if x.endswith('.java') and x not in ('package-info.java', 'module-info.java'):
                    p = os.path.join(dp, x)
                    out.append((name, p, os.path.relpath(p, leaf)))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--leaf', default=LEAF)
    ap.add_argument('--out', default=os.path.join(ROOT, 'crates/horizon-server/src'))
    ap.add_argument('--index', default=os.path.join(ROOT, '.local/leaf-port'))
    ap.add_argument('--index-only', action='store_true')
    ap.add_argument('--only', help='emit only files whose Leaf-relative path starts with this')
    a = ap.parse_args()
    files = source_sets(a.leaf)
    seen = {}
    for ss, p, rel in files:
        # A class both in the API and the server tree (the same fqn): keep the first (server/NMS) one.
        f = parse_file(p, rel, ss)
    # package tables
    dup = 0
    for f in FILES:
        for top in f.types:
            PKG_TYPES[f.package][TYPES[top].name] = top
    craft_mapping_needed = True
    assign_names()
    compute_kinds()
    craft_mapping()
    # externals implemented by in-tree types are traits, and so are Java's functional interfaces
    for fq in ('java.lang.Runnable', 'java.util.Comparator', 'java.util.concurrent.Callable'):
        EXT_IMPLEMENTED.add(fq)
    for f in FILES:
        for v in f.imports.values():
            if v.startswith('java.util.function.'):
                EXT_IMPLEMENTED.add(v)
    for t in TYPES.values():
        for i in interfaces(t):
            if i and i not in TYPES:
                EXT_IMPLEMENTED.add(i)
    compute_method_names()
    os.makedirs(a.index, exist_ok=True)
    mods = collections.defaultdict(set)
    if not a.index_only:
        for f in FILES:
            if a.only and not f.rel.startswith(a.only):
                continue
            if not f.types:
                continue
            segs = emit_file(f, a.out)
            for i in range(len(segs)):
                mods[tuple(segs[:i])].add(segs[i])
        write_mods(mods, a.out)
    write_index(a.index)
    print(f'types {len(TYPES)} files {len(FILES)} poly {len(POLY)} value {len(VALUE)} externals {len(EXTERNAL)}',
          file=sys.stderr)


RUNTIME_EXTRAS = {
    # path -> (arity, is_trait): runtime types the type mapping names directly
    'crate::java::util::JIterator': (1, False), 'crate::java::util::PriorityQueue': (1, False),
    'crate::java::util::BitSet': (0, False), 'crate::java::util::stream::Stream': (1, False),
    'crate::java::lang::Throwable': (0, False), 'crate::java::lang::Class': (0, False),
}


def ext_stub_table():
    """Every mirrored external type: crate path -> (arity, is_trait)."""
    table = dict(RUNTIME_EXTRAS)
    for fq, n in EXTERNAL.items():
        r = ext_rust(fq, ['A'] * EXT_ARITY.get(fq, 0))
        m = re.search(r'(crate::(?:[a-z_][a-z0-9_]*::)+[A-Z]\w*)', r)
        if not m:
            continue
        path = m.group(1)
        if not r.lstrip('JRef<dyn ').startswith(path) and not r.startswith(path) and not r.startswith('JRef<dyn ' + path):
            continue
        ar = EXT_ARITY.get(fq, 0)
        old = table.get(path, (0, False))
        table[path] = (max(ar, old[0]), old[1] or fq in EXT_IMPLEMENTED)
    return table


def write_mods(mods, out):
    # external stubs, one ext_stubs.rs per package
    stubs = collections.defaultdict(list)
    for path, (ar, is_trait) in ext_stub_table().items():
        segs = path.split('::')[1:-1]
        stubs[tuple(segs)].append((path.rsplit('::', 1)[1], ar, is_trait))
        mods.setdefault(tuple(segs), set())
        for i in range(len(segs)):
            mods[tuple(segs[:i])].add(segs[i])
    for segs, items in stubs.items():
        d = os.path.join(out, *segs)
        os.makedirs(d, exist_ok=True)
        lines = ['//! Stand-ins for the external library types this package\'s users name (tools/leaf_port/gen.py).',
                 '//! Replace each with its port.', 'use crate::prelude::*;', '']
        for name, ar, is_trait in sorted(set(items)):
            gp = [f'A{i}' for i in range(ar)]
            if is_trait:
                g = ('<' + ', '.join(f'{x} = JObjectRef' for x in gp) + '>') if gp else ''
                lines.append(f'pub trait {name}{g}: Send + Sync {{}}')
            else:
                g = ('<' + ', '.join(f'{x} = JObjectRef' for x in gp) + '>') if gp else ''
                gu = ('<' + ', '.join(gp) + '>') if gp else ''
                ph = '(' + ', '.join(f'fn() -> {x}' for x in gp) + (',)' if len(gp) == 1 else ')') if gp else '()'
                lines.append(f'pub struct {name}{g}(pub std::marker::PhantomData<{ph}>);')
                gi = ('<' + ', '.join(gp) + '>') if gp else ''
                lines.append(f'impl{gi} Clone for {name}{gu} {{ fn clone(&self) -> Self {{ Self(std::marker::PhantomData) }} }}')
        open(os.path.join(d, 'ext_stubs.rs'), 'w').write('\n'.join(lines) + '\n')
    for parent, children in mods.items():
        if not parent:
            continue
        d = os.path.join(out, *parent)
        os.makedirs(d, exist_ok=True)
        path = d + '/mod.rs'
        lines = []
        if parent == ('java',):
            lines += ['//! Java\'s runtime as the port uses it (runtime.rs) and the stand-ins for its library.',
                      'pub mod runtime;', 'pub use runtime::*;']
        for c in sorted(children):
            lines.append(f'pub mod {c};')
        if parent in stubs:
            lines += ['pub mod ext_stubs;', 'pub use self::ext_stubs::*;']
        with open(path, 'w') as fh:
            fh.write('\n'.join(lines) + '\n')
            for c in sorted(children):
                fp = os.path.join(d, c + '.rs')
                if os.path.exists(fp):
                    names = re.findall(r'^pub (?:struct|enum|trait) (\w+)', open(fp).read(), re.M)
                    if names:
                        fh.write(f'pub use self::{c}::{{{", ".join(names)}}};\n')
    top = sorted(mods[()])
    with open(os.path.join(out, 'generated_mods.rs'), 'w') as fh:
        fh.write('// The Java package tree (tools/leaf_port/gen.py).\n')
        for c in top:
            fh.write(f'pub mod {c};\n')
    # trait preludes: every trait in scope anonymously, as Java resolves methods on any type
    nms, api = [], []
    for fq, tr in TRAIT.items():
        t = TYPES[fq]
        (api if t.file.source_set == 'api' else nms).append(tr)
    with open(os.path.join(out, 'prelude_traits.rs'), 'w') as fh:
        fh.write('// Every server-side trait, imported anonymously so its methods resolve anywhere (gen.py).\n')
        for tr in sorted(nms):
            fh.write(f'pub use {tr} as _;\n')
    with open(os.path.join(out, 'api_prelude.rs'), 'w') as fh:
        fh.write('// Every Bukkit/Paper API trait, imported anonymously in API code (gen.py).\n')
        for tr in sorted(api):
            fh.write(f'pub use {tr} as _;\n')


def write_index(d):
    idx = {}
    for fq, t in TYPES.items():
        if fq not in RPATH:
            continue
        ms = []
        for m, rn in OWN_RUST.get(fq, []):
            ms.append({'java': m['name'], 'rust': rn, 'static': 'static' in m['mods'], 'abstract': 'abstract' in m['mods'],
                       'params': [strip_generics(txt(p['type'])) + '[]' * p['dims'] + ' ' + p['name'] for p in m['params']],
                       'ret': strip_generics(txt(m['ret'])) if m['ret'] is not None else 'void', 'line': m['line']})
        idx[fq] = {'rust': RPATH[fq], 'trait': TRAIT.get(fq), 'kind': t.kind, 'file': t.file.rel, 'line': t.line,
                   'set': t.file.source_set, 'poly': fq in POLY, 'value': fq in VALUE,
                   'super': superclass(t) if t.kind == 'class' else None,
                   'interfaces': [i for i in interfaces(t) if i],
                   'fields': [{'java': fl['name'], 'static': 'static' in fl['mods'],
                               'rust': snake(fl['name']) if 'static' not in fl['mods'] else upper_snake(fl['name'])}
                              for fl in t.fields],
                   'methods': ms, 'craft_api': CRAFT_MAP.get(fq)}
    json.dump(idx, open(os.path.join(d, 'index.json'), 'w'))
    json.dump({k: [v, EXT_ARITY.get(k, 0), k in EXT_IMPLEMENTED] for k, v in EXTERNAL.most_common()},
              open(os.path.join(d, 'externals.json'), 'w'), indent=0)


if __name__ == '__main__':
    main()
