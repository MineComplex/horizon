//! Port of `com.mojang.datafixers.util.Function6` (DFU): a cheap-to-clone handle on a closure
//! (`Function6::new(|t1, t2, t3, t4, t5, t6| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function6
pub struct Function6<T1, T2, T3, T4, T5, T6, R>(
    pub Arc<dyn Fn(T1, T2, T3, T4, T5, T6) -> R + Send + Sync>,
);

impl<T1, T2, T3, T4, T5, T6, R> Clone for Function6<T1, T2, T3, T4, T5, T6, R> {
    fn clone(&self) -> Self {
        Function6(self.0.clone())
    }
}

impl<T1, T2, T3, T4, T5, T6, R> Function6<T1, T2, T3, T4, T5, T6, R> {
    pub fn new(f: impl Fn(T1, T2, T3, T4, T5, T6) -> R + Send + Sync + 'static) -> Self {
        Function6(Arc::new(f))
    }
    pub fn apply(&self, t1: T1, t2: T2, t3: T3, t4: T4, t5: T5, t6: T6) -> R {
        (self.0)(t1, t2, t3, t4, t5, t6)
    }
}

impl<T1, T2, T3, T4, T5, T6, R, F: Fn(T1, T2, T3, T4, T5, T6) -> R + Send + Sync + 'static> From<F>
    for Function6<T1, T2, T3, T4, T5, T6, R>
{
    fn from(f: F) -> Self {
        Function6(Arc::new(f))
    }
}
