//! Port of `com.mojang.datafixers.util.Function12` (DFU): a cheap-to-clone handle on a closure
//! (`Function12::new(|t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function12
pub struct Function12<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, R>(
    pub Arc<dyn Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12) -> R + Send + Sync>,
);

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, R> Clone
    for Function12<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, R>
{
    fn clone(&self) -> Self {
        Function12(self.0.clone())
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, R>
    Function12<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, R>
{
    pub fn new(
        f: impl Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12) -> R + Send + Sync + 'static,
    ) -> Self {
        Function12(Arc::new(f))
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
    ) -> R {
        (self.0)(t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12)
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
    R,
    F: Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12) -> R + Send + Sync + 'static,
> From<F> for Function12<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, R>
{
    fn from(f: F) -> Self {
        Function12(Arc::new(f))
    }
}
