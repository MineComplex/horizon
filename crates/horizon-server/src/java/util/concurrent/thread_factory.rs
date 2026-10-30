//! Port of `java.util.concurrent.ThreadFactory`.
use crate::java::lang::{Runnable, Thread};
use crate::prelude::*;

/// `Thread newThread(Runnable r)`. Implemented for closures `Fn(JRef<dyn Runnable>) -> Thread`.
pub trait ThreadFactory: Send + Sync {
    fn new_thread(&self, r: JRef<dyn Runnable>) -> Thread;
}
impl<F: Fn(JRef<dyn Runnable>) -> Thread + Send + Sync + 'static> ThreadFactory for F {
    fn new_thread(&self, r: JRef<dyn Runnable>) -> Thread {
        self(r)
    }
}
