//! Port of `io.netty.buffer.PooledByteBufAllocator` (no real pooling: see `ByteBufAllocator`).
use crate::io::netty::buffer::byte_buf_allocator::ByteBufAllocator;
use crate::prelude::*;

/// `PooledByteBufAllocator.DEFAULT`.
pub static DEFAULT: LazyLock<ByteBufAllocator> =
    LazyLock::new(|| crate::io::netty::buffer::byte_buf_allocator::DEFAULT.clone());

pub struct PooledByteBufAllocator;

impl PooledByteBufAllocator {
    /// `new PooledByteBufAllocator(preferDirect)`.
    pub fn new(prefer_direct: bool) -> ByteBufAllocator {
        ByteBufAllocator::of(true, prefer_direct)
    }
}
