//! Port of `java.util.concurrent.ThreadPoolExecutor` and its rejection policies
//! (`ThreadPoolExecutor.AbortPolicy`... -> `ThreadPoolExecutorAbortPolicy`...).
//!
//! The pool follows Java's rules: below `corePoolSize` every `execute` starts a worker; then tasks are
//! offered to the queue; when the queue refuses, a worker is started up to `maximumPoolSize`; else the
//! task is rejected. Workers above the core size (or all of them, with `allowCoreThreadTimeOut`) exit after
//! `keepAliveTime` idle. Worker threads come from the `ThreadFactory` and are started with
//! `Thread.start()`. `ThreadPoolExecutor` is a value handle: clones share the pool.
use crate::java::lang::{Runnable, Thread, Throwable};
use crate::java::util::concurrent::executor_service::submit_on;
use crate::java::util::concurrent::support::{self, catch};
use crate::java::util::concurrent::{
    BlockingQueue, Callable, ExecutorService, ExecutorServiceTrait, Executors, Future, FutureTask, RejectedExecutionHandler,
    ThreadFactory, TimeUnit,
};
use crate::java::util::concurrent::Executor;
use crate::prelude::*;
use parking_lot::{Condvar, Mutex};

const RUNNING: u8 = 0;
const SHUTDOWN: u8 = 1;
const STOP: u8 = 2;
const TERMINATED: u8 = 3;

/// How long an idle worker without a keep-alive deadline sleeps before rechecking the pool's state
/// (Java interrupts idle workers on shutdown instead).
const IDLE_RECHECK_NANOS: i64 = 250_000_000;

struct TpeState {
    workers: i32,
    run_state: u8,
    active: i32,
    completed: i64,
    largest: i32,
    threads: Vec<Thread>,
}

pub struct ThreadPoolExecutorInner {
    core_pool_size: AtomicI32,
    maximum_pool_size: AtomicI32,
    keep_alive_nanos: AtomicI64,
    allow_core_thread_time_out: AtomicBool,
    queue: BlockingQueue<JRef<dyn Runnable>>,
    thread_factory: Mutex<JRef<dyn ThreadFactory>>,
    handler: Mutex<JRef<dyn RejectedExecutionHandler>>,
    state: Mutex<TpeState>,
    termination: Condvar,
}

pub struct ThreadPoolExecutor(pub JRef<ThreadPoolExecutorInner>);
impl Clone for ThreadPoolExecutor {
    fn clone(&self) -> Self {
        ThreadPoolExecutor(self.0.clone())
    }
}
impl PartialEq for ThreadPoolExecutor {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// `ThreadPoolExecutor.defaultHandler`.
pub static DEFAULT_HANDLER: LazyLock<JRef<dyn RejectedExecutionHandler>> = LazyLock::new(|| Arc::new(ThreadPoolExecutorAbortPolicy));

impl ThreadPoolExecutor {
    /// `new ThreadPoolExecutor(core, max, keepAliveTime, unit, workQueue)`.
    pub fn new(core_pool_size: i32, maximum_pool_size: i32, keep_alive_time: i64, unit: TimeUnit, work_queue: BlockingQueue<JRef<dyn Runnable>>) -> Self {
        Self::new_7(core_pool_size, maximum_pool_size, keep_alive_time, unit, work_queue, Executors::default_thread_factory(), DEFAULT_HANDLER.clone())
    }
    /// `new ThreadPoolExecutor(core, max, keepAliveTime, unit, workQueue, threadFactory)`.
    pub fn new_6(
        core_pool_size: i32,
        maximum_pool_size: i32,
        keep_alive_time: i64,
        unit: TimeUnit,
        work_queue: BlockingQueue<JRef<dyn Runnable>>,
        thread_factory: JRef<dyn ThreadFactory>,
    ) -> Self {
        Self::new_7(core_pool_size, maximum_pool_size, keep_alive_time, unit, work_queue, thread_factory, DEFAULT_HANDLER.clone())
    }
    /// `new ThreadPoolExecutor(core, max, keepAliveTime, unit, workQueue, handler)`.
    pub fn new_6_2(
        core_pool_size: i32,
        maximum_pool_size: i32,
        keep_alive_time: i64,
        unit: TimeUnit,
        work_queue: BlockingQueue<JRef<dyn Runnable>>,
        handler: JRef<dyn RejectedExecutionHandler>,
    ) -> Self {
        Self::new_7(core_pool_size, maximum_pool_size, keep_alive_time, unit, work_queue, Executors::default_thread_factory(), handler)
    }
    /// `new ThreadPoolExecutor(core, max, keepAliveTime, unit, workQueue, threadFactory, handler)`.
    pub fn new_7(
        core_pool_size: i32,
        maximum_pool_size: i32,
        keep_alive_time: i64,
        unit: TimeUnit,
        work_queue: BlockingQueue<JRef<dyn Runnable>>,
        thread_factory: JRef<dyn ThreadFactory>,
        handler: JRef<dyn RejectedExecutionHandler>,
    ) -> Self {
        if core_pool_size < 0 || maximum_pool_size <= 0 || maximum_pool_size < core_pool_size || keep_alive_time < 0 {
            support::illegal_argument("ThreadPoolExecutor parameters");
        }
        ThreadPoolExecutor(Arc::new(ThreadPoolExecutorInner {
            core_pool_size: AtomicI32::new(core_pool_size),
            maximum_pool_size: AtomicI32::new(maximum_pool_size),
            keep_alive_nanos: AtomicI64::new(unit.to_nanos(keep_alive_time)),
            allow_core_thread_time_out: AtomicBool::new(false),
            queue: work_queue,
            thread_factory: Mutex::new(thread_factory),
            handler: Mutex::new(handler),
            state: Mutex::new(TpeState { workers: 0, run_state: RUNNING, active: 0, completed: 0, largest: 0, threads: Vec::new() }),
            termination: Condvar::new(),
        }))
    }

    /// The pool as an `ExecutorService`.
    pub fn as_executor_service(&self) -> ExecutorService {
        ExecutorService(Arc::new(self.clone()))
    }

    // ---- workers

    /// `addWorker(firstTask, core)`.
    fn add_worker(&self, first_task: Option<JRef<dyn Runnable>>, core: bool) -> bool {
        {
            let mut s = self.0.state.lock();
            if s.run_state >= STOP || (s.run_state == SHUTDOWN && (first_task.is_some() || self.0.queue.is_empty())) {
                return false;
            }
            let bound = if core { self.0.core_pool_size.load(AtomicOrdering::SeqCst) } else { self.0.maximum_pool_size.load(AtomicOrdering::SeqCst) };
            if s.workers >= bound {
                return false;
            }
            s.workers += 1;
            s.largest = s.largest.max(s.workers);
        }
        let pool = self.clone();
        let worker = support::runnable(move || pool.run_worker(first_task));
        let factory = self.0.thread_factory.lock().clone();
        match catch(|| factory.new_thread(worker)) {
            Ok(thread) => {
                self.0.state.lock().threads.push(thread.clone());
                support::start(&thread);
                true
            }
            Err(ex) => {
                log::error!("ThreadPoolExecutor: thread factory failed: {}", support::class_name(&ex));
                self.worker_exited();
                false
            }
        }
    }

    fn worker_exited(&self) {
        {
            let mut s = self.0.state.lock();
            s.workers -= 1;
        }
        self.try_terminate();
    }

    /// `runWorker`.
    fn run_worker(&self, first_task: Option<JRef<dyn Runnable>>) {
        let mut task = first_task;
        loop {
            let t = match task.take() {
                Some(t) => t,
                None => match self.get_task() {
                    Some(t) => t,
                    None => break,
                },
            };
            self.0.state.lock().active += 1;
            if let Err(ex) = catch(|| t.run()) {
                log::error!("Exception in thread pool task: {}", support::class_name(&ex));
            }
            let mut s = self.0.state.lock();
            s.active -= 1;
            s.completed += 1;
        }
        // get_task already took this worker off the count
        self.try_terminate();
    }

    /// `getTask`: the next task, or `None` (having decremented the worker count) when this worker must exit.
    fn get_task(&self) -> Option<JRef<dyn Runnable>> {
        let mut timed_out = false;
        loop {
            let timed;
            {
                let mut s = self.0.state.lock();
                if s.run_state >= STOP || (s.run_state >= SHUTDOWN && self.0.queue.is_empty()) {
                    s.workers -= 1;
                    return None;
                }
                let core = self.0.core_pool_size.load(AtomicOrdering::SeqCst);
                let max = self.0.maximum_pool_size.load(AtomicOrdering::SeqCst);
                timed = self.0.allow_core_thread_time_out.load(AtomicOrdering::SeqCst) || s.workers > core;
                if (s.workers > max || (timed && timed_out)) && (s.workers > 1 || self.0.queue.is_empty()) {
                    s.workers -= 1;
                    return None;
                }
            }
            let wait = if timed { self.0.keep_alive_nanos.load(AtomicOrdering::SeqCst) } else { IDLE_RECHECK_NANOS };
            match self.0.queue.poll_2(wait, TimeUnit::NANOSECONDS) {
                Ok(Some(r)) => return Some(r),
                _ => {
                    if timed {
                        timed_out = true;
                    }
                }
            }
        }
    }

    fn try_terminate(&self) {
        let mut s = self.0.state.lock();
        if s.workers == 0 && (s.run_state == STOP || (s.run_state == SHUTDOWN && self.0.queue.is_empty())) {
            s.run_state = TERMINATED;
            s.threads.clear();
            drop(s);
            self.0.termination.notify_all();
        }
    }

    /// `reject(command)`.
    fn reject(&self, command: JRef<dyn Runnable>) {
        let handler = self.0.handler.lock().clone();
        handler.rejected_execution(command, self.clone());
    }

    /// `ensurePrestart()` (used by `ScheduledThreadPoolExecutor`).
    pub(crate) fn ensure_prestart(&self) {
        let workers = self.0.state.lock().workers;
        if workers < self.0.core_pool_size.load(AtomicOrdering::SeqCst) {
            self.add_worker(None, true);
        } else if workers == 0 {
            self.add_worker(None, false);
        }
    }

    // ---- ExecutorService

    /// `execute(command)`.
    pub fn execute(&self, command: JRef<dyn Runnable>) {
        let workers = self.0.state.lock().workers;
        if workers < self.0.core_pool_size.load(AtomicOrdering::SeqCst) && self.add_worker(Some(command.clone()), true) {
            return;
        }
        if !self.is_shutdown() && self.0.queue.offer(command.clone()) {
            let (shutdown, workers) = {
                let s = self.0.state.lock();
                (s.run_state != RUNNING, s.workers)
            };
            if shutdown && self.remove(&command) {
                self.reject(command);
            } else if workers == 0 {
                self.add_worker(None, false);
            }
        } else if !self.add_worker(Some(command.clone()), false) {
            self.reject(command);
        }
    }
    pub fn submit<T: JGeneric>(&self, task: JRef<dyn Callable<T>>) -> JRef<dyn Future<T>> {
        submit_on(self, FutureTask::new(task))
    }
    pub fn submit_1(&self, task: JRef<dyn Runnable>) -> JRef<dyn Future<()>> {
        submit_on(self, FutureTask::new_2(task, ()))
    }
    pub fn submit_2<T: JGeneric>(&self, task: JRef<dyn Runnable>, result: T) -> JRef<dyn Future<T>> {
        submit_on(self, FutureTask::new_2(task, result))
    }
    pub fn shutdown(&self) {
        {
            let mut s = self.0.state.lock();
            if s.run_state < SHUTDOWN {
                s.run_state = SHUTDOWN;
            }
        }
        self.try_terminate();
    }
    pub fn shutdown_now(&self) -> Vec<JRef<dyn Runnable>> {
        {
            let mut s = self.0.state.lock();
            if s.run_state < STOP {
                s.run_state = STOP;
            }
        }
        let mut tasks = Vec::new();
        self.0.queue.drain_to(&mut tasks);
        self.try_terminate();
        tasks
    }
    pub fn is_shutdown(&self) -> bool {
        self.0.state.lock().run_state >= SHUTDOWN
    }
    pub fn is_terminating(&self) -> bool {
        let s = self.0.state.lock().run_state;
        s >= SHUTDOWN && s < TERMINATED
    }
    pub fn is_terminated(&self) -> bool {
        self.0.state.lock().run_state == TERMINATED
    }
    pub fn await_termination(&self, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable> {
        let deadline = support::nano_time().saturating_add(unit.to_nanos(timeout));
        let mut s = self.0.state.lock();
        while s.run_state != TERMINATED {
            if !support::wait_until(&self.0.termination, &mut s, deadline) {
                return Ok(s.run_state == TERMINATED);
            }
        }
        Ok(true)
    }

    // ---- configuration and statistics

    pub fn get_queue(&self) -> BlockingQueue<JRef<dyn Runnable>> {
        self.0.queue.clone()
    }
    /// `remove(task)`: removes a queued task (by identity).
    pub fn remove(&self, task: &JRef<dyn Runnable>) -> bool {
        let mut done = false;
        let removed = self.0.queue.remove_if(&mut |t| {
            if !done && same(t, task) {
                done = true;
                true
            } else {
                false
            }
        });
        self.try_terminate();
        removed
    }
    /// `purge()`: removes cancelled `Future` tasks (no-op here: cancelled `FutureTask`s do nothing when run).
    pub fn purge(&self) {}
    pub fn get_pool_size(&self) -> i32 {
        let s = self.0.state.lock();
        if s.run_state == TERMINATED { 0 } else { s.workers }
    }
    pub fn get_active_count(&self) -> i32 {
        self.0.state.lock().active
    }
    pub fn get_largest_pool_size(&self) -> i32 {
        self.0.state.lock().largest
    }
    pub fn get_completed_task_count(&self) -> i64 {
        self.0.state.lock().completed
    }
    pub fn get_task_count(&self) -> i64 {
        let s = self.0.state.lock();
        s.completed + s.active as i64 + self.0.queue.size() as i64
    }
    pub fn get_core_pool_size(&self) -> i32 {
        self.0.core_pool_size.load(AtomicOrdering::SeqCst)
    }
    pub fn set_core_pool_size(&self, core_pool_size: i32) {
        if core_pool_size < 0 || self.get_maximum_pool_size() < core_pool_size {
            support::illegal_argument("corePoolSize");
        }
        let old = self.0.core_pool_size.swap(core_pool_size, AtomicOrdering::SeqCst);
        if core_pool_size > old {
            let mut k = (core_pool_size - old).min(self.0.queue.size());
            while k > 0 && self.add_worker(None, true) {
                k -= 1;
                if self.0.queue.is_empty() {
                    break;
                }
            }
        }
    }
    pub fn get_maximum_pool_size(&self) -> i32 {
        self.0.maximum_pool_size.load(AtomicOrdering::SeqCst)
    }
    pub fn set_maximum_pool_size(&self, maximum_pool_size: i32) {
        if maximum_pool_size <= 0 || maximum_pool_size < self.get_core_pool_size() {
            support::illegal_argument("maximumPoolSize");
        }
        self.0.maximum_pool_size.store(maximum_pool_size, AtomicOrdering::SeqCst);
    }
    pub fn get_keep_alive_time(&self, unit: TimeUnit) -> i64 {
        unit.convert(self.0.keep_alive_nanos.load(AtomicOrdering::SeqCst), TimeUnit::NANOSECONDS)
    }
    pub fn set_keep_alive_time(&self, time: i64, unit: TimeUnit) {
        if time < 0 || (time == 0 && self.allows_core_thread_time_out()) {
            support::illegal_argument("Core threads must have nonzero keep alive times");
        }
        self.0.keep_alive_nanos.store(unit.to_nanos(time), AtomicOrdering::SeqCst);
    }
    pub fn allows_core_thread_time_out(&self) -> bool {
        self.0.allow_core_thread_time_out.load(AtomicOrdering::SeqCst)
    }
    pub fn allow_core_thread_time_out(&self, value: bool) {
        if value && self.0.keep_alive_nanos.load(AtomicOrdering::SeqCst) <= 0 {
            support::illegal_argument("Core threads must have nonzero keep alive times");
        }
        self.0.allow_core_thread_time_out.store(value, AtomicOrdering::SeqCst);
    }
    pub fn prestart_core_thread(&self) -> bool {
        self.0.state.lock().workers < self.get_core_pool_size() && self.add_worker(None, true)
    }
    pub fn prestart_all_core_threads(&self) -> i32 {
        let mut n = 0;
        while self.add_worker(None, true) {
            n += 1;
        }
        n
    }
    pub fn get_thread_factory(&self) -> JRef<dyn ThreadFactory> {
        self.0.thread_factory.lock().clone()
    }
    pub fn set_thread_factory(&self, thread_factory: JRef<dyn ThreadFactory>) {
        *self.0.thread_factory.lock() = thread_factory;
    }
    pub fn get_rejected_execution_handler(&self) -> JRef<dyn RejectedExecutionHandler> {
        self.0.handler.lock().clone()
    }
    pub fn set_rejected_execution_handler(&self, handler: JRef<dyn RejectedExecutionHandler>) {
        *self.0.handler.lock() = handler;
    }
}

impl Executor for ThreadPoolExecutor {
    fn execute(&self, command: JRef<dyn Runnable>) {
        ThreadPoolExecutor::execute(self, command)
    }
}
impl ExecutorServiceTrait for ThreadPoolExecutor {
    fn shutdown(&self) {
        ThreadPoolExecutor::shutdown(self)
    }
    fn shutdown_now(&self) -> Vec<JRef<dyn Runnable>> {
        ThreadPoolExecutor::shutdown_now(self)
    }
    fn is_shutdown(&self) -> bool {
        ThreadPoolExecutor::is_shutdown(self)
    }
    fn is_terminated(&self) -> bool {
        ThreadPoolExecutor::is_terminated(self)
    }
    fn await_termination(&self, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable> {
        ThreadPoolExecutor::await_termination(self, timeout, unit)
    }
}
impl From<ThreadPoolExecutor> for ExecutorService {
    fn from(e: ThreadPoolExecutor) -> Self {
        ExecutorService(Arc::new(e))
    }
}

fn rejected(r: &JRef<dyn Runnable>, e: &ThreadPoolExecutor) -> Throwable {
    let _ = (r, e);
    support::new_exception(support::REJECTED_EXECUTION_EXCEPTION, Some("Task rejected from ThreadPoolExecutor".to_string()), None)
}

/// `ThreadPoolExecutor.AbortPolicy`: throws `RejectedExecutionException`.
pub struct ThreadPoolExecutorAbortPolicy;
impl ThreadPoolExecutorAbortPolicy {
    pub fn new() -> JRef<Self> {
        Arc::new(ThreadPoolExecutorAbortPolicy)
    }
}
impl RejectedExecutionHandler for ThreadPoolExecutorAbortPolicy {
    fn rejected_execution(&self, r: JRef<dyn Runnable>, executor: ThreadPoolExecutor) {
        support::throw(rejected(&r, &executor))
    }
}
crate::jobject!(ThreadPoolExecutorAbortPolicy: dyn RejectedExecutionHandler);

/// `ThreadPoolExecutor.CallerRunsPolicy`: runs the task on the caller unless the pool is shut down.
pub struct ThreadPoolExecutorCallerRunsPolicy;
impl ThreadPoolExecutorCallerRunsPolicy {
    pub fn new() -> JRef<Self> {
        Arc::new(ThreadPoolExecutorCallerRunsPolicy)
    }
}
impl RejectedExecutionHandler for ThreadPoolExecutorCallerRunsPolicy {
    fn rejected_execution(&self, r: JRef<dyn Runnable>, executor: ThreadPoolExecutor) {
        if !executor.is_shutdown() {
            r.run();
        }
    }
}
crate::jobject!(ThreadPoolExecutorCallerRunsPolicy: dyn RejectedExecutionHandler);

/// `ThreadPoolExecutor.DiscardPolicy`: drops the task.
pub struct ThreadPoolExecutorDiscardPolicy;
impl ThreadPoolExecutorDiscardPolicy {
    pub fn new() -> JRef<Self> {
        Arc::new(ThreadPoolExecutorDiscardPolicy)
    }
}
impl RejectedExecutionHandler for ThreadPoolExecutorDiscardPolicy {
    fn rejected_execution(&self, _r: JRef<dyn Runnable>, _executor: ThreadPoolExecutor) {}
}
crate::jobject!(ThreadPoolExecutorDiscardPolicy: dyn RejectedExecutionHandler);

/// `ThreadPoolExecutor.DiscardOldestPolicy`: drops the oldest queued task and retries.
pub struct ThreadPoolExecutorDiscardOldestPolicy;
impl ThreadPoolExecutorDiscardOldestPolicy {
    pub fn new() -> JRef<Self> {
        Arc::new(ThreadPoolExecutorDiscardOldestPolicy)
    }
}
impl RejectedExecutionHandler for ThreadPoolExecutorDiscardOldestPolicy {
    fn rejected_execution(&self, r: JRef<dyn Runnable>, executor: ThreadPoolExecutor) {
        if !executor.is_shutdown() {
            executor.get_queue().poll();
            executor.execute(r);
        }
    }
}
crate::jobject!(ThreadPoolExecutorDiscardOldestPolicy: dyn RejectedExecutionHandler);
