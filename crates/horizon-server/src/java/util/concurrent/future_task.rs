//! Port of `java.util.concurrent.FutureTask`: a cancellable computation that is both a `Runnable` and a
//! `Future`. A value handle; clones share the task.
use crate::java::lang::{Runnable, Throwable};
use crate::java::util::concurrent::support::catch;
use crate::java::util::concurrent::{Callable, CompletableFuture, Future, FutureState, TimeUnit};
use crate::prelude::*;
use parking_lot::Mutex;

type Task<V> = Box<dyn FnOnce() -> Result<V, Throwable> + Send>;

pub struct FutureTaskInner<V: JGeneric> {
    task: Mutex<Option<Task<V>>>,
    result: CompletableFuture<V>,
}

pub struct FutureTask<V: JGeneric = JObjectRef>(pub JRef<FutureTaskInner<V>>);
impl<V: JGeneric> Clone for FutureTask<V> {
    fn clone(&self) -> Self {
        FutureTask(self.0.clone())
    }
}

impl<V: JGeneric> FutureTask<V> {
    /// `new FutureTask<>(callable)`.
    pub fn new(callable: JRef<dyn Callable<V>>) -> Self {
        Self::of(move || callable.call())
    }
    /// `new FutureTask<>(runnable, result)`.
    pub fn new_2(runnable: JRef<dyn Runnable>, result: V) -> Self {
        Self::of(move || {
            runnable.run();
            Ok(result)
        })
    }
    /// A task from a closure (Java: `new FutureTask<>(() -> ...)`).
    pub fn of(f: impl FnOnce() -> Result<V, Throwable> + Send + 'static) -> Self {
        FutureTask(Arc::new(FutureTaskInner { task: Mutex::new(Some(Box::new(f))), result: CompletableFuture::new() }))
    }
    /// `run()`: computes the result unless already run or cancelled.
    pub fn run(&self) {
        let task = self.0.task.lock().take();
        if let Some(task) = task {
            match catch(task).and_then(|r| r) {
                Ok(v) => {
                    self.0.result.complete(v);
                }
                Err(ex) => {
                    self.0.result.complete_exceptionally(ex);
                }
            }
        }
    }
    /// `runAndReset()` (periodic tasks): runs without setting a result; false if it threw or was cancelled.
    pub(crate) fn run_and_reset(&self, f: &dyn Fn() -> Result<(), Throwable>) -> bool {
        if self.0.result.is_done() {
            return false;
        }
        match catch(f).and_then(|r| r) {
            Ok(()) => !self.0.result.is_done(),
            Err(ex) => {
                self.0.result.complete_exceptionally(ex);
                false
            }
        }
    }
    pub fn cancel(&self, may_interrupt_if_running: bool) -> bool {
        let had_task = self.0.task.lock().take().is_some();
        let _ = may_interrupt_if_running;
        if had_task { self.0.result.cancel(false) } else { false }
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.result.is_cancelled()
    }
    pub fn is_done(&self) -> bool {
        self.0.result.is_done()
    }
    /// `get()`: `Err(ExecutionException)` if the task threw, `Err(CancellationException)` if cancelled.
    pub fn get(&self) -> Result<V, Throwable> {
        self.0.result.get()
    }
    pub fn get_2(&self, timeout: i64, unit: TimeUnit) -> Result<V, Throwable> {
        self.0.result.get_2(timeout, unit)
    }
    pub fn state(&self) -> FutureState {
        self.0.result.state()
    }
    /// `set(v)` (protected in Java).
    pub fn set(&self, v: V) {
        self.0.task.lock().take();
        self.0.result.complete(v);
    }
    /// `setException(t)` (protected in Java).
    pub fn set_exception(&self, t: Throwable) {
        self.0.task.lock().take();
        self.0.result.complete_exceptionally(t);
    }
    /// The task as a `Runnable`.
    pub fn as_runnable(&self) -> JRef<dyn Runnable> {
        Arc::new(self.clone())
    }
}

impl<V: JGeneric> Runnable for FutureTask<V> {
    fn run(&self) {
        FutureTask::run(self)
    }
}
impl<V: JGeneric> JObject for FutureTask<V> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn cast_arc(self: Arc<Self>, target: std::any::TypeId) -> Option<Box<dyn std::any::Any>> {
        if target == std::any::TypeId::of::<Arc<Self>>() {
            return Some(Box::new(self));
        }
        if target == std::any::TypeId::of::<Arc<dyn Runnable>>() {
            return Some(Box::new(self as Arc<dyn Runnable>));
        }
        if target == std::any::TypeId::of::<Arc<dyn Future<V>>>() {
            return Some(Box::new(self as Arc<dyn Future<V>>));
        }
        None
    }
}

impl<V: JGeneric> Future<V> for FutureTask<V> {
    fn cancel(&self, may_interrupt_if_running: bool) -> bool {
        FutureTask::cancel(self, may_interrupt_if_running)
    }
    fn is_cancelled(&self) -> bool {
        FutureTask::is_cancelled(self)
    }
    fn is_done(&self) -> bool {
        FutureTask::is_done(self)
    }
    fn get(&self) -> Result<V, Throwable> {
        FutureTask::get(self)
    }
    fn get_2(&self, timeout: i64, unit: TimeUnit) -> Result<V, Throwable> {
        FutureTask::get_2(self, timeout, unit)
    }
    fn state(&self) -> FutureState {
        FutureTask::state(self)
    }
}
