//! What the `java.util.concurrent` port needs from `java.lang` (`Throwable`, `Thread`, `Runnable`), in one
//! place: every call this package makes into those classes goes through here, so the port of
//! `java.lang` only has to match these few lines.
use crate::java::lang::{Runnable, Thread, Throwable};
use crate::prelude::*;
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

// ---- Throwable

/// `new <class>(message, cause)` for the exceptions this package throws (`CompletionException`,
/// `ExecutionException`, `CancellationException`, `TimeoutException`, `RejectedExecutionException`,
/// `IllegalStateException`...). `class` is the Java class's binary name.
pub fn new_exception(
    class: &'static str,
    message: Option<String>,
    cause: Option<Throwable>,
) -> Throwable {
    // PORT: java.lang.Throwable's constructor for a named exception class (java.lang port)
    Throwable::of_class(class, message, cause)
}

/// The Java class name of `t` (`t.getClass().getName()`).
pub fn class_name(t: &Throwable) -> String {
    // PORT: java.lang.Throwable class name accessor (java.lang port)
    t.get_class_name()
}

/// `t.getCause()`.
pub fn cause(t: &Throwable) -> Option<Throwable> {
    // PORT: java.lang.Throwable.getCause (java.lang port)
    t.get_cause()
}

/// A Rust panic payload as the `Throwable` Java would have thrown (a panic carrying a `Throwable`
/// is that throwable; any other panic is a `RuntimeException` with its message).
pub fn from_panic(payload: Box<dyn Any + Send>) -> Throwable {
    match payload.downcast::<Throwable>() {
        Ok(t) => *t,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
                Some(s.to_string())
            } else {
                payload.downcast_ref::<String>().cloned()
            };
            new_exception("java.lang.RuntimeException", msg, None)
        }
    }
}

/// Java's `throw t` for an unchecked exception: unwinds with the throwable as the panic payload.
pub fn throw(t: Throwable) -> ! {
    std::panic::panic_any(t)
}

/// `try { f() } catch (Throwable t) { .. }`.
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, Throwable> {
    catch_unwind(AssertUnwindSafe(f)).map_err(from_panic)
}

pub fn is_instance(t: &Throwable, class: &str) -> bool {
    class_name(t) == class
}

pub const COMPLETION_EXCEPTION: &str = "java.util.concurrent.CompletionException";
pub const EXECUTION_EXCEPTION: &str = "java.util.concurrent.ExecutionException";
pub const CANCELLATION_EXCEPTION: &str = "java.util.concurrent.CancellationException";
pub const TIMEOUT_EXCEPTION: &str = "java.util.concurrent.TimeoutException";
pub const REJECTED_EXECUTION_EXCEPTION: &str = "java.util.concurrent.RejectedExecutionException";
pub const BROKEN_BARRIER_EXCEPTION: &str = "java.util.concurrent.BrokenBarrierException";
pub const INTERRUPTED_EXCEPTION: &str = "java.lang.InterruptedException";
pub const ILLEGAL_STATE_EXCEPTION: &str = "java.lang.IllegalStateException";
pub const ILLEGAL_ARGUMENT_EXCEPTION: &str = "java.lang.IllegalArgumentException";
pub const NULL_POINTER_EXCEPTION: &str = "java.lang.NullPointerException";
pub const NO_SUCH_ELEMENT_EXCEPTION: &str = "java.util.NoSuchElementException";
pub const UNSUPPORTED_OPERATION_EXCEPTION: &str = "java.lang.UnsupportedOperationException";

/// `throw new IllegalStateException(msg)`.
pub fn illegal_state(msg: impl Into<String>) -> ! {
    throw(new_exception(
        ILLEGAL_STATE_EXCEPTION,
        Some(msg.into()),
        None,
    ))
}

/// `throw new IllegalArgumentException(msg)`.
pub fn illegal_argument(msg: impl Into<String>) -> ! {
    throw(new_exception(
        ILLEGAL_ARGUMENT_EXCEPTION,
        Some(msg.into()),
        None,
    ))
}

// ---- Runnable

/// A `Runnable` made from a closure that runs at most once (lambdas handed to executors).
pub struct FnOnceRunnable(parking_lot::Mutex<Option<Box<dyn FnOnce() + Send>>>);
impl Runnable for FnOnceRunnable {
    fn run(&self) {
        let f = self.0.lock().take();
        if let Some(f) = f {
            f()
        }
    }
}
crate::jobject!(FnOnceRunnable: dyn Runnable);

/// `() -> { ... }` as a `Runnable` that runs once.
pub fn runnable(f: impl FnOnce() + Send + 'static) -> JRef<dyn Runnable> {
    Arc::new(FnOnceRunnable(parking_lot::Mutex::new(Some(Box::new(f)))))
}

/// A `Runnable` made from a closure that can run many times.
pub struct FnRunnable(pub Box<dyn Fn() + Send + Sync>);
impl Runnable for FnRunnable {
    fn run(&self) {
        (self.0)()
    }
}
crate::jobject!(FnRunnable: dyn Runnable);

pub fn runnable_fn(f: impl Fn() + Send + Sync + 'static) -> JRef<dyn Runnable> {
    Arc::new(FnRunnable(Box::new(f)))
}

// ---- Thread

/// `new Thread(target, name)`, daemon as given.
pub fn new_thread(target: JRef<dyn Runnable>, name: &str, daemon: bool) -> Thread {
    // PORT: java.lang.Thread(Runnable, String) + setDaemon (java.lang port)
    let t = Thread::new_2(target, name);
    t.set_daemon(daemon);
    t
}

/// `thread.start()`.
pub fn start(thread: &Thread) {
    // PORT: java.lang.Thread.start (java.lang port)
    thread.start();
}

/// `thread.getName()`.
pub fn thread_name(thread: &Thread) -> String {
    // PORT: java.lang.Thread.getName (java.lang port)
    thread.get_name()
}

/// `LockSupport.unpark(thread)`: wakes the OS thread behind a Java `Thread`.
pub fn unpark(thread: &Thread) {
    // PORT: the std::thread::Thread behind a java.lang.Thread (java.lang port)
    if let Some(t) = thread.native_thread() {
        t.unpark();
    }
}

/// `Thread.currentThread()`.
pub fn current_thread() -> Thread {
    // PORT: java.lang.Thread.currentThread (java.lang port)
    Thread::current_thread()
}

/// `System.nanoTime()`.
pub fn nano_time() -> i64 {
    static START: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);
    // offset so that values look like Java's (an arbitrary origin)
    START.elapsed().as_nanos() as i64 + 1_000_000_000_000
}

/// `Runtime.getRuntime().availableProcessors()`.
pub fn available_processors() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(1)
}

/// Waits on `cv` until `deadline` (a `nano_time()` value); returns false once the deadline has passed.
pub fn wait_until<T>(
    cv: &parking_lot::Condvar,
    guard: &mut parking_lot::MutexGuard<'_, T>,
    deadline: i64,
) -> bool {
    let now = nano_time();
    if now >= deadline {
        return false;
    }
    let _ = cv.wait_for(
        guard,
        std::time::Duration::from_nanos((deadline - now) as u64),
    );
    true
}
