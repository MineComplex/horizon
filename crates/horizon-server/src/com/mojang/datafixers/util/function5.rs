//! Port of `com.mojang.datafixers.util.Function5` (DFU): a cheap-to-clone handle on a closure
//! (`Function5::new(|t1, t2, t3, t4, t5| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function5
pub struct Function5<T1, T2, T3, T4, T5, R>(pub Arc<dyn Fn(T1, T2, T3, T4, T5) -> R + Send + Sync>);

impl<T1, T2, T3, T4, T5, R> Clone for Function5<T1, T2, T3, T4, T5, R> {
    fn clone(&self) -> Self {
        Function5(self.0.clone())
    }
}

impl<T1, T2, T3, T4, T5, R> Function5<T1, T2, T3, T4, T5, R> {
    pub fn new(f: impl Fn(T1, T2, T3, T4, T5) -> R + Send + Sync + 'static) -> Self {
        Function5(Arc::new(f))
    }
    pub fn apply(&self, t1: T1, t2: T2, t3: T3, t4: T4, t5: T5) -> R {
        (self.0)(t1, t2, t3, t4, t5)
    }
}

impl<T1, T2, T3, T4, T5, R, F: Fn(T1, T2, T3, T4, T5) -> R + Send + Sync + 'static> From<F>
    for Function5<T1, T2, T3, T4, T5, R>
{
    fn from(f: F) -> Self {
        Function5(Arc::new(f))
    }
}
