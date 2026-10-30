//! Port of `java.util.concurrent.ExecutorService` (with `AbstractExecutorService`'s `submit`/`invokeAll`).
//!
//! `ExecutorServiceTrait` is the interface the pools implement; `ExecutorService` is the value handle the
//! skeleton passes around (`Executors.newFixedThreadPool(..)` returns one; a `ThreadPoolExecutor` converts
//! with `.into()` / `as_executor_service()`). The handle is itself an `Executor`:
//! `let e: JRef<dyn Executor> = Arc::new(service.clone());`.
use crate::java::lang::{Runnable, Throwable};
use crate::java::util::concurrent::{Callable, Executor, Future, FutureTask, TimeUnit};
use crate::prelude::*;

pub trait ExecutorServiceTrait: Executor {
    fn shutdown(&self);
    /// `shutdownNow()`: the tasks that never ran.
    fn shutdown_now(&self) -> Vec<JRef<dyn Runnable>>;
    fn is_shutdown(&self) -> bool;
    fn is_terminated(&self) -> bool;
    /// `awaitTermination(timeout, unit)` (`throws InterruptedException`).
    fn await_termination(&self, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable>;
    /// `close()`: shutdown, then wait for termination.
    fn close(&self) {
        self.shutdown();
        while !self.is_terminated() {
            let _ = self.await_termination(1, TimeUnit::DAYS);
        }
    }
}

pub struct ExecutorService(pub JRef<dyn ExecutorServiceTrait>);
impl Clone for ExecutorService {
    fn clone(&self) -> Self {
        ExecutorService(self.0.clone())
    }
}
impl PartialEq for ExecutorService {
    fn eq(&self, other: &Self) -> bool {
        same(&self.0, &other.0)
    }
}

impl ExecutorService {
    /// Wraps an implementation (Java: the upcast to `ExecutorService`).
    pub fn of(service: impl ExecutorServiceTrait + 'static) -> Self {
        ExecutorService(Arc::new(service))
    }
    pub fn execute(&self, command: JRef<dyn Runnable>) {
        self.0.execute(command)
    }
    pub fn shutdown(&self) {
        self.0.shutdown()
    }
    pub fn shutdown_now(&self) -> Vec<JRef<dyn Runnable>> {
        self.0.shutdown_now()
    }
    pub fn is_shutdown(&self) -> bool {
        self.0.is_shutdown()
    }
    pub fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
    pub fn await_termination(&self, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable> {
        self.0.await_termination(timeout, unit)
    }
    pub fn close(&self) {
        self.0.close()
    }
    /// `submit(Callable)`.
    pub fn submit<T: JGeneric>(&self, task: JRef<dyn Callable<T>>) -> JRef<dyn Future<T>> {
        submit_on(&*self.0, FutureTask::new(task))
    }
    /// `submit(Runnable)`.
    pub fn submit_1(&self, task: JRef<dyn Runnable>) -> JRef<dyn Future<()>> {
        submit_on(&*self.0, FutureTask::new_2(task, ()))
    }
    /// `submit(Runnable, T result)`.
    pub fn submit_2<T: JGeneric>(&self, task: JRef<dyn Runnable>, result: T) -> JRef<dyn Future<T>> {
        submit_on(&*self.0, FutureTask::new_2(task, result))
    }
    /// `submit(() -> ...)` with a closure (Java lambdas passed as `Callable`).
    pub fn submit_fn<T: JGeneric>(&self, f: impl FnOnce() -> T + Send + 'static) -> JRef<dyn Future<T>> {
        submit_on(&*self.0, FutureTask::of(move || Ok(f())))
    }
    /// `invokeAll(tasks)`.
    pub fn invoke_all<T: JGeneric>(&self, tasks: Vec<JRef<dyn Callable<T>>>) -> Result<Vec<JRef<dyn Future<T>>>, Throwable> {
        let futures: Vec<FutureTask<T>> = tasks.into_iter().map(FutureTask::new).collect();
        for f in &futures {
            self.0.execute(f.as_runnable());
        }
        for f in &futures {
            let _ = f.get();
        }
        Ok(futures.into_iter().map(|f| Arc::new(f) as JRef<dyn Future<T>>).collect())
    }
    /// `invokeAny(tasks)`: the result of the first task to finish without throwing.
    pub fn invoke_any<T: JGeneric>(&self, tasks: Vec<JRef<dyn Callable<T>>>) -> Result<T, Throwable> {
        let futures: Vec<FutureTask<T>> = tasks.into_iter().map(FutureTask::new).collect();
        for f in &futures {
            self.0.execute(f.as_runnable());
        }
        let mut last = None;
        for f in &futures {
            match f.get() {
                Ok(v) => {
                    for g in &futures {
                        g.cancel(true);
                    }
                    return Ok(v);
                }
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| crate::java::util::concurrent::support::new_exception(
            crate::java::util::concurrent::support::ILLEGAL_ARGUMENT_EXCEPTION,
            None,
            None,
        )))
    }
}

/// `AbstractExecutorService.submit`: runs the task on `executor`, returns it as the `Future`.
pub fn submit_on<T: JGeneric>(executor: &dyn Executor, task: FutureTask<T>) -> JRef<dyn Future<T>> {
    executor.execute(task.as_runnable());
    Arc::new(task)
}

impl Executor for ExecutorService {
    fn execute(&self, command: JRef<dyn Runnable>) {
        self.0.execute(command)
    }
}
impl ExecutorServiceTrait for ExecutorService {
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
