//! Port of `com.mojang.datafixers.util.Function13` (DFU): a cheap-to-clone handle on a closure
//! (`Function13::new(|t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12, t13| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function13
pub struct Function13<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, R>(
    pub Arc<dyn Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13) -> R + Send + Sync>,
);

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, R> Clone
    for Function13<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, R>
{
    fn clone(&self) -> Self {
        Function13(self.0.clone())
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, R>
    Function13<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, R>
{
    pub fn new(
        f: impl Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13) -> R + Send + Sync + 'static,
    ) -> Self {
        Function13(Arc::new(f))
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
    ) -> R {
        (self.0)(t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12, t13)
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
    R,
    F: Fn(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13) -> R + Send + Sync + 'static,
> From<F> for Function13<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, R>
{
    fn from(f: F) -> Self {
        Function13(Arc::new(f))
    }
}
