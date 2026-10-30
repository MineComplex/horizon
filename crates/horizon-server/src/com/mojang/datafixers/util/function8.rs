//! Port of `com.mojang.datafixers.util.Function8` (DFU): a cheap-to-clone handle on a closure
//! (`Function8::new(|t1, t2, t3, t4, t5, t6, t7, t8| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function8
pub struct Function8<T1, T2, T3, T4, T5, T6, T7, T8, R>(
    pub Arc<dyn Fn(T1, T2, T3, T4, T5, T6, T7, T8) -> R + Send + Sync>,
);

impl<T1, T2, T3, T4, T5, T6, T7, T8, R> Clone for Function8<T1, T2, T3, T4, T5, T6, T7, T8, R> {
    fn clone(&self) -> Self {
        Function8(self.0.clone())
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, R> Function8<T1, T2, T3, T4, T5, T6, T7, T8, R> {
    pub fn new(f: impl Fn(T1, T2, T3, T4, T5, T6, T7, T8) -> R + Send + Sync + 'static) -> Self {
        Function8(Arc::new(f))
    }
    pub fn apply(&self, t1: T1, t2: T2, t3: T3, t4: T4, t5: T5, t6: T6, t7: T7, t8: T8) -> R {
        (self.0)(t1, t2, t3, t4, t5, t6, t7, t8)
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
    R,
    F: Fn(T1, T2, T3, T4, T5, T6, T7, T8) -> R + Send + Sync + 'static,
> From<F> for Function8<T1, T2, T3, T4, T5, T6, T7, T8, R>
{
    fn from(f: F) -> Self {
        Function8(Arc::new(f))
    }
}
