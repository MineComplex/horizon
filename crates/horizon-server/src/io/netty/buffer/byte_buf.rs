//! Port of `io.netty.buffer.ByteBuf` (with the behaviour of `AbstractByteBuf`, `UnpooledHeapByteBuf`,
//! the derived buffers `slice`/`duplicate`/`order`/`asReadOnly`, and `AbstractReferenceCountedByteBuf`).
//!
//! `ByteBuf` is a cheap handle: `clone()` is another Java reference to the same buffer (same bytes, same
//! reader/writer indices). Derived buffers (`slice`, `duplicate`, `retained_slice`...) share the bytes and
//! the reference count with their parent but have their own indices, as in Netty. Reference counting is
//! kept (`retain`/`release`); a buffer released to 0 frees its bytes and throws on access, as Netty's.
//!
//! Method names follow `FriendlyByteBuf` (which delegates every one of them): overloads are renamed by
//! arity (`read_bytes(len)`, `read_bytes_1(dst)`, `read_bytes_1_2(&mut [i8])`...). Java `byte[]`
//! destinations are `&mut [i8]`, sources `&[i8]`.
use crate::io::netty::buffer::byte_buf_allocator::ByteBufAllocator;
use crate::io::netty::util::byte_processor::ByteProcessor;
use crate::io::netty::util::illegal_reference_count_exception::IllegalReferenceCountException;
use crate::io::netty::util::internal::throwable_util;
use crate::io::netty::util::reference_counted::ReferenceCounted;
use crate::java::io::InputStream;
use crate::java::io::OutputStream;
use crate::java::lang::Throwable;
use crate::java::nio::ByteBuffer;
use crate::java::nio::ByteOrder;
use crate::java::nio::channels::FileChannel;
use crate::java::nio::channels::GatheringByteChannel;
use crate::java::nio::channels::ScatteringByteChannel;
use crate::java::nio::charset::Charset;
use crate::prelude::*;

/// Netty's `AbstractByteBufAllocator.CALCULATE_THRESHOLD`: 4 MiB.
pub const CALCULATE_THRESHOLD: i32 = 1048576 * 4;

/// The bytes a buffer and all its derived buffers share, with the shared reference count.
pub(crate) struct Shared {
    pub(crate) bytes: Mut<Vec<u8>>,
    pub(crate) ref_cnt: AtomicI32,
    pub(crate) max_capacity: Mut<i32>,
    pub(crate) direct: bool,
    pub(crate) alloc: ByteBufAllocator,
}

/// Reader/writer indices and their marks (shared by a buffer and its `order()` view).
#[derive(Default)]
pub(crate) struct Indices {
    reader: Mut<i32>,
    writer: Mut<i32>,
    marked_reader: Mut<i32>,
    marked_writer: Mut<i32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// A root buffer (`UnpooledHeapByteBuf`, `PooledByteBuf`...).
    Root,
    Slice,
    Duplicate,
    ReadOnly,
    Swapped,
}

pub struct ByteBufInner {
    pub(crate) shared: Arc<Shared>,
    pub(crate) idx: Arc<Indices>,
    /// Start of this buffer in the shared bytes.
    pub(crate) offset: i32,
    /// A slice's fixed capacity; `None` follows the shared bytes' length.
    pub(crate) fixed_capacity: Option<i32>,
    pub(crate) read_only: bool,
    /// An `order(LITTLE_ENDIAN)` view: multi-byte accessors are swapped.
    pub(crate) little_endian: bool,
    pub(crate) kind: Kind,
    pub(crate) parent: Option<ByteBuf>,
}

/// `io.netty.buffer.ByteBuf`.
#[derive(Clone)]
pub struct ByteBuf(pub(crate) Arc<ByteBufInner>);

crate::jobject!(ByteBuf: dyn ReferenceCounted);

impl From<ByteBuf> for JObjectRef {
    fn from(b: ByteBuf) -> JObjectRef {
        Arc::new(b)
    }
}

impl std::fmt::Debug for ByteBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string())
    }
}

fn ioobe(msg: String) -> ! {
    panic!("IndexOutOfBoundsException: {msg}")
}

impl ByteBuf {
    // ---- construction (used by Unpooled and the allocators)

    pub(crate) fn new_root(
        bytes: Vec<u8>,
        writer_index: i32,
        max_capacity: i32,
        direct: bool,
        alloc: ByteBufAllocator,
    ) -> ByteBuf {
        if (bytes.len() as i64) > max_capacity as i64 {
            panic!(
                "IllegalArgumentException: initialCapacity({}) > maxCapacity({})",
                bytes.len(),
                max_capacity
            );
        }
        let shared = Arc::new(Shared {
            bytes: Mut::new(bytes),
            ref_cnt: AtomicI32::new(1),
            max_capacity: Mut::new(max_capacity),
            direct,
            alloc,
        });
        let idx = Arc::new(Indices::default());
        idx.writer.set(writer_index);
        ByteBuf(Arc::new(ByteBufInner {
            shared,
            idx,
            offset: 0,
            fixed_capacity: None,
            read_only: false,
            little_endian: false,
            kind: Kind::Root,
            parent: None,
        }))
    }

    fn derive(
        &self,
        kind: Kind,
        offset: i32,
        fixed_capacity: Option<i32>,
        reader: i32,
        writer: i32,
    ) -> ByteBuf {
        let idx = Arc::new(Indices::default());
        idx.reader.set(reader);
        idx.writer.set(writer);
        ByteBuf(Arc::new(ByteBufInner {
            shared: self.0.shared.clone(),
            idx,
            offset: self.0.offset + offset,
            fixed_capacity,
            read_only: self.0.read_only || kind == Kind::ReadOnly,
            little_endian: self.0.little_endian,
            kind,
            parent: Some(self.clone()),
        }))
    }

    /// Wraps this buffer as a message object for the pipeline (`Object msg = buf`).
    pub fn into_object(self) -> JObjectRef {
        Arc::new(self)
    }
    /// `msg instanceof ByteBuf buf`: the buffer a message object is.
    pub fn from_object(msg: &JObjectRef) -> Option<ByteBuf> {
        cast::<ByteBuf, dyn JObject>(msg).map(|b| (*b).clone())
    }
    /// Java's `==` on buffers.
    pub fn same(&self, other: &ByteBuf) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    // ---- internals

    #[inline]
    fn r(&self) -> i32 {
        self.0.idx.reader.get()
    }
    #[inline]
    fn w(&self) -> i32 {
        self.0.idx.writer.get()
    }
    #[inline]
    fn ensure_accessible(&self) {
        let c = self.0.shared.ref_cnt.load(AtomicOrdering::Acquire);
        if c == 0 {
            throwable_util::throw_(IllegalReferenceCountException::of_ref_cnt(0).into());
        }
    }
    #[inline]
    fn check_index(&self, index: i32, length: i32) {
        self.ensure_accessible();
        let cap = self.capacity();
        if index < 0 || length < 0 || (index as i64 + length as i64) > cap as i64 {
            ioobe(format!(
                "index: {index}, length: {length} (expected: range(0, {cap}))"
            ));
        }
    }
    #[inline]
    fn check_writable_access(&self) {
        if self.0.read_only {
            panic!("ReadOnlyBufferException");
        }
    }
    #[inline]
    fn check_readable_bytes(&self, minimum_readable_bytes: i32) {
        if minimum_readable_bytes < 0 {
            panic!(
                "IllegalArgumentException: minimumReadableBytes: {minimum_readable_bytes} (expected: >= 0)"
            );
        }
        self.ensure_accessible();
        let (r, w) = (self.r(), self.w());
        if r > w - minimum_readable_bytes {
            ioobe(format!(
                "readerIndex({r}) + length({minimum_readable_bytes}) exceeds writerIndex({w}): {}",
                self.to_string()
            ));
        }
    }
    /// The bytes of this buffer (`0..capacity`). Keep the borrow to one statement.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    fn bytes(&self) -> &mut [u8] {
        let all = self.0.shared.bytes.borrow_mut();
        let off = self.0.offset as usize;
        let cap = self.capacity() as usize;
        &mut all[off..off + cap]
    }
    fn copy_out(&self, index: i32, length: i32) -> Vec<u8> {
        self.bytes()[index as usize..(index + length) as usize].to_vec()
    }
    fn copy_in(&self, index: i32, src: &[u8]) {
        self.bytes()[index as usize..index as usize + src.len()].copy_from_slice(src);
    }
    #[inline]
    fn get_raw<const N: usize>(&self, index: i32) -> [u8; N] {
        self.check_index(index, N as i32);
        let mut out = [0u8; N];
        out.copy_from_slice(&self.bytes()[index as usize..index as usize + N]);
        out
    }
    #[inline]
    fn set_raw<const N: usize>(&self, index: i32, v: [u8; N]) {
        self.check_writable_access();
        self.check_index(index, N as i32);
        self.bytes()[index as usize..index as usize + N].copy_from_slice(&v);
    }
    #[inline]
    fn read_raw<const N: usize>(&self) -> [u8; N] {
        self.check_readable_bytes(N as i32);
        let r = self.r();
        let v = self.get_raw::<N>(r);
        self.0.idx.reader.set(r + N as i32);
        v
    }
    #[inline]
    fn write_raw<const N: usize>(&self, v: [u8; N]) {
        self.ensure_writable0(N as i32);
        let w = self.w();
        self.set_raw::<N>(w, v);
        self.0.idx.writer.set(w + N as i32);
    }
    /// `big` = this accessor reads big-endian on a big-endian buffer.
    #[inline]
    fn be(&self, big: bool) -> bool {
        big != self.0.little_endian
    }
    fn i16_of(&self, b: [u8; 2], big: bool) -> i16 {
        if self.be(big) {
            i16::from_be_bytes(b)
        } else {
            i16::from_le_bytes(b)
        }
    }
    fn i32_of(&self, b: [u8; 4], big: bool) -> i32 {
        if self.be(big) {
            i32::from_be_bytes(b)
        } else {
            i32::from_le_bytes(b)
        }
    }
    fn i64_of(&self, b: [u8; 8], big: bool) -> i64 {
        if self.be(big) {
            i64::from_be_bytes(b)
        } else {
            i64::from_le_bytes(b)
        }
    }
    fn u24_of(&self, b: [u8; 3], big: bool) -> i32 {
        if self.be(big) {
            ((b[0] as i32) << 16) | ((b[1] as i32) << 8) | b[2] as i32
        } else {
            ((b[2] as i32) << 16) | ((b[1] as i32) << 8) | b[0] as i32
        }
    }
    fn bytes16(&self, v: i32, big: bool) -> [u8; 2] {
        if self.be(big) {
            (v as i16).to_be_bytes()
        } else {
            (v as i16).to_le_bytes()
        }
    }
    fn bytes24(&self, v: i32, big: bool) -> [u8; 3] {
        let b = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        if self.be(big) { b } else { [b[2], b[1], b[0]] }
    }
    fn bytes32(&self, v: i32, big: bool) -> [u8; 4] {
        if self.be(big) {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }
    fn bytes64(&self, v: i64, big: bool) -> [u8; 8] {
        if self.be(big) {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }
    fn medium(u: i32) -> i32 {
        if u & 0x800000 != 0 {
            u | 0xff000000u32 as i32
        } else {
            u
        }
    }
    fn char_of(v: i16) -> char {
        char::from_u32(v as u16 as u32).unwrap_or('\u{FFFD}')
    }
    fn ensure_writable0(&self, min_writable_bytes: i32) {
        self.ensure_accessible();
        let w = self.w();
        let writable = self.capacity() - w;
        if min_writable_bytes <= writable {
            return;
        }
        self.check_writable_access();
        let max = self.max_capacity();
        if min_writable_bytes > max - w {
            ioobe(format!(
                "writerIndex({w}) + minWritableBytes({min_writable_bytes}) exceeds maxCapacity({max}): {}",
                self.to_string()
            ));
        }
        let new_capacity = Self::calculate_new_capacity(w + min_writable_bytes, max);
        self.capacity_1(new_capacity);
    }
    /// `AbstractByteBufAllocator.calculateNewCapacity`.
    pub fn calculate_new_capacity(min_new_capacity: i32, max_capacity: i32) -> i32 {
        if min_new_capacity < 0 || min_new_capacity > max_capacity {
            panic!(
                "IllegalArgumentException: minNewCapacity: {min_new_capacity} (expected: 0-{max_capacity})"
            );
        }
        let threshold = CALCULATE_THRESHOLD;
        if min_new_capacity == threshold {
            return threshold;
        }
        if min_new_capacity > threshold {
            let mut new_capacity = min_new_capacity / threshold * threshold;
            if new_capacity > max_capacity - threshold {
                new_capacity = max_capacity;
            } else {
                new_capacity += threshold;
            }
            return new_capacity;
        }
        let mut new_capacity: i32 = 64;
        while new_capacity < min_new_capacity {
            new_capacity <<= 1;
        }
        new_capacity.min(max_capacity)
    }
    fn this_buf(&self) -> ByteBuf {
        self.clone()
    }

    // ---- capacity, allocator, order

    pub fn capacity(&self) -> i32 {
        match self.0.fixed_capacity {
            Some(c) => c,
            None => self.0.shared.bytes.borrow().len() as i32 - self.0.offset,
        }
    }
    /// `capacity(newCapacity)`: grows or truncates the buffer (root buffers and duplicates of them).
    pub fn capacity_1(&self, new_capacity: i32) -> ByteBuf {
        self.ensure_accessible();
        if self.0.fixed_capacity.is_some() || self.0.offset != 0 {
            panic!("UnsupportedOperationException: sliced buffer");
        }
        if new_capacity < 0 || new_capacity > self.max_capacity() {
            panic!(
                "IllegalArgumentException: newCapacity: {new_capacity} (expected: 0-{})",
                self.max_capacity()
            );
        }
        self.0
            .shared
            .bytes
            .borrow_mut()
            .resize(new_capacity as usize, 0);
        if new_capacity < self.w() {
            self.0.idx.writer.set(new_capacity);
            if self.r() > new_capacity {
                self.0.idx.reader.set(new_capacity);
            }
        }
        self.this_buf()
    }
    pub fn max_capacity(&self) -> i32 {
        match self.0.fixed_capacity {
            Some(c) => c,
            None => self.0.shared.max_capacity.get() - self.0.offset,
        }
    }
    pub fn alloc(&self) -> ByteBufAllocator {
        self.0.shared.alloc.clone()
    }
    /// `order()`: `ByteOrder.BIG_ENDIAN` unless this is an `order(LITTLE_ENDIAN)` view.
    pub fn order(&self) -> ByteOrder {
        // PORT: java.nio.ByteOrder's constants (java.nio port).
        if self.0.little_endian {
            crate::java::nio::byte_order::LITTLE_ENDIAN.clone()
        } else {
            crate::java::nio::byte_order::BIG_ENDIAN.clone()
        }
    }
    /// Whether this buffer reads multi-byte values little-endian (an `order(LITTLE_ENDIAN)` view).
    pub fn is_little_endian(&self) -> bool {
        self.0.little_endian
    }
    /// `order(endianness)`: this buffer, or a swapped view sharing its bytes and indices.
    pub fn order_1(&self, endianness: ByteOrder) -> ByteBuf {
        // PORT: java.nio.ByteOrder's constants (java.nio port).
        let little = endianness == *crate::java::nio::byte_order::LITTLE_ENDIAN;
        self.order_le(little)
    }
    /// `order(little ? LITTLE_ENDIAN : BIG_ENDIAN)`.
    pub fn order_le(&self, little: bool) -> ByteBuf {
        if little == self.0.little_endian {
            return self.this_buf();
        }
        ByteBuf(Arc::new(ByteBufInner {
            shared: self.0.shared.clone(),
            idx: self.0.idx.clone(),
            offset: self.0.offset,
            fixed_capacity: self.0.fixed_capacity,
            read_only: self.0.read_only,
            little_endian: little,
            kind: Kind::Swapped,
            parent: Some(self.clone()),
        }))
    }
    /// `unwrap()`: the buffer this one derives from, `None` (Java's null) for a root buffer.
    pub fn unwrap(&self) -> Option<ByteBuf> {
        self.0.parent.clone()
    }
    pub fn is_direct(&self) -> bool {
        self.0.shared.direct
    }
    pub fn is_read_only(&self) -> bool {
        self.0.read_only
    }
    /// `asReadOnly()`: a read-only view with its own indices.
    pub fn as_read_only(&self) -> ByteBuf {
        if self.0.read_only {
            return self.this_buf();
        }
        let cap = self.0.fixed_capacity;
        self.derive(Kind::ReadOnly, 0, cap, self.r(), self.w())
    }
    pub fn is_accessible(&self) -> bool {
        self.ref_cnt() != 0
    }
    pub fn is_contiguous(&self) -> bool {
        true
    }
    pub fn max_fast_writable_bytes(&self) -> i32 {
        self.writable_bytes()
    }
    pub fn as_byte_buf(&self) -> ByteBuf {
        self.this_buf()
    }

    // ---- indices

    pub fn reader_index(&self) -> i32 {
        self.r()
    }
    pub fn reader_index_1(&self, reader_index: i32) -> ByteBuf {
        if reader_index < 0 || reader_index > self.w() {
            ioobe(format!(
                "readerIndex: {reader_index} (expected: 0 <= readerIndex <= writerIndex({}))",
                self.w()
            ));
        }
        self.0.idx.reader.set(reader_index);
        self.this_buf()
    }
    pub fn writer_index(&self) -> i32 {
        self.w()
    }
    pub fn writer_index_1(&self, writer_index: i32) -> ByteBuf {
        if writer_index < self.r() || writer_index > self.capacity() {
            ioobe(format!(
                "writerIndex: {writer_index} (expected: readerIndex({}) <= writerIndex <= capacity({}))",
                self.r(),
                self.capacity()
            ));
        }
        self.0.idx.writer.set(writer_index);
        self.this_buf()
    }
    pub fn set_index(&self, reader_index: i32, writer_index: i32) -> ByteBuf {
        if reader_index < 0 || reader_index > writer_index || writer_index > self.capacity() {
            ioobe(format!(
                "readerIndex: {reader_index}, writerIndex: {writer_index} (expected: 0 <= readerIndex <= writerIndex <= capacity({}))",
                self.capacity()
            ));
        }
        self.0.idx.reader.set(reader_index);
        self.0.idx.writer.set(writer_index);
        self.this_buf()
    }
    pub fn readable_bytes(&self) -> i32 {
        self.w() - self.r()
    }
    pub fn writable_bytes(&self) -> i32 {
        self.capacity() - self.w()
    }
    pub fn max_writable_bytes(&self) -> i32 {
        self.max_capacity() - self.w()
    }
    pub fn is_readable(&self) -> bool {
        self.w() > self.r()
    }
    pub fn is_readable_1(&self, size: i32) -> bool {
        self.w() - self.r() >= size
    }
    pub fn is_writable(&self) -> bool {
        self.capacity() > self.w()
    }
    pub fn is_writable_1(&self, size: i32) -> bool {
        self.capacity() - self.w() >= size
    }
    pub fn clear(&self) -> ByteBuf {
        self.0.idx.reader.set(0);
        self.0.idx.writer.set(0);
        self.this_buf()
    }
    pub fn mark_reader_index(&self) -> ByteBuf {
        self.0.idx.marked_reader.set(self.r());
        self.this_buf()
    }
    pub fn reset_reader_index(&self) -> ByteBuf {
        self.reader_index_1(self.0.idx.marked_reader.get())
    }
    pub fn mark_writer_index(&self) -> ByteBuf {
        self.0.idx.marked_writer.set(self.w());
        self.this_buf()
    }
    pub fn reset_writer_index(&self) -> ByteBuf {
        self.writer_index_1(self.0.idx.marked_writer.get())
    }
    fn adjust_markers(&self, decrement: i32) {
        let mr = self.0.idx.marked_reader.get();
        let mw = self.0.idx.marked_writer.get();
        if mr <= decrement {
            self.0.idx.marked_reader.set(0);
            self.0
                .idx
                .marked_writer
                .set(if mw <= decrement { 0 } else { mw - decrement });
        } else {
            self.0.idx.marked_reader.set(mr - decrement);
            self.0.idx.marked_writer.set(mw - decrement);
        }
    }
    pub fn discard_read_bytes(&self) -> ByteBuf {
        let (r, w) = (self.r(), self.w());
        if r == 0 {
            self.ensure_accessible();
            return self.this_buf();
        }
        self.check_writable_access();
        if r != w {
            self.bytes().copy_within(r as usize..w as usize, 0);
            self.0.idx.writer.set(w - r);
            self.adjust_markers(r);
            self.0.idx.reader.set(0);
        } else {
            self.ensure_accessible();
            self.adjust_markers(r);
            self.0.idx.writer.set(0);
            self.0.idx.reader.set(0);
        }
        self.this_buf()
    }
    pub fn discard_some_read_bytes(&self) -> ByteBuf {
        let (r, w) = (self.r(), self.w());
        if r > 0 {
            if r == w {
                self.ensure_accessible();
                self.adjust_markers(r);
                self.0.idx.writer.set(0);
                self.0.idx.reader.set(0);
                return self.this_buf();
            }
            if r >= ((self.capacity() as u32) >> 1) as i32 {
                self.check_writable_access();
                self.bytes().copy_within(r as usize..w as usize, 0);
                self.0.idx.writer.set(w - r);
                self.adjust_markers(r);
                self.0.idx.reader.set(0);
                return self.this_buf();
            }
        }
        self.ensure_accessible();
        self.this_buf()
    }
    pub fn ensure_writable(&self, min_writable_bytes: i32) -> ByteBuf {
        if min_writable_bytes < 0 {
            panic!(
                "IllegalArgumentException: minWritableBytes: {min_writable_bytes} (expected: >= 0)"
            );
        }
        self.ensure_writable0(min_writable_bytes);
        self.this_buf()
    }
    /// `ensureWritable(min, force)`: 0 = enough room, 1 = not enough and not grown, 2 = grown to the
    /// maximum (still not enough), 3 = grown.
    pub fn ensure_writable_2(&self, min_writable_bytes: i32, force: bool) -> i32 {
        self.ensure_accessible();
        if min_writable_bytes < 0 {
            panic!(
                "IllegalArgumentException: minWritableBytes: {min_writable_bytes} (expected: >= 0)"
            );
        }
        if min_writable_bytes <= self.writable_bytes() {
            return 0;
        }
        let max = self.max_capacity();
        let w = self.w();
        if min_writable_bytes > max - w {
            if !force || self.capacity() == max {
                return 1;
            }
            self.capacity_1(max);
            return 2;
        }
        self.capacity_1(Self::calculate_new_capacity(w + min_writable_bytes, max));
        3
    }

    // ---- absolute get

    pub fn get_boolean(&self, index: i32) -> bool {
        self.get_byte(index) != 0
    }
    pub fn get_byte(&self, index: i32) -> i8 {
        self.get_raw::<1>(index)[0] as i8
    }
    pub fn get_unsigned_byte(&self, index: i32) -> i16 {
        self.get_raw::<1>(index)[0] as i16
    }
    pub fn get_short(&self, index: i32) -> i16 {
        let b = self.get_raw::<2>(index);
        self.i16_of(b, true)
    }
    pub fn get_short_le(&self, index: i32) -> i16 {
        let b = self.get_raw::<2>(index);
        self.i16_of(b, false)
    }
    pub fn get_unsigned_short(&self, index: i32) -> i32 {
        self.get_short(index) as u16 as i32
    }
    pub fn get_unsigned_short_le(&self, index: i32) -> i32 {
        self.get_short_le(index) as u16 as i32
    }
    pub fn get_medium(&self, index: i32) -> i32 {
        Self::medium(self.get_unsigned_medium(index))
    }
    pub fn get_medium_le(&self, index: i32) -> i32 {
        Self::medium(self.get_unsigned_medium_le(index))
    }
    pub fn get_unsigned_medium(&self, index: i32) -> i32 {
        let b = self.get_raw::<3>(index);
        self.u24_of(b, true)
    }
    pub fn get_unsigned_medium_le(&self, index: i32) -> i32 {
        let b = self.get_raw::<3>(index);
        self.u24_of(b, false)
    }
    pub fn get_int(&self, index: i32) -> i32 {
        let b = self.get_raw::<4>(index);
        self.i32_of(b, true)
    }
    pub fn get_int_le(&self, index: i32) -> i32 {
        let b = self.get_raw::<4>(index);
        self.i32_of(b, false)
    }
    pub fn get_unsigned_int(&self, index: i32) -> i64 {
        self.get_int(index) as u32 as i64
    }
    pub fn get_unsigned_int_le(&self, index: i32) -> i64 {
        self.get_int_le(index) as u32 as i64
    }
    pub fn get_long(&self, index: i32) -> i64 {
        let b = self.get_raw::<8>(index);
        self.i64_of(b, true)
    }
    pub fn get_long_le(&self, index: i32) -> i64 {
        let b = self.get_raw::<8>(index);
        self.i64_of(b, false)
    }
    pub fn get_char(&self, index: i32) -> char {
        Self::char_of(self.get_short(index))
    }
    pub fn get_float(&self, index: i32) -> f32 {
        f32::from_bits(self.get_int(index) as u32)
    }
    pub fn get_float_le(&self, index: i32) -> f32 {
        f32::from_bits(self.get_int_le(index) as u32)
    }
    pub fn get_double(&self, index: i32) -> f64 {
        f64::from_bits(self.get_long(index) as u64)
    }
    pub fn get_double_le(&self, index: i32) -> f64 {
        f64::from_bits(self.get_long_le(index) as u64)
    }
    /// `getBytes(index, dst)`: fills `dst`'s writable bytes and advances its writer index.
    pub fn get_bytes(&self, index: i32, dst: ByteBuf) -> ByteBuf {
        let n = dst.writable_bytes();
        self.get_bytes_3(index, dst, n)
    }
    /// `getBytes(index, byte[] dst)`.
    pub fn get_bytes_2(&self, index: i32, dst: &mut [i8]) -> ByteBuf {
        let n = dst.len() as i32;
        self.get_bytes_4_2(index, dst, 0, n)
    }
    /// `getBytes(index, ByteBuffer dst)`: fills `dst`'s remaining bytes.
    pub fn get_bytes_2_2(&self, index: i32, dst: ByteBuffer) -> ByteBuf {
        // PORT: java.nio.ByteBuffer's `remaining()` and `put(byte[], int, int)` (java.nio port).
        let n = dst.remaining();
        self.check_index(index, n);
        let bytes = self.copy_out(index, n);
        let signed: Vec<i8> = bytes.into_iter().map(|b| b as i8).collect();
        dst.put_3(&signed, 0, n);
        self.this_buf()
    }
    /// `getBytes(index, dst, length)`: advances `dst`'s writer index.
    pub fn get_bytes_3(&self, index: i32, dst: ByteBuf, length: i32) -> ByteBuf {
        if length > dst.writable_bytes() {
            ioobe(format!(
                "length({length}) exceeds dst.writableBytes({}) where dst is: {}",
                dst.writable_bytes(),
                dst.to_string()
            ));
        }
        let dw = dst.w();
        self.get_bytes_4(index, dst.clone(), dw, length);
        dst.0.idx.writer.set(dw + length);
        self.this_buf()
    }
    /// `getBytes(index, OutputStream out, length)`.
    pub fn get_bytes_3_2(
        &self,
        index: i32,
        out: OutputStream,
        length: i32,
    ) -> Result<ByteBuf, Throwable> {
        self.check_index(index, length);
        let signed: Vec<i8> = self
            .copy_out(index, length)
            .into_iter()
            .map(|b| b as i8)
            .collect();
        // PORT: java.io.OutputStream's `write(byte[], int, int)` (java.io port).
        out.write_3(&signed, 0, length)?;
        Ok(self.this_buf())
    }
    /// `getBytes(index, GatheringByteChannel out, length)`: the bytes written.
    pub fn get_bytes_3_3(
        &self,
        index: i32,
        out: GatheringByteChannel,
        length: i32,
    ) -> Result<i32, Throwable> {
        self.check_index(index, length);
        let signed: Vec<i8> = self
            .copy_out(index, length)
            .into_iter()
            .map(|b| b as i8)
            .collect();
        // PORT: java.nio's `ByteBuffer.wrap(byte[])` and `WritableByteChannel.write(ByteBuffer)`.
        out.write(ByteBuffer::wrap(signed))
    }
    /// `getBytes(index, dst, dstIndex, length)`.
    pub fn get_bytes_4(&self, index: i32, dst: ByteBuf, dst_index: i32, length: i32) -> ByteBuf {
        self.check_index(index, length);
        dst.check_index(dst_index, length);
        dst.check_writable_access();
        let tmp = self.copy_out(index, length);
        dst.copy_in(dst_index, &tmp);
        self.this_buf()
    }
    /// `getBytes(index, byte[] dst, dstIndex, length)`.
    pub fn get_bytes_4_2(
        &self,
        index: i32,
        dst: &mut [i8],
        dst_index: i32,
        length: i32,
    ) -> ByteBuf {
        self.check_index(index, length);
        if dst_index < 0 || length < 0 || dst_index as usize + length as usize > dst.len() {
            ioobe(format!(
                "dstIndex: {dst_index}, length: {length} (expected: range(0, {}))",
                dst.len()
            ));
        }
        let src = &self.bytes()[index as usize..(index + length) as usize];
        for (d, s) in dst[dst_index as usize..(dst_index + length) as usize]
            .iter_mut()
            .zip(src)
        {
            *d = *s as i8;
        }
        self.this_buf()
    }
    /// `getBytes(index, FileChannel out, position, length)`: the bytes written.
    pub fn get_bytes_4_3(
        &self,
        index: i32,
        out: FileChannel,
        position: i64,
        length: i32,
    ) -> Result<i32, Throwable> {
        self.check_index(index, length);
        let signed: Vec<i8> = self
            .copy_out(index, length)
            .into_iter()
            .map(|b| b as i8)
            .collect();
        // PORT: java.nio's `ByteBuffer.wrap(byte[])` and `FileChannel.write(ByteBuffer, long)`.
        out.write_2(ByteBuffer::wrap(signed), position)
    }
    /// The raw bytes `index..index+length` (a Rust convenience for `getBytes`).
    pub fn get_byte_vec(&self, index: i32, length: i32) -> Vec<u8> {
        self.check_index(index, length);
        self.copy_out(index, length)
    }
    /// `getCharSequence(index, length, charset)`.
    pub fn get_char_sequence(&self, index: i32, length: i32, charset: Charset) -> String {
        self.check_index(index, length);
        decode_charset(&self.copy_out(index, length), &charset)
    }

    // ---- absolute set

    pub fn set_boolean(&self, index: i32, value: bool) -> ByteBuf {
        self.set_byte(index, value as i32)
    }
    pub fn set_byte(&self, index: i32, value: i32) -> ByteBuf {
        self.set_raw::<1>(index, [value as u8]);
        self.this_buf()
    }
    pub fn set_short(&self, index: i32, value: i32) -> ByteBuf {
        let b = self.bytes16(value, true);
        self.set_raw(index, b);
        self.this_buf()
    }
    pub fn set_short_le(&self, index: i32, value: i32) -> ByteBuf {
        let b = self.bytes16(value, false);
        self.set_raw(index, b);
        self.this_buf()
    }
    pub fn set_medium(&self, index: i32, value: i32) -> ByteBuf {
        let b = self.bytes24(value, true);
        self.set_raw(index, b);
        self.this_buf()
    }
    pub fn set_medium_le(&self, index: i32, value: i32) -> ByteBuf {
        let b = self.bytes24(value, false);
        self.set_raw(index, b);
        self.this_buf()
    }
    pub fn set_int(&self, index: i32, value: i32) -> ByteBuf {
        let b = self.bytes32(value, true);
        self.set_raw(index, b);
        self.this_buf()
    }
    pub fn set_int_le(&self, index: i32, value: i32) -> ByteBuf {
        let b = self.bytes32(value, false);
        self.set_raw(index, b);
        self.this_buf()
    }
    pub fn set_long(&self, index: i32, value: i64) -> ByteBuf {
        let b = self.bytes64(value, true);
        self.set_raw(index, b);
        self.this_buf()
    }
    pub fn set_long_le(&self, index: i32, value: i64) -> ByteBuf {
        let b = self.bytes64(value, false);
        self.set_raw(index, b);
        self.this_buf()
    }
    pub fn set_char(&self, index: i32, value: i32) -> ByteBuf {
        self.set_short(index, value)
    }
    pub fn set_float(&self, index: i32, value: f32) -> ByteBuf {
        self.set_int(index, value.to_bits() as i32)
    }
    pub fn set_float_le(&self, index: i32, value: f32) -> ByteBuf {
        self.set_int_le(index, value.to_bits() as i32)
    }
    pub fn set_double(&self, index: i32, value: f64) -> ByteBuf {
        self.set_long(index, value.to_bits() as i64)
    }
    pub fn set_double_le(&self, index: i32, value: f64) -> ByteBuf {
        self.set_long_le(index, value.to_bits() as i64)
    }
    /// `setBytes(index, src)`: copies `src`'s readable bytes and advances its reader index.
    pub fn set_bytes(&self, index: i32, src: ByteBuf) -> ByteBuf {
        let n = src.readable_bytes();
        self.set_bytes_3(index, src, n)
    }
    pub fn set_bytes_2(&self, index: i32, src: &[i8]) -> ByteBuf {
        self.set_bytes_4_2(index, src, 0, src.len() as i32)
    }
    /// `setBytes(index, ByteBuffer src)`: copies `src`'s remaining bytes.
    pub fn set_bytes_2_2(&self, index: i32, src: ByteBuffer) -> ByteBuf {
        // PORT: java.nio.ByteBuffer's `remaining()` and `get(byte[], int, int)` (java.nio port).
        let n = src.remaining();
        let mut tmp = vec![0i8; n as usize];
        src.get_3(&mut tmp, 0, n);
        self.set_bytes_2(index, &tmp)
    }
    /// `setBytes(index, src, length)`: advances `src`'s reader index.
    pub fn set_bytes_3(&self, index: i32, src: ByteBuf, length: i32) -> ByteBuf {
        self.check_index(index, length);
        if length > src.readable_bytes() {
            ioobe(format!(
                "length({length}) exceeds src.readableBytes({}) where src is: {}",
                src.readable_bytes(),
                src.to_string()
            ));
        }
        let sr = src.r();
        self.set_bytes_4(index, src.clone(), sr, length);
        src.0.idx.reader.set(sr + length);
        self.this_buf()
    }
    /// `setBytes(index, InputStream in, length)`: the bytes read, -1 at end of stream.
    pub fn set_bytes_3_2(
        &self,
        index: i32,
        in_: InputStream,
        length: i32,
    ) -> Result<i32, Throwable> {
        self.check_writable_access();
        self.check_index(index, length);
        let mut tmp = vec![0i8; length as usize];
        // PORT: java.io.InputStream's `read(byte[], int, int)` (java.io port).
        let n = in_.read_3(&mut tmp, 0, length)?;
        if n > 0 {
            self.set_bytes_4_2(index, &tmp, 0, n);
        }
        Ok(n)
    }
    /// `setBytes(index, ScatteringByteChannel in, length)`: the bytes read, -1 at end of stream.
    pub fn set_bytes_3_3(
        &self,
        index: i32,
        in_: ScatteringByteChannel,
        length: i32,
    ) -> Result<i32, Throwable> {
        self.check_writable_access();
        self.check_index(index, length);
        // PORT: java.nio's `ByteBuffer.allocate(int)`, `ReadableByteChannel.read(ByteBuffer)`, `flip()`.
        let bb = ByteBuffer::allocate(length);
        let n = in_.read(bb.clone())?;
        if n > 0 {
            bb.flip();
            self.set_bytes_2_2(index, bb);
        }
        Ok(n)
    }
    /// `setBytes(index, src, srcIndex, length)`.
    pub fn set_bytes_4(&self, index: i32, src: ByteBuf, src_index: i32, length: i32) -> ByteBuf {
        self.check_writable_access();
        self.check_index(index, length);
        src.check_index(src_index, length);
        let tmp = src.copy_out(src_index, length);
        self.copy_in(index, &tmp);
        self.this_buf()
    }
    /// `setBytes(index, byte[] src, srcIndex, length)`.
    pub fn set_bytes_4_2(&self, index: i32, src: &[i8], src_index: i32, length: i32) -> ByteBuf {
        self.check_writable_access();
        self.check_index(index, length);
        if src_index < 0 || length < 0 || src_index as usize + length as usize > src.len() {
            ioobe(format!(
                "srcIndex: {src_index}, length: {length} (expected: range(0, {}))",
                src.len()
            ));
        }
        let dst = &mut self.bytes()[index as usize..(index + length) as usize];
        for (d, s) in dst
            .iter_mut()
            .zip(&src[src_index as usize..(src_index + length) as usize])
        {
            *d = *s as u8;
        }
        self.this_buf()
    }
    /// `setBytes(index, FileChannel in, position, length)`: the bytes read, -1 at end of file.
    pub fn set_bytes_4_3(
        &self,
        index: i32,
        in_: FileChannel,
        position: i64,
        length: i32,
    ) -> Result<i32, Throwable> {
        self.check_writable_access();
        self.check_index(index, length);
        // PORT: java.nio's `ByteBuffer.allocate(int)`, `FileChannel.read(ByteBuffer, long)`, `flip()`.
        let bb = ByteBuffer::allocate(length);
        let n = in_.read_2(bb.clone(), position)?;
        if n > 0 {
            bb.flip();
            self.set_bytes_2_2(index, bb);
        }
        Ok(n)
    }
    /// Copies raw bytes in at `index` (a Rust convenience for `setBytes`).
    pub fn set_byte_slice(&self, index: i32, src: &[u8]) -> ByteBuf {
        self.check_writable_access();
        self.check_index(index, src.len() as i32);
        self.copy_in(index, src);
        self.this_buf()
    }
    pub fn set_zero(&self, index: i32, length: i32) -> ByteBuf {
        if length == 0 {
            return self.this_buf();
        }
        self.check_writable_access();
        self.check_index(index, length);
        self.bytes()[index as usize..(index + length) as usize].fill(0);
        self.this_buf()
    }
    /// `setCharSequence(index, sequence, charset)`: the bytes written.
    pub fn set_char_sequence(&self, index: i32, sequence: &str, charset: Charset) -> i32 {
        let bytes = encode_charset(sequence, &charset);
        self.set_byte_slice(index, &bytes);
        bytes.len() as i32
    }

    // ---- relative read

    pub fn read_boolean(&self) -> bool {
        self.read_byte() != 0
    }
    pub fn read_byte(&self) -> i8 {
        self.read_raw::<1>()[0] as i8
    }
    pub fn read_unsigned_byte(&self) -> i16 {
        self.read_raw::<1>()[0] as i16
    }
    pub fn read_short(&self) -> i16 {
        let b = self.read_raw::<2>();
        self.i16_of(b, true)
    }
    pub fn read_short_le(&self) -> i16 {
        let b = self.read_raw::<2>();
        self.i16_of(b, false)
    }
    pub fn read_unsigned_short(&self) -> i32 {
        self.read_short() as u16 as i32
    }
    pub fn read_unsigned_short_le(&self) -> i32 {
        self.read_short_le() as u16 as i32
    }
    pub fn read_medium(&self) -> i32 {
        Self::medium(self.read_unsigned_medium())
    }
    pub fn read_medium_le(&self) -> i32 {
        Self::medium(self.read_unsigned_medium_le())
    }
    pub fn read_unsigned_medium(&self) -> i32 {
        let b = self.read_raw::<3>();
        self.u24_of(b, true)
    }
    pub fn read_unsigned_medium_le(&self) -> i32 {
        let b = self.read_raw::<3>();
        self.u24_of(b, false)
    }
    pub fn read_int(&self) -> i32 {
        let b = self.read_raw::<4>();
        self.i32_of(b, true)
    }
    pub fn read_int_le(&self) -> i32 {
        let b = self.read_raw::<4>();
        self.i32_of(b, false)
    }
    pub fn read_unsigned_int(&self) -> i64 {
        self.read_int() as u32 as i64
    }
    pub fn read_unsigned_int_le(&self) -> i64 {
        self.read_int_le() as u32 as i64
    }
    pub fn read_long(&self) -> i64 {
        let b = self.read_raw::<8>();
        self.i64_of(b, true)
    }
    pub fn read_long_le(&self) -> i64 {
        let b = self.read_raw::<8>();
        self.i64_of(b, false)
    }
    pub fn read_char(&self) -> char {
        Self::char_of(self.read_short())
    }
    pub fn read_float(&self) -> f32 {
        f32::from_bits(self.read_int() as u32)
    }
    pub fn read_float_le(&self) -> f32 {
        f32::from_bits(self.read_int_le() as u32)
    }
    pub fn read_double(&self) -> f64 {
        f64::from_bits(self.read_long() as u64)
    }
    pub fn read_double_le(&self) -> f64 {
        f64::from_bits(self.read_long_le() as u64)
    }
    /// `readBytes(length)`: a new buffer (from this buffer's allocator) with the next `length` bytes.
    pub fn read_bytes(&self, length: i32) -> ByteBuf {
        self.check_readable_bytes(length);
        if length == 0 {
            return crate::io::netty::buffer::unpooled::EMPTY_BUFFER.clone();
        }
        let buf = self.alloc().buffer_2(length, self.max_capacity());
        let r = self.r();
        let tmp = self.copy_out(r, length);
        buf.copy_in(0, &tmp);
        buf.0.idx.writer.set(length);
        self.0.idx.reader.set(r + length);
        buf
    }
    /// `readBytes(dst)`: fills `dst`'s writable bytes.
    pub fn read_bytes_1(&self, dst: ByteBuf) -> ByteBuf {
        let n = dst.writable_bytes();
        self.read_bytes_2(dst, n)
    }
    /// `readBytes(byte[] dst)`.
    pub fn read_bytes_1_2(&self, dst: &mut [i8]) -> ByteBuf {
        let n = dst.len() as i32;
        self.read_bytes_3_2(dst, 0, n)
    }
    /// `readBytes(ByteBuffer dst)`: fills `dst`'s remaining bytes.
    pub fn read_bytes_1_3(&self, dst: ByteBuffer) -> ByteBuf {
        // PORT: java.nio.ByteBuffer's `remaining()` (java.nio port).
        let length = dst.remaining();
        self.check_readable_bytes(length);
        let r = self.r();
        self.get_bytes_2_2(r, dst);
        self.0.idx.reader.set(r + length);
        self.this_buf()
    }
    /// `readBytes(dst, length)`: advances `dst`'s writer index.
    pub fn read_bytes_2(&self, dst: ByteBuf, length: i32) -> ByteBuf {
        if length > dst.writable_bytes() {
            ioobe(format!(
                "length({length}) exceeds dst.writableBytes({}) where dst is: {}",
                dst.writable_bytes(),
                dst.to_string()
            ));
        }
        let dw = dst.w();
        self.read_bytes_3(dst.clone(), dw, length);
        dst.0.idx.writer.set(dw + length);
        self.this_buf()
    }
    /// `readBytes(OutputStream out, length)`.
    pub fn read_bytes_2_2(&self, out: OutputStream, length: i32) -> Result<ByteBuf, Throwable> {
        self.check_readable_bytes(length);
        let r = self.r();
        self.get_bytes_3_2(r, out, length)?;
        self.0.idx.reader.set(r + length);
        Ok(self.this_buf())
    }
    /// `readBytes(GatheringByteChannel out, length)`: the bytes written.
    pub fn read_bytes_2_3(&self, out: GatheringByteChannel, length: i32) -> Result<i32, Throwable> {
        self.check_readable_bytes(length);
        let r = self.r();
        let n = self.get_bytes_3_3(r, out, length)?;
        self.0.idx.reader.set(r + n);
        Ok(n)
    }
    /// `readBytes(dst, dstIndex, length)`.
    pub fn read_bytes_3(&self, dst: ByteBuf, dst_index: i32, length: i32) -> ByteBuf {
        self.check_readable_bytes(length);
        let r = self.r();
        self.get_bytes_4(r, dst, dst_index, length);
        self.0.idx.reader.set(r + length);
        self.this_buf()
    }
    /// `readBytes(byte[] dst, dstIndex, length)`.
    pub fn read_bytes_3_2(&self, dst: &mut [i8], dst_index: i32, length: i32) -> ByteBuf {
        self.check_readable_bytes(length);
        let r = self.r();
        self.get_bytes_4_2(r, dst, dst_index, length);
        self.0.idx.reader.set(r + length);
        self.this_buf()
    }
    /// `readBytes(FileChannel out, position, length)`: the bytes written.
    pub fn read_bytes_3_3(
        &self,
        out: FileChannel,
        position: i64,
        length: i32,
    ) -> Result<i32, Throwable> {
        self.check_readable_bytes(length);
        let r = self.r();
        let n = self.get_bytes_4_3(r, out, position, length)?;
        self.0.idx.reader.set(r + n);
        Ok(n)
    }
    /// The next `length` raw bytes (a Rust convenience for `readBytes(byte[])`).
    pub fn read_byte_vec(&self, length: i32) -> Vec<u8> {
        self.check_readable_bytes(length);
        let r = self.r();
        let v = self.copy_out(r, length);
        self.0.idx.reader.set(r + length);
        v
    }
    /// `readSlice(length)`: a slice of the next `length` bytes (shares this buffer's bytes).
    pub fn read_slice(&self, length: i32) -> ByteBuf {
        self.check_readable_bytes(length);
        let r = self.r();
        let s = self.slice_2(r, length);
        self.0.idx.reader.set(r + length);
        s
    }
    pub fn read_retained_slice(&self, length: i32) -> ByteBuf {
        self.check_readable_bytes(length);
        let r = self.r();
        let s = self.retained_slice_2(r, length);
        self.0.idx.reader.set(r + length);
        s
    }
    pub fn read_char_sequence(&self, length: i32, charset: Charset) -> String {
        let s = self.get_char_sequence(self.r(), length, charset);
        self.0.idx.reader.set(self.r() + length);
        s
    }
    pub fn read_string(&self, length: i32, charset: Charset) -> String {
        self.read_char_sequence(length, charset)
    }
    pub fn skip_bytes(&self, length: i32) -> ByteBuf {
        self.check_readable_bytes(length);
        self.0.idx.reader.set(self.r() + length);
        self.this_buf()
    }

    // ---- relative write

    pub fn write_boolean(&self, value: bool) -> ByteBuf {
        self.write_byte(value as i32)
    }
    pub fn write_byte(&self, value: i32) -> ByteBuf {
        self.write_raw::<1>([value as u8]);
        self.this_buf()
    }
    pub fn write_short(&self, value: i32) -> ByteBuf {
        let b = self.bytes16(value, true);
        self.write_raw(b);
        self.this_buf()
    }
    pub fn write_short_le(&self, value: i32) -> ByteBuf {
        let b = self.bytes16(value, false);
        self.write_raw(b);
        self.this_buf()
    }
    pub fn write_medium(&self, value: i32) -> ByteBuf {
        let b = self.bytes24(value, true);
        self.write_raw(b);
        self.this_buf()
    }
    pub fn write_medium_le(&self, value: i32) -> ByteBuf {
        let b = self.bytes24(value, false);
        self.write_raw(b);
        self.this_buf()
    }
    pub fn write_int(&self, value: i32) -> ByteBuf {
        let b = self.bytes32(value, true);
        self.write_raw(b);
        self.this_buf()
    }
    pub fn write_int_le(&self, value: i32) -> ByteBuf {
        let b = self.bytes32(value, false);
        self.write_raw(b);
        self.this_buf()
    }
    pub fn write_long(&self, value: i64) -> ByteBuf {
        let b = self.bytes64(value, true);
        self.write_raw(b);
        self.this_buf()
    }
    pub fn write_long_le(&self, value: i64) -> ByteBuf {
        let b = self.bytes64(value, false);
        self.write_raw(b);
        self.this_buf()
    }
    pub fn write_char(&self, value: i32) -> ByteBuf {
        self.write_short(value)
    }
    pub fn write_float(&self, value: f32) -> ByteBuf {
        self.write_int(value.to_bits() as i32)
    }
    pub fn write_float_le(&self, value: f32) -> ByteBuf {
        self.write_int_le(value.to_bits() as i32)
    }
    pub fn write_double(&self, value: f64) -> ByteBuf {
        self.write_long(value.to_bits() as i64)
    }
    pub fn write_double_le(&self, value: f64) -> ByteBuf {
        self.write_long_le(value.to_bits() as i64)
    }
    /// `writeBytes(src)`: all of `src`'s readable bytes; advances its reader index.
    pub fn write_bytes(&self, src: ByteBuf) -> ByteBuf {
        let n = src.readable_bytes();
        self.write_bytes_2(src, n)
    }
    pub fn write_bytes_1(&self, src: &[i8]) -> ByteBuf {
        self.write_bytes_3_2(src, 0, src.len() as i32)
    }
    /// `writeBytes(ByteBuffer src)`: `src`'s remaining bytes.
    pub fn write_bytes_1_2(&self, src: ByteBuffer) -> ByteBuf {
        // PORT: java.nio.ByteBuffer's `remaining()` (java.nio port).
        let length = src.remaining();
        self.ensure_writable0(length);
        let w = self.w();
        self.set_bytes_2_2(w, src);
        self.0.idx.writer.set(w + length);
        self.this_buf()
    }
    /// `writeBytes(src, length)`: advances `src`'s reader index.
    pub fn write_bytes_2(&self, src: ByteBuf, length: i32) -> ByteBuf {
        if length > src.readable_bytes() {
            ioobe(format!(
                "length({length}) exceeds src.readableBytes({}) where src is: {}",
                src.readable_bytes(),
                src.to_string()
            ));
        }
        let sr = src.r();
        self.write_bytes_3(src.clone(), sr, length);
        src.0.idx.reader.set(sr + length);
        self.this_buf()
    }
    /// `writeBytes(InputStream in, length)`: the bytes read, -1 at end of stream.
    pub fn write_bytes_2_2(&self, in_: InputStream, length: i32) -> Result<i32, Throwable> {
        self.ensure_writable(length);
        let w = self.w();
        let n = self.set_bytes_3_2(w, in_, length)?;
        if n > 0 {
            self.0.idx.writer.set(w + n);
        }
        Ok(n)
    }
    /// `writeBytes(ScatteringByteChannel in, length)`: the bytes read, -1 at end of stream.
    pub fn write_bytes_2_3(
        &self,
        in_: ScatteringByteChannel,
        length: i32,
    ) -> Result<i32, Throwable> {
        self.ensure_writable(length);
        let w = self.w();
        let n = self.set_bytes_3_3(w, in_, length)?;
        if n > 0 {
            self.0.idx.writer.set(w + n);
        }
        Ok(n)
    }
    /// `writeBytes(src, srcIndex, length)`.
    pub fn write_bytes_3(&self, src: ByteBuf, src_index: i32, length: i32) -> ByteBuf {
        self.ensure_writable(length);
        let w = self.w();
        self.set_bytes_4(w, src, src_index, length);
        self.0.idx.writer.set(w + length);
        self.this_buf()
    }
    /// `writeBytes(byte[] src, srcIndex, length)`.
    pub fn write_bytes_3_2(&self, src: &[i8], src_index: i32, length: i32) -> ByteBuf {
        self.ensure_writable(length);
        let w = self.w();
        self.set_bytes_4_2(w, src, src_index, length);
        self.0.idx.writer.set(w + length);
        self.this_buf()
    }
    /// `writeBytes(FileChannel in, position, length)`: the bytes read, -1 at end of file.
    pub fn write_bytes_3_3(
        &self,
        in_: FileChannel,
        position: i64,
        length: i32,
    ) -> Result<i32, Throwable> {
        self.ensure_writable(length);
        let w = self.w();
        let n = self.set_bytes_4_3(w, in_, position, length)?;
        if n > 0 {
            self.0.idx.writer.set(w + n);
        }
        Ok(n)
    }
    /// Appends raw bytes (a Rust convenience for `writeBytes(byte[])`).
    pub fn write_byte_slice(&self, src: &[u8]) -> ByteBuf {
        let length = src.len() as i32;
        self.ensure_writable(length);
        let w = self.w();
        self.set_byte_slice(w, src);
        self.0.idx.writer.set(w + length);
        self.this_buf()
    }
    pub fn write_zero(&self, length: i32) -> ByteBuf {
        if length == 0 {
            return self.this_buf();
        }
        self.ensure_writable(length);
        let w = self.w();
        self.set_zero(w, length);
        self.0.idx.writer.set(w + length);
        self.this_buf()
    }
    /// `writeCharSequence(sequence, charset)`: the bytes written.
    pub fn write_char_sequence(&self, sequence: &str, charset: Charset) -> i32 {
        let bytes = encode_charset(sequence, &charset);
        self.write_byte_slice(&bytes);
        bytes.len() as i32
    }

    // ---- search

    /// `indexOf(fromIndex, toIndex, value)`: searches forwards (or backwards when `from > to`); -1 if absent.
    pub fn index_of(&self, from_index: i32, to_index: i32, value: i8) -> i32 {
        self.ensure_accessible();
        let cap = self.capacity();
        let b = value as u8;
        if from_index <= to_index {
            let from = from_index.max(0);
            let to = to_index.min(cap);
            if from >= to {
                return -1;
            }
            self.bytes()[from as usize..to as usize]
                .iter()
                .position(|&x| x == b)
                .map_or(-1, |p| from + p as i32)
        } else {
            let from = from_index.min(cap);
            let to = to_index.max(0);
            if from <= to {
                return -1;
            }
            self.bytes()[to as usize..from as usize]
                .iter()
                .rposition(|&x| x == b)
                .map_or(-1, |p| to + p as i32)
        }
    }
    /// `bytesBefore(value)`: the bytes between the reader index and the first `value`; -1 if absent.
    pub fn bytes_before(&self, value: i8) -> i32 {
        self.bytes_before_2(self.readable_bytes(), value)
    }
    pub fn bytes_before_2(&self, length: i32, value: i8) -> i32 {
        self.check_readable_bytes(length);
        self.bytes_before_3(self.r(), length, value)
    }
    pub fn bytes_before_3(&self, index: i32, length: i32, value: i8) -> i32 {
        let end_index = self.index_of(index, index + length, value);
        if end_index < 0 { -1 } else { end_index - index }
    }
    /// `forEachByte(processor)` over the readable bytes: the index where it stopped, -1 if it never did.
    pub fn for_each_byte(&self, processor: ByteProcessor) -> i32 {
        self.ensure_accessible();
        self.for_each_byte_3(self.r(), self.readable_bytes(), processor)
    }
    pub fn for_each_byte_3(&self, index: i32, length: i32, processor: ByteProcessor) -> i32 {
        self.check_index(index, length);
        for i in index..index + length {
            let b = self.bytes()[i as usize] as i8;
            if !processor.process(b) {
                return i;
            }
        }
        -1
    }
    pub fn for_each_byte_desc(&self, processor: ByteProcessor) -> i32 {
        self.ensure_accessible();
        self.for_each_byte_desc_3(self.r(), self.readable_bytes(), processor)
    }
    pub fn for_each_byte_desc_3(&self, index: i32, length: i32, processor: ByteProcessor) -> i32 {
        self.check_index(index, length);
        let mut i = index + length - 1;
        while i >= index {
            let b = self.bytes()[i as usize] as i8;
            if !processor.process(b) {
                return i;
            }
            i -= 1;
        }
        -1
    }

    // ---- copies and derived buffers

    /// `copy()`: a new buffer with a copy of the readable bytes.
    pub fn copy(&self) -> ByteBuf {
        self.copy_2(self.r(), self.readable_bytes())
    }
    pub fn copy_2(&self, index: i32, length: i32) -> ByteBuf {
        self.check_index(index, length);
        let bytes = self.copy_out(index, length);
        let alloc = self.alloc();
        ByteBuf::new_root(
            bytes,
            length,
            self.max_capacity().max(length),
            self.is_direct(),
            alloc,
        )
    }
    /// `slice()`: the readable bytes, sharing this buffer's bytes and reference count.
    pub fn slice(&self) -> ByteBuf {
        self.slice_2(self.r(), self.readable_bytes())
    }
    pub fn slice_2(&self, index: i32, length: i32) -> ByteBuf {
        self.check_index(index, length);
        self.derive(Kind::Slice, index, Some(length), 0, length)
    }
    pub fn retained_slice(&self) -> ByteBuf {
        self.retain_buf().slice()
    }
    pub fn retained_slice_2(&self, index: i32, length: i32) -> ByteBuf {
        self.retain_buf().slice_2(index, length)
    }
    /// `duplicate()`: the whole buffer with its own indices (starting at this buffer's).
    pub fn duplicate(&self) -> ByteBuf {
        self.ensure_accessible();
        let cap = self.0.fixed_capacity;
        self.derive(Kind::Duplicate, 0, cap, self.r(), self.w())
    }
    pub fn retained_duplicate(&self) -> ByteBuf {
        self.retain_buf().duplicate()
    }

    // ---- NIO and arrays

    pub fn nio_buffer_count(&self) -> i32 {
        1
    }
    /// `nioBuffer()`: the readable bytes as a `ByteBuffer`. PORT: a copy (Java shares the memory); writes to
    /// it don't reach this buffer.
    pub fn nio_buffer(&self) -> ByteBuffer {
        self.nio_buffer_2(self.r(), self.readable_bytes())
    }
    pub fn nio_buffer_2(&self, index: i32, length: i32) -> ByteBuffer {
        self.check_index(index, length);
        let signed: Vec<i8> = self
            .copy_out(index, length)
            .into_iter()
            .map(|b| b as i8)
            .collect();
        // PORT: java.nio.ByteBuffer.wrap(byte[]) (java.nio port).
        ByteBuffer::wrap(signed)
    }
    pub fn internal_nio_buffer(&self, index: i32, length: i32) -> ByteBuffer {
        self.nio_buffer_2(index, length)
    }
    pub fn nio_buffers(&self) -> Vec<ByteBuffer> {
        vec![self.nio_buffer()]
    }
    pub fn nio_buffers_2(&self, index: i32, length: i32) -> Vec<ByteBuffer> {
        vec![self.nio_buffer_2(index, length)]
    }
    pub fn has_array(&self) -> bool {
        !self.is_direct()
    }
    /// `array()`: PORT: a copy of the backing array (`0..capacity`; Java returns the array itself, so writes
    /// to it don't reach this buffer). Use `array_offset()` as in Java.
    pub fn array(&self) -> Vec<i8> {
        self.ensure_accessible();
        if self.is_direct() {
            panic!("UnsupportedOperationException: direct buffer");
        }
        let all = self.0.shared.bytes.borrow();
        all.iter().map(|&b| b as i8).collect()
    }
    pub fn array_offset(&self) -> i32 {
        self.0.offset
    }
    pub fn has_memory_address(&self) -> bool {
        false
    }
    pub fn memory_address(&self) -> i64 {
        panic!("UnsupportedOperationException")
    }
    /// Runs `f` on the readable bytes without copying (a Rust convenience).
    pub fn with_readable<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        self.ensure_accessible();
        let (r, w) = (self.r() as usize, self.w() as usize);
        f(&self.bytes()[r..w])
    }
    /// Runs `f` on the bytes `index..index+length` for writing (a Rust convenience).
    pub fn with_bytes_mut<R>(&self, index: i32, length: i32, f: impl FnOnce(&mut [u8]) -> R) -> R {
        self.check_writable_access();
        self.check_index(index, length);
        f(&mut self.bytes()[index as usize..(index + length) as usize])
    }

    // ---- Object

    /// `toString()`: "UnpooledHeapByteBuf(ridx: 0, widx: 3, cap: 256)".
    pub fn to_string(&self) -> String {
        if self.ref_cnt() == 0 {
            return format!("{}(freed)", self.simple_class_name());
        }
        let mut s = format!(
            "{}(ridx: {}, widx: {}, cap: {}",
            self.simple_class_name(),
            self.r(),
            self.w(),
            self.capacity()
        );
        if self.max_capacity() != i32::MAX {
            s.push('/');
            s.push_str(&self.max_capacity().to_string());
        }
        if let Some(p) = &self.0.parent {
            s.push_str(", unwrapped: ");
            s.push_str(&p.to_string());
        }
        s.push(')');
        s
    }
    fn simple_class_name(&self) -> &'static str {
        match self.0.kind {
            Kind::Root => {
                if self.0.shared.alloc.is_pooled() {
                    if self.is_direct() {
                        "PooledUnsafeDirectByteBuf"
                    } else {
                        "PooledUnsafeHeapByteBuf"
                    }
                } else if self.is_direct() {
                    "UnpooledUnsafeDirectByteBuf"
                } else {
                    "UnpooledHeapByteBuf"
                }
            }
            Kind::Slice => "UnpooledSlicedByteBuf",
            Kind::Duplicate => "UnpooledDuplicatedByteBuf",
            Kind::ReadOnly => "ReadOnlyByteBuf",
            Kind::Swapped => "SwappedByteBuf",
        }
    }
    /// `toString(charset)`: the readable bytes decoded.
    pub fn to_string_1(&self, charset: Charset) -> String {
        self.to_string_3(self.r(), self.readable_bytes(), charset)
    }
    pub fn to_string_3(&self, index: i32, length: i32, charset: Charset) -> String {
        self.get_char_sequence(index, length, charset)
    }
    /// The readable bytes as UTF-8 (`toString(StandardCharsets.UTF_8)`).
    pub fn to_string_utf8(&self) -> String {
        self.with_readable(|b| String::from_utf8_lossy(b).into_owned())
    }
    /// `hashCode()` (`ByteBufUtil.hashCode`).
    pub fn hash_code(&self) -> i32 {
        crate::io::netty::buffer::byte_buf_util::ByteBufUtil::hash_code(self.clone())
    }
    /// `equals(obj)`: another buffer with the same readable bytes.
    pub fn equals(&self, obj: JObjectRef) -> bool {
        match ByteBuf::from_object(&obj) {
            Some(o) => {
                self.same(&o)
                    || crate::io::netty::buffer::byte_buf_util::ByteBufUtil::equals(self.clone(), o)
            }
            None => false,
        }
    }
    pub fn compare_to(&self, buffer: ByteBuf) -> i32 {
        crate::io::netty::buffer::byte_buf_util::ByteBufUtil::compare(self.clone(), buffer)
    }

    // ---- reference counting

    pub fn ref_cnt(&self) -> i32 {
        self.0.shared.ref_cnt.load(AtomicOrdering::Acquire)
    }
    fn retain0(&self, increment: i32) {
        if increment <= 0 {
            panic!("IllegalArgumentException: increment: {increment} (expected: > 0)");
        }
        let rc = &self.0.shared.ref_cnt;
        let mut cur = rc.load(AtomicOrdering::Acquire);
        loop {
            if cur <= 0 || cur.checked_add(increment).is_none() {
                throwable_util::throw_(
                    IllegalReferenceCountException::of_increment(cur, increment).into(),
                );
            }
            match rc.compare_exchange_weak(
                cur,
                cur + increment,
                AtomicOrdering::AcqRel,
                AtomicOrdering::Acquire,
            ) {
                Ok(_) => return,
                Err(v) => cur = v,
            }
        }
    }
    fn retain_buf(&self) -> ByteBuf {
        self.retain0(1);
        self.this_buf()
    }
    /// `retain()`.
    pub fn retain(&self) -> ByteBuf {
        self.retain_buf()
    }
    pub fn retain_1(&self, increment: i32) -> ByteBuf {
        self.retain0(increment);
        self.this_buf()
    }
    pub fn touch(&self) -> ByteBuf {
        self.this_buf()
    }
    pub fn touch_1(&self, hint: JObjectRef) -> ByteBuf {
        let _ = hint;
        self.this_buf()
    }
    /// `release()`: true when the count reached 0 (the bytes are freed).
    pub fn release(&self) -> bool {
        self.release_1(1)
    }
    pub fn release_1(&self, decrement: i32) -> bool {
        if decrement <= 0 {
            panic!("IllegalArgumentException: decrement: {decrement} (expected: > 0)");
        }
        let rc = &self.0.shared.ref_cnt;
        let mut cur = rc.load(AtomicOrdering::Acquire);
        loop {
            if cur < decrement {
                throwable_util::throw_(
                    IllegalReferenceCountException::of_increment(cur, -decrement).into(),
                );
            }
            match rc.compare_exchange_weak(
                cur,
                cur - decrement,
                AtomicOrdering::AcqRel,
                AtomicOrdering::Acquire,
            ) {
                Ok(_) => break,
                Err(v) => cur = v,
            }
        }
        if cur == decrement {
            // deallocate()
            self.0.shared.bytes.set(Vec::new());
            true
        } else {
            false
        }
    }
}

impl ReferenceCounted for ByteBuf {
    fn ref_cnt(&self) -> i32 {
        ByteBuf::ref_cnt(self)
    }
    fn retain(&self) {
        ByteBuf::retain(self);
    }
    fn retain_1(&self, increment: i32) {
        ByteBuf::retain_1(self, increment);
    }
    fn release(&self) -> bool {
        ByteBuf::release(self)
    }
    fn release_1(&self, decrement: i32) -> bool {
        ByteBuf::release_1(self, decrement)
    }
}

/// The charsets the port decodes: UTF-8, US-ASCII, ISO-8859-1, UTF-16 (BE/LE).
fn charset_kind(charset: &Charset) -> String {
    // PORT: java.nio.charset.Charset's `name()` (java.nio port).
    charset.name().to_ascii_uppercase()
}

pub(crate) fn decode_charset(bytes: &[u8], charset: &Charset) -> String {
    match charset_kind(charset).as_str() {
        "US-ASCII" => bytes
            .iter()
            .map(|&b| if b < 0x80 { b as char } else { '\u{FFFD}' })
            .collect(),
        "ISO-8859-1" => bytes.iter().map(|&b| b as char).collect(),
        "UTF-16BE" | "UTF-16" => {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        "UTF-16LE" => {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

pub(crate) fn encode_charset(s: &str, charset: &Charset) -> Vec<u8> {
    match charset_kind(charset).as_str() {
        "US-ASCII" => s
            .chars()
            .map(|c| if (c as u32) < 0x80 { c as u8 } else { b'?' })
            .collect(),
        "ISO-8859-1" => s
            .chars()
            .map(|c| {
                if (c as u32) < 0x100 {
                    c as u32 as u8
                } else {
                    b'?'
                }
            })
            .collect(),
        "UTF-16BE" | "UTF-16" => s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect(),
        "UTF-16LE" => s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(),
        _ => s.as_bytes().to_vec(),
    }
}
