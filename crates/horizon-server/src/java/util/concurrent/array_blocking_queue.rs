//! Port of `java.util.concurrent.ArrayBlockingQueue`: a bounded FIFO (same behaviour as a bounded
//! `LinkedBlockingQueue`).
use crate::java::lang::Throwable;
use crate::java::util::concurrent::linked_blocking_queue::{QueueCore, deadline};
use crate::java::util::concurrent::{BlockingQueue, BlockingQueueTrait, TimeUnit};
use crate::prelude::*;

pub struct ArrayBlockingQueue<E: JGeneric = JObjectRef>(pub JRef<QueueCore<E>>);
impl<E: JGeneric> Clone for ArrayBlockingQueue<E> {
    fn clone(&self) -> Self {
        ArrayBlockingQueue(self.0.clone())
    }
}
impl<E: JGeneric> ArrayBlockingQueue<E> {
    /// `new ArrayBlockingQueue<>(capacity)`.
    pub fn new(capacity: i32) -> Self {
        ArrayBlockingQueue(Arc::new(QueueCore::new(capacity)))
    }
    /// `new ArrayBlockingQueue<>(capacity, fair)`.
    pub fn new_2(capacity: i32, _fair: bool) -> Self {
        Self::new(capacity)
    }
    pub fn as_blocking_queue(&self) -> BlockingQueue<E> {
        BlockingQueue(Arc::new(self.clone()))
    }
}
impl<E: JGeneric> From<ArrayBlockingQueue<E>> for BlockingQueue<E> {
    fn from(q: ArrayBlockingQueue<E>) -> Self {
        BlockingQueue(Arc::new(q))
    }
}
impl<E: JGeneric> BlockingQueueTrait<E> for ArrayBlockingQueue<E> {
    fn offer(&self, e: E) -> bool {
        self.0.offer_at(e, false)
    }
    fn offer_3(&self, e: E, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable> {
        Ok(self.0.offer_wait(e, false, Some(deadline(timeout, unit))))
    }
    fn put(&self, e: E) -> Result<(), Throwable> {
        self.0.offer_wait(e, false, None);
        Ok(())
    }
    fn poll(&self) -> Option<E> {
        self.0.poll_at(true)
    }
    fn poll_2(&self, timeout: i64, unit: TimeUnit) -> Result<Option<E>, Throwable> {
        Ok(self.0.poll_wait(true, Some(deadline(timeout, unit))))
    }
    fn take(&self) -> Result<E, Throwable> {
        Ok(self.0.poll_wait(true, None).expect("take"))
    }
    fn peek(&self) -> Option<E> {
        self.0.peek_at(true)
    }
    fn size(&self) -> i32 {
        self.0.size()
    }
    fn remaining_capacity(&self) -> i32 {
        self.0.remaining_capacity()
    }
    fn clear(&self) {
        self.0.clear()
    }
    fn remove_if(&self, filter: &mut dyn FnMut(&E) -> bool) -> bool {
        self.0.remove_if(filter)
    }
    fn to_vec(&self) -> Vec<E> {
        self.0.to_vec()
    }
}
