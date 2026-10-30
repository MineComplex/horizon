//! Port of `com.mojang.datafixers.util.Function3` (DFU): a cheap-to-clone handle on a closure
//! (`Function3::new(|t1, t2, t3| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function3
pub struct Function3<T1, T2, T3, R>(pub Arc<dyn Fn(T1, T2, T3) -> R + Send + Sync>);

impl<T1, T2, T3, R> Clone for Function3<T1, T2, T3, R> {
    fn clone(&self) -> Self {
        Function3(self.0.clone())
    }
}

impl<T1, T2, T3, R> Function3<T1, T2, T3, R> {
    pub fn new(f: impl Fn(T1, T2, T3) -> R + Send + Sync + 'static) -> Self {
        Function3(Arc::new(f))
    }
    pub fn apply(&self, t1: T1, t2: T2, t3: T3) -> R {
        (self.0)(t1, t2, t3)
    }
}

impl<T1, T2, T3, R, F: Fn(T1, T2, T3) -> R + Send + Sync + 'static> From<F>
    for Function3<T1, T2, T3, R>
{
    fn from(f: F) -> Self {
        Function3(Arc::new(f))
    }
}
