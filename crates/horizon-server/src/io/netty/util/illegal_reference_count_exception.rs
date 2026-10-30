//! Port of `io.netty.util.IllegalReferenceCountException` (extends IllegalStateException).
use crate::prelude::*;

crate::netty_exception!(IllegalReferenceCountException);

impl IllegalReferenceCountException {
    /// `new IllegalReferenceCountException(refCnt)`: "refCnt: 0".
    pub fn of_ref_cnt(ref_cnt: i32) -> JRef<Self> {
        Self::new_1(&format!("refCnt: {ref_cnt}"))
    }
    /// `new IllegalReferenceCountException(refCnt, increment)`.
    pub fn of_increment(ref_cnt: i32, increment: i32) -> JRef<Self> {
        if increment > 0 {
            Self::new_1(&format!("refCnt: {ref_cnt}, increment: {increment}"))
        } else {
            Self::new_1(&format!("refCnt: {ref_cnt}, decrement: {}", -increment))
        }
    }
}
