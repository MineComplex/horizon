//! Port of `java.util.concurrent.Callable`.
use crate::java::lang::Throwable;

/// `V call() throws Exception`. Implemented for closures `Fn() -> Result<V, Throwable>`.
pub trait Callable<V = crate::java::JObjectRef>: Send + Sync {
    fn call(&self) -> Result<V, Throwable>;
}
impl<V, F: Fn() -> Result<V, Throwable> + Send + Sync + 'static> Callable<V> for F {
    fn call(&self) -> Result<V, Throwable> {
        self()
    }
}
