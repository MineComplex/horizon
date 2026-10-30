//! Port of `java.util.concurrent.Executor`.
use crate::java::lang::Runnable;
use crate::prelude::*;

/// `void execute(Runnable command)`. Implemented for closures `Fn(JRef<dyn Runnable>)`
/// (`Runnable::run` is an executor), for every executor handle of this package, and by in-tree classes.
pub trait Executor: Send + Sync {
    fn execute(&self, command: JRef<dyn Runnable>);
}
impl<F: Fn(JRef<dyn Runnable>) + Send + Sync + 'static> Executor for F {
    fn execute(&self, command: JRef<dyn Runnable>) {
        self(command)
    }
}

/// Runs a closure on `executor` (Java: `executor.execute(() -> ...)`).
pub fn execute_fn(executor: &dyn Executor, f: impl FnOnce() + Send + 'static) {
    executor.execute(crate::java::util::concurrent::support::runnable(f));
}
