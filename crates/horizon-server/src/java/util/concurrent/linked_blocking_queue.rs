//! Port of `java.util.concurrent.LinkedBlockingQueue` (also the core of `ArrayBlockingQueue` and
//! `LinkedBlockingDeque`): an optionally bounded FIFO with blocking `put`/`take`.
use crate::java::lang::Throwable;
use crate::java::util::concurrent::support;
use crate::java::util::concurrent::{BlockingQueue, BlockingQueueTrait, TimeUnit};
use crate::prelude::*;
use parking_lot::{Condvar, Mutex};

/// A bounded deque with blocking waits on both ends (shared by the linked/array queues).
pub struct QueueCore<E> {
    pub(crate) items: Mutex<VecDeque<E>>,
    not_empty: Condvar,
    not_full: Condvar,
    capacity: usize,
}

impl<E: JGeneric> QueueCore<E> {
    pub fn new(capacity: i32) -> Self {
        if capacity <= 0 {
            support::illegal_argument("capacity <= 0");
        }
        QueueCore { items: Mutex::new(VecDeque::new()), not_empty: Condvar::new(), not_full: Condvar::new(), capacity: capacity as usize }
    }
    pub fn offer_at(&self, e: E, front: bool) -> bool {
        let mut q = self.items.lock();
        if q.len() >= self.capacity {
            return false;
        }
        if front {
            q.push_front(e);
        } else {
            q.push_back(e);
        }
        drop(q);
        self.not_empty.notify_one();
        true
    }
    pub fn offer_wait(&self, e: E, front: bool, deadline: Option<i64>) -> bool {
        let mut q = self.items.lock();
        while q.len() >= self.capacity {
            match deadline {
                None => self.not_full.wait(&mut q),
                Some(d) => {
                    if !support::wait_until(&self.not_full, &mut q, d) {
                        return false;
                    }
                }
            }
        }
        if front {
            q.push_front(e);
        } else {
            q.push_back(e);
        }
        drop(q);
        self.not_empty.notify_one();
        true
    }
    pub fn poll_at(&self, front: bool) -> Option<E> {
        let mut q = self.items.lock();
        let r = if front { q.pop_front() } else { q.pop_back() };
        drop(q);
        if r.is_some() {
            self.not_full.notify_one();
        }
        r
    }
    pub fn poll_wait(&self, front: bool, deadline: Option<i64>) -> Option<E> {
        let mut q = self.items.lock();
        loop {
            let r = if front { q.pop_front() } else { q.pop_back() };
            if let Some(e) = r {
                drop(q);
                self.not_full.notify_one();
                return Some(e);
            }
            match deadline {
                None => self.not_empty.wait(&mut q),
                Some(d) => {
                    if !support::wait_until(&self.not_empty, &mut q, d) {
                        return None;
                    }
                }
            }
        }
    }
    pub fn peek_at(&self, front: bool) -> Option<E> {
        let q = self.items.lock();
        if front { q.front().cloned() } else { q.back().cloned() }
    }
    pub fn size(&self) -> i32 {
        self.items.lock().len() as i32
    }
    pub fn remaining_capacity(&self) -> i32 {
        (self.capacity - self.items.lock().len()).min(i32::MAX as usize) as i32
    }
    pub fn clear(&self) {
        self.items.lock().clear();
        self.not_full.notify_all();
    }
    pub fn remove_if(&self, filter: &mut dyn FnMut(&E) -> bool) -> bool {
        let mut q = self.items.lock();
        let before = q.len();
        q.retain(|e| !filter(e));
        let removed = q.len() != before;
        drop(q);
        if removed {
            self.not_full.notify_all();
        }
        removed
    }
    pub fn to_vec(&self) -> Vec<E> {
        self.items.lock().iter().cloned().collect()
    }
}

pub(crate) fn deadline(timeout: i64, unit: TimeUnit) -> i64 {
    support::nano_time().saturating_add(unit.to_nanos(timeout))
}

/// `LinkedBlockingQueue<E>`: a value handle; clones share the queue.
pub struct LinkedBlockingQueue<E: JGeneric = JObjectRef>(pub JRef<QueueCore<E>>);
impl<E: JGeneric> Clone for LinkedBlockingQueue<E> {
    fn clone(&self) -> Self {
        LinkedBlockingQueue(self.0.clone())
    }
}
impl<E: JGeneric> Default for LinkedBlockingQueue<E> {
    fn default() -> Self {
        Self::new()
    }
}
impl<E: JGeneric> LinkedBlockingQueue<E> {
    /// `new LinkedBlockingQueue<>()` (capacity `Integer.MAX_VALUE`).
    pub fn new() -> Self {
        LinkedBlockingQueue(Arc::new(QueueCore::new(i32::MAX)))
    }
    /// `new LinkedBlockingQueue<>(capacity)`.
    pub fn new_1(capacity: i32) -> Self {
        LinkedBlockingQueue(Arc::new(QueueCore::new(capacity)))
    }
    /// `new LinkedBlockingQueue<>(collection)`.
    pub fn new_1_2(c: Vec<E>) -> Self {
        let ret = Self::new();
        for e in c {
            ret.offer(e);
        }
        ret
    }
    /// The queue as a `BlockingQueue<E>`.
    pub fn as_blocking_queue(&self) -> BlockingQueue<E> {
        BlockingQueue(Arc::new(self.clone()))
    }
}
impl<E: JGeneric> From<LinkedBlockingQueue<E>> for BlockingQueue<E> {
    fn from(q: LinkedBlockingQueue<E>) -> Self {
        BlockingQueue(Arc::new(q))
    }
}

impl<E: JGeneric> BlockingQueueTrait<E> for LinkedBlockingQueue<E> {
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
