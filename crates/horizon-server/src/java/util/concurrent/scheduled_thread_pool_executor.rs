//! Port of `java.util.concurrent.ScheduledThreadPoolExecutor` (with its `DelayedWorkQueue`).
//!
//! A `ThreadPoolExecutor` (its `base`, reached through `Deref`) of `corePoolSize` workers taking from a
//! delay queue. As in Java: `execute`/`submit` schedule with zero delay; on `shutdown` periodic tasks
//! stop and delayed one-shot tasks still run.
use crate::java::lang::{Runnable, Throwable};
use crate::java::util::concurrent::linked_blocking_queue::deadline;
use crate::java::util::concurrent::scheduled_future::ScheduledFutureInner;
use crate::java::util::concurrent::support;
use crate::java::util::concurrent::{
    BlockingQueue, BlockingQueueTrait, Callable, Executor, ExecutorService, ExecutorServiceTrait, Executors, FutureTask,
    RejectedExecutionHandler, ScheduledFuture, ThreadFactory, ThreadPoolExecutor, TimeUnit,
};
use crate::java::util::concurrent::thread_pool_executor::DEFAULT_HANDLER;
use crate::prelude::*;
use parking_lot::{Condvar, Mutex};

const DEFAULT_KEEPALIVE_MILLIS: i64 = 10;

struct Entry {
    time: i64,
    seq: i64,
    periodic: bool,
    task: JRef<dyn Runnable>,
}

struct DwqState {
    /// Sorted by (time, seq): the next task first.
    entries: Vec<Entry>,
}

pub struct DelayedWorkQueueInner {
    state: Mutex<DwqState>,
    available: Condvar,
}

impl DelayedWorkQueueInner {
    fn insert(&self, e: Entry) {
        let mut s = self.state.lock();
        let idx = s.entries.partition_point(|x| (x.time, x.seq) <= (e.time, e.seq));
        s.entries.insert(idx, e);
        drop(s);
        self.available.notify_all();
    }
    fn take_ready(&self, deadline: Option<i64>) -> Option<JRef<dyn Runnable>> {
        let mut s = self.state.lock();
        loop {
            let now = support::nano_time();
            let next = s.entries.first().map(|e| e.time);
            match next {
                Some(t) if t <= now => return Some(s.entries.remove(0).task),
                _ => {}
            }
            let wake = match (next, deadline) {
                (Some(t), Some(d)) => t.min(d),
                (Some(t), None) => t,
                (None, Some(d)) => d,
                (None, None) => {
                    self.available.wait(&mut s);
                    continue;
                }
            };
            if !support::wait_until(&self.available, &mut s, wake) {
                if let Some(d) = deadline {
                    if support::nano_time() >= d {
                        let now = support::nano_time();
                        if s.entries.first().is_some_and(|e| e.time <= now) {
                            return Some(s.entries.remove(0).task);
                        }
                        return None;
                    }
                }
            }
        }
    }
}

/// `ScheduledThreadPoolExecutor.DelayedWorkQueue` as a `BlockingQueue` (plain offers are due now).
pub struct DelayedWorkQueue(pub JRef<DelayedWorkQueueInner>, AtomicI64);
impl BlockingQueueTrait<JRef<dyn Runnable>> for DelayedWorkQueue {
    fn offer(&self, e: JRef<dyn Runnable>) -> bool {
        let seq = self.1.fetch_add(1, AtomicOrdering::SeqCst);
        self.0.insert(Entry { time: support::nano_time(), seq: i64::MIN / 2 + seq, periodic: false, task: e });
        true
    }
    fn offer_3(&self, e: JRef<dyn Runnable>, _timeout: i64, _unit: TimeUnit) -> Result<bool, Throwable> {
        Ok(self.offer(e))
    }
    fn put(&self, e: JRef<dyn Runnable>) -> Result<(), Throwable> {
        self.offer(e);
        Ok(())
    }
    fn poll(&self) -> Option<JRef<dyn Runnable>> {
        let mut s = self.0.state.lock();
        let now = support::nano_time();
        if s.entries.first().is_some_and(|e| e.time <= now) { Some(s.entries.remove(0).task) } else { None }
    }
    fn poll_2(&self, timeout: i64, unit: TimeUnit) -> Result<Option<JRef<dyn Runnable>>, Throwable> {
        Ok(self.0.take_ready(Some(deadline(timeout, unit))))
    }
    fn take(&self) -> Result<JRef<dyn Runnable>, Throwable> {
        Ok(self.0.take_ready(None).expect("take"))
    }
    fn peek(&self) -> Option<JRef<dyn Runnable>> {
        self.0.state.lock().entries.first().map(|e| e.task.clone())
    }
    fn size(&self) -> i32 {
        self.0.state.lock().entries.len() as i32
    }
    fn remaining_capacity(&self) -> i32 {
        i32::MAX
    }
    fn clear(&self) {
        self.0.state.lock().entries.clear();
    }
    fn remove_if(&self, filter: &mut dyn FnMut(&JRef<dyn Runnable>) -> bool) -> bool {
        let mut s = self.0.state.lock();
        let before = s.entries.len();
        s.entries.retain(|e| !filter(&e.task));
        s.entries.len() != before
    }
    fn to_vec(&self) -> Vec<JRef<dyn Runnable>> {
        self.0.state.lock().entries.iter().map(|e| e.task.clone()).collect()
    }
}

pub struct ScheduledThreadPoolExecutor {
    pub base: ThreadPoolExecutor,
    queue: JRef<DelayedWorkQueueInner>,
    sequencer: JRef<AtomicI64>,
}
impl Clone for ScheduledThreadPoolExecutor {
    fn clone(&self) -> Self {
        ScheduledThreadPoolExecutor { base: self.base.clone(), queue: self.queue.clone(), sequencer: self.sequencer.clone() }
    }
}
impl std::ops::Deref for ScheduledThreadPoolExecutor {
    type Target = ThreadPoolExecutor;
    fn deref(&self) -> &ThreadPoolExecutor {
        &self.base
    }
}

impl ScheduledThreadPoolExecutor {
    /// `new ScheduledThreadPoolExecutor(corePoolSize)`.
    pub fn new(core_pool_size: i32) -> Self {
        Self::new_3(core_pool_size, Executors::default_thread_factory(), DEFAULT_HANDLER.clone())
    }
    /// `new ScheduledThreadPoolExecutor(corePoolSize, threadFactory)`.
    pub fn new_2(core_pool_size: i32, thread_factory: JRef<dyn ThreadFactory>) -> Self {
        Self::new_3(core_pool_size, thread_factory, DEFAULT_HANDLER.clone())
    }
    /// `new ScheduledThreadPoolExecutor(corePoolSize, handler)`.
    pub fn new_2_2(core_pool_size: i32, handler: JRef<dyn RejectedExecutionHandler>) -> Self {
        Self::new_3(core_pool_size, Executors::default_thread_factory(), handler)
    }
    /// `new ScheduledThreadPoolExecutor(corePoolSize, threadFactory, handler)`.
    pub fn new_3(core_pool_size: i32, thread_factory: JRef<dyn ThreadFactory>, handler: JRef<dyn RejectedExecutionHandler>) -> Self {
        let queue = Arc::new(DelayedWorkQueueInner { state: Mutex::new(DwqState { entries: Vec::new() }), available: Condvar::new() });
        let bq: BlockingQueue<JRef<dyn Runnable>> = BlockingQueue(Arc::new(DelayedWorkQueue(queue.clone(), AtomicI64::new(0))));
        let base = ThreadPoolExecutor::new_7(
            core_pool_size,
            i32::MAX,
            DEFAULT_KEEPALIVE_MILLIS,
            TimeUnit::MILLISECONDS,
            bq,
            thread_factory,
            handler,
        );
        ScheduledThreadPoolExecutor { base, queue, sequencer: Arc::new(AtomicI64::new(0)) }
    }

    pub fn as_executor_service(&self) -> ExecutorService {
        ExecutorService(Arc::new(self.clone()))
    }

    fn trigger_time(delay: i64, unit: TimeUnit) -> i64 {
        support::nano_time().saturating_add(unit.to_nanos(delay.max(0)))
    }

    /// `delayedExecute(task)`.
    fn delayed_execute<V: JGeneric>(&self, task: ScheduledFuture<V>) -> ScheduledFuture<V> {
        if self.base.is_shutdown() {
            let r: JRef<dyn Runnable> = Arc::new(task.clone());
            let handler = self.base.get_rejected_execution_handler();
            handler.rejected_execution(r, self.base.clone());
            return task;
        }
        *task.0.executor.lock() = Some(self.clone());
        self.queue.insert(Entry {
            time: task.0.time.load(AtomicOrdering::SeqCst),
            seq: task.0.sequence,
            periodic: task.0.period != 0,
            task: Arc::new(task.clone()),
        });
        self.base.ensure_prestart();
        task
    }

    /// `reExecutePeriodic(task)`.
    pub(crate) fn reexecute_periodic<V: JGeneric>(&self, task: &ScheduledFuture<V>) {
        if self.base.is_shutdown() {
            return;
        }
        self.queue.insert(Entry {
            time: task.0.time.load(AtomicOrdering::SeqCst),
            seq: task.0.sequence,
            periodic: true,
            task: Arc::new(task.clone()),
        });
        self.base.ensure_prestart();
    }

    /// Removes a cancelled task from the queue (`setRemoveOnCancelPolicy(true)` behaviour).
    pub(crate) fn remove_scheduled(&self, sequence: i64) {
        self.queue.state.lock().entries.retain(|e| e.seq != sequence);
    }

    fn make<V: JGeneric>(
        &self,
        task: FutureTask<V>,
        periodic_action: Option<JRef<dyn Fn() -> Result<(), Throwable> + Send + Sync>>,
        time: i64,
        period: i64,
    ) -> ScheduledFuture<V> {
        ScheduledFuture(Arc::new(ScheduledFutureInner {
            task,
            periodic_action,
            time: AtomicI64::new(time),
            period,
            sequence: self.sequencer.fetch_add(1, AtomicOrdering::SeqCst),
            executor: Mutex::new(None),
        }))
    }

    /// `schedule(Runnable, delay, unit)`.
    pub fn schedule(&self, command: JRef<dyn Runnable>, delay: i64, unit: TimeUnit) -> ScheduledFuture<()> {
        let t = self.make(FutureTask::new_2(command, ()), None, Self::trigger_time(delay, unit), 0);
        self.delayed_execute(t)
    }
    /// `schedule(Callable, delay, unit)`.
    pub fn schedule_3<V: JGeneric>(&self, callable: JRef<dyn Callable<V>>, delay: i64, unit: TimeUnit) -> ScheduledFuture<V> {
        let t = self.make(FutureTask::new(callable), None, Self::trigger_time(delay, unit), 0);
        self.delayed_execute(t)
    }
    /// `scheduleAtFixedRate(command, initialDelay, period, unit)`.
    pub fn schedule_at_fixed_rate(&self, command: JRef<dyn Runnable>, initial_delay: i64, period: i64, unit: TimeUnit) -> ScheduledFuture<()> {
        if period <= 0 {
            support::illegal_argument("period <= 0");
        }
        let action: JRef<dyn Fn() -> Result<(), Throwable> + Send + Sync> = Arc::new(move || {
            command.run();
            Ok(())
        });
        let t = self.make(FutureTask::of(|| Ok(())), Some(action), Self::trigger_time(initial_delay, unit), unit.to_nanos(period));
        self.delayed_execute(t)
    }
    /// `scheduleWithFixedDelay(command, initialDelay, delay, unit)`.
    pub fn schedule_with_fixed_delay(&self, command: JRef<dyn Runnable>, initial_delay: i64, delay: i64, unit: TimeUnit) -> ScheduledFuture<()> {
        if delay <= 0 {
            support::illegal_argument("delay <= 0");
        }
        let action: JRef<dyn Fn() -> Result<(), Throwable> + Send + Sync> = Arc::new(move || {
            command.run();
            Ok(())
        });
        let t = self.make(FutureTask::of(|| Ok(())), Some(action), Self::trigger_time(initial_delay, unit), -unit.to_nanos(delay));
        self.delayed_execute(t)
    }
    /// `execute(command)`: `schedule(command, 0, NANOSECONDS)`.
    pub fn execute(&self, command: JRef<dyn Runnable>) {
        self.schedule(command, 0, TimeUnit::NANOSECONDS);
    }
    /// `shutdown()`: periodic tasks are dropped, delayed one-shot tasks still run.
    pub fn shutdown(&self) {
        self.queue.state.lock().entries.retain(|e| !e.periodic);
        self.base.shutdown();
    }
    pub fn shutdown_now(&self) -> Vec<JRef<dyn Runnable>> {
        self.base.shutdown_now()
    }
    /// `setRemoveOnCancelPolicy` (cancelled tasks are always removed here).
    pub fn set_remove_on_cancel_policy(&self, _value: bool) {}
    pub fn set_continue_existing_periodic_tasks_after_shutdown_policy(&self, _value: bool) {}
    pub fn set_execute_existing_delayed_tasks_after_shutdown_policy(&self, _value: bool) {}
}

impl Executor for ScheduledThreadPoolExecutor {
    fn execute(&self, command: JRef<dyn Runnable>) {
        ScheduledThreadPoolExecutor::execute(self, command)
    }
}
impl ExecutorServiceTrait for ScheduledThreadPoolExecutor {
    fn shutdown(&self) {
        ScheduledThreadPoolExecutor::shutdown(self)
    }
    fn shutdown_now(&self) -> Vec<JRef<dyn Runnable>> {
        ScheduledThreadPoolExecutor::shutdown_now(self)
    }
    fn is_shutdown(&self) -> bool {
        self.base.is_shutdown()
    }
    fn is_terminated(&self) -> bool {
        self.base.is_terminated()
    }
    fn await_termination(&self, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable> {
        self.base.await_termination(timeout, unit)
    }
}
impl From<ScheduledThreadPoolExecutor> for ExecutorService {
    fn from(e: ScheduledThreadPoolExecutor) -> Self {
        ExecutorService(Arc::new(e))
    }
}
