//! Port of `io.netty.buffer.ByteBufAllocator` (with `AbstractByteBufAllocator`'s behaviour). A cheap handle;
//! `ByteBufAllocator::DEFAULT` is the pooled allocator as in Netty. The port does no pooling (Rust's allocator
//! serves every buffer); "pooled", "direct" and "heap" only change what `toString`/`isDirect` report.
use crate::io::netty::buffer::byte_buf::ByteBuf;
use crate::prelude::*;

pub const DEFAULT_INITIAL_CAPACITY: i32 = 256;
pub const DEFAULT_MAX_CAPACITY: i32 = i32::MAX;

pub(crate) struct AllocInner {
    pub(crate) pooled: bool,
    pub(crate) prefer_direct: bool,
}

#[derive(Clone)]
pub struct ByteBufAllocator(pub(crate) Arc<AllocInner>);

/// `ByteBufAllocator.DEFAULT` (`ByteBufUtil.DEFAULT_ALLOCATOR`: pooled, preferring direct buffers).
pub static DEFAULT: LazyLock<ByteBufAllocator> = LazyLock::new(|| ByteBufAllocator::of(true, true));

fn validate(initial_capacity: i32, max_capacity: i32) {
    if initial_capacity < 0 {
        panic!("IllegalArgumentException: initialCapacity: {initial_capacity} (expected: 0+)");
    }
    if initial_capacity > max_capacity {
        panic!(
            "IllegalArgumentException: initialCapacity: {initial_capacity} (expected: not greater than maxCapacity({max_capacity})"
        );
    }
}

impl ByteBufAllocator {
    pub(crate) fn of(pooled: bool, prefer_direct: bool) -> ByteBufAllocator {
        ByteBufAllocator(Arc::new(AllocInner {
            pooled,
            prefer_direct,
        }))
    }
    pub fn is_pooled(&self) -> bool {
        self.0.pooled
    }
    fn new_buffer(&self, initial_capacity: i32, max_capacity: i32, direct: bool) -> ByteBuf {
        validate(initial_capacity, max_capacity);
        ByteBuf::new_root(
            vec![0u8; initial_capacity as usize],
            0,
            max_capacity,
            direct,
            self.clone(),
        )
    }
    /// `buffer()`: direct or heap as the allocator prefers.
    pub fn buffer(&self) -> ByteBuf {
        self.buffer_2(DEFAULT_INITIAL_CAPACITY, DEFAULT_MAX_CAPACITY)
    }
    pub fn buffer_1(&self, initial_capacity: i32) -> ByteBuf {
        self.buffer_2(initial_capacity, DEFAULT_MAX_CAPACITY)
    }
    pub fn buffer_2(&self, initial_capacity: i32, max_capacity: i32) -> ByteBuf {
        self.new_buffer(initial_capacity, max_capacity, self.0.prefer_direct)
    }
    /// `ioBuffer()`: a buffer for I/O (direct).
    pub fn io_buffer(&self) -> ByteBuf {
        self.direct_buffer()
    }
    pub fn io_buffer_1(&self, initial_capacity: i32) -> ByteBuf {
        self.direct_buffer_1(initial_capacity)
    }
    pub fn io_buffer_2(&self, initial_capacity: i32, max_capacity: i32) -> ByteBuf {
        self.direct_buffer_2(initial_capacity, max_capacity)
    }
    pub fn heap_buffer(&self) -> ByteBuf {
        self.heap_buffer_2(DEFAULT_INITIAL_CAPACITY, DEFAULT_MAX_CAPACITY)
    }
    pub fn heap_buffer_1(&self, initial_capacity: i32) -> ByteBuf {
        self.heap_buffer_2(initial_capacity, DEFAULT_MAX_CAPACITY)
    }
    pub fn heap_buffer_2(&self, initial_capacity: i32, max_capacity: i32) -> ByteBuf {
        self.new_buffer(initial_capacity, max_capacity, false)
    }
    pub fn direct_buffer(&self) -> ByteBuf {
        self.direct_buffer_2(DEFAULT_INITIAL_CAPACITY, DEFAULT_MAX_CAPACITY)
    }
    pub fn direct_buffer_1(&self, initial_capacity: i32) -> ByteBuf {
        self.direct_buffer_2(initial_capacity, DEFAULT_MAX_CAPACITY)
    }
    pub fn direct_buffer_2(&self, initial_capacity: i32, max_capacity: i32) -> ByteBuf {
        self.new_buffer(initial_capacity, max_capacity, true)
    }
    pub fn is_direct_buffer_pooled(&self) -> bool {
        self.0.pooled
    }
    pub fn calculate_new_capacity(&self, min_new_capacity: i32, max_capacity: i32) -> i32 {
        ByteBuf::calculate_new_capacity(min_new_capacity, max_capacity)
    }
}
