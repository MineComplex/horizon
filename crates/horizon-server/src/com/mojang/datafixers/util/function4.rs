//! Port of `com.mojang.datafixers.util.Function4` (DFU): a cheap-to-clone handle on a closure
//! (`Function4::new(|t1, t2, t3, t4| ..)`, or `.into()` from a closure).
use crate::prelude::*;

// ---- interface com.mojang.datafixers.util.Function4
pub struct Function4<T1, T2, T3, T4, R>(pub Arc<dyn Fn(T1, T2, T3, T4) -> R + Send + Sync>);

impl<T1, T2, T3, T4, R> Clone for Function4<T1, T2, T3, T4, R> {
    fn clone(&self) -> Self {
        Function4(self.0.clone())
    }
}

impl<T1, T2, T3, T4, R> Function4<T1, T2, T3, T4, R> {
    pub fn new(f: impl Fn(T1, T2, T3, T4) -> R + Send + Sync + 'static) -> Self {
        Function4(Arc::new(f))
    }
    pub fn apply(&self, t1: T1, t2: T2, t3: T3, t4: T4) -> R {
        (self.0)(t1, t2, t3, t4)
    }
}

impl<T1, T2, T3, T4, R, F: Fn(T1, T2, T3, T4) -> R + Send + Sync + 'static> From<F>
    for Function4<T1, T2, T3, T4, R>
{
    fn from(f: F) -> Self {
        Function4(Arc::new(f))
    }
}
