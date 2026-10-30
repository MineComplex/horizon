//! Port of `java.util.concurrent.ScheduledFuture` (and `Delayed`): the handle a scheduled task returns
//! (Java's `ScheduledThreadPoolExecutor.ScheduledFutureTask`). Clones share the task. `ScheduledFuture<?>`
//! of a scheduled `Runnable` is `ScheduledFuture<()>`.
use crate::java::lang::{Runnable, Throwable};
use crate::java::util::concurrent::support;
use crate::java::util::concurrent::{Future, FutureState, FutureTask, ScheduledThreadPoolExecutor, TimeUnit};
use crate::prelude::*;

/// `Delayed`.
pub trait Delayed: Send + Sync {
    fn get_delay(&self, unit: TimeUnit) -> i64;
}

pub struct ScheduledFutureInner<V: JGeneric> {
    pub(crate) task: FutureTask<V>,
    /// For periodic tasks: the action, run again each period.
    pub(crate) periodic_action: Option<JRef<dyn Fn() -> Result<(), Throwable> + Send + Sync>>,
    /// `System.nanoTime()` at which the task is due.
    pub(crate) time: AtomicI64,
    /// 0: one-shot; > 0: fixed rate; < 0: fixed delay (Java's encoding).
    pub(crate) period: i64,
    pub(crate) sequence: i64,
    pub(crate) executor: Mutex<Option<ScheduledThreadPoolExecutor>>,
}

use parking_lot::Mutex;

pub struct ScheduledFuture<V: JGeneric = JObjectRef>(pub JRef<ScheduledFutureInner<V>>);
impl<V: JGeneric> Clone for ScheduledFuture<V> {
    fn clone(&self) -> Self {
        ScheduledFuture(self.0.clone())
    }
}

impl<V: JGeneric> ScheduledFuture<V> {
    pub fn get_delay(&self, unit: TimeUnit) -> i64 {
        unit.convert(self.0.time.load(AtomicOrdering::SeqCst) - support::nano_time(), TimeUnit::NANOSECONDS)
    }
    pub fn is_periodic(&self) -> bool {
        self.0.period != 0
    }
    pub fn cancel(&self, may_interrupt_if_running: bool) -> bool {
        let cancelled = self.0.task.cancel(may_interrupt_if_running);
        if cancelled {
            if let Some(e) = self.0.executor.lock().as_ref() {
                e.remove_scheduled(self.0.sequence);
            }
        }
        cancelled
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.task.is_cancelled()
    }
    pub fn is_done(&self) -> bool {
        self.0.task.is_done()
    }
    pub fn get(&self) -> Result<V, Throwable> {
        self.0.task.get()
    }
    pub fn get_2(&self, timeout: i64, unit: TimeUnit) -> Result<V, Throwable> {
        self.0.task.get_2(timeout, unit)
    }
    pub fn state(&self) -> FutureState {
        self.0.task.state()
    }
    /// `compareTo(Delayed)`.
    pub fn compare_to(&self, other: &ScheduledFuture<V>) -> i32 {
        let d = self.0.time.load(AtomicOrdering::SeqCst) - other.0.time.load(AtomicOrdering::SeqCst);
        if d < 0 {
            -1
        } else if d > 0 {
            1
        } else if self.0.sequence < other.0.sequence {
            -1
        } else {
            1
        }
    }

    /// `ScheduledFutureTask.run()`.
    pub fn run(&self) {
        let executor = self.0.executor.lock().clone();
        match &self.0.periodic_action {
            None => self.0.task.run(),
            Some(action) => {
                let action = action.clone();
                if self.0.task.run_and_reset(&move || action()) {
                    // setNextRunTime
                    let p = self.0.period;
                    if p > 0 {
                        self.0.time.fetch_add(p, AtomicOrdering::SeqCst);
                    } else {
                        self.0.time.store(support::nano_time().saturating_add(-p), AtomicOrdering::SeqCst);
                    }
                    if let Some(e) = executor {
                        e.reexecute_periodic(self);
                    }
                }
            }
        }
    }
}

impl<V: JGeneric> Runnable for ScheduledFuture<V> {
    fn run(&self) {
        ScheduledFuture::run(self)
    }
}
impl<V: JGeneric> JObject for ScheduledFuture<V> {
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
        None
    }
}
impl<V: JGeneric> Delayed for ScheduledFuture<V> {
    fn get_delay(&self, unit: TimeUnit) -> i64 {
        ScheduledFuture::get_delay(self, unit)
    }
}
impl<V: JGeneric> Future<V> for ScheduledFuture<V> {
    fn cancel(&self, may_interrupt_if_running: bool) -> bool {
        ScheduledFuture::cancel(self, may_interrupt_if_running)
    }
    fn is_cancelled(&self) -> bool {
        ScheduledFuture::is_cancelled(self)
    }
    fn is_done(&self) -> bool {
        ScheduledFuture::is_done(self)
    }
    fn get(&self) -> Result<V, Throwable> {
        ScheduledFuture::get(self)
    }
    fn get_2(&self, timeout: i64, unit: TimeUnit) -> Result<V, Throwable> {
        ScheduledFuture::get_2(self, timeout, unit)
    }
    fn state(&self) -> FutureState {
        ScheduledFuture::state(self)
    }
}
