//! Port of `java.util.concurrent.ScheduledExecutorService`: the value handle `Executors` returns for
//! scheduled pools. Its one implementation is `ScheduledThreadPoolExecutor` (reached through `Deref`, so
//! `service.schedule(..)`, `service.shutdown()`... work directly).
use crate::java::lang::{Runnable, Throwable};
use crate::java::util::concurrent::{Executor, ExecutorService, ExecutorServiceTrait, ScheduledThreadPoolExecutor, TimeUnit};
use crate::prelude::*;

pub struct ScheduledExecutorService(pub ScheduledThreadPoolExecutor);
impl Clone for ScheduledExecutorService {
    fn clone(&self) -> Self {
        ScheduledExecutorService(self.0.clone())
    }
}
impl std::ops::Deref for ScheduledExecutorService {
    type Target = ScheduledThreadPoolExecutor;
    fn deref(&self) -> &ScheduledThreadPoolExecutor {
        &self.0
    }
}
impl ScheduledExecutorService {
    pub fn as_executor_service(&self) -> ExecutorService {
        self.0.as_executor_service()
    }
}
impl From<ScheduledThreadPoolExecutor> for ScheduledExecutorService {
    fn from(e: ScheduledThreadPoolExecutor) -> Self {
        ScheduledExecutorService(e)
    }
}
impl From<ScheduledExecutorService> for ExecutorService {
    fn from(e: ScheduledExecutorService) -> Self {
        e.0.as_executor_service()
    }
}
impl Executor for ScheduledExecutorService {
    fn execute(&self, command: JRef<dyn Runnable>) {
        self.0.execute(command)
    }
}
impl ExecutorServiceTrait for ScheduledExecutorService {
    fn shutdown(&self) {
        self.0.shutdown()
    }
    fn shutdown_now(&self) -> Vec<JRef<dyn Runnable>> {
        self.0.shutdown_now()
    }
    fn is_shutdown(&self) -> bool {
        self.0.is_shutdown()
    }
    fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
    fn await_termination(&self, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable> {
        self.0.await_termination(timeout, unit)
    }
}
