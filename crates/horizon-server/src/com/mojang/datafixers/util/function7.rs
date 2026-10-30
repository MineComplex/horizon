//! Port of `com.mojang.datafixers.util.Function7` (DFU): a cheap-to-clone handle on a closure
//! (`Function7::new(|t1, t2, t3, t4, t5, t6, t7| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function7
pub struct Function7<T1, T2, T3, T4, T5, T6, T7, R>(
    pub Arc<dyn Fn(T1, T2, T3, T4, T5, T6, T7) -> R + Send + Sync>,
);

impl<T1, T2, T3, T4, T5, T6, T7, R> Clone for Function7<T1, T2, T3, T4, T5, T6, T7, R> {
    fn clone(&self) -> Self {
        Function7(self.0.clone())
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, R> Function7<T1, T2, T3, T4, T5, T6, T7, R> {
    pub fn new(f: impl Fn(T1, T2, T3, T4, T5, T6, T7) -> R + Send + Sync + 'static) -> Self {
        Function7(Arc::new(f))
    }
    pub fn apply(&self, t1: T1, t2: T2, t3: T3, t4: T4, t5: T5, t6: T6, t7: T7) -> R {
        (self.0)(t1, t2, t3, t4, t5, t6, t7)
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, R, F: Fn(T1, T2, T3, T4, T5, T6, T7) -> R + Send + Sync + 'static>
    From<F> for Function7<T1, T2, T3, T4, T5, T6, T7, R>
{
    fn from(f: F) -> Self {
        Function7(Arc::new(f))
    }
}
