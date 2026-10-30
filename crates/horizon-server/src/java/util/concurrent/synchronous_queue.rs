//! Port of `java.util.concurrent.SynchronousQueue`: no capacity; `offer` succeeds only when a taker is
//! waiting, `put` waits for one.
use crate::java::lang::Throwable;
use crate::java::util::concurrent::linked_blocking_queue::deadline;
use crate::java::util::concurrent::support;
use crate::java::util::concurrent::{BlockingQueue, BlockingQueueTrait, TimeUnit};
use crate::prelude::*;
use parking_lot::{Condvar, Mutex};

struct SqState<E> {
    /// Takers currently blocked in `poll`/`take` that no element has been handed to yet.
    waiting_takers: usize,
    /// Elements handed off to waiting takers, not yet picked up.
    handed: VecDeque<E>,
    /// Elements whose `put` is waiting for a taker.
    waiting_puts: VecDeque<(u64, E)>,
    /// Ids of `put`s whose element has been taken.
    taken: HashSet<u64>,
    next_id: u64,
}

pub struct SynchronousQueueInner<E> {
    state: Mutex<SqState<E>>,
    takers: Condvar,
    putters: Condvar,
}

pub struct SynchronousQueue<E: JGeneric = JObjectRef>(pub JRef<SynchronousQueueInner<E>>);
impl<E: JGeneric> Clone for SynchronousQueue<E> {
    fn clone(&self) -> Self {
        SynchronousQueue(self.0.clone())
    }
}
impl<E: JGeneric> Default for SynchronousQueue<E> {
    fn default() -> Self {
        Self::new()
    }
}
impl<E: JGeneric> SynchronousQueue<E> {
    /// `new SynchronousQueue<>()`.
    pub fn new() -> Self {
        SynchronousQueue(Arc::new(SynchronousQueueInner {
            state: Mutex::new(SqState {
                waiting_takers: 0,
                handed: VecDeque::new(),
                waiting_puts: VecDeque::new(),
                taken: HashSet::new(),
                next_id: 0,
            }),
            takers: Condvar::new(),
            putters: Condvar::new(),
        }))
    }
    /// `new SynchronousQueue<>(fair)`.
    pub fn new_1(_fair: bool) -> Self {
        Self::new()
    }
    pub fn as_blocking_queue(&self) -> BlockingQueue<E> {
        BlockingQueue(Arc::new(self.clone()))
    }

    fn put_wait(&self, e: E, deadline: Option<i64>) -> bool {
        let inner = &*self.0;
        let mut s = inner.state.lock();
        if s.waiting_takers > s.handed.len() {
            s.handed.push_back(e);
            drop(s);
            inner.takers.notify_all();
            return true;
        }
        let id = s.next_id;
        s.next_id += 1;
        s.waiting_puts.push_back((id, e));
        inner.takers.notify_all();
        loop {
            if s.taken.remove(&id) {
                return true;
            }
            let timed_out = match deadline {
                None => {
                    inner.putters.wait(&mut s);
                    false
                }
                Some(d) => !support::wait_until(&inner.putters, &mut s, d),
            };
            if timed_out {
                if s.taken.remove(&id) {
                    return true;
                }
                s.waiting_puts.retain(|(i, _)| *i != id);
                return false;
            }
        }
    }

    fn take_wait(&self, deadline: Option<i64>, wait: bool) -> Option<E> {
        let inner = &*self.0;
        let mut s = inner.state.lock();
        if let Some((id, e)) = s.waiting_puts.pop_front() {
            s.taken.insert(id);
            drop(s);
            inner.putters.notify_all();
            return Some(e);
        }
        if !wait {
            return None;
        }
        s.waiting_takers += 1;
        loop {
            if let Some(e) = s.handed.pop_front() {
                s.waiting_takers -= 1;
                return Some(e);
            }
            if let Some((id, e)) = s.waiting_puts.pop_front() {
                s.waiting_takers -= 1;
                s.taken.insert(id);
                drop(s);
                inner.putters.notify_all();
                return Some(e);
            }
            let timed_out = match deadline {
                None => {
                    inner.takers.wait(&mut s);
                    false
                }
                Some(d) => !support::wait_until(&inner.takers, &mut s, d),
            };
            if timed_out {
                if let Some(e) = s.handed.pop_front() {
                    s.waiting_takers -= 1;
                    return Some(e);
                }
                s.waiting_takers -= 1;
                return None;
            }
        }
    }
}
impl<E: JGeneric> From<SynchronousQueue<E>> for BlockingQueue<E> {
    fn from(q: SynchronousQueue<E>) -> Self {
        BlockingQueue(Arc::new(q))
    }
}
impl<E: JGeneric> BlockingQueueTrait<E> for SynchronousQueue<E> {
    fn offer(&self, e: E) -> bool {
        let inner = &*self.0;
        let mut s = inner.state.lock();
        if s.waiting_takers > s.handed.len() {
            s.handed.push_back(e);
            drop(s);
            inner.takers.notify_all();
            true
        } else {
            false
        }
    }
    fn offer_3(&self, e: E, timeout: i64, unit: TimeUnit) -> Result<bool, Throwable> {
        Ok(self.put_wait(e, Some(deadline(timeout, unit))))
    }
    fn put(&self, e: E) -> Result<(), Throwable> {
        self.put_wait(e, None);
        Ok(())
    }
    fn poll(&self) -> Option<E> {
        self.take_wait(None, false)
    }
    fn poll_2(&self, timeout: i64, unit: TimeUnit) -> Result<Option<E>, Throwable> {
        Ok(self.take_wait(Some(deadline(timeout, unit)), true))
    }
    fn take(&self) -> Result<E, Throwable> {
        Ok(self.take_wait(None, true).expect("take"))
    }
    fn peek(&self) -> Option<E> {
        None
    }
    fn size(&self) -> i32 {
        0
    }
    fn remaining_capacity(&self) -> i32 {
        0
    }
    fn clear(&self) {}
    fn remove_if(&self, _filter: &mut dyn FnMut(&E) -> bool) -> bool {
        false
    }
    fn to_vec(&self) -> Vec<E> {
        Vec::new()
    }
}
