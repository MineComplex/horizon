//! Port of `java.util.concurrent.BlockingQueue`.
//!
//! `BlockingQueueTrait<E>` is the interface (the queues of this package implement it, and so may in-tree
//! classes that extend one, like Paper's `ScalingThreadPool.Queue`); `BlockingQueue<E>` is the value
//! handle the skeleton passes around (a shared `JRef<dyn BlockingQueueTrait<E>>`). The checked
//! `InterruptedException` of the blocking methods is the `Err` of their `Result`.
use crate::java::lang::Throwable;
use crate::java::util::concurrent::TimeUnit;
use crate::java::util::concurrent::support;
use crate::prelude::*;

pub trait BlockingQueueTrait<E>: Send + Sync {
    /// `offer(e)`: adds without blocking; false when full.
    fn offer(&self, e: E) -> bool;
    /// `offer(e, timeout, unit)`.
    fn offer_3(&self, e: E, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable>;
    /// `put(e)`: waits for space.
    fn put(&self, e: E) -> Result<(), Throwable>;
    /// `poll()`.
    fn poll(&self) -> Option<E>;
    /// `poll(timeout, unit)`.
    fn poll_2(&self, timeout: i64, unit: TimeUnit) -> Result<Option<E>, Throwable>;
    /// `take()`: waits for an element.
    fn take(&self) -> Result<E, Throwable>;
    /// `peek()`.
    fn peek(&self) -> Option<E>;
    fn size(&self) -> i32;
    fn remaining_capacity(&self) -> i32;
    fn clear(&self);
    /// `removeIf(filter)` (also `remove(o)`: `remove_if(&mut |e| e == o)`).
    fn remove_if(&self, filter: &mut dyn FnMut(&E) -> bool) -> bool;
    /// `drainTo(c)`.
    fn drain_to(&self, c: &mut Vec<E>) -> i32 {
        self.drain_to_2(c, i32::MAX)
    }
    /// `drainTo(c, maxElements)`.
    fn drain_to_2(&self, c: &mut Vec<E>, max_elements: i32) -> i32 {
        let mut n = 0;
        while n < max_elements {
            match self.poll() {
                Some(e) => {
                    c.push(e);
                    n += 1;
                }
                None => break,
            }
        }
        n
    }
    /// `toArray()` / iteration: a snapshot in queue order.
    fn to_vec(&self) -> Vec<E>;
    fn is_empty(&self) -> bool {
        self.size() == 0
    }
    /// `add(e)`: `IllegalStateException("Queue full")` when full.
    fn add(&self, e: E) -> bool {
        if self.offer(e) {
            true
        } else {
            support::illegal_state("Queue full")
        }
    }
    /// `remove()`: `NoSuchElementException` when empty.
    fn remove(&self) -> E {
        match self.poll() {
            Some(e) => e,
            None => support::throw(support::new_exception(support::NO_SUCH_ELEMENT_EXCEPTION, None, None)),
        }
    }
    /// `element()`.
    fn element(&self) -> E {
        match self.peek() {
            Some(e) => e,
            None => support::throw(support::new_exception(support::NO_SUCH_ELEMENT_EXCEPTION, None, None)),
        }
    }
}

/// `BlockingQueue<E>` as a value: a shared reference to any blocking queue.
pub struct BlockingQueue<E: JGeneric = JObjectRef>(pub JRef<dyn BlockingQueueTrait<E>>);
impl<E: JGeneric> Clone for BlockingQueue<E> {
    fn clone(&self) -> Self {
        BlockingQueue(self.0.clone())
    }
}
impl<E: JGeneric> std::ops::Deref for BlockingQueue<E> {
    type Target = dyn BlockingQueueTrait<E>;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
impl<E: JGeneric> BlockingQueue<E> {
    /// Wraps any queue implementation (Java: the upcast to `BlockingQueue`).
    pub fn of(queue: impl BlockingQueueTrait<E> + 'static) -> Self {
        BlockingQueue(Arc::new(queue))
    }
    /// `remove(o)` by equality.
    pub fn remove_1(&self, o: &E) -> bool
    where
        E: PartialEq,
    {
        let mut done = false;
        self.0.remove_if(&mut |e| {
            if !done && e == o {
                done = true;
                true
            } else {
                false
            }
        })
    }
    /// `contains(o)`.
    pub fn contains(&self, o: &E) -> bool
    where
        E: PartialEq,
    {
        self.0.to_vec().iter().any(|e| e == o)
    }
}
