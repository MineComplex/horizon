#!/usr/bin/env python3
"""Replaces the shaded-library stand-ins in crates/horizon-server with Rust crate types (user's decision:
only NMS code is ported). Rewrites every reference to a stub type, drops impls/supertraits of Java
interfaces that are now plain Rust types, then deletes the stub packages."""
import os, re, sys, shutil, collections

SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../crates/horizon-server/src')
SRC = os.path.normpath(SRC)
FN = 'Arc<dyn Fn({a}){r} + Send + Sync>'


def fn(args, ret=None):
    return lambda a: FN.format(a=args(a), r=(' -> ' + ret(a)) if ret else '')


A = lambda i: (lambda a: a[i] if i < len(a) else 'JObjectRef')
U = '()'
DROP = None   # Java interface only meaningful in Java: removed from impls/supertraits, `()` elsewhere

# simple name (last path segment) of a stub type in a package prefix -> replacement
T = {
    # java.lang
    'java::lang::Throwable': 'anyhow::Error', 'java::lang::Class': 'crate::rt::Class',
    'java::lang::Runnable': 'Arc<dyn Fn() + Send + Sync>', 'java::lang::Thread': 'std::thread::Thread',
    'java::lang::ThreadLocal': lambda a: f'thread_local::ThreadLocal<{A(0)(a)}>',
    'java::lang::AutoCloseable': DROP, 'java::lang::Comparable': DROP, 'java::lang::Cloneable': DROP,
    'java::lang::Iterable': DROP, 'java::lang::StackTraceElement': 'String', 'java::lang::Enum': 'JObjectRef',
    'java::JObject': 'JObjectRef', 'java::Logger': U,
    # java.util.function
    'java::util::function::Function': fn(A(0), A(1)), 'java::util::function::BiFunction': fn(lambda a: f'{A(0)(a)}, {A(1)(a)}', A(2)),
    'java::util::function::Consumer': fn(A(0)), 'java::util::function::BiConsumer': fn(lambda a: f'{A(0)(a)}, {A(1)(a)}'),
    'java::util::function::Predicate': fn(A(0), lambda a: 'bool'),
    'java::util::function::BiPredicate': fn(lambda a: f'{A(0)(a)}, {A(1)(a)}', lambda a: 'bool'),
    'java::util::function::Supplier': fn(lambda a: '', A(0)), 'java::util::function::UnaryOperator': fn(A(0), A(0)),
    'java::util::function::BinaryOperator': fn(lambda a: f'{A(0)(a)}, {A(0)(a)}', A(0)),
    'java::util::function::IntFunction': fn(lambda a: 'i32', A(0)), 'java::util::function::LongFunction': fn(lambda a: 'i64', A(0)),
    'java::util::function::ToIntFunction': fn(A(0), lambda a: 'i32'), 'java::util::function::ToDoubleFunction': fn(A(0), lambda a: 'f64'),
    'java::util::function::ToLongFunction': fn(A(0), lambda a: 'i64'),
    'java::util::function::BooleanSupplier': fn(lambda a: '', lambda a: 'bool'), 'java::util::function::IntSupplier': fn(lambda a: '', lambda a: 'i32'),
    'java::util::function::DoubleSupplier': fn(lambda a: '', lambda a: 'f64'), 'java::util::function::LongSupplier': fn(lambda a: '', lambda a: 'i64'),
    'java::util::function::IntConsumer': fn(lambda a: 'i32'), 'java::util::function::LongConsumer': fn(lambda a: 'i64'),
    'java::util::function::IntPredicate': fn(lambda a: 'i32', lambda a: 'bool'), 'java::util::function::LongPredicate': fn(lambda a: 'i64', lambda a: 'bool'),
    'java::util::function::DoublePredicate': fn(lambda a: 'f64', lambda a: 'bool'),
    'java::util::function::IntUnaryOperator': fn(lambda a: 'i32', lambda a: 'i32'),
    'java::util::function::ObjIntConsumer': fn(lambda a: f'{A(0)(a)}, i32'),
    'java::util::concurrent::Callable': fn(lambda a: '', A(0)), 'java::util::Comparator': lambda a: f'Arc<dyn Fn(&{A(0)(a)}, &{A(0)(a)}) -> std::cmp::Ordering + Send + Sync>',
    'com::google::common::base::Function': fn(A(0), A(1)), 'com::google::common::base::Supplier': fn(lambda a: '', A(0)),
    'com::google::common::base::Predicate': fn(A(0), lambda a: 'bool'),
    'it::unimi::dsi::fastutil::ints::Int2IntFunction': fn(lambda a: 'i32', lambda a: 'i32'),
    'it::unimi::dsi::fastutil::longs::LongConsumer': fn(lambda a: 'i64'),
    'it::unimi::dsi::fastutil::floats::Float2FloatFunction': fn(lambda a: 'f32', lambda a: 'f32'),
    'it::unimi::dsi::fastutil::longs::Long2ObjectFunction': fn(lambda a: 'i64', A(0)),
    'org::apache::commons::lang3::function::TriFunction': fn(lambda a: f'{A(0)(a)}, {A(1)(a)}, {A(2)(a)}', A(3)),
    'org::apache::commons::lang3::function::TriConsumer': fn(lambda a: f'{A(0)(a)}, {A(1)(a)}, {A(2)(a)}'),
    'org::apache::commons::io::function::IORunnable': 'Arc<dyn Fn() -> std::io::Result<()> + Send + Sync>',
    # java.util / streams / iterators
    'java::util::stream::Stream': lambda a: f'Box<dyn Iterator<Item = {A(0)(a)}> + Send>',
    'java::util::JIterator': lambda a: f'Box<dyn Iterator<Item = {A(0)(a)}> + Send>',
    'java::util::Enumeration': lambda a: f'Box<dyn Iterator<Item = {A(0)(a)}> + Send>',
    'java::util::Entry': lambda a: f'({A(0)(a)}, {A(1)(a)})', 'java::util::BitSet': 'bit_set::BitSet',
    'java::util::Random': 'rand::rngs::StdRng', 'java::util::random::RandomGenerator': 'rand::rngs::StdRng',
    'java::util::Date': 'chrono::DateTime<chrono::Utc>', 'java::util::Locale': 'String',
    'java::util::Properties': 'IndexMap<String, String>', 'java::util::PriorityQueue': lambda a: f'std::collections::BinaryHeap<{A(0)(a)}>',
    'java::util::stream::Collector': U,
    # concurrency
    'java::util::concurrent::CompletableFuture': lambda a: f'crate::rt::CompletableFuture<{A(0)(a)}>',
    'java::util::concurrent::Future': lambda a: f'crate::rt::CompletableFuture<{A(0)(a)}>',
    'java::util::concurrent::ScheduledFuture': lambda a: f'crate::rt::CompletableFuture<{A(0)(a)}>',
    'java::util::concurrent::Executor': 'crate::rt::Executor', 'java::util::concurrent::ExecutorService': 'crate::rt::Executor',
    'java::util::concurrent::ThreadPoolExecutor': 'crate::rt::Executor', 'java::util::concurrent::ScheduledExecutorService': 'crate::rt::Executor',
    'java::util::concurrent::ThreadFactory': 'std::thread::Builder', 'java::util::concurrent::TimeUnit': 'crate::rt::TimeUnit',
    'java::util::concurrent::RejectedExecutionHandler': U,
    'java::util::concurrent::atomic::AtomicReference': lambda a: f'parking_lot::Mutex<{A(0)(a)}>',
    'java::util::concurrent::BlockingQueue': lambda a: f'crossbeam::queue::SegQueue<{A(0)(a)}>',
    'java::util::concurrent::ConcurrentLinkedQueue': lambda a: f'crossbeam::queue::SegQueue<{A(0)(a)}>',
    'java::util::concurrent::locks::ReentrantLock': 'parking_lot::ReentrantMutex<()>',
    'java::util::concurrent::locks::ReadWriteLock': 'parking_lot::RwLock<()>', 'java::util::concurrent::Semaphore': U,
    # io / nio / net / time / text / security
    'java::io::InputStream': 'Box<dyn std::io::Read + Send>', 'java::io::DataInput': 'Box<dyn std::io::Read + Send>',
    'java::io::DataInputStream': 'Box<dyn std::io::Read + Send>', 'java::io::Reader': 'Box<dyn std::io::BufRead + Send>',
    'java::io::BufferedReader': 'Box<dyn std::io::BufRead + Send>', 'java::io::OutputStream': 'Box<dyn std::io::Write + Send>',
    'java::io::DataOutput': 'Box<dyn std::io::Write + Send>', 'java::io::DataOutputStream': 'Box<dyn std::io::Write + Send>',
    'java::io::Writer': 'Box<dyn std::io::Write + Send>', 'java::io::ObjectOutputStream': U,
    'java::nio::ByteBuffer': 'bytes::BytesMut', 'java::nio::channels::FileChannel': 'std::fs::File',
    'java::nio::channels::SeekableByteChannel': 'std::fs::File', 'java::nio::channels::ScatteringByteChannel': 'std::fs::File',
    'java::nio::channels::GatheringByteChannel': 'std::fs::File', 'java::nio::file::attribute::BasicFileAttributes': 'std::fs::Metadata',
    'java::time::Instant': 'std::time::SystemTime', 'java::time::Duration': 'std::time::Duration',
    'java::time::format::DateTimeFormatter': 'String', 'java::text::DecimalFormat': 'String', 'java::text::SimpleDateFormat': 'String',
    'java::security::PublicKey': 'rsa::RsaPublicKey', 'java::security::PrivateKey': 'rsa::RsaPrivateKey',
    'java::security::KeyPair': '(rsa::RsaPrivateKey, rsa::RsaPublicKey)', 'javax::crypto::SecretKey': '[u8; 16]',
    'javax::crypto::Cipher': 'crate::rt::Cfb8Cipher', 'java::awt::Color': '[u8; 4]', 'java::awt::Image': 'image::RgbaImage',
    'java::awt::image::BufferedImage': 'image::RgbaImage', 'java::math::BigInteger': 'num_bigint::BigInt',
    'java::math::BigDecimal': 'f64', 'java::util::logging::Level': 'log::Level', 'java::util::logging::Logger': U,
    # libraries
    'io::netty::buffer::ByteBuf': 'bytes::BytesMut',
    'org::joml::Quaternionf': 'glam::Quat', 'org::joml::Quaternionfc': 'glam::Quat', 'org::joml::Vector3f': 'glam::Vec3',
    'org::joml::Vector3fc': 'glam::Vec3', 'org::joml::Vector3i': 'glam::IVec3', 'org::joml::Vector3ic': 'glam::IVec3',
    'org::joml::Vector3d': 'glam::DVec3', 'org::joml::Vector3dc': 'glam::DVec3', 'org::joml::Vector4f': 'glam::Vec4',
    'org::joml::Vector4fc': 'glam::Vec4', 'org::joml::Vector2f': 'glam::Vec2', 'org::joml::Vector2fc': 'glam::Vec2',
    'org::joml::Matrix4f': 'glam::Mat4', 'org::joml::Matrix4fc': 'glam::Mat4', 'org::joml::Matrix3f': 'glam::Mat3',
    'org::joml::Matrix3fc': 'glam::Mat3', 'org::joml::AxisAngle4f': '(glam::Vec3, f32)',
    'com::google::gson::JsonElement': 'serde_json::Value', 'com::google::gson::JsonObject': 'serde_json::Map<String, serde_json::Value>',
    'com::google::gson::JsonArray': 'Vec<serde_json::Value>', 'com::google::gson::JsonPrimitive': 'serde_json::Value',
    'com::google::common::collect::ImmutableListBuilder': lambda a: f'Vec<{A(0)(a)}>',
    'com::google::common::collect::ImmutableSetBuilder': lambda a: f'IndexSet<{A(0)(a)}>',
    'com::google::common::collect::ImmutableMapBuilder': lambda a: f'IndexMap<{A(0)(a)}, {A(1)(a)}>',
    'com::google::common::collect::Interner': lambda a: f'HashSet<{A(0)(a)}>',
    'com::google::common::collect::Range': lambda a: f'std::ops::RangeInclusive<{A(0)(a)}>',
    'com::google::common::graph::Graph': lambda a: f'HashMap<{A(0)(a)}, Vec<{A(0)(a)}>>',
    'com::google::common::graph::MutableGraph': lambda a: f'HashMap<{A(0)(a)}, Vec<{A(0)(a)}>>',
    'com::google::common::cache::LoadingCache': lambda a: f'moka::sync::Cache<{A(0)(a)}, {A(1)(a)}>',
    'com::google::common::cache::Cache': lambda a: f'moka::sync::Cache<{A(0)(a)}, {A(1)(a)}>',
    'com::github::benmanes::caffeine::cache::Cache': lambda a: f'moka::sync::Cache<{A(0)(a)}, {A(1)(a)}>',
    'com::github::benmanes::caffeine::cache::LoadingCache': lambda a: f'moka::sync::Cache<{A(0)(a)}, {A(1)(a)}>',
    'com::google::common::hash::HashCode': 'u64',
    'it::unimi::dsi::fastutil::doubles::DoubleDoubleImmutablePair': '(f64, f64)',
    'it::unimi::dsi::fastutil::Pair': lambda a: f'({A(0)(a)}, {A(1)(a)})',
    'org::apache::commons::lang3::mutable::MutableInt': 'i32', 'org::apache::commons::lang3::mutable::MutableBoolean': 'bool',
    'org::apache::commons::lang3::mutable::MutableFloat': 'f32', 'org::apache::commons::lang3::mutable::MutableDouble': 'f64',
    'org::apache::commons::lang3::mutable::MutableObject': A(0), 'org::apache::commons::lang3::math::Fraction': 'f64',
    'org::apache::logging::log4j::Level': 'log::Level', 'org::slf4j::event::Level': 'log::Level',
    'org::spongepowered::configurate::ConfigurationNode': 'serde_yaml::Value',
    'org::spongepowered::configurate::CommentedConfigurationNode': 'serde_yaml::Value',
    'org::spongepowered::configurate::NodePath': 'Vec<String>',
    'org::spongepowered::configurate::serialize::SerializationException': 'anyhow::Error',
    'org::yaml::snakeyaml::nodes::Node': 'serde_yaml::Value', 'org::yaml::snakeyaml::nodes::MappingNode': 'serde_yaml::Mapping',
    'io::github::thatsmusic99::configurationmaster::api::ConfigFile': 'serde_yaml::Mapping',
    'org::jline::reader::LineReader': 'rustyline::DefaultEditor', 'org::jline::reader::Candidate': 'String',
    'org::jline::utils::AttributedString': 'String',
    'com::mojang::authlib::GameProfile': 'crate::rt::GameProfile', 'com::mojang::authlib::properties::Property': 'crate::rt::Property',
    'com::mojang::authlib::properties::PropertyMap': 'crate::rt::PropertyMap',
    'com::mojang::authlib::services::response::NameAndId': '(Uuid, String)',
    'ca::spottedleaf::concurrentutil::util::Priority': 'crate::rt::Priority',
    'ca::spottedleaf::concurrentutil::map::concurrent::longs::ConcurrentChainedLong2ReferenceHashTable': lambda a: f'dashmap::DashMap<i64, {A(0)(a)}>',
    'ca::spottedleaf::concurrentutil::collection::MultiThreadedQueue': lambda a: f'crossbeam::queue::SegQueue<{A(0)(a)}>',
    'ca::spottedleaf::concurrentutil::lock::ReentrantAreaLock': 'parking_lot::ReentrantMutex<()>',
    'ca::spottedleaf::concurrentutil::completable::Completable': lambda a: f'crate::rt::CompletableFuture<{A(0)(a)}>',
    'ca::spottedleaf::concurrentutil::completable::CallbackCompletable': lambda a: f'crate::rt::CompletableFuture<{A(0)(a)}>',
    'ca::spottedleaf::concurrentutil::executor::thread::BalancedPrioritisedThreadPool': 'crate::rt::Executor',
    'ca::spottedleaf::concurrentutil::executor::PrioritisedExecutorPrioritisedTask': 'Arc<dyn Fn() + Send + Sync>',
    'com::mojang::datafixers::util::Pair': lambda a: f'({A(0)(a)}, {A(1)(a)})',
    'com::mojang::datafixers::util::Either': lambda a: f'either::Either<{A(0)(a)}, {A(1)(a)}>',
    'com::mojang::datafixers::util::Unit': U,
}
DROP_IF_IFACE = {'java::lang::AutoCloseable', 'java::lang::Comparable', 'java::lang::Cloneable', 'java::lang::Iterable'}
# packages removed entirely (stubs + anything left under them)
REMOVE = ['java', 'javax', 'com/google', 'it', 'org/spongepowered', 'org/apache', 'io/netty', 'org/joml', 'org/yaml/snakeyaml',
          'org/jline', 'org/objectweb', 'net/md_5', 'org/eclipse', 'net/jpountz', 'org/slf4j', 'io/github', 'com/velocitypowered',
          'com/github', 'joptsimple', 'co/aikar', 'io/leangen', 'com/mojang/authlib', 'com/mojang/jtracy', 'com/mojang/logging',
          'ca/spottedleaf/concurrentutil', 'ca/spottedleaf/common', 'com/mojang/datafixers', 'org/apache']
KEEP = {'java/runtime.rs', 'prelude.rs', 'prelude_traits.rs', 'api_prelude.rs', 'lib.rs', 'main.rs', 'rt.rs', 'generated_mods.rs'}
PKG_RE = '|'.join(re.escape(p.replace('/', '::')) for p in REMOVE)
PATH_RE = re.compile(r'crate::((?:' + PKG_RE + r')(?:::[a-z_0-9]+)*)::([A-Z]\w*)')
USE_RE = re.compile(r'^use crate::((?:' + PKG_RE + r')(?:::[a-z_0-9]+)*)::([A-Z]\w*);\n', re.M)
unmapped = collections.Counter()


def lookup(path, name):
    key = path + '::' + name
    if key in T:
        return T[key], key
    # stub modules re-export at package level; a stub's real module may be one deeper (codec::Codec)
    for k, v in T.items():
        if k.rsplit('::', 1)[1] == name and (path.startswith(k.rsplit('::', 1)[0]) or k.rsplit('::', 1)[0].startswith(path)):
            return v, k
    unmapped[key] += 1
    return U, key


def split_args(s):
    out, depth, cur = [], 0, ''
    for ch in s:
        if ch in '<([':
            depth += 1
        elif ch in '>)]':
            depth -= 1
        if ch == ',' and depth == 0:
            out.append(cur.strip()); cur = ''
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def match_angle(text, i):
    """text[i] == '<': index after the matching '>'."""
    depth = 0
    j = i
    while j < len(text):
        c = text[j]
        if c == '<':
            depth += 1
        elif c == '>' and text[j - 1] != '-':
            depth -= 1
            if depth == 0:
                return j + 1
        j += 1
    return len(text)


def match_brace(text, i):
    depth = 0
    for j in range(i, len(text)):
        if text[j] == '{':
            depth += 1
        elif text[j] == '}':
            depth -= 1
            if depth == 0:
                return j + 1
    return len(text)


def render(rep, args):
    if rep is None:
        return '()'
    if callable(rep):
        return rep(args)
    return rep


def rewrite(text):
    names = {}
    for m in USE_RE.finditer(text):
        names[m.group(2)] = m.group(1)
    text = USE_RE.sub('', text)
    text = re.sub(r'^use crate::(?:' + PKG_RE + r')(?:::\w+)*(?:::\{[^}]*\})?;[^\n]*\n', '', text, flags=re.M)
    # fully qualified -> short
    def fq(m):
        rep, _ = lookup(m.group(1), m.group(2))
        return '\x00' + m.group(1) + '|' + m.group(2) + '\x00'
    text = PATH_RE.sub(fq, text)
    # fully qualified references are replaced in place (marker form)
    def fqr(m):
        path, name = m.group(1), m.group(2)
        j = m.end()
        return render(lookup(path, name)[0], [])
    text = re.sub(r'\x00([^|\x00]*)\|(\w+)\x00(?!<)', fqr, text)
    while '\x00' in text:
        i = text.index('\x00'); k = text.index('\x00', i + 1)
        path, name = text[i + 1:k].split('|'); j = k + 1; args = []
        if j < len(text) and text[j] == '<':
            e = match_angle(text, j); args = split_args(text[j + 1:e - 1]); j = e
        text = text[:i] + render(lookup(path, name)[0], args) + text[j:]
    local = set(re.findall(r'\b(?:struct|enum|trait|type) (\w+)', text))
    local |= set(re.findall(r'[<,]\s*([A-Z]\w*)\s*:', text))
    for n in list(names):
        if n in local:
            del names[n]
    if not names:
        return text
    # remove impls of stub traits:  impl<..> Name<..> for X { ... }
    for n in names:
        while True:
            m = re.search(r'^impl(?:<[^\n{]*?>)? ' + re.escape(n) + r'\b[^\n{]* for [^\n{]+\{', text, re.M)
            if not m:
                break
            end = match_brace(text, m.end() - 1)
            text = text[:m.start()] + text[end:].lstrip('\n')
    # supertraits: drop stub names from `pub trait X: A + B {`
    def sup(m):
        parts = [p for p in split_plus(m.group(2)) if strip_gen(p).strip() not in names]
        return m.group(1) + (' + '.join(parts) if parts else 'JObject') + m.group(3)
    text = re.sub(r'^(pub trait \w+(?:<[^\n]*?>)?: )([^\n{]+)( \{)', sup, text, flags=re.M)
    # jobject! targets
    def job(m):
        items = [x for x in split_args(m.group(2)) if strip_gen(x.replace('dyn ', '')).strip() not in names]
        return m.group(1) + (': ' + ', '.join(items) if items else '') + ');'
    text = re.sub(r'(crate::jobject!\(\w+)(?:: ([^\n]*))?\);', lambda m: job(m) if m.group(2) else m.group(0), text)
    # type references
    pat = re.compile(r'(JRef<dyn |&dyn |dyn )?\b(' + '|'.join(re.escape(n) for n in sorted(names, key=len, reverse=True)) + r')\b(?!::|\()')
    out, i = [], 0
    while True:
        m = pat.search(text, i)
        if not m:
            out.append(text[i:]); break
        pre = text[max(0, m.start() - 2):m.start()]
        if pre.endswith('::') or (m.start() > 0 and (text[m.start() - 1].isalnum() or text[m.start() - 1] == '_')):
            out.append(text[i:m.end()]); i = m.end(); continue
        name = m.group(2)
        j = m.end()
        args = []
        if j < len(text) and text[j] == '<':
            k = match_angle(text, j)
            args = split_args(text[j + 1:k - 1])
            j = k
        rep, key = lookup(names[name], name)
        r = render(rep, args)
        wrapper = m.group(1) or ''
        if wrapper == 'JRef<dyn ':
            if j < len(text) and text[j] == '>':
                j += 1
            else:
                r = 'JRef<dyn ' + r   # unusual shape: keep
        out.append(text[i:m.start()] + r)
        i = j
    return ''.join(out)


def strip_gen(s):
    d, o = 0, ''
    for c in s:
        if c == '<':
            d += 1
        elif c == '>':
            d -= 1
        elif d == 0:
            o += c
    return o


def split_plus(s):
    out, d, cur = [], 0, ''
    for c in s:
        if c == '<':
            d += 1
        elif c == '>':
            d -= 1
        if c == '+' and d == 0:
            out.append(cur.strip()); cur = ''
        else:
            cur += c
    if cur.strip():
        out.append(cur.strip())
    return out


def main():
    removed_dirs = [os.path.join(SRC, p) for p in REMOVE]
    n = 0
    for dp, dn, fn_ in os.walk(SRC):
        if any(dp == r or dp.startswith(r + '/') for r in removed_dirs) and not dp.endswith('/java'):
            continue
        for f in fn_:
            if not f.endswith('.rs'):
                continue
            p = os.path.join(dp, f)
            if os.path.relpath(p, SRC) in KEEP or (dp.endswith('/java') and f != 'runtime.rs'):
                continue
            s = open(p).read()
            t = rewrite(s)
            if t != s:
                open(p, 'w').write(t); n += 1
    for r in removed_dirs:
        if r.endswith('/java'):
            for x in os.listdir(r):
                if x != 'runtime.rs':
                    q = os.path.join(r, x)
                    shutil.rmtree(q) if os.path.isdir(q) else os.remove(q)
            open(os.path.join(r, 'mod.rs'), 'w').write('//! The Java object model the port runs on.\npub mod runtime;\npub use runtime::*;\n')
        else:
            shutil.rmtree(r, ignore_errors=True)
    # module tree and preludes: drop what no longer exists
    gone = '|'.join(re.escape(p.replace('/', '::')) for p in REMOVE if p != 'java')
    for f in ('prelude_traits.rs', 'api_prelude.rs'):
        q = os.path.join(SRC, f)
        lines = [l for l in open(q).read().split('\n') if not re.search(r'crate::(' + gone + r')::', l) and 'crate::java::' not in l]
        open(q, 'w').write('\n'.join(lines))
    changed = True
    while changed:
        changed = False
        for dp, dn, fs in os.walk(SRC):
            for f in fs:
                if f not in ('mod.rs', 'generated_mods.rs'):
                    continue
                q = os.path.join(dp, f); s = open(q).read(); keep = []
                for line in s.split('\n'):
                    m = re.match(r'pub mod (\w+);', line) or re.match(r'pub use self::(\w+)::', line)
                    if m and not (os.path.exists(os.path.join(dp, m.group(1) + '.rs')) or os.path.isdir(os.path.join(dp, m.group(1)))):
                        changed = True; continue
                    keep.append(line)
                open(q, 'w').write('\n'.join(keep))
                if f == 'mod.rs' and not [l for l in keep if l.startswith('pub ')]:
                    os.remove(q); changed = True
                    if not os.listdir(dp):
                        os.rmdir(dp)
    print('rewrote', n, 'files; unmapped stub types (now `()`):', len(unmapped))
    for k, v in unmapped.most_common():
        print(f'  {v:4} {k}')


main()
