//! Port of `java.util.concurrent.LinkedBlockingDeque`.
use crate::java::lang::Throwable;
use crate::java::util::concurrent::linked_blocking_queue::{QueueCore, deadline};
use crate::java::util::concurrent::support;
use crate::java::util::concurrent::{BlockingQueue, BlockingQueueTrait, TimeUnit};
use crate::prelude::*;

pub struct LinkedBlockingDeque<E: JGeneric = JObjectRef>(pub JRef<QueueCore<E>>);
impl<E: JGeneric> Clone for LinkedBlockingDeque<E> {
    fn clone(&self) -> Self {
        LinkedBlockingDeque(self.0.clone())
    }
}
impl<E: JGeneric> Default for LinkedBlockingDeque<E> {
    fn default() -> Self {
        Self::new()
    }
}
impl<E: JGeneric> LinkedBlockingDeque<E> {
    pub fn new() -> Self {
        LinkedBlockingDeque(Arc::new(QueueCore::new(i32::MAX)))
    }
    pub fn new_1(capacity: i32) -> Self {
        LinkedBlockingDeque(Arc::new(QueueCore::new(capacity)))
    }
    pub fn as_blocking_queue(&self) -> BlockingQueue<E> {
        BlockingQueue(Arc::new(self.clone()))
    }
    pub fn offer_first(&self, e: E) -> bool {
        self.0.offer_at(e, true)
    }
    pub fn offer_last(&self, e: E) -> bool {
        self.0.offer_at(e, false)
    }
    pub fn add_first(&self, e: E) {
        if !self.offer_first(e) {
            support::illegal_state("Deque full");
        }
    }
    pub fn add_last(&self, e: E) {
        if !self.offer_last(e) {
            support::illegal_state("Deque full");
        }
    }
    pub fn put_first(&self, e: E) -> Result<(), Throwable> {
        self.0.offer_wait(e, true, None);
        Ok(())
    }
    pub fn put_last(&self, e: E) -> Result<(), Throwable> {
        self.0.offer_wait(e, false, None);
        Ok(())
    }
    pub fn poll_first(&self) -> Option<E> {
        self.0.poll_at(true)
    }
    pub fn poll_last(&self) -> Option<E> {
        self.0.poll_at(false)
    }
    pub fn poll_first_2(&self, timeout: i64, unit: TimeUnit) -> Result<Option<E>, Throwable> {
        Ok(self.0.poll_wait(true, Some(deadline(timeout, unit))))
    }
    pub fn poll_last_2(&self, timeout: i64, unit: TimeUnit) -> Result<Option<E>, Throwable> {
        Ok(self.0.poll_wait(false, Some(deadline(timeout, unit))))
    }
    pub fn take_first(&self) -> Result<E, Throwable> {
        Ok(self.0.poll_wait(true, None).expect("take"))
    }
    pub fn take_last(&self) -> Result<E, Throwable> {
        Ok(self.0.poll_wait(false, None).expect("take"))
    }
    pub fn peek_first(&self) -> Option<E> {
        self.0.peek_at(true)
    }
    pub fn peek_last(&self) -> Option<E> {
        self.0.peek_at(false)
    }
    /// `push(e)` = `addFirst`.
    pub fn push(&self, e: E) {
        self.add_first(e)
    }
    /// `pop()` = `removeFirst`.
    pub fn pop(&self) -> E {
        match self.poll_first() {
            Some(e) => e,
            None => support::throw(support::new_exception(support::NO_SUCH_ELEMENT_EXCEPTION, None, None)),
        }
    }
}
impl<E: JGeneric> From<LinkedBlockingDeque<E>> for BlockingQueue<E> {
    fn from(q: LinkedBlockingDeque<E>) -> Self {
        BlockingQueue(Arc::new(q))
    }
}
impl<E: JGeneric> BlockingQueueTrait<E> for LinkedBlockingDeque<E> {
    fn offer(&self, e: E) -> bool {
        self.0.offer_at(e, false)
    }
    fn offer_3(&self, e: E, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable> {
        Ok(self.0.offer_wait(e, false, Some(deadline(timeout, unit))))
    }
    fn put(&self, e: E) -> Result<(), Throwable> {
        self.put_last(e)
    }
    fn poll(&self) -> Option<E> {
        self.0.poll_at(true)
    }
    fn poll_2(&self, timeout: i64, unit: TimeUnit) -> Result<Option<E>, Throwable> {
        self.poll_first_2(timeout, unit)
    }
    fn take(&self) -> Result<E, Throwable> {
        self.take_first()
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
