//! Port of `java.util.concurrent.PriorityBlockingQueue`: an unbounded priority queue (least element
//! first, by the comparator or `Ord`).
use crate::java::lang::Throwable;
use crate::java::util::concurrent::linked_blocking_queue::deadline;
use crate::java::util::concurrent::support;
use crate::java::util::concurrent::{BlockingQueue, BlockingQueueTrait, TimeUnit};
use crate::prelude::*;
use parking_lot::{Condvar, Mutex};
use std::cmp::Ordering;

pub type Comparator<E> = JRef<dyn Fn(&E, &E) -> Ordering + Send + Sync>;

pub struct PriorityBlockingQueueInner<E> {
    /// A binary min-heap ordered by `cmp`.
    heap: Mutex<Vec<E>>,
    cmp: Comparator<E>,
    not_empty: Condvar,
}

pub struct PriorityBlockingQueue<E: JGeneric = JObjectRef>(pub JRef<PriorityBlockingQueueInner<E>>);
impl<E: JGeneric> Clone for PriorityBlockingQueue<E> {
    fn clone(&self) -> Self {
        PriorityBlockingQueue(self.0.clone())
    }
}

impl<E: JGeneric + Ord> PriorityBlockingQueue<E> {
    /// `new PriorityBlockingQueue<>()` (natural ordering).
    pub fn new() -> Self {
        Self::new_2(11, Arc::new(|a: &E, b: &E| a.cmp(b)))
    }
}
impl<E: JGeneric + Ord> Default for PriorityBlockingQueue<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: JGeneric> PriorityBlockingQueue<E> {
    /// `new PriorityBlockingQueue<>(initialCapacity, comparator)`.
    pub fn new_2(initial_capacity: i32, comparator: Comparator<E>) -> Self {
        if initial_capacity < 1 {
            support::illegal_argument("initialCapacity < 1");
        }
        PriorityBlockingQueue(Arc::new(PriorityBlockingQueueInner {
            heap: Mutex::new(Vec::with_capacity(initial_capacity as usize)),
            cmp: comparator,
            not_empty: Condvar::new(),
        }))
    }
    pub fn as_blocking_queue(&self) -> BlockingQueue<E> {
        BlockingQueue(Arc::new(self.clone()))
    }
    pub fn comparator(&self) -> Comparator<E> {
        self.0.cmp.clone()
    }

    fn sift_up(cmp: &Comparator<E>, h: &mut [E], mut k: usize) {
        while k > 0 {
            let parent = (k - 1) >> 1;
            if cmp(&h[k], &h[parent]) != Ordering::Less {
                break;
            }
            h.swap(k, parent);
            k = parent;
        }
    }
    fn sift_down(cmp: &Comparator<E>, h: &mut [E], mut k: usize) {
        let n = h.len();
        loop {
            let mut child = (k << 1) + 1;
            if child >= n {
                break;
            }
            let right = child + 1;
            if right < n && cmp(&h[child], &h[right]) == Ordering::Greater {
                child = right;
            }
            if cmp(&h[k], &h[child]) != Ordering::Greater {
                break;
            }
            h.swap(k, child);
            k = child;
        }
    }
    fn dequeue(&self, h: &mut Vec<E>) -> Option<E> {
        if h.is_empty() {
            return None;
        }
        let last = h.len() - 1;
        h.swap(0, last);
        let r = h.pop();
        Self::sift_down(&self.0.cmp, h, 0);
        r
    }
    fn wait_dequeue(&self, deadline: Option<i64>) -> Option<E> {
        let mut h = self.0.heap.lock();
        loop {
            if let Some(e) = self.dequeue(&mut h) {
                return Some(e);
            }
            match deadline {
                None => self.0.not_empty.wait(&mut h),
                Some(d) => {
                    if !support::wait_until(&self.0.not_empty, &mut h, d) {
                        return None;
                    }
                }
            }
        }
    }
}
impl<E: JGeneric> From<PriorityBlockingQueue<E>> for BlockingQueue<E> {
    fn from(q: PriorityBlockingQueue<E>) -> Self {
        BlockingQueue(Arc::new(q))
    }
}
impl<E: JGeneric> BlockingQueueTrait<E> for PriorityBlockingQueue<E> {
    fn offer(&self, e: E) -> bool {
        let mut h = self.0.heap.lock();
        h.push(e);
        let k = h.len() - 1;
        Self::sift_up(&self.0.cmp, &mut h, k);
        drop(h);
        self.0.not_empty.notify_one();
        true
    }
    fn offer_3(&self, e: E, _timeout: i64, _unit: TimeUnit) -> Result<bool, Throwable> {
        Ok(self.offer(e))
    }
    fn put(&self, e: E) -> Result<(), Throwable> {
        self.offer(e);
        Ok(())
    }
    fn poll(&self) -> Option<E> {
        let mut h = self.0.heap.lock();
        self.dequeue(&mut h)
    }
    fn poll_2(&self, timeout: i64, unit: TimeUnit) -> Result<Option<E>, Throwable> {
        Ok(self.wait_dequeue(Some(deadline(timeout, unit))))
    }
    fn take(&self) -> Result<E, Throwable> {
        Ok(self.wait_dequeue(None).expect("take"))
    }
    fn peek(&self) -> Option<E> {
        self.0.heap.lock().first().cloned()
    }
    fn size(&self) -> i32 {
        self.0.heap.lock().len() as i32
    }
    fn remaining_capacity(&self) -> i32 {
        i32::MAX
    }
    fn clear(&self) {
        self.0.heap.lock().clear();
    }
    fn remove_if(&self, filter: &mut dyn FnMut(&E) -> bool) -> bool {
        let mut h = self.0.heap.lock();
        let before = h.len();
        let kept: Vec<E> = std::mem::take(&mut *h).into_iter().filter(|e| !filter(e)).collect();
        for e in kept {
            h.push(e);
            let k = h.len() - 1;
            Self::sift_up(&self.0.cmp, &mut h, k);
        }
        h.len() != before
    }
    /// The elements in heap order (as Java's iterator: no particular order).
    fn to_vec(&self) -> Vec<E> {
        self.0.heap.lock().clone()
    }
}
