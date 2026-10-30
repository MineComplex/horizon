//! Port of `io.netty.buffer.UnpooledByteBufAllocator`.
use crate::io::netty::buffer::byte_buf_allocator::ByteBufAllocator;
use crate::prelude::*;

/// `UnpooledByteBufAllocator.DEFAULT` (prefers direct buffers, as Netty's does where it can).
pub static DEFAULT: LazyLock<ByteBufAllocator> =
    LazyLock::new(|| ByteBufAllocator::of(false, true));

pub struct UnpooledByteBufAllocator;

impl UnpooledByteBufAllocator {
    /// `new UnpooledByteBufAllocator(preferDirect)`.
    pub fn new(prefer_direct: bool) -> ByteBufAllocator {
        ByteBufAllocator::of(false, prefer_direct)
    }
}
