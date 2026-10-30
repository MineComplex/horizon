//! Port of `io.netty.util.ByteProcessor`: a callback for `ByteBuf.forEachByte`. A cheap handle around a
//! closure (`ByteProcessor::new(|b| b != 0)`); `process` returns false to stop the scan.
use crate::prelude::*;

#[derive(Clone)]
pub struct ByteProcessor(pub Arc<dyn Fn(i8) -> bool + Send + Sync>);

impl ByteProcessor {
    pub fn new(f: impl Fn(i8) -> bool + Send + Sync + 'static) -> Self {
        ByteProcessor(Arc::new(f))
    }
    /// `process(value)`: true to continue, false to stop.
    pub fn process(&self, value: i8) -> bool {
        (self.0)(value)
    }
}

impl<F: Fn(i8) -> bool + Send + Sync + 'static> From<F> for ByteProcessor {
    fn from(f: F) -> Self {
        ByteProcessor::new(f)
    }
}

/// `ByteProcessor.IndexOfProcessor`: stops at `byte_to_find`.
pub fn index_of_processor(byte_to_find: i8) -> ByteProcessor {
    ByteProcessor::new(move |v| v != byte_to_find)
}
/// `ByteProcessor.IndexNotOfProcessor`: stops at any other byte.
pub fn index_not_of_processor(byte_to_not_find: i8) -> ByteProcessor {
    ByteProcessor::new(move |v| v == byte_to_not_find)
}

pub static FIND_NUL: LazyLock<ByteProcessor> = LazyLock::new(|| index_of_processor(0));
pub static FIND_NON_NUL: LazyLock<ByteProcessor> = LazyLock::new(|| index_not_of_processor(0));
pub static FIND_CR: LazyLock<ByteProcessor> = LazyLock::new(|| index_of_processor(b'\r' as i8));
pub static FIND_NON_CR: LazyLock<ByteProcessor> =
    LazyLock::new(|| index_not_of_processor(b'\r' as i8));
pub static FIND_LF: LazyLock<ByteProcessor> = LazyLock::new(|| index_of_processor(b'\n' as i8));
pub static FIND_NON_LF: LazyLock<ByteProcessor> =
    LazyLock::new(|| index_not_of_processor(b'\n' as i8));
pub static FIND_SEMI_COLON: LazyLock<ByteProcessor> =
    LazyLock::new(|| index_of_processor(b';' as i8));
pub static FIND_COMMA: LazyLock<ByteProcessor> = LazyLock::new(|| index_of_processor(b',' as i8));
pub static FIND_ASCII_SPACE: LazyLock<ByteProcessor> =
    LazyLock::new(|| index_of_processor(b' ' as i8));
pub static FIND_CRLF: LazyLock<ByteProcessor> =
    LazyLock::new(|| ByteProcessor::new(|v| v != b'\r' as i8 && v != b'\n' as i8));
pub static FIND_NON_CRLF: LazyLock<ByteProcessor> =
    LazyLock::new(|| ByteProcessor::new(|v| v == b'\r' as i8 || v == b'\n' as i8));
pub static FIND_LINEAR_WHITESPACE: LazyLock<ByteProcessor> =
    LazyLock::new(|| ByteProcessor::new(|v| v != b' ' as i8 && v != b'\t' as i8));
pub static FIND_NON_LINEAR_WHITESPACE: LazyLock<ByteProcessor> =
    LazyLock::new(|| ByteProcessor::new(|v| v == b' ' as i8 || v == b'\t' as i8));
