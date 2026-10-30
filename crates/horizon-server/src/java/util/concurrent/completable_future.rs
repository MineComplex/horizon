//! Port of `java.util.concurrent.CompletableFuture` (and `CompletionStage`, which only it implements).
//!
//! `CompletableFuture<T>` is a value handle: clones share the one future, as Java references do. Stage
//! functions are closures (`f.then_apply(|v| ..)`), run once. Exceptions follow Java exactly: a stage
//! completed by `complete_exceptionally(ex)` holds `ex`; a dependent of a failed stage, or a stage whose
//! function threw (panicked), holds a `CompletionException` wrapping the cause; `join` throws (panics
//! with) a `CompletionException`/`CancellationException`; `get` returns `Err(ExecutionException)`.
//! `null` results: use `T = ()` for `Void` and `Option<..>` for nullable values.
use crate::java::lang::{Runnable, Throwable};
use crate::java::util::concurrent::fork_join_pool::ForkJoinPool;
use crate::java::util::concurrent::support::{self, catch};
use crate::java::util::concurrent::{Executor, Future, FutureState, TimeUnit};
use crate::prelude::*;
use parking_lot::{Condvar, Mutex};
use std::collections::BinaryHeap;

type Outcome<T> = Result<T, Throwable>;
type Dep<T> = Box<dyn FnOnce(Outcome<T>) + Send>;

struct CfState<T> {
    result: Option<Outcome<T>>,
    cancelled: bool,
    deps: Vec<Dep<T>>,
}

pub struct CompletableFutureInner<T> {
    state: Mutex<CfState<T>>,
    cond: Condvar,
}

/// `CompletableFuture<T>`.
pub struct CompletableFuture<T: JGeneric = JObjectRef>(pub JRef<CompletableFutureInner<T>>);

impl<T: JGeneric> Clone for CompletableFuture<T> {
    fn clone(&self) -> Self {
        CompletableFuture(self.0.clone())
    }
}
impl<T: JGeneric> Default for CompletableFuture<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: JGeneric> PartialEq for CompletableFuture<T> {
    /// Java's `==`: the same future.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// `CompletionStage<T>`: the stage interface; its (generic) stage methods are `CompletableFuture`'s.
pub trait CompletionStage<T: JGeneric>: Send + Sync {
    fn to_completable_future(&self) -> CompletableFuture<T>;
}
impl<T: JGeneric> CompletionStage<T> for CompletableFuture<T> {
    fn to_completable_future(&self) -> CompletableFuture<T> {
        self.clone()
    }
}

/// `new CompletionException(ex)` unless `ex` already is one (Java's `encodeThrowable`).
fn wrap(ex: Throwable) -> Throwable {
    if support::is_instance(&ex, support::COMPLETION_EXCEPTION) {
        ex
    } else {
        support::new_exception(support::COMPLETION_EXCEPTION, None, Some(ex))
    }
}

fn cancellation() -> Throwable {
    support::new_exception(support::CANCELLATION_EXCEPTION, None, None)
}

/// The default executor of the `*Async` methods: `ForkJoinPool.commonPool()`.
pub fn async_pool() -> JRef<dyn Executor> {
    Arc::new(ForkJoinPool::common_pool())
}

impl<T: JGeneric> CompletableFuture<T> {
    // ---- construction

    /// `new CompletableFuture<>()`.
    pub fn new() -> Self {
        CompletableFuture(Arc::new(CompletableFutureInner {
            state: Mutex::new(CfState {
                result: None,
                cancelled: false,
                deps: Vec::new(),
            }),
            cond: Condvar::new(),
        }))
    }
    /// `newIncompleteFuture()`.
    pub fn new_incomplete_future<U: JGeneric>(&self) -> CompletableFuture<U> {
        CompletableFuture::new()
    }
    /// `CompletableFuture.completedFuture(value)`.
    pub fn completed_future(value: T) -> Self {
        let ret = Self::new();
        ret.complete_outcome(Ok(value), false);
        ret
    }
    /// `CompletableFuture.completedStage(value)`.
    pub fn completed_stage(value: T) -> Self {
        Self::completed_future(value)
    }
    /// `CompletableFuture.failedFuture(ex)`.
    pub fn failed_future(ex: Throwable) -> Self {
        let ret = Self::new();
        ret.complete_outcome(Err(ex), false);
        ret
    }
    /// `CompletableFuture.failedStage(ex)`.
    pub fn failed_stage(ex: Throwable) -> Self {
        Self::failed_future(ex)
    }
    /// `CompletableFuture.supplyAsync(supplier)`.
    pub fn supply_async(supplier: impl FnOnce() -> T + Send + 'static) -> Self {
        Self::supply_async_2(supplier, async_pool())
    }
    /// `CompletableFuture.supplyAsync(supplier, executor)`.
    pub fn supply_async_2(
        supplier: impl FnOnce() -> T + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> Self {
        let ret = Self::new();
        let dst = ret.clone();
        dst.clone().execute_on(&executor, move || {
            dst.complete_outcome(catch(supplier).map_err(wrap), false);
        });
        ret
    }

    // ---- internals

    fn complete_outcome(&self, outcome: Outcome<T>, cancelled: bool) -> bool {
        let deps = {
            let mut s = self.0.state.lock();
            if s.result.is_some() {
                return false;
            }
            s.result = Some(outcome.clone());
            s.cancelled = cancelled;
            std::mem::take(&mut s.deps)
        };
        self.0.cond.notify_all();
        // Java pops its dependents' stack: the most recently added first.
        for dep in deps.into_iter().rev() {
            dep(outcome.clone());
        }
        true
    }

    /// Runs `f` with this future's outcome once it completes (now, if it has).
    fn on_complete(&self, f: impl FnOnce(Outcome<T>) + Send + 'static) {
        let mut s = self.0.state.lock();
        match &s.result {
            Some(r) => {
                let r = r.clone();
                drop(s);
                f(r);
            }
            None => s.deps.push(Box::new(f)),
        }
    }

    /// Runs `f` on `executor`, failing this future if the executor rejects the task.
    fn execute_on(self, executor: &JRef<dyn Executor>, f: impl FnOnce() + Send + 'static) {
        if let Err(ex) = catch(|| executor.execute(support::runnable(f))) {
            self.complete_outcome(Err(wrap(ex)), false);
        }
    }

    fn outcome_now(&self) -> Option<Outcome<T>> {
        self.0.state.lock().result.clone()
    }

    fn wait_outcome(&self) -> Outcome<T> {
        let mut s = self.0.state.lock();
        loop {
            if let Some(r) = &s.result {
                return r.clone();
            }
            self.0.cond.wait(&mut s);
        }
    }

    fn wait_outcome_until(&self, deadline: i64) -> Option<Outcome<T>> {
        let mut s = self.0.state.lock();
        loop {
            if let Some(r) = &s.result {
                return Some(r.clone());
            }
            if !support::wait_until(&self.0.cond, &mut s, deadline) {
                return s.result.clone();
            }
        }
    }

    /// Java's `reportJoin`.
    fn report_join(r: Outcome<T>) -> T {
        match r {
            Ok(v) => v,
            Err(ex) => {
                if support::is_instance(&ex, support::CANCELLATION_EXCEPTION) {
                    support::throw(ex)
                }
                support::throw(wrap(ex))
            }
        }
    }

    /// Java's `reportGet`.
    fn report_get(r: Outcome<T>) -> Result<T, Throwable> {
        match r {
            Ok(v) => Ok(v),
            Err(ex) => {
                if support::is_instance(&ex, support::CANCELLATION_EXCEPTION) {
                    return Err(ex);
                }
                let cause = if support::is_instance(&ex, support::COMPLETION_EXCEPTION) {
                    support::cause(&ex).unwrap_or(ex)
                } else {
                    ex
                };
                Err(support::new_exception(
                    support::EXECUTION_EXCEPTION,
                    None,
                    Some(cause),
                ))
            }
        }
    }

    // ---- completion

    /// `complete(value)`.
    pub fn complete(&self, value: T) -> bool {
        self.complete_outcome(Ok(value), false)
    }
    /// `completeExceptionally(ex)`.
    pub fn complete_exceptionally(&self, ex: Throwable) -> bool {
        self.complete_outcome(Err(ex), false)
    }
    /// `cancel(mayInterruptIfRunning)`.
    pub fn cancel(&self, _may_interrupt_if_running: bool) -> bool {
        let cancelled = self.complete_outcome(Err(cancellation()), true);
        cancelled || self.is_cancelled()
    }
    /// `completeAsync(supplier)`.
    pub fn complete_async(&self, supplier: impl FnOnce() -> T + Send + 'static) -> Self {
        self.complete_async_2(supplier, async_pool())
    }
    /// `completeAsync(supplier, executor)`.
    pub fn complete_async_2(
        &self,
        supplier: impl FnOnce() -> T + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> Self {
        let dst = self.clone();
        self.clone().execute_on(&executor, move || {
            dst.complete_outcome(catch(supplier).map_err(wrap), false);
        });
        self.clone()
    }
    /// `obtrudeValue(value)`.
    pub fn obtrude_value(&self, value: T) {
        {
            let mut s = self.0.state.lock();
            s.result = Some(Ok(value));
            s.cancelled = false;
        }
        self.0.cond.notify_all();
    }
    /// `obtrudeException(ex)`.
    pub fn obtrude_exception(&self, ex: Throwable) {
        {
            let mut s = self.0.state.lock();
            let cancelled = support::is_instance(&ex, support::CANCELLATION_EXCEPTION);
            s.result = Some(Err(ex));
            s.cancelled = cancelled;
        }
        self.0.cond.notify_all();
    }
    /// `orTimeout(timeout, unit)`: completes exceptionally with a `TimeoutException` if not done in time.
    pub fn or_timeout(&self, timeout: i64, unit: TimeUnit) -> Self {
        if !self.is_done() {
            let me = self.clone();
            delay(unit.to_nanos(timeout), move || {
                me.complete_exceptionally(support::new_exception(
                    support::TIMEOUT_EXCEPTION,
                    None,
                    None,
                ));
            });
        }
        self.clone()
    }
    /// `completeOnTimeout(value, timeout, unit)`.
    pub fn complete_on_timeout(&self, value: T, timeout: i64, unit: TimeUnit) -> Self {
        if !self.is_done() {
            let me = self.clone();
            delay(unit.to_nanos(timeout), move || {
                me.complete(value);
            });
        }
        self.clone()
    }

    // ---- queries

    pub fn is_done(&self) -> bool {
        self.0.state.lock().result.is_some()
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.state.lock().cancelled
    }
    pub fn is_completed_exceptionally(&self) -> bool {
        matches!(self.0.state.lock().result, Some(Err(_)))
    }
    /// `state()`.
    pub fn state(&self) -> FutureState {
        let s = self.0.state.lock();
        match &s.result {
            None => FutureState::RUNNING,
            Some(Ok(_)) => FutureState::SUCCESS,
            Some(Err(_)) if s.cancelled => FutureState::CANCELLED,
            Some(Err(_)) => FutureState::FAILED,
        }
    }
    /// `join()`: the value, or throws (panics with) the `CompletionException`/`CancellationException`.
    pub fn join(&self) -> T {
        Self::report_join(self.wait_outcome())
    }
    /// `join()` with the exception it would throw as `Err` (Java: `try { f.join() } catch (CompletionException e)`).
    pub fn try_join(&self) -> Result<T, Throwable> {
        match self.wait_outcome() {
            Ok(v) => Ok(v),
            Err(ex) if support::is_instance(&ex, support::CANCELLATION_EXCEPTION) => Err(ex),
            Err(ex) => Err(wrap(ex)),
        }
    }
    /// `get()` (`throws InterruptedException, ExecutionException`).
    pub fn get(&self) -> Result<T, Throwable> {
        Self::report_get(self.wait_outcome())
    }
    /// `get(timeout, unit)` (also `throws TimeoutException`).
    pub fn get_2(&self, timeout: i64, unit: TimeUnit) -> Result<T, Throwable> {
        let deadline = support::nano_time().saturating_add(unit.to_nanos(timeout));
        match self.wait_outcome_until(deadline) {
            Some(r) => Self::report_get(r),
            None => Err(support::new_exception(
                support::TIMEOUT_EXCEPTION,
                None,
                None,
            )),
        }
    }
    /// `getNow(valueIfAbsent)`.
    pub fn get_now(&self, value_if_absent: T) -> T {
        match self.outcome_now() {
            None => value_if_absent,
            Some(r) => Self::report_join(r),
        }
    }
    /// `resultNow()`.
    pub fn result_now(&self) -> T {
        match self.outcome_now() {
            None => support::illegal_state("Task has not completed"),
            Some(Ok(v)) => v,
            Some(Err(_)) if self.is_cancelled() => support::illegal_state("Task was cancelled"),
            Some(Err(_)) => support::illegal_state("Task completed with exception"),
        }
    }
    /// `exceptionNow()`.
    pub fn exception_now(&self) -> Throwable {
        match self.outcome_now() {
            None => support::illegal_state("Task has not completed"),
            Some(Ok(_)) => support::illegal_state("Task completed with a result"),
            Some(Err(_)) if self.is_cancelled() => support::illegal_state("Task was cancelled"),
            Some(Err(ex)) => {
                if support::is_instance(&ex, support::COMPLETION_EXCEPTION) {
                    support::cause(&ex).unwrap_or(ex)
                } else {
                    ex
                }
            }
        }
    }
    /// `getNumberOfDependents()`.
    pub fn get_number_of_dependents(&self) -> i32 {
        self.0.state.lock().deps.len() as i32
    }
    /// `toCompletableFuture()`.
    pub fn to_completable_future(&self) -> Self {
        self.clone()
    }
    /// `copy()`.
    pub fn copy(&self) -> Self {
        self.then_apply(|v| v)
    }
    /// `minimalCompletionStage()`.
    pub fn minimal_completion_stage(&self) -> Self {
        self.copy()
    }
    /// `defaultExecutor()`.
    pub fn default_executor(&self) -> JRef<dyn Executor> {
        async_pool()
    }
    /// This future as a `CompletableFuture<Void>` that completes when it does (for `allOf` over futures
    /// of different types, Java's `CompletableFuture<?>[]`).
    pub fn to_void(&self) -> CompletableFuture<()> {
        self.then_apply(|_| ())
    }

    // ---- stages

    fn uni<U: JGeneric>(
        &self,
        executor: Option<JRef<dyn Executor>>,
        f: impl FnOnce(T) -> Outcome<U> + Send + 'static,
    ) -> CompletableFuture<U> {
        let dst = CompletableFuture::<U>::new();
        let d = dst.clone();
        self.on_complete(move |r| match r {
            Err(ex) => {
                d.complete_outcome(Err(wrap(ex)), false);
            }
            Ok(v) => match executor {
                None => {
                    d.complete_outcome(catch(move || f(v)).and_then(|x| x).map_err(wrap), false);
                }
                Some(e) => {
                    let d2 = d.clone();
                    d.execute_on(&e, move || {
                        d2.complete_outcome(
                            catch(move || f(v)).and_then(|x| x).map_err(wrap),
                            false,
                        );
                    });
                }
            },
        });
        dst
    }

    fn bi<U: JGeneric>(
        &self,
        executor: Option<JRef<dyn Executor>>,
        f: impl FnOnce(Outcome<T>) -> Outcome<U> + Send + 'static,
    ) -> CompletableFuture<U> {
        let dst = CompletableFuture::<U>::new();
        let d = dst.clone();
        self.on_complete(move |r| match executor {
            None => {
                d.complete_outcome(catch(move || f(r)).and_then(|x| x), false);
            }
            Some(e) => {
                let d2 = d.clone();
                d.execute_on(&e, move || {
                    d2.complete_outcome(catch(move || f(r)).and_then(|x| x), false);
                });
            }
        });
        dst
    }

    /// `thenApply(fn)`.
    pub fn then_apply<U: JGeneric>(
        &self,
        f: impl FnOnce(T) -> U + Send + 'static,
    ) -> CompletableFuture<U> {
        self.uni(None, move |v| Ok(f(v)))
    }
    /// `thenApplyAsync(fn)`.
    pub fn then_apply_async<U: JGeneric>(
        &self,
        f: impl FnOnce(T) -> U + Send + 'static,
    ) -> CompletableFuture<U> {
        self.uni(Some(async_pool()), move |v| Ok(f(v)))
    }
    /// `thenApplyAsync(fn, executor)`.
    pub fn then_apply_async_2<U: JGeneric>(
        &self,
        f: impl FnOnce(T) -> U + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<U> {
        self.uni(Some(executor), move |v| Ok(f(v)))
    }
    /// `thenAccept(action)`.
    pub fn then_accept(&self, f: impl FnOnce(T) + Send + 'static) -> CompletableFuture<()> {
        self.uni(None, move |v| Ok(f(v)))
    }
    /// `thenAcceptAsync(action)`.
    pub fn then_accept_async(&self, f: impl FnOnce(T) + Send + 'static) -> CompletableFuture<()> {
        self.uni(Some(async_pool()), move |v| Ok(f(v)))
    }
    /// `thenAcceptAsync(action, executor)`.
    pub fn then_accept_async_2(
        &self,
        f: impl FnOnce(T) + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<()> {
        self.uni(Some(executor), move |v| Ok(f(v)))
    }
    /// `thenRun(action)`.
    pub fn then_run(&self, f: impl FnOnce() + Send + 'static) -> CompletableFuture<()> {
        self.uni(None, move |_| Ok(f()))
    }
    /// `thenRunAsync(action)`.
    pub fn then_run_async(&self, f: impl FnOnce() + Send + 'static) -> CompletableFuture<()> {
        self.uni(Some(async_pool()), move |_| Ok(f()))
    }
    /// `thenRunAsync(action, executor)`.
    pub fn then_run_async_2(
        &self,
        f: impl FnOnce() + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<()> {
        self.uni(Some(executor), move |_| Ok(f()))
    }

    fn compose<U: JGeneric>(
        &self,
        executor: Option<JRef<dyn Executor>>,
        f: impl FnOnce(T) -> CompletableFuture<U> + Send + 'static,
    ) -> CompletableFuture<U> {
        let dst = CompletableFuture::<U>::new();
        let d = dst.clone();
        let relay = move |d: CompletableFuture<U>, v: T| match catch(move || f(v)) {
            Err(ex) => {
                d.complete_outcome(Err(wrap(ex)), false);
            }
            Ok(g) => {
                g.on_complete(move |r| {
                    d.complete_outcome(r.map_err(wrap), false);
                });
            }
        };
        self.on_complete(move |r| match r {
            Err(ex) => {
                d.complete_outcome(Err(wrap(ex)), false);
            }
            Ok(v) => match executor {
                None => relay(d, v),
                Some(e) => {
                    let d2 = d.clone();
                    d.execute_on(&e, move || relay(d2, v));
                }
            },
        });
        dst
    }
    /// `thenCompose(fn)`.
    pub fn then_compose<U: JGeneric>(
        &self,
        f: impl FnOnce(T) -> CompletableFuture<U> + Send + 'static,
    ) -> CompletableFuture<U> {
        self.compose(None, f)
    }
    /// `thenComposeAsync(fn)`.
    pub fn then_compose_async<U: JGeneric>(
        &self,
        f: impl FnOnce(T) -> CompletableFuture<U> + Send + 'static,
    ) -> CompletableFuture<U> {
        self.compose(Some(async_pool()), f)
    }
    /// `thenComposeAsync(fn, executor)`.
    pub fn then_compose_async_2<U: JGeneric>(
        &self,
        f: impl FnOnce(T) -> CompletableFuture<U> + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<U> {
        self.compose(Some(executor), f)
    }

    fn combine<U: JGeneric, V: JGeneric>(
        &self,
        other: &CompletableFuture<U>,
        executor: Option<JRef<dyn Executor>>,
        f: impl FnOnce(T, U) -> V + Send + 'static,
    ) -> CompletableFuture<V> {
        let other = other.clone();
        let dst = CompletableFuture::<V>::new();
        let d = dst.clone();
        self.on_complete(move |r1| {
            other.on_complete(move |r2| match (r1, r2) {
                (Err(ex), _) | (_, Err(ex)) => {
                    d.complete_outcome(Err(wrap(ex)), false);
                }
                (Ok(a), Ok(b)) => match executor {
                    None => {
                        d.complete_outcome(catch(move || f(a, b)).map_err(wrap), false);
                    }
                    Some(e) => {
                        let d2 = d.clone();
                        d.execute_on(&e, move || {
                            d2.complete_outcome(catch(move || f(a, b)).map_err(wrap), false);
                        });
                    }
                },
            });
        });
        dst
    }
    /// `thenCombine(other, fn)`.
    pub fn then_combine<U: JGeneric, V: JGeneric>(
        &self,
        other: &CompletableFuture<U>,
        f: impl FnOnce(T, U) -> V + Send + 'static,
    ) -> CompletableFuture<V> {
        self.combine(other, None, f)
    }
    /// `thenCombineAsync(other, fn)`.
    pub fn then_combine_async<U: JGeneric, V: JGeneric>(
        &self,
        other: &CompletableFuture<U>,
        f: impl FnOnce(T, U) -> V + Send + 'static,
    ) -> CompletableFuture<V> {
        self.combine(other, Some(async_pool()), f)
    }
    /// `thenCombineAsync(other, fn, executor)`.
    pub fn then_combine_async_3<U: JGeneric, V: JGeneric>(
        &self,
        other: &CompletableFuture<U>,
        f: impl FnOnce(T, U) -> V + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<V> {
        self.combine(other, Some(executor), f)
    }
    /// `thenAcceptBoth(other, action)`.
    pub fn then_accept_both<U: JGeneric>(
        &self,
        other: &CompletableFuture<U>,
        f: impl FnOnce(T, U) + Send + 'static,
    ) -> CompletableFuture<()> {
        self.combine(other, None, f)
    }
    /// `runAfterBoth(other, action)`.
    pub fn run_after_both<U: JGeneric>(
        &self,
        other: &CompletableFuture<U>,
        f: impl FnOnce() + Send + 'static,
    ) -> CompletableFuture<()> {
        self.combine(other, None, move |_, _| f())
    }

    fn either<V: JGeneric>(
        &self,
        other: &CompletableFuture<T>,
        executor: Option<JRef<dyn Executor>>,
        f: impl FnOnce(T) -> V + Send + 'static,
    ) -> CompletableFuture<V> {
        let first = any_of(&[self.clone(), other.clone()]);
        first.uni(executor, move |v| Ok(f(v)))
    }
    /// `applyToEither(other, fn)`.
    pub fn apply_to_either<V: JGeneric>(
        &self,
        other: &CompletableFuture<T>,
        f: impl FnOnce(T) -> V + Send + 'static,
    ) -> CompletableFuture<V> {
        self.either(other, None, f)
    }
    /// `acceptEither(other, action)`.
    pub fn accept_either(
        &self,
        other: &CompletableFuture<T>,
        f: impl FnOnce(T) + Send + 'static,
    ) -> CompletableFuture<()> {
        self.either(other, None, f)
    }
    /// `runAfterEither(other, action)`.
    pub fn run_after_either(
        &self,
        other: &CompletableFuture<T>,
        f: impl FnOnce() + Send + 'static,
    ) -> CompletableFuture<()> {
        self.either(other, None, move |_| f())
    }

    fn when(
        &self,
        executor: Option<JRef<dyn Executor>>,
        f: impl FnOnce(Option<T>, Option<Throwable>) + Send + 'static,
    ) -> CompletableFuture<T> {
        self.bi(executor, move |r| {
            let (v, x) = match &r {
                Ok(v) => (Some(v.clone()), None),
                Err(x) => (None, Some(x.clone())),
            };
            match catch(move || f(v, x)) {
                Ok(()) => r.map_err(wrap),
                Err(thrown) => match r {
                    Ok(_) => Err(wrap(thrown)),
                    // Java: the source's exception wins (the thrown one is added as suppressed)
                    Err(x) => Err(wrap(x)),
                },
            }
        })
    }
    /// `whenComplete(action)`: `action(result, exception)`, one of them `None`.
    pub fn when_complete(
        &self,
        f: impl FnOnce(Option<T>, Option<Throwable>) + Send + 'static,
    ) -> CompletableFuture<T> {
        self.when(None, f)
    }
    /// `whenCompleteAsync(action)`.
    pub fn when_complete_async(
        &self,
        f: impl FnOnce(Option<T>, Option<Throwable>) + Send + 'static,
    ) -> CompletableFuture<T> {
        self.when(Some(async_pool()), f)
    }
    /// `whenCompleteAsync(action, executor)`.
    pub fn when_complete_async_2(
        &self,
        f: impl FnOnce(Option<T>, Option<Throwable>) + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<T> {
        self.when(Some(executor), f)
    }

    fn handle_on<U: JGeneric>(
        &self,
        executor: Option<JRef<dyn Executor>>,
        f: impl FnOnce(Option<T>, Option<Throwable>) -> U + Send + 'static,
    ) -> CompletableFuture<U> {
        self.bi(executor, move |r| {
            let (v, x) = match r {
                Ok(v) => (Some(v), None),
                Err(x) => (None, Some(x)),
            };
            catch(move || f(v, x)).map_err(wrap)
        })
    }
    /// `handle(fn)`: `fn(result, exception)`, one of them `None`.
    pub fn handle<U: JGeneric>(
        &self,
        f: impl FnOnce(Option<T>, Option<Throwable>) -> U + Send + 'static,
    ) -> CompletableFuture<U> {
        self.handle_on(None, f)
    }
    /// `handleAsync(fn)`.
    pub fn handle_async<U: JGeneric>(
        &self,
        f: impl FnOnce(Option<T>, Option<Throwable>) -> U + Send + 'static,
    ) -> CompletableFuture<U> {
        self.handle_on(Some(async_pool()), f)
    }
    /// `handleAsync(fn, executor)`.
    pub fn handle_async_2<U: JGeneric>(
        &self,
        f: impl FnOnce(Option<T>, Option<Throwable>) -> U + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<U> {
        self.handle_on(Some(executor), f)
    }

    fn exceptionally_on(
        &self,
        executor: Option<JRef<dyn Executor>>,
        f: impl FnOnce(Throwable) -> T + Send + 'static,
    ) -> CompletableFuture<T> {
        let dst = CompletableFuture::<T>::new();
        let d = dst.clone();
        self.on_complete(move |r| match r {
            Ok(v) => {
                d.complete_outcome(Ok(v), false);
            }
            Err(ex) => match executor {
                None => {
                    d.complete_outcome(catch(move || f(ex)).map_err(wrap), false);
                }
                Some(e) => {
                    let d2 = d.clone();
                    d.execute_on(&e, move || {
                        d2.complete_outcome(catch(move || f(ex)).map_err(wrap), false);
                    });
                }
            },
        });
        dst
    }
    /// `exceptionally(fn)`.
    pub fn exceptionally(
        &self,
        f: impl FnOnce(Throwable) -> T + Send + 'static,
    ) -> CompletableFuture<T> {
        self.exceptionally_on(None, f)
    }
    /// `exceptionallyAsync(fn)`.
    pub fn exceptionally_async(
        &self,
        f: impl FnOnce(Throwable) -> T + Send + 'static,
    ) -> CompletableFuture<T> {
        self.exceptionally_on(Some(async_pool()), f)
    }
    /// `exceptionallyAsync(fn, executor)`.
    pub fn exceptionally_async_2(
        &self,
        f: impl FnOnce(Throwable) -> T + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<T> {
        self.exceptionally_on(Some(executor), f)
    }
    /// `exceptionallyCompose(fn)`.
    pub fn exceptionally_compose(
        &self,
        f: impl FnOnce(Throwable) -> CompletableFuture<T> + Send + 'static,
    ) -> CompletableFuture<T> {
        let dst = CompletableFuture::<T>::new();
        let d = dst.clone();
        self.on_complete(move |r| match r {
            Ok(v) => {
                d.complete_outcome(Ok(v), false);
            }
            Err(ex) => match catch(move || f(ex)) {
                Err(thrown) => {
                    d.complete_outcome(Err(wrap(thrown)), false);
                }
                Ok(g) => g.on_complete(move |r| {
                    d.complete_outcome(r.map_err(wrap), false);
                }),
            },
        });
        dst
    }
}

impl CompletableFuture<()> {
    /// `CompletableFuture.runAsync(runnable)`.
    pub fn run_async(f: impl FnOnce() + Send + 'static) -> CompletableFuture<()> {
        Self::supply_async(f)
    }
    /// `CompletableFuture.runAsync(runnable, executor)`.
    pub fn run_async_2(
        f: impl FnOnce() + Send + 'static,
        executor: JRef<dyn Executor>,
    ) -> CompletableFuture<()> {
        Self::supply_async_2(f, executor)
    }
    /// `CompletableFuture.allOf(cfs...)`; for futures of different types pass `f.to_void()`s.
    pub fn all_of<T: JGeneric>(cfs: &[CompletableFuture<T>]) -> CompletableFuture<()> {
        all_of(cfs)
    }
}

/// `CompletableFuture.allOf(cfs...)`: completes when all have; exceptionally (with a
/// `CompletionException`) if any did, with the first such one in array order.
pub fn all_of<T: JGeneric>(cfs: &[CompletableFuture<T>]) -> CompletableFuture<()> {
    let dst = CompletableFuture::<()>::new();
    if cfs.is_empty() {
        dst.complete(());
        return dst;
    }
    let remaining = Arc::new(std::sync::atomic::AtomicUsize::new(cfs.len()));
    let all: Arc<Vec<CompletableFuture<T>>> = Arc::new(cfs.to_vec());
    for cf in cfs {
        let remaining = remaining.clone();
        let all = all.clone();
        let d = dst.clone();
        cf.on_complete(move |_| {
            if remaining.fetch_sub(1, AtomicOrdering::AcqRel) == 1 {
                for f in all.iter() {
                    if let Some(Err(ex)) = f.outcome_now() {
                        d.complete_outcome(Err(wrap(ex)), false);
                        return;
                    }
                }
                d.complete_outcome(Ok(()), false);
            }
        });
    }
    dst
}

/// `CompletableFuture.anyOf(cfs...)`: completes as the first of them to complete does.
pub fn any_of<T: JGeneric>(cfs: &[CompletableFuture<T>]) -> CompletableFuture<T> {
    let dst = CompletableFuture::<T>::new();
    for cf in cfs {
        let d = dst.clone();
        cf.on_complete(move |r| {
            d.complete_outcome(r.map_err(wrap), false);
        });
    }
    dst
}

impl<T: JGeneric> CompletableFuture<T> {
    /// `CompletableFuture.anyOf(cfs...)`.
    pub fn any_of(cfs: &[CompletableFuture<T>]) -> CompletableFuture<T> {
        any_of(cfs)
    }
}

impl<T: JGeneric> Future<T> for CompletableFuture<T> {
    fn cancel(&self, may_interrupt_if_running: bool) -> bool {
        CompletableFuture::cancel(self, may_interrupt_if_running)
    }
    fn is_cancelled(&self) -> bool {
        CompletableFuture::is_cancelled(self)
    }
    fn is_done(&self) -> bool {
        CompletableFuture::is_done(self)
    }
    fn get(&self) -> Result<T, Throwable> {
        CompletableFuture::get(self)
    }
    fn get_2(&self, timeout: i64, unit: TimeUnit) -> Result<T, Throwable> {
        CompletableFuture::get_2(self, timeout, unit)
    }
    fn result_now(&self) -> T {
        CompletableFuture::result_now(self)
    }
    fn exception_now(&self) -> Throwable {
        CompletableFuture::exception_now(self)
    }
    fn state(&self) -> FutureState {
        CompletableFuture::state(self)
    }
}

// ---- delayed execution (Java: CompletableFuture.Delayer, a daemon ScheduledThreadPoolExecutor)

struct DelayedTask {
    at: i64,
    seq: u64,
    run: Box<dyn FnOnce() + Send>,
}
impl PartialEq for DelayedTask {
    fn eq(&self, o: &Self) -> bool {
        self.at == o.at && self.seq == o.seq
    }
}
impl Eq for DelayedTask {}
impl PartialOrd for DelayedTask {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for DelayedTask {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        // min-heap on (at, seq)
        o.at.cmp(&self.at).then(o.seq.cmp(&self.seq))
    }
}

struct Delayer {
    queue: Mutex<(BinaryHeap<DelayedTask>, u64)>,
    cond: Condvar,
}

static DELAYER: LazyLock<Arc<Delayer>> = LazyLock::new(|| {
    let d = Arc::new(Delayer {
        queue: Mutex::new((BinaryHeap::new(), 0)),
        cond: Condvar::new(),
    });
    let worker = d.clone();
    std::thread::Builder::new()
        .name("CompletableFutureDelayScheduler".into())
        .spawn(move || {
            loop {
                let task = {
                    let mut q = worker.queue.lock();
                    loop {
                        let now = support::nano_time();
                        match q.0.peek() {
                            None => {
                                worker.cond.wait(&mut q);
                            }
                            Some(t) if t.at <= now => break q.0.pop().unwrap(),
                            Some(t) => {
                                let wait = (t.at - now) as u64;
                                let _ = worker
                                    .cond
                                    .wait_for(&mut q, std::time::Duration::from_nanos(wait));
                            }
                        }
                    }
                };
                if let Err(ex) = catch(task.run) {
                    log::error!(
                        "Exception in CompletableFutureDelayScheduler: {:?}",
                        support::class_name(&ex)
                    );
                }
            }
        })
        .expect("CompletableFutureDelayScheduler");
    d
});

/// Runs `f` on the delay scheduler thread after `nanos`.
fn delay(nanos: i64, f: impl FnOnce() + Send + 'static) {
    let d = &*DELAYER;
    {
        let mut q = d.queue.lock();
        let seq = q.1;
        q.1 += 1;
        q.0.push(DelayedTask {
            at: support::nano_time().saturating_add(nanos.max(0)),
            seq,
            run: Box::new(f),
        });
    }
    d.cond.notify_all();
}

/// `CompletableFuture.delayedExecutor(delay, unit)`.
pub fn delayed_executor(delay_: i64, unit: TimeUnit) -> JRef<dyn Executor> {
    delayed_executor_3(delay_, unit, async_pool())
}

/// `CompletableFuture.delayedExecutor(delay, unit, executor)`.
pub fn delayed_executor_3(
    delay_: i64,
    unit: TimeUnit,
    executor: JRef<dyn Executor>,
) -> JRef<dyn Executor> {
    let nanos = unit.to_nanos(delay_);
    Arc::new(move |r: JRef<dyn Runnable>| {
        let executor = executor.clone();
        delay(nanos, move || executor.execute(r));
    })
}
