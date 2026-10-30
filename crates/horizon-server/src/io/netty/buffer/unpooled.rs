//! Port of `io.netty.buffer.Unpooled`: new heap/direct buffers, wrapped and copied arrays.
//!
//! PORT: `wrappedBuffer(byte[])` copies the array (a Rust `Vec` can't be shared with a Java-style array);
//! code that writes into a wrapped array and reads the array back must read the buffer instead.
use crate::io::netty::buffer::byte_buf::ByteBuf;
use crate::io::netty::buffer::byte_buf_allocator::{
    ByteBufAllocator, DEFAULT_INITIAL_CAPACITY, DEFAULT_MAX_CAPACITY,
};
use crate::io::netty::buffer::unpooled_byte_buf_allocator;
use crate::prelude::*;

/// `Unpooled.EMPTY_BUFFER`: capacity 0, max capacity 0.
pub static EMPTY_BUFFER: LazyLock<ByteBuf> =
    LazyLock::new(|| ByteBuf::new_root(Vec::new(), 0, 0, false, alloc()));

fn alloc() -> ByteBufAllocator {
    unpooled_byte_buf_allocator::DEFAULT.clone()
}

fn signed_to_bytes(a: &[i8]) -> Vec<u8> {
    a.iter().map(|&b| b as u8).collect()
}

pub struct Unpooled;

impl Unpooled {
    /// `buffer()`: a heap buffer, 256 bytes growing to `i32::MAX`.
    pub fn buffer() -> ByteBuf {
        alloc().heap_buffer_2(DEFAULT_INITIAL_CAPACITY, DEFAULT_MAX_CAPACITY)
    }
    pub fn buffer_1(initial_capacity: i32) -> ByteBuf {
        alloc().heap_buffer_1(initial_capacity)
    }
    pub fn buffer_2(initial_capacity: i32, max_capacity: i32) -> ByteBuf {
        alloc().heap_buffer_2(initial_capacity, max_capacity)
    }
    pub fn direct_buffer() -> ByteBuf {
        alloc().direct_buffer()
    }
    pub fn direct_buffer_1(initial_capacity: i32) -> ByteBuf {
        alloc().direct_buffer_1(initial_capacity)
    }
    pub fn direct_buffer_2(initial_capacity: i32, max_capacity: i32) -> ByteBuf {
        alloc().direct_buffer_2(initial_capacity, max_capacity)
    }
    /// `wrappedBuffer(byte[])`: readable bytes = the array, capacity = its length (PORT: a copy).
    pub fn wrapped_buffer(array: &[i8]) -> ByteBuf {
        if array.is_empty() {
            return EMPTY_BUFFER.clone();
        }
        let len = array.len() as i32;
        ByteBuf::new_root(signed_to_bytes(array), len, len, false, alloc())
    }
    /// `wrappedBuffer(byte[], offset, length)`: a slice of the wrapped array.
    pub fn wrapped_buffer_3(array: &[i8], offset: i32, length: i32) -> ByteBuf {
        if length == 0 {
            return EMPTY_BUFFER.clone();
        }
        if offset == 0 && length as usize == array.len() {
            return Self::wrapped_buffer(array);
        }
        Self::wrapped_buffer(array).slice_2(offset, length)
    }
    /// `wrappedBuffer(ByteBuf)`: the buffer's readable bytes as a slice, or `EMPTY_BUFFER`.
    pub fn wrapped_buffer_1(buffer: ByteBuf) -> ByteBuf {
        if buffer.is_readable() {
            buffer.slice()
        } else {
            buffer.release();
            EMPTY_BUFFER.clone()
        }
    }
    /// Wraps raw bytes (a Rust convenience for `wrappedBuffer(byte[])`; takes the vector without copying).
    pub fn wrapped_bytes(bytes: Vec<u8>) -> ByteBuf {
        let len = bytes.len() as i32;
        ByteBuf::new_root(bytes, len, len.max(0), false, alloc())
    }
    /// `copiedBuffer(byte[])`.
    pub fn copied_buffer(array: &[i8]) -> ByteBuf {
        Self::wrapped_buffer(array)
    }
    pub fn copied_buffer_3(array: &[i8], offset: i32, length: i32) -> ByteBuf {
        Self::wrapped_buffer(&array[offset as usize..(offset + length) as usize])
    }
    /// `copiedBuffer(ByteBuf)`: a copy of the readable bytes.
    pub fn copied_buffer_1(buffer: ByteBuf) -> ByteBuf {
        let readable = buffer.readable_bytes();
        if readable > 0 {
            let copy = Self::buffer_1(readable);
            copy.write_bytes_3(buffer.clone(), buffer.reader_index(), readable);
            copy
        } else {
            EMPTY_BUFFER.clone()
        }
    }
    /// `copiedBuffer(CharSequence, UTF_8)`.
    pub fn copied_buffer_utf8(string: &str) -> ByteBuf {
        Self::wrapped_bytes(string.as_bytes().to_vec())
    }
    /// `unreleasableBuffer(buf)`: PORT: the buffer itself (the port does not track release misuse).
    pub fn unreleasable_buffer(buf: ByteBuf) -> ByteBuf {
        buf
    }
    /// `unmodifiableBuffer(buf)`: a read-only view.
    pub fn unmodifiable_buffer(buf: ByteBuf) -> ByteBuf {
        buf.as_read_only()
    }
    pub fn copy_int(value: i32) -> ByteBuf {
        let b = Self::buffer_1(4);
        b.write_int(value);
        b
    }
    pub fn copy_long(value: i64) -> ByteBuf {
        let b = Self::buffer_1(8);
        b.write_long(value);
        b
    }
    pub fn copy_short(value: i32) -> ByteBuf {
        let b = Self::buffer_1(2);
        b.write_short(value);
        b
    }
    pub fn copy_boolean(value: bool) -> ByteBuf {
        let b = Self::buffer_1(1);
        b.write_boolean(value);
        b
    }
}
