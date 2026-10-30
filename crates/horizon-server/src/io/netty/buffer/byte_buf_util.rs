//! Port of `io.netty.buffer.ByteBufUtil`: UTF-8 helpers, equality, hashing and hex dumps of buffers.
use crate::io::netty::buffer::byte_buf::ByteBuf;
use crate::io::netty::buffer::byte_buf_allocator::{self, ByteBufAllocator};
use crate::io::netty::util::illegal_reference_count_exception::IllegalReferenceCountException;
use crate::io::netty::util::internal::throwable_util;
use crate::prelude::*;

/// `ByteBufUtil.DEFAULT_ALLOCATOR`.
pub static DEFAULT_ALLOCATOR: LazyLock<ByteBufAllocator> =
    LazyLock::new(|| byte_buf_allocator::DEFAULT.clone());

const MAX_BYTES_PER_CHAR_UTF8: i32 = 3;

pub struct ByteBufUtil;

impl ByteBufUtil {
    /// `utf8MaxBytes(seqLength)`: the most bytes `seqLength` UTF-16 chars can take in UTF-8.
    pub fn utf8_max_bytes(seq_length: i32) -> i32 {
        seq_length * MAX_BYTES_PER_CHAR_UTF8
    }
    /// `utf8MaxBytes(CharSequence)` (Java's length counts UTF-16 units).
    pub fn utf8_max_bytes_1(seq: &str) -> i32 {
        Self::utf8_max_bytes(seq.encode_utf16().count() as i32)
    }
    /// `utf8Bytes(seq)`: the exact UTF-8 length.
    pub fn utf8_bytes(seq: &str) -> i32 {
        seq.len() as i32
    }
    /// `writeUtf8(buf, seq)`: writes `seq` as UTF-8; the bytes written.
    pub fn write_utf8(buf: ByteBuf, seq: &str) -> i32 {
        buf.write_byte_slice(seq.as_bytes());
        seq.len() as i32
    }
    /// `reserveAndWriteUtf8(buf, seq, reserveBytes)`.
    pub fn reserve_and_write_utf8(buf: ByteBuf, seq: &str, reserve_bytes: i32) -> i32 {
        buf.ensure_writable(reserve_bytes);
        Self::write_utf8(buf, seq)
    }
    /// `writeUtf8(alloc, seq)`: a new buffer holding `seq`.
    pub fn write_utf8_alloc(alloc: ByteBufAllocator, seq: &str) -> ByteBuf {
        let buf = alloc.buffer_1(Self::utf8_max_bytes_1(seq));
        Self::write_utf8(buf.clone(), seq);
        buf
    }
    /// `writeAscii(buf, seq)`.
    pub fn write_ascii(buf: ByteBuf, seq: &str) -> i32 {
        let bytes: Vec<u8> = seq
            .chars()
            .map(|c| if (c as u32) < 0x80 { c as u8 } else { b'?' })
            .collect();
        buf.write_byte_slice(&bytes);
        bytes.len() as i32
    }
    /// `ensureAccessible(buf)`: `buf`, or IllegalReferenceCountException when it was released.
    pub fn ensure_accessible(buf: ByteBuf) -> ByteBuf {
        if !buf.is_accessible() {
            throwable_util::throw_(
                IllegalReferenceCountException::of_ref_cnt(buf.ref_cnt()).into(),
            );
        }
        buf
    }
    /// `hashCode(buffer)`.
    pub fn hash_code(buffer: ByteBuf) -> i32 {
        let a_len = buffer.readable_bytes();
        let int_count = ((a_len as u32) >> 2) as i32;
        let byte_count = a_len & 3;
        let mut hash_code: i32 = 1;
        let mut array_index = buffer.reader_index();
        for _ in 0..int_count {
            let v = if buffer.is_little_endian() {
                buffer.get_int(array_index).swap_bytes()
            } else {
                buffer.get_int(array_index)
            };
            hash_code = hash_code.wrapping_mul(31).wrapping_add(v);
            array_index += 4;
        }
        for _ in 0..byte_count {
            hash_code = hash_code
                .wrapping_mul(31)
                .wrapping_add(buffer.get_byte(array_index) as i32);
            array_index += 1;
        }
        if hash_code == 0 {
            hash_code = 1;
        }
        hash_code
    }
    /// `equals(a, b)`: the same readable bytes.
    pub fn equals(buffer_a: ByteBuf, buffer_b: ByteBuf) -> bool {
        let a_len = buffer_a.readable_bytes();
        if a_len != buffer_b.readable_bytes() {
            return false;
        }
        Self::equals_5(
            buffer_a.clone(),
            buffer_a.reader_index(),
            buffer_b.clone(),
            buffer_b.reader_index(),
            a_len,
        )
    }
    /// `equals(a, aStartIndex, b, bStartIndex, length)`.
    pub fn equals_5(
        a: ByteBuf,
        a_start_index: i32,
        b: ByteBuf,
        b_start_index: i32,
        length: i32,
    ) -> bool {
        if a_start_index < 0 || b_start_index < 0 || length < 0 {
            panic!("IllegalArgumentException: All indexes and lengths must be non-negative");
        }
        if a.writer_index() - length < a_start_index || b.writer_index() - length < b_start_index {
            return false;
        }
        a.get_byte_vec(a_start_index, length) == b.get_byte_vec(b_start_index, length)
    }
    /// `compare(a, b)`: unsigned lexicographic order of the readable bytes, then length.
    pub fn compare(buffer_a: ByteBuf, buffer_b: ByteBuf) -> i32 {
        if buffer_a.same(&buffer_b) {
            return 0;
        }
        let a = buffer_a.with_readable(|b| b.to_vec());
        let b = buffer_b.with_readable(|b| b.to_vec());
        for (x, y) in a.iter().zip(&b) {
            if x != y {
                return if x < y { -1 } else { 1 };
            }
        }
        a.len() as i32 - b.len() as i32
    }
    /// `hexDump(buffer)`: the readable bytes in lowercase hex.
    pub fn hex_dump(buffer: ByteBuf) -> String {
        buffer.with_readable(|b| b.iter().map(|x| format!("{x:02x}")).collect())
    }
    pub fn hex_dump_3(buffer: ByteBuf, from_index: i32, length: i32) -> String {
        buffer
            .get_byte_vec(from_index, length)
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    }
    /// `getBytes(buf)`: a copy of the readable bytes.
    pub fn get_bytes(buf: ByteBuf) -> Vec<i8> {
        buf.with_readable(|b| b.iter().map(|&x| x as i8).collect())
    }
    pub fn get_bytes_3(buf: ByteBuf, start: i32, length: i32) -> Vec<i8> {
        buf.get_byte_vec(start, length)
            .into_iter()
            .map(|x| x as i8)
            .collect()
    }
}
