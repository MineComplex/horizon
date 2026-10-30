//! Port of `java.util.concurrent.Future` (and its nested `Future.State`, `FutureState`).
use crate::java::lang::Throwable;
use crate::java::util::concurrent::TimeUnit;
use crate::java::util::concurrent::support;

/// `Future<V>`. `get` throws the checked `InterruptedException`/`ExecutionException`
/// (`Err`); `get_2` also `TimeoutException`.
pub trait Future<V = crate::java::JObjectRef>: Send + Sync {
    fn cancel(&self, may_interrupt_if_running: bool) -> bool;
    fn is_cancelled(&self) -> bool;
    fn is_done(&self) -> bool;
    /// `get()`.
    fn get(&self) -> Result<V, Throwable>;
    /// `get(long timeout, TimeUnit unit)`.
    fn get_2(&self, timeout: i64, unit: TimeUnit) -> Result<V, Throwable>;
    /// `resultNow()`: the result, or `IllegalStateException` if the task did not complete with one.
    fn result_now(&self) -> V {
        if !self.is_done() {
            support::illegal_state("Task has not completed");
        }
        if self.is_cancelled() {
            support::illegal_state("Task was cancelled");
        }
        match self.get() {
            Ok(v) => v,
            Err(_) => support::illegal_state("Task completed with exception"),
        }
    }
    /// `exceptionNow()`.
    fn exception_now(&self) -> Throwable {
        if !self.is_done() {
            support::illegal_state("Task has not completed");
        }
        if self.is_cancelled() {
            support::illegal_state("Task was cancelled");
        }
        match self.get() {
            Ok(_) => support::illegal_state("Task completed with a result"),
            Err(e) => support::cause(&e).unwrap_or(e),
        }
    }
    /// `state()`.
    fn state(&self) -> FutureState {
        if !self.is_done() {
            return FutureState::RUNNING;
        }
        if self.is_cancelled() {
            return FutureState::CANCELLED;
        }
        match self.get() {
            Ok(_) => FutureState::SUCCESS,
            Err(_) => FutureState::FAILED,
        }
    }
}

/// `Future.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum FutureState {
    RUNNING,
    SUCCESS,
    FAILED,
    CANCELLED,
}
impl FutureState {
    pub const VALUES: [FutureState; 4] = [
        FutureState::RUNNING,
        FutureState::SUCCESS,
        FutureState::FAILED,
        FutureState::CANCELLED,
    ];
    pub fn ordinal(self) -> i32 {
        self as i32
    }
    pub fn name(self) -> &'static str {
        match self {
            FutureState::RUNNING => "RUNNING",
            FutureState::SUCCESS => "SUCCESS",
            FutureState::FAILED => "FAILED",
            FutureState::CANCELLED => "CANCELLED",
        }
    }
}
