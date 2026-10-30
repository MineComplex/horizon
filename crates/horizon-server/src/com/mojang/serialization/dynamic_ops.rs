//! Port of `com.mojang.serialization.DynamicOps` (DFU 8).
//!
//! `DynamicOps<T>` is an object-safe trait over the ops' value type `T` (`JRef<dyn Tag>` for `NbtOps`,
//! `JsonElement` for `JsonOps`, `JObjectRef` for `JavaOps`...). Java's generic methods
//! (`<U> U convertTo(DynamicOps<U>, T)`, `convertList`, `convertMap`, `withEncoder`...) are inherent methods
//! on `dyn DynamicOps<T>`; the trait carries their erased forms (`convert_to` takes the out-ops as
//! `JRef<dyn DynamicOps<JObjectRef>>` and returns its value erased, see `erased.rs`).
//!
//! Codecs run on erased ops (`JRef<dyn DynamicOps<JObjectRef>>`): the generic entry points
//! (`codec.encode_start(ops, a)`, `codec.parse(ops, t)`...) wrap a typed ops with `erase_ops`, which calls
//! the real ops for every operation (each ops keeps its own semantics; nothing is converted up front).
use crate::com::mojang::datafixers::util::Pair;
use crate::com::mojang::serialization::data_result::DataResult;
use crate::com::mojang::serialization::decoder::Decoder;
use crate::com::mojang::serialization::encoder::Encoder;
use crate::com::mojang::serialization::erased::{erase_ops, unerase};
use crate::com::mojang::serialization::list_builder::{ListBuilder, ListBuilderBuilder};
use crate::com::mojang::serialization::map_like::MapLike;
use crate::com::mojang::serialization::record_builder::{RecordBuilder, RecordBuilderMapBuilder};
use crate::java::nio::ByteBuffer;
use crate::java::util::function::{BiConsumer, Consumer};
use crate::java::util::stream::Stream;
use crate::prelude::*;

/// `java.lang.Number` as DynamicOps see it: the boxed primitive keeps its width, so `Codec.LONG` reads
/// a `LongTag` without going through `double`.
#[derive(Clone, Copy, Debug)]
pub enum Number {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

impl Number {
    pub fn byte_value(&self) -> i8 {
        match *self {
            Number::Byte(v) => v,
            Number::Short(v) => v as i8,
            Number::Int(v) => v as i8,
            Number::Long(v) => v as i8,
            Number::Float(v) => (v as i32) as i8,
            Number::Double(v) => (v as i32) as i8,
        }
    }
    pub fn short_value(&self) -> i16 {
        match *self {
            Number::Byte(v) => v as i16,
            Number::Short(v) => v,
            Number::Int(v) => v as i16,
            Number::Long(v) => v as i16,
            Number::Float(v) => (v as i32) as i16,
            Number::Double(v) => (v as i32) as i16,
        }
    }
    pub fn int_value(&self) -> i32 {
        match *self {
            Number::Byte(v) => v as i32,
            Number::Short(v) => v as i32,
            Number::Int(v) => v,
            Number::Long(v) => v as i32,
            Number::Float(v) => v as i32,
            Number::Double(v) => v as i32,
        }
    }
    pub fn long_value(&self) -> i64 {
        match *self {
            Number::Byte(v) => v as i64,
            Number::Short(v) => v as i64,
            Number::Int(v) => v as i64,
            Number::Long(v) => v,
            Number::Float(v) => v as i64,
            Number::Double(v) => v as i64,
        }
    }
    pub fn float_value(&self) -> f32 {
        match *self {
            Number::Byte(v) => v as f32,
            Number::Short(v) => v as f32,
            Number::Int(v) => v as f32,
            Number::Long(v) => v as f32,
            Number::Float(v) => v,
            Number::Double(v) => v as f32,
        }
    }
    pub fn double_value(&self) -> f64 {
        match *self {
            Number::Byte(v) => v as f64,
            Number::Short(v) => v as f64,
            Number::Int(v) => v as f64,
            Number::Long(v) => v as f64,
            Number::Float(v) => v as f64,
            Number::Double(v) => v,
        }
    }
    /// Java's `toString()` of the boxed value (`1`, `1.0`, `1.0E10`).
    pub fn to_string(&self) -> String {
        match *self {
            Number::Byte(v) => v.to_string(),
            Number::Short(v) => v.to_string(),
            Number::Int(v) => v.to_string(),
            Number::Long(v) => v.to_string(),
            Number::Float(v) => java_double_to_string(v as f64, true),
            Number::Double(v) => java_double_to_string(v, false),
        }
    }
}

/// Java's `Double.toString`/`Float.toString` shape (plain between 1e-3 and 1e7, else `d.dddE±n`).
pub fn java_double_to_string(v: f64, is_float: bool) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    let a = v.abs();
    let shortest = if is_float {
        format!("{}", v as f32)
    } else {
        format!("{}", v)
    };
    if (1e-3..1e7).contains(&a) {
        if shortest.contains('.') {
            shortest
        } else {
            format!("{}.0", shortest)
        }
    } else {
        let s = if is_float {
            format!("{:e}", v as f32)
        } else {
            format!("{:e}", v)
        };
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{}.0", mantissa)
        };
        format!("{}E{}", mantissa, exp)
    }
}

impl PartialEq for Number {
    /// Java's `equals` on boxed numbers: same box type and value.
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Number::Byte(a), Number::Byte(b)) => a == b,
            (Number::Short(a), Number::Short(b)) => a == b,
            (Number::Int(a), Number::Int(b)) => a == b,
            (Number::Long(a), Number::Long(b)) => a == b,
            (Number::Float(a), Number::Float(b)) => a.to_bits() == b.to_bits(),
            (Number::Double(a), Number::Double(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

impl std::fmt::Display for Number {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&Number::to_string(self))
    }
}

macro_rules! number_from {
    ($($t:ty => $v:ident),*) => {$(
        impl From<$t> for Number {
            fn from(x: $t) -> Number {
                Number::$v(x)
            }
        }
    )*};
}
number_from!(i8 => Byte, i16 => Short, i32 => Int, i64 => Long, f32 => Float, f64 => Double);

/// Takes a one-shot stream out of a `Fn` closure (Java streams are single-use too).
fn take_once<S>(cell: &parking_lot::Mutex<Option<S>>) -> Option<S> {
    cell.lock().take()
}

// ---- interface com.mojang.serialization.DynamicOps
/// `DynamicOps<T>`: overload names follow the arity rule (`merge_to_list_2(T, List)`,
/// `merge_to_map(T, Map)`, `merge_to_map_2(T, MapLike)`, `merge_to_map_3(T, T, T)`,
/// `create_map(Stream)`, `create_map_1(Map)`, `get_number_value_2(T, Number)`).
/// Java's `Map<T, T>` parameters are ordered entry lists (`&Vec<(T, T)>`): `T` has no `Hash`.
pub trait DynamicOps<T: JGeneric>: Send + Sync + 'static {
    /// This ops as a `JRef` (Java's `this` where the defaults pass it on): `{ self.this() }`.
    fn this_ops(&self) -> JRef<dyn DynamicOps<T>>;

    fn empty(&self) -> T;

    fn empty_map(&self) -> T {
        self.create_map_1(&Vec::new())
    }

    fn empty_list(&self) -> T {
        self.create_list(Vec::new().into_iter().collect())
    }

    /// `<U> U convertTo(DynamicOps<U> outOps, T input)`, erased: `out_ops` is erased (`erase_ops`) and the
    /// result is `out_ops`' value erased (`erase`). Implement it by instantiating the ops' generic body
    /// at `U = JObjectRef`: `fn convert_to(&self, o, i) -> JObjectRef { NbtOps::convert_to::<JObjectRef>(self, o, i) }`.
    /// Call it through the generic `(dyn DynamicOps<T>)::convert_to::<U>`.
    fn convert_to(&self, out_ops: JRef<dyn DynamicOps<JObjectRef>>, input: T) -> JObjectRef;

    fn get_number_value(&self, input: T) -> DataResult<Number>;

    fn get_number_value_2(&self, input: T, default_value: Number) -> Number {
        self.get_number_value(input)
            .result()
            .unwrap_or(default_value)
    }

    fn create_numeric(&self, i: Number) -> T;

    fn create_byte(&self, value: i8) -> T {
        self.create_numeric(Number::Byte(value))
    }

    fn create_short(&self, value: i16) -> T {
        self.create_numeric(Number::Short(value))
    }

    fn create_int(&self, value: i32) -> T {
        self.create_numeric(Number::Int(value))
    }

    fn create_long(&self, value: i64) -> T {
        self.create_numeric(Number::Long(value))
    }

    fn create_float(&self, value: f32) -> T {
        self.create_numeric(Number::Float(value))
    }

    fn create_double(&self, value: f64) -> T {
        self.create_numeric(Number::Double(value))
    }

    fn get_boolean_value(&self, input: T) -> DataResult<bool> {
        self.get_number_value(input)
            .map(|number| number.byte_value() != 0)
    }

    fn create_boolean(&self, value: bool) -> T {
        self.create_byte(if value { 1 } else { 0 })
    }

    fn get_string_value(&self, input: T) -> DataResult<String>;

    fn create_string(&self, value: &str) -> T;

    fn merge_to_list(&self, list: T, value: T) -> DataResult<T>;

    fn merge_to_list_2(&self, list: T, values: &Vec<T>) -> DataResult<T> {
        let mut result = DataResult::success(list);
        for value in values {
            let value = value.clone();
            result = result.flat_map(|r| self.merge_to_list(r, value));
        }
        result
    }

    fn merge_to_map_3(&self, map: T, key: T, value: T) -> DataResult<T>;

    /// `mergeToMap(T map, Map<T, T> values)` = `mergeToMap(map, MapLike.forMap(values, this))`.
    fn merge_to_map(&self, map: T, values: &Vec<(T, T)>) -> DataResult<T> {
        let mut result = DataResult::success(map);
        for (k, v) in values {
            let (k, v) = (k.clone(), v.clone());
            result = result.flat_map(|r| self.merge_to_map_3(r, k, v));
        }
        result
    }

    fn merge_to_map_2(&self, map: T, values: MapLike<T>) -> DataResult<T> {
        let mut result = DataResult::success(map);
        for entry in values.entries() {
            result = result.flat_map(|r| self.merge_to_map_3(r, entry.first, entry.second));
        }
        result
    }

    fn merge_to_primitive(&self, prefix: T, value: T) -> DataResult<T> {
        let empty = self.empty();
        if !self.value_equals(&prefix, &empty) {
            let message = format!(
                "Do not know how to append a primitive value {} to {}",
                self.value_to_string(&value),
                self.value_to_string(&prefix)
            );
            return DataResult::error_2(move || message.clone(), value);
        }
        DataResult::success(value)
    }

    fn get_map_values(&self, input: T) -> DataResult<Stream<Pair<T, T>>>;

    fn get_map_entries(
        &self,
        input: T,
    ) -> DataResult<JRef<dyn Consumer<JRef<dyn BiConsumer<T, T>>>>> {
        self.get_map_values(input).map(|s| {
            let cell = parking_lot::Mutex::new(Some(s));
            let c: JRef<dyn Consumer<JRef<dyn BiConsumer<T, T>>>> =
                Arc::new(move |c: JRef<dyn BiConsumer<T, T>>| {
                    if let Some(s) = take_once(&cell) {
                        for p in s {
                            c.accept(p.first, p.second);
                        }
                    }
                });
            c
        })
    }

    fn create_map(&self, map: Stream<Pair<T, T>>) -> T;

    fn get_map(&self, input: T) -> DataResult<MapLike<T>> {
        self.get_map_values(input).flat_map(|s| {
            let mut entries: Vec<(T, T)> = Vec::new();
            for p in s {
                if let Some((_, existing)) = entries.iter().find(|(k, _)| self.value_equals(k, &p.first)) {
                    let message = format!(
                        "Error while building map: Duplicate key {} (attempted merging values {} and {})",
                        self.value_to_string(&p.first),
                        self.value_to_string(existing),
                        self.value_to_string(&p.second)
                    );
                    return DataResult::error(move || message.clone());
                }
                entries.push((p.first, p.second));
            }
            DataResult::success(MapLike::for_map(entries, self.this_ops()))
        })
    }

    /// `createMap(Map<T, T> map)`.
    fn create_map_1(&self, map: &Vec<(T, T)>) -> T {
        self.create_map(
            map.iter()
                .map(|(k, v)| Pair::of(k.clone(), v.clone()))
                .collect(),
        )
    }

    fn get_stream(&self, input: T) -> DataResult<Stream<T>>;

    fn get_list(&self, input: T) -> DataResult<JRef<dyn Consumer<JRef<dyn Consumer<T>>>>> {
        self.get_stream(input).map(|s| {
            let cell = parking_lot::Mutex::new(Some(s));
            let c: JRef<dyn Consumer<JRef<dyn Consumer<T>>>> =
                Arc::new(move |c: JRef<dyn Consumer<T>>| {
                    if let Some(s) = take_once(&cell) {
                        for e in s {
                            c.accept(e);
                        }
                    }
                });
            c
        })
    }

    fn create_list(&self, input: Stream<T>) -> T;

    fn get_byte_buffer(&self, input: T) -> DataResult<ByteBuffer> {
        let input_string = self.value_to_string(&input);
        self.get_stream(input).flat_map(|stream| {
            let list: Vec<T> = stream.into_iter().collect();
            if list
                .iter()
                .all(|element| self.get_number_value(element.clone()).is_success())
            {
                let bytes: Vec<i8> = list
                    .iter()
                    .map(|e| self.get_number_value(e.clone()).get_or_throw().byte_value())
                    .collect();
                return DataResult::success(ByteBuffer::wrap(bytes));
            }
            DataResult::error(move || format!("Some elements are not bytes: {}", input_string))
        })
    }

    fn create_byte_list(&self, input: ByteBuffer) -> T {
        let n = input.capacity();
        self.create_list((0..n).map(|i| self.create_byte(input.get(i))).collect())
    }

    fn get_int_stream(&self, input: T) -> DataResult<Stream<i32>> {
        let input_string = self.value_to_string(&input);
        self.get_stream(input).flat_map(|stream| {
            let list: Vec<T> = stream.into_iter().collect();
            if list
                .iter()
                .all(|element| self.get_number_value(element.clone()).is_success())
            {
                return DataResult::success(
                    list.into_iter()
                        .map(|e| self.get_number_value(e).get_or_throw().int_value())
                        .collect(),
                );
            }
            DataResult::error(move || format!("Some elements are not ints: {}", input_string))
        })
    }

    fn create_int_list(&self, input: Stream<i32>) -> T {
        self.create_list(input.into_iter().map(|i| self.create_int(i)).collect())
    }

    fn get_long_stream(&self, input: T) -> DataResult<Stream<i64>> {
        let input_string = self.value_to_string(&input);
        self.get_stream(input).flat_map(|stream| {
            let list: Vec<T> = stream.into_iter().collect();
            if list
                .iter()
                .all(|element| self.get_number_value(element.clone()).is_success())
            {
                return DataResult::success(
                    list.into_iter()
                        .map(|e| self.get_number_value(e).get_or_throw().long_value())
                        .collect(),
                );
            }
            DataResult::error(move || format!("Some elements are not longs: {}", input_string))
        })
    }

    fn create_long_list(&self, input: Stream<i64>) -> T {
        self.create_list(input.into_iter().map(|i| self.create_long(i)).collect())
    }

    fn remove(&self, input: T, key: &str) -> T;

    fn compress_maps(&self) -> bool {
        false
    }

    fn get(&self, input: T, key: &str) -> DataResult<T> {
        self.get_generic(input, self.create_string(key))
    }

    fn get_generic(&self, input: T, key: T) -> DataResult<T> {
        let input_string = self.value_to_string(&input);
        self.get_map(input.clone())
            .flat_map(|map| match map.get(key.clone()) {
                Some(v) => DataResult::success(v),
                None => {
                    let message = format!(
                        "No element {} in the map {}",
                        self.value_to_string(&key),
                        input_string
                    );
                    DataResult::error(move || message.clone())
                }
            })
    }

    fn set(&self, input: T, key: &str, value: T) -> T {
        self.merge_to_map_3(input.clone(), self.create_string(key), value)
            .result()
            .unwrap_or(input)
    }

    /// `update(T input, String key, Function<T, T> function)`.
    fn update(&self, input: T, key: &str, function: &dyn Fn(T) -> T) -> T {
        self.get(input.clone(), key)
            .map(|value| self.set(input.clone(), key, function(value)))
            .result()
            .unwrap_or(input)
    }

    /// `updateGeneric(T input, T key, Function<T, T> function)`.
    fn update_generic(&self, input: T, key: T, function: &dyn Fn(T) -> T) -> T {
        self.get_generic(input.clone(), key.clone())
            .flat_map(|value| self.merge_to_map_3(input.clone(), key.clone(), function(value)))
            .result()
            .unwrap_or(input)
    }

    fn list_builder(&self) -> JRef<dyn ListBuilder<T>> {
        ListBuilderBuilder::new(self.this_ops())
    }

    fn map_builder(&self) -> JRef<dyn RecordBuilder<T>> {
        RecordBuilderMapBuilder::new(self.this_ops())
    }

    // ---- Rust-only hooks (Java's `Object` methods and `instanceof`)

    /// `toString()` of the ops (`NbtOps` -> "NBT", `JsonOps` -> "JSON").
    fn to_string(&self) -> String {
        "DynamicOps".to_string()
    }

    /// `Objects.equals(a, b)` on two values of this ops (Java's `T.equals`). The default compares
    /// through the ops' own accessors (strings, numbers with their box type, lists, maps, empties);
    /// ops whose values have a structural `equals` (tags, JSON) should override it.
    fn value_equals(&self, a: &T, b: &T) -> bool {
        let (sa, sb) = (
            self.get_string_value(a.clone()).result(),
            self.get_string_value(b.clone()).result(),
        );
        if sa.is_some() || sb.is_some() {
            return sa == sb;
        }
        let (na, nb) = (
            self.get_number_value(a.clone()).result(),
            self.get_number_value(b.clone()).result(),
        );
        if na.is_some() || nb.is_some() {
            return na == nb;
        }
        let (ma, mb) = (
            self.get_map_values(a.clone()).result(),
            self.get_map_values(b.clone()).result(),
        );
        if let (Some(ma), Some(mb)) = (ma, mb) {
            let ma: Vec<Pair<T, T>> = ma.into_iter().collect();
            let mb: Vec<Pair<T, T>> = mb.into_iter().collect();
            return ma.len() == mb.len()
                && ma.iter().all(|pa| {
                    mb.iter().any(|pb| {
                        self.value_equals(&pa.first, &pb.first)
                            && self.value_equals(&pa.second, &pb.second)
                    })
                });
        }
        let (la, lb) = (
            self.get_stream(a.clone()).result(),
            self.get_stream(b.clone()).result(),
        );
        if let (Some(la), Some(lb)) = (la, lb) {
            let la: Vec<T> = la.into_iter().collect();
            let lb: Vec<T> = lb.into_iter().collect();
            return la.len() == lb.len()
                && la
                    .iter()
                    .zip(lb.iter())
                    .all(|(x, y)| self.value_equals(x, y));
        }
        // Neither a primitive nor a container: both are this ops' empty value, or opaque.
        true
    }

    /// `String.valueOf(value)` (Java's `T.toString`), used in error messages. The default renders the
    /// value through the ops' accessors; ops should override it with their values' own `toString`.
    fn value_to_string(&self, value: &T) -> String {
        if let Some(s) = self.get_string_value(value.clone()).result() {
            return format!("\"{}\"", s);
        }
        if let Some(n) = self.get_number_value(value.clone()).result() {
            return n.to_string();
        }
        if let Some(m) = self.get_map_values(value.clone()).result() {
            let parts: Vec<String> = m
                .into_iter()
                .map(|p| {
                    format!(
                        "{}: {}",
                        self.value_to_string(&p.first),
                        self.value_to_string(&p.second)
                    )
                })
                .collect();
            return format!("{{{}}}", parts.join(", "));
        }
        if let Some(l) = self.get_stream(value.clone()).result() {
            let parts: Vec<String> = l.into_iter().map(|e| self.value_to_string(&e)).collect();
            return format!("[{}]", parts.join(", "));
        }
        "<empty>".to_string()
    }

    /// The identity Java's `Objects.equals(ops1, ops2)` compares (wrappers made by `erase_ops` and
    /// `typed_ops` return their inner ops' identity, so `Dynamic.castTyped` still matches).
    fn ops_identity(&self) -> usize {
        self as *const Self as *const () as usize
    }

    /// The ops object as `java.lang.Object`, for `ops instanceof RegistryOps`/`LocaleAwareOps` checks
    /// (`cast::<dyn X, _>(&o)`). Ops that are tested this way return `Some(self.this())` (as `JObjectRef`);
    /// the erasure wrappers forward to their inner ops.
    fn as_jobject(&self) -> Option<JObjectRef> {
        None
    }

    /// For `typed_ops` wrappers: the erased ops they view (so erasing them again unwraps instead of
    /// stacking wrappers).
    fn erased_inner(&self) -> Option<JRef<dyn DynamicOps<JObjectRef>>> {
        None
    }
}

/// Java's generic default methods of `DynamicOps`, on the trait object.
impl<T: JGeneric> dyn DynamicOps<T> {
    /// `<U> U convertTo(DynamicOps<U> outOps, T input)`.
    pub fn convert_to<U: JGeneric>(&self, out_ops: JRef<dyn DynamicOps<U>>, input: T) -> U {
        unerase(DynamicOps::convert_to(self, erase_ops(out_ops), input))
    }

    /// `<U> U convertList(DynamicOps<U> outOps, T input)`.
    pub fn convert_list<U: JGeneric>(&self, out_ops: JRef<dyn DynamicOps<U>>, input: T) -> U {
        let elements: Vec<T> = self
            .get_stream(input)
            .result()
            .map(|s| s.into_iter().collect())
            .unwrap_or_default();
        out_ops.create_list(
            elements
                .into_iter()
                .map(|e| self.convert_to(out_ops.clone(), e))
                .collect(),
        )
    }

    /// `<U> U convertMap(DynamicOps<U> outOps, T input)`.
    pub fn convert_map<U: JGeneric>(&self, out_ops: JRef<dyn DynamicOps<U>>, input: T) -> U {
        let entries: Vec<Pair<T, T>> = self
            .get_map_values(input)
            .result()
            .map(|s| s.into_iter().collect())
            .unwrap_or_default();
        out_ops.create_map(
            entries
                .into_iter()
                .map(|e| {
                    Pair::of(
                        self.convert_to(out_ops.clone(), e.first),
                        self.convert_to(out_ops.clone(), e.second),
                    )
                })
                .collect(),
        )
    }

    /// `<E> Function<E, DataResult<T>> withEncoder(Encoder<E> encoder)`.
    pub fn with_encoder<E: JGeneric>(
        self: &Arc<Self>,
        encoder: Encoder<E>,
    ) -> impl Fn(E) -> DataResult<T> + Send + Sync + 'static {
        let ops = self.clone();
        move |e| encoder.encode_start(ops.clone(), e)
    }

    /// `<E> Function<T, DataResult<Pair<E, T>>> withDecoder(Decoder<E> decoder)`.
    pub fn with_decoder<E: JGeneric>(
        self: &Arc<Self>,
        decoder: Decoder<E>,
    ) -> impl Fn(T) -> DataResult<Pair<E, T>> + Send + Sync + 'static {
        let ops = self.clone();
        move |t| decoder.decode(ops.clone(), t)
    }

    /// `<E> Function<T, DataResult<E>> withParser(Decoder<E> decoder)`.
    pub fn with_parser<E: JGeneric>(
        self: &Arc<Self>,
        decoder: Decoder<E>,
    ) -> impl Fn(T) -> DataResult<E> + Send + Sync + 'static {
        let ops = self.clone();
        move |t| decoder.parse(ops.clone(), t)
    }
}
