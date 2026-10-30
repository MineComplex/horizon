//! Port of `com.mojang.datafixers.util.Function16` (DFU): a cheap-to-clone handle on a closure
//! (`Function16::new(|t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12, t13, t14, t15, t16| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function16
pub struct Function16<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, R>(
    pub  Arc<
        dyn Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16) -> R
            + Send
            + Sync,
    >,
);

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, R> Clone
    for Function16<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, R>
{
    fn clone(&self) -> Self {
        Function16(self.0.clone())
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, R>
    Function16<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, R>
{
    pub fn new(
        f: impl Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16) -> R
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Function16(Arc::new(f))
    }
    pub fn apply(
        &self,
        t1: T1,
        t2: T2,
        t3: T3,
        t4: T4,
        t5: T5,
        t6: T6,
        t7: T7,
        t8: T8,
        t9: T9,
        t10: T10,
        t11: T11,
        t12: T12,
        t13: T13,
        t14: T14,
        t15: T15,
        t16: T16,
    ) -> R {
        (self.0)(
            t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12, t13, t14, t15, t16,
        )
    }
}

impl<
    T1,
    T2,
    T3,
    T4,
    T5,
    T6,
    T7,
    T8,
    T9,
    T10,
    T11,
    T12,
    T13,
    T14,
    T15,
    T16,
    R,
    F: Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16) -> R
        + Send
        + Sync
        + 'static,
> From<F> for Function16<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, R>
{
    fn from(f: F) -> Self {
        Function16(Arc::new(f))
    }
}
