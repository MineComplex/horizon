//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct BlockingQueue<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for BlockingQueue<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Callable<A0 = JObjectRef>: Send + Sync {}
pub struct CompletableFuture<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for CompletableFuture<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ConcurrentLinkedDeque<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ConcurrentLinkedDeque<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ConcurrentLinkedQueue<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ConcurrentLinkedQueue<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct CountDownLatch(pub std::marker::PhantomData<()>);
impl Clone for CountDownLatch { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Executor: Send + Sync {}
pub struct ExecutorService(pub std::marker::PhantomData<()>);
impl Clone for ExecutorService { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Future<A0 = JObjectRef>: Send + Sync {}
pub struct FutureTask<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for FutureTask<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait RejectedExecutionHandler: Send + Sync {}
pub struct ScheduledExecutorService(pub std::marker::PhantomData<()>);
impl Clone for ScheduledExecutorService { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ScheduledFuture<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ScheduledFuture<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ScheduledThreadPoolExecutor(pub std::marker::PhantomData<()>);
impl Clone for ScheduledThreadPoolExecutor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Semaphore(pub std::marker::PhantomData<()>);
impl Clone for Semaphore { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait ThreadFactory: Send + Sync {}
pub struct ThreadPoolExecutor(pub std::marker::PhantomData<()>);
impl Clone for ThreadPoolExecutor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TimeUnit(pub std::marker::PhantomData<()>);
impl Clone for TimeUnit { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
