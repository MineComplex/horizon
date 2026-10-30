//! Port of `java.util.concurrent.RejectedExecutionHandler`.
use crate::java::lang::Runnable;
use crate::java::util::concurrent::ThreadPoolExecutor;
use crate::prelude::*;

/// `void rejectedExecution(Runnable r, ThreadPoolExecutor executor)`; a `RejectedExecutionException`
/// is thrown by panicking with it (`support::throw`). Implemented for closures.
pub trait RejectedExecutionHandler: Send + Sync {
    fn rejected_execution(&self, r: JRef<dyn Runnable>, executor: ThreadPoolExecutor);
}
impl<F: Fn(JRef<dyn Runnable>, ThreadPoolExecutor) + Send + Sync + 'static> RejectedExecutionHandler
    for F
{
    fn rejected_execution(&self, r: JRef<dyn Runnable>, executor: ThreadPoolExecutor) {
        self(r, executor)
    }
}
