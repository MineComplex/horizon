//! `java.nio.ByteBuffer`, `IntBuffer` and `ByteOrder`.
//!
//! A buffer is a handle (clones are the same Java object: they share contents *and* position).
//! `duplicate()`, `slice()` and `as_int_buffer()` share the contents with a position of their own,
//! as in Java. `wrap(byte[])` copies the array (a Rust `Vec<i8>` cannot be shared): read the bytes
//! back with `array()`. Netty's port can share storage with [`ByteBuffer::from_shared`].
//! `BufferUnderflowException`/`BufferOverflowException`/`IndexOutOfBoundsException` are unchecked:
//! they panic, as Java throws.
use crate::java::io::bytes::{as_i8, as_u8, check_range, to_i8_vec};
use parking_lot::Mutex;
use std::sync::Arc;

/// `java.nio.ByteOrder`.
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ByteOrder {
    BIG_ENDIAN,
    LITTLE_ENDIAN,
}
/// `ByteOrder`'s static fields (`byte_order::BIG_ENDIAN`).
pub mod byte_order {
    pub const BIG_ENDIAN: super::ByteOrder = super::ByteOrder::BIG_ENDIAN;
    pub const LITTLE_ENDIAN: super::ByteOrder = super::ByteOrder::LITTLE_ENDIAN;
}
impl ByteOrder {
    pub fn native_order() -> ByteOrder {
        if cfg!(target_endian = "big") { ByteOrder::BIG_ENDIAN } else { ByteOrder::LITTLE_ENDIAN }
    }
    pub fn to_string(&self) -> String {
        match self {
            ByteOrder::BIG_ENDIAN => "BIG_ENDIAN".into(),
            ByteOrder::LITTLE_ENDIAN => "LITTLE_ENDIAN".into(),
        }
    }
}

#[derive(Clone, Copy)]
struct State {
    position: usize,
    limit: usize,
    mark: Option<usize>,
    order: ByteOrder,
}

/// The parts every buffer has: shared bytes, a window on them and its own position/limit/mark.
#[derive(Clone)]
struct Core {
    data: Arc<Mutex<Vec<u8>>>,
    /// Byte offset of element 0 in `data`.
    offset: usize,
    /// Capacity in elements.
    capacity: usize,
    state: Arc<Mutex<State>>,
    read_only: bool,
    direct: bool,
}

impl Core {
    fn new(data: Arc<Mutex<Vec<u8>>>, offset: usize, capacity: usize, order: ByteOrder) -> Core {
        Core {
            data,
            offset,
            capacity,
            state: Arc::new(Mutex::new(State { position: 0, limit: capacity, mark: None, order })),
            read_only: false,
            direct: false,
        }
    }
    fn st(&self) -> State {
        *self.state.lock()
    }
    fn set_position(&self, p: i32) {
        let mut s = self.state.lock();
        if p < 0 || p as usize > s.limit {
            panic!("java.lang.IllegalArgumentException: newPosition > limit: ({p} > {})", s.limit);
        }
        s.position = p as usize;
        if s.mark.is_some_and(|m| m > s.position) {
            s.mark = None;
        }
    }
    fn set_limit(&self, l: i32) {
        let mut s = self.state.lock();
        if l < 0 || l as usize > self.capacity {
            panic!("java.lang.IllegalArgumentException: newLimit > capacity: ({l} > {})", self.capacity);
        }
        s.limit = l as usize;
        if s.position > s.limit {
            s.position = s.limit;
        }
        if s.mark.is_some_and(|m| m > s.limit) {
            s.mark = None;
        }
    }
    /// Advances the position by `n` elements, returning the old one.
    fn next(&self, n: usize, write: bool) -> usize {
        if write && self.read_only {
            panic!("java.nio.ReadOnlyBufferException");
        }
        let mut s = self.state.lock();
        if s.limit - s.position < n {
            if write {
                panic!("java.nio.BufferOverflowException");
            }
            panic!("java.nio.BufferUnderflowException");
        }
        let p = s.position;
        s.position += n;
        p
    }
    fn check_index(&self, i: i32, n: usize) -> usize {
        let s = self.state.lock();
        if i < 0 || i as usize + n > s.limit {
            panic!("java.lang.IndexOutOfBoundsException: index {i} out of bounds for length {}", s.limit);
        }
        i as usize
    }
    fn view(&self, offset_bytes: usize, capacity: usize, order: ByteOrder) -> Core {
        let mut c = Core::new(self.data.clone(), offset_bytes, capacity, order);
        c.read_only = self.read_only;
        c.direct = self.direct;
        c
    }
}

macro_rules! buffer_common {
    ($t:ident) => {
        impl $t {
            pub fn capacity(&self) -> i32 {
                self.core.capacity as i32
            }
            pub fn position(&self) -> i32 {
                self.core.st().position as i32
            }
            /// `position(int)`: returns this buffer.
            pub fn position_1(&self, new_position: i32) -> $t {
                self.core.set_position(new_position);
                self.clone()
            }
            pub fn limit(&self) -> i32 {
                self.core.st().limit as i32
            }
            /// `limit(int)`: returns this buffer.
            pub fn limit_1(&self, new_limit: i32) -> $t {
                self.core.set_limit(new_limit);
                self.clone()
            }
            pub fn mark(&self) -> $t {
                let mut s = self.core.state.lock();
                s.mark = Some(s.position);
                drop(s);
                self.clone()
            }
            pub fn reset(&self) -> $t {
                let mut s = self.core.state.lock();
                match s.mark {
                    Some(m) => s.position = m,
                    None => panic!("java.nio.InvalidMarkException"),
                }
                drop(s);
                self.clone()
            }
            pub fn clear(&self) -> $t {
                let mut s = self.core.state.lock();
                s.position = 0;
                s.limit = self.core.capacity;
                s.mark = None;
                drop(s);
                self.clone()
            }
            pub fn flip(&self) -> $t {
                let mut s = self.core.state.lock();
                s.limit = s.position;
                s.position = 0;
                s.mark = None;
                drop(s);
                self.clone()
            }
            pub fn rewind(&self) -> $t {
                let mut s = self.core.state.lock();
                s.position = 0;
                s.mark = None;
                drop(s);
                self.clone()
            }
            pub fn remaining(&self) -> i32 {
                let s = self.core.st();
                (s.limit - s.position) as i32
            }
            pub fn has_remaining(&self) -> bool {
                let s = self.core.st();
                s.position < s.limit
            }
            pub fn is_read_only(&self) -> bool {
                self.core.read_only
            }
            pub fn is_direct(&self) -> bool {
                self.core.direct
            }
            pub fn has_array(&self) -> bool {
                !self.core.read_only && !self.core.direct
            }
            pub fn order(&self) -> ByteOrder {
                self.core.st().order
            }
            /// Identity (Java `==`).
            pub fn same(&self, other: &$t) -> bool {
                Arc::ptr_eq(&self.core.state, &other.core.state)
            }
        }
        impl std::fmt::Debug for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let s = self.core.st();
                write!(f, "{}[pos={} lim={} cap={}]", stringify!($t), s.position, s.limit, self.core.capacity)
            }
        }
    };
}

/// `java.nio.ByteBuffer`.
#[derive(Clone)]
pub struct ByteBuffer {
    core: Core,
}
buffer_common!(ByteBuffer);

/// `ByteBuffer.get(...)`'s one-argument forms: `get(int index) -> byte` and `get(byte[] dst) -> ByteBuffer`.
pub trait ByteBufferGet1 {
    type Out;
    fn get_from(self, b: &ByteBuffer) -> Self::Out;
}
impl ByteBufferGet1 for i32 {
    type Out = i8;
    fn get_from(self, b: &ByteBuffer) -> i8 {
        let i = b.core.check_index(self, 1);
        b.core.data.lock()[b.core.offset + i] as i8
    }
}
impl ByteBufferGet1 for &mut [i8] {
    type Out = ByteBuffer;
    fn get_from(self, b: &ByteBuffer) -> ByteBuffer {
        let len = self.len() as i32;
        b.get_3(self, 0, len)
    }
}
impl ByteBufferGet1 for &mut Vec<i8> {
    type Out = ByteBuffer;
    fn get_from(self, b: &ByteBuffer) -> ByteBuffer {
        let len = self.len() as i32;
        b.get_3(self, 0, len)
    }
}

/// `ByteBuffer.put(...)`'s one-argument bulk forms: `put(byte[])` and `put(ByteBuffer)`.
pub trait ByteBufferPut1 {
    fn put_into(self, b: &ByteBuffer);
}
impl ByteBufferPut1 for &[i8] {
    fn put_into(self, b: &ByteBuffer) {
        b.put_u8(as_u8(self));
    }
}
impl ByteBufferPut1 for &Vec<i8> {
    fn put_into(self, b: &ByteBuffer) {
        b.put_u8(as_u8(self));
    }
}
impl<const N: usize> ByteBufferPut1 for &[i8; N] {
    fn put_into(self, b: &ByteBuffer) {
        b.put_u8(as_u8(self));
    }
}
impl ByteBufferPut1 for &ByteBuffer {
    fn put_into(self, b: &ByteBuffer) {
        if self.same(b) {
            panic!("java.lang.IllegalArgumentException: The source buffer is this buffer");
        }
        let src = self.take_remaining();
        b.put_u8(&src);
    }
}
impl ByteBufferPut1 for ByteBuffer {
    fn put_into(self, b: &ByteBuffer) {
        (&self).put_into(b)
    }
}

macro_rules! prim_access {
    ($get:ident, $get_1:ident, $put:ident, $put_2:ident, $ty:ty, $n:expr) => {
        pub fn $get(&self) -> $ty {
            let p = self.core.next($n, false);
            let (b, be) = self.load::<$n>(p);
            if be { <$ty>::from_be_bytes(b) } else { <$ty>::from_le_bytes(b) }
        }
        pub fn $get_1(&self, index: i32) -> $ty {
            let p = self.core.check_index(index, $n);
            let (b, be) = self.load::<$n>(p);
            if be { <$ty>::from_be_bytes(b) } else { <$ty>::from_le_bytes(b) }
        }
        pub fn $put(&self, v: $ty) -> ByteBuffer {
            let p = self.core.next($n, true);
            self.store::<$n>(p, v.to_be_bytes(), v.to_le_bytes());
            self.clone()
        }
        pub fn $put_2(&self, index: i32, v: $ty) -> ByteBuffer {
            if self.core.read_only {
                panic!("java.nio.ReadOnlyBufferException");
            }
            let p = self.core.check_index(index, $n);
            self.store::<$n>(p, v.to_be_bytes(), v.to_le_bytes());
            self.clone()
        }
    };
}

impl ByteBuffer {
    fn from_core(core: Core) -> Self {
        ByteBuffer { core }
    }
    /// `ByteBuffer.allocate(int)`.
    pub fn allocate(capacity: i32) -> ByteBuffer {
        if capacity < 0 {
            panic!("java.lang.IllegalArgumentException: capacity < 0: ({capacity} < 0)");
        }
        let data = Arc::new(Mutex::new(vec![0u8; capacity as usize]));
        Self::from_core(Core::new(data, 0, capacity as usize, ByteOrder::BIG_ENDIAN))
    }
    /// `ByteBuffer.allocateDirect(int)` (a heap buffer that reports `isDirect()`).
    pub fn allocate_direct(capacity: i32) -> ByteBuffer {
        let mut b = Self::allocate(capacity);
        b.core.direct = true;
        b
    }
    /// `ByteBuffer.wrap(byte[])` (copies the array; see the module docs).
    pub fn wrap(array: &[i8]) -> ByteBuffer {
        Self::wrap_bytes(as_u8(array).to_vec())
    }
    /// `ByteBuffer.wrap(byte[], int offset, int length)`: position `offset`, limit `offset + length`.
    pub fn wrap_3(array: &[i8], offset: i32, length: i32) -> ByteBuffer {
        let (s, e) = check_range(offset, length, array.len());
        let b = Self::wrap(array);
        b.core.set_limit(e as i32);
        b.core.set_position(s as i32);
        b
    }
    /// A buffer over Rust bytes (taken).
    pub fn wrap_bytes(data: Vec<u8>) -> ByteBuffer {
        let n = data.len();
        Self::from_core(Core::new(Arc::new(Mutex::new(data)), 0, n, ByteOrder::BIG_ENDIAN))
    }
    /// A buffer over `length` bytes of shared storage starting at `offset` (netty's `nioBuffer()`).
    pub fn from_shared(data: Arc<Mutex<Vec<u8>>>, offset: usize, length: usize) -> ByteBuffer {
        Self::from_core(Core::new(data, offset, length, ByteOrder::BIG_ENDIAN))
    }
    /// The shared storage and this buffer's offset into it.
    pub fn shared(&self) -> (Arc<Mutex<Vec<u8>>>, usize) {
        (self.core.data.clone(), self.core.offset)
    }

    fn load<const N: usize>(&self, p: usize) -> ([u8; N], bool) {
        let mut b = [0u8; N];
        let o = self.core.offset + p;
        b.copy_from_slice(&self.core.data.lock()[o..o + N]);
        (b, self.order() == ByteOrder::BIG_ENDIAN)
    }
    fn store<const N: usize>(&self, p: usize, be: [u8; N], le: [u8; N]) {
        let bytes = if self.order() == ByteOrder::BIG_ENDIAN { be } else { le };
        let o = self.core.offset + p;
        self.core.data.lock()[o..o + N].copy_from_slice(&bytes);
    }

    /// `get()`.
    pub fn get(&self) -> i8 {
        let p = self.core.next(1, false);
        self.core.data.lock()[self.core.offset + p] as i8
    }
    /// `get(int index)` -> `byte`, or `get(byte[] dst)` -> this buffer.
    pub fn get_1<A: ByteBufferGet1>(&self, a: A) -> A::Out {
        a.get_from(self)
    }
    /// `get(int index, byte[] dst)`.
    pub fn get_2(&self, index: i32, dst: &mut [i8]) -> ByteBuffer {
        let len = dst.len() as i32;
        self.get_4(index, dst, 0, len)
    }
    /// `get(byte[] dst, int offset, int length)`.
    pub fn get_3(&self, dst: &mut [i8], offset: i32, length: i32) -> ByteBuffer {
        let (s, e) = check_range(offset, length, dst.len());
        let p = self.core.next(e - s, false);
        let o = self.core.offset + p;
        dst[s..e].copy_from_slice(as_i8(&self.core.data.lock()[o..o + (e - s)]));
        self.clone()
    }
    /// `get(int index, byte[] dst, int offset, int length)`.
    pub fn get_4(&self, index: i32, dst: &mut [i8], offset: i32, length: i32) -> ByteBuffer {
        let (s, e) = check_range(offset, length, dst.len());
        let p = self.core.check_index(index, e - s);
        let o = self.core.offset + p;
        dst[s..e].copy_from_slice(as_i8(&self.core.data.lock()[o..o + (e - s)]));
        self.clone()
    }
    /// `put(byte)`.
    pub fn put(&self, b: i8) -> ByteBuffer {
        let p = self.core.next(1, true);
        self.core.data.lock()[self.core.offset + p] = b as u8;
        self.clone()
    }
    /// `put(byte[])` / `put(ByteBuffer)`.
    pub fn put_1<A: ByteBufferPut1>(&self, src: A) -> ByteBuffer {
        src.put_into(self);
        self.clone()
    }
    /// `put(int index, byte b)`.
    pub fn put_2(&self, index: i32, b: i8) -> ByteBuffer {
        if self.core.read_only {
            panic!("java.nio.ReadOnlyBufferException");
        }
        let p = self.core.check_index(index, 1);
        self.core.data.lock()[self.core.offset + p] = b as u8;
        self.clone()
    }
    /// `put(byte[] src, int offset, int length)`.
    pub fn put_3(&self, src: &[i8], offset: i32, length: i32) -> ByteBuffer {
        let (s, e) = check_range(offset, length, src.len());
        self.put_u8(as_u8(&src[s..e]));
        self.clone()
    }
    /// `put(int index, byte[] src)`.
    pub fn put_bytes_at(&self, index: i32, src: &[i8]) -> ByteBuffer {
        if self.core.read_only {
            panic!("java.nio.ReadOnlyBufferException");
        }
        let p = self.core.check_index(index, src.len());
        let o = self.core.offset + p;
        self.core.data.lock()[o..o + src.len()].copy_from_slice(as_u8(src));
        self.clone()
    }
    /// Relative bulk put of Rust bytes.
    pub fn put_u8(&self, src: &[u8]) {
        let p = self.core.next(src.len(), true);
        let o = self.core.offset + p;
        self.core.data.lock()[o..o + src.len()].copy_from_slice(src);
    }
    /// Reads every remaining byte (advancing the position to the limit).
    pub fn take_remaining(&self) -> Vec<u8> {
        let n = self.remaining() as usize;
        let p = self.core.next(n, false);
        let o = self.core.offset + p;
        self.core.data.lock()[o..o + n].to_vec()
    }
    /// The remaining bytes, without moving the position.
    pub fn remaining_bytes(&self) -> Vec<u8> {
        let s = self.core.st();
        let o = self.core.offset;
        self.core.data.lock()[o + s.position..o + s.limit].to_vec()
    }

    prim_access!(get_short, get_short_1, put_short, put_short_2, i16, 2);
    prim_access!(get_int, get_int_1, put_int, put_int_2, i32, 4);
    prim_access!(get_long, get_long_1, put_long, put_long_2, i64, 8);
    prim_access!(get_float, get_float_1, put_float, put_float_2, f32, 4);
    prim_access!(get_double, get_double_1, put_double, put_double_2, f64, 8);
    prim_access!(get_char_unit, get_char_unit_1, put_char_unit, put_char_unit_2, u16, 2);

    /// `getChar()` (a lone surrogate reads as U+FFFD).
    pub fn get_char(&self) -> char {
        char::from_u32(self.get_char_unit() as u32).unwrap_or('\u{FFFD}')
    }
    pub fn get_char_1(&self, index: i32) -> char {
        char::from_u32(self.get_char_unit_1(index) as u32).unwrap_or('\u{FFFD}')
    }
    /// `putChar(char)` (characters past U+FFFF are truncated to 16 bits, as a Java `char` cast).
    pub fn put_char(&self, c: char) -> ByteBuffer {
        self.put_char_unit(c as u32 as u16)
    }
    pub fn put_char_2(&self, index: i32, c: char) -> ByteBuffer {
        self.put_char_unit_2(index, c as u32 as u16)
    }

    /// `order(ByteOrder)`: returns this buffer.
    pub fn order_1(&self, order: ByteOrder) -> ByteBuffer {
        self.core.state.lock().order = order;
        self.clone()
    }
    /// `array()`: a copy of the whole backing array (index it with `array_offset()`).
    pub fn array(&self) -> Vec<i8> {
        if self.core.read_only {
            panic!("java.nio.ReadOnlyBufferException");
        }
        to_i8_vec(self.core.data.lock().clone())
    }
    pub fn array_offset(&self) -> i32 {
        self.core.offset as i32
    }
    /// `slice()`: shares the remaining bytes, position 0, big-endian.
    pub fn slice(&self) -> ByteBuffer {
        let s = self.core.st();
        Self::from_core(self.core.view(self.core.offset + s.position, s.limit - s.position, ByteOrder::BIG_ENDIAN))
    }
    /// `slice(int index, int length)`.
    pub fn slice_2(&self, index: i32, length: i32) -> ByteBuffer {
        let (s, e) = check_range(index, length, self.limit() as usize);
        Self::from_core(self.core.view(self.core.offset + s, e - s, ByteOrder::BIG_ENDIAN))
    }
    /// `duplicate()`: shares the contents; copies position, limit and mark (order: big-endian, as Java).
    pub fn duplicate(&self) -> ByteBuffer {
        let c = self.core.view(self.core.offset, self.core.capacity, ByteOrder::BIG_ENDIAN);
        let mut s = self.core.st();
        s.order = ByteOrder::BIG_ENDIAN;
        *c.state.lock() = s;
        Self::from_core(c)
    }
    pub fn as_read_only_buffer(&self) -> ByteBuffer {
        let b = self.duplicate();
        let mut c = b.core.clone();
        c.read_only = true;
        Self::from_core(c)
    }
    /// `compact()`: moves the remaining bytes to the start; position = remaining, limit = capacity.
    pub fn compact(&self) -> ByteBuffer {
        if self.core.read_only {
            panic!("java.nio.ReadOnlyBufferException");
        }
        let mut s = self.core.state.lock();
        let o = self.core.offset;
        let n = s.limit - s.position;
        self.core.data.lock().copy_within(o + s.position..o + s.limit, o);
        s.position = n;
        s.limit = self.core.capacity;
        s.mark = None;
        drop(s);
        self.clone()
    }
    /// `asIntBuffer()`: an `int` view of the remaining bytes in this buffer's order.
    pub fn as_int_buffer(&self) -> IntBuffer {
        let s = self.core.st();
        IntBuffer { core: self.core.view(self.core.offset + s.position, (s.limit - s.position) / 4, s.order) }
    }
    pub fn to_string(&self) -> String {
        let s = self.core.st();
        format!("java.nio.HeapByteBuffer[pos={} lim={} cap={}]", s.position, s.limit, self.core.capacity)
    }
}

impl PartialEq for ByteBuffer {
    /// Java `equals`: the remaining bytes are equal.
    fn eq(&self, o: &ByteBuffer) -> bool {
        self.remaining_bytes() == o.remaining_bytes()
    }
}
impl Eq for ByteBuffer {}
impl std::hash::Hash for ByteBuffer {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.remaining_bytes().hash(h)
    }
}

/// `java.nio.IntBuffer`.
#[derive(Clone)]
pub struct IntBuffer {
    core: Core,
}
buffer_common!(IntBuffer);

/// `IntBuffer.get(...)`'s one-argument forms: `get(int index) -> int` and `get(int[] dst)`.
pub trait IntBufferGet1 {
    type Out;
    fn get_from(self, b: &IntBuffer) -> Self::Out;
}
impl IntBufferGet1 for i32 {
    type Out = i32;
    fn get_from(self, b: &IntBuffer) -> i32 {
        let i = b.core.check_index(self, 1);
        b.read(i)
    }
}
impl IntBufferGet1 for &mut [i32] {
    type Out = IntBuffer;
    fn get_from(self, b: &IntBuffer) -> IntBuffer {
        let p = b.core.next(self.len(), false);
        for (k, x) in self.iter_mut().enumerate() {
            *x = b.read(p + k);
        }
        b.clone()
    }
}
impl IntBufferGet1 for &mut Vec<i32> {
    type Out = IntBuffer;
    fn get_from(self, b: &IntBuffer) -> IntBuffer {
        self.as_mut_slice().get_from(b)
    }
}
/// `IntBuffer.put(int[])` / `put(IntBuffer)`.
pub trait IntBufferPut1 {
    fn put_into(self, b: &IntBuffer);
}
impl IntBufferPut1 for &[i32] {
    fn put_into(self, b: &IntBuffer) {
        let p = b.core.next(self.len(), true);
        for (k, &x) in self.iter().enumerate() {
            b.write(p + k, x);
        }
    }
}
impl IntBufferPut1 for &Vec<i32> {
    fn put_into(self, b: &IntBuffer) {
        self.as_slice().put_into(b)
    }
}
impl IntBufferPut1 for &IntBuffer {
    fn put_into(self, b: &IntBuffer) {
        let n = self.remaining() as usize;
        let p = self.core.next(n, false);
        let v: Vec<i32> = (0..n).map(|k| self.read(p + k)).collect();
        v.as_slice().put_into(b)
    }
}
impl IntBufferPut1 for IntBuffer {
    fn put_into(self, b: &IntBuffer) {
        (&self).put_into(b)
    }
}

impl IntBuffer {
    fn read(&self, i: usize) -> i32 {
        let o = self.core.offset + i * 4;
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.core.data.lock()[o..o + 4]);
        if self.order() == ByteOrder::BIG_ENDIAN { i32::from_be_bytes(b) } else { i32::from_le_bytes(b) }
    }
    fn write(&self, i: usize, v: i32) {
        let o = self.core.offset + i * 4;
        let b = if self.order() == ByteOrder::BIG_ENDIAN { v.to_be_bytes() } else { v.to_le_bytes() };
        self.core.data.lock()[o..o + 4].copy_from_slice(&b);
    }
    /// `IntBuffer.allocate(int)`.
    pub fn allocate(capacity: i32) -> IntBuffer {
        if capacity < 0 {
            panic!("java.lang.IllegalArgumentException: capacity < 0: ({capacity} < 0)");
        }
        let data = Arc::new(Mutex::new(vec![0u8; capacity as usize * 4]));
        IntBuffer { core: Core::new(data, 0, capacity as usize, ByteOrder::native_order()) }
    }
    /// `IntBuffer.wrap(int[])` (copies the array).
    pub fn wrap(array: &[i32]) -> IntBuffer {
        let b = Self::allocate(array.len() as i32);
        for (k, &x) in array.iter().enumerate() {
            b.write(k, x);
        }
        b
    }
    pub fn get(&self) -> i32 {
        let p = self.core.next(1, false);
        self.read(p)
    }
    /// `get(int index)` -> `int`, or `get(int[] dst)` -> this buffer.
    pub fn get_1<A: IntBufferGet1>(&self, a: A) -> A::Out {
        a.get_from(self)
    }
    pub fn put(&self, v: i32) -> IntBuffer {
        let p = self.core.next(1, true);
        self.write(p, v);
        self.clone()
    }
    /// `put(int[])` / `put(IntBuffer)`.
    pub fn put_1<A: IntBufferPut1>(&self, src: A) -> IntBuffer {
        src.put_into(self);
        self.clone()
    }
    /// `put(int index, int i)`.
    pub fn put_2(&self, index: i32, v: i32) -> IntBuffer {
        if self.core.read_only {
            panic!("java.nio.ReadOnlyBufferException");
        }
        let p = self.core.check_index(index, 1);
        self.write(p, v);
        self.clone()
    }
    /// `put(int[] src, int offset, int length)`.
    pub fn put_3(&self, src: &[i32], offset: i32, length: i32) -> IntBuffer {
        let (s, e) = check_range(offset, length, src.len());
        src[s..e].put_into(self);
        self.clone()
    }
    /// `get(int[] dst, int offset, int length)`.
    pub fn get_3(&self, dst: &mut [i32], offset: i32, length: i32) -> IntBuffer {
        let (s, e) = check_range(offset, length, dst.len());
        (&mut dst[s..e]).get_from(self)
    }
    /// `array()`: a copy of the ints (only for buffers made by `allocate`/`wrap`).
    pub fn array(&self) -> Vec<i32> {
        (0..self.core.capacity).map(|i| self.read(i)).collect()
    }
    pub fn array_offset(&self) -> i32 {
        0
    }
    pub fn slice(&self) -> IntBuffer {
        let s = self.core.st();
        IntBuffer { core: self.core.view(self.core.offset + s.position * 4, s.limit - s.position, s.order) }
    }
    pub fn duplicate(&self) -> IntBuffer {
        let c = self.core.view(self.core.offset, self.core.capacity, self.order());
        *c.state.lock() = self.core.st();
        IntBuffer { core: c }
    }
}
