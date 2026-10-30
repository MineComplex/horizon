//! Port of `io.netty.util.Attribute<T>`: a value slot on an `AttributeMap` (a channel). A cheap handle:
//! clones share the slot. Java's `null` is `None`.
use crate::io::netty::util::attribute_key::AttributeKey;
use crate::prelude::*;
use std::any::Any;

pub(crate) type Slot = Arc<parking_lot::Mutex<Option<Box<dyn Any + Send + Sync>>>>;

pub struct Attribute<T = JObjectRef> {
    key: AttributeKey<T>,
    slot: Slot,
}

impl<T> Clone for Attribute<T> {
    fn clone(&self) -> Self {
        Attribute {
            key: self.key.clone(),
            slot: self.slot.clone(),
        }
    }
}

impl<T: JGeneric> Attribute<T> {
    pub(crate) fn new(key: AttributeKey<T>, slot: Slot) -> Self {
        Attribute { key, slot }
    }
    fn read(v: &Option<Box<dyn Any + Send + Sync>>) -> Option<T> {
        v.as_ref().and_then(|b| b.downcast_ref::<T>()).cloned()
    }
    pub fn key(&self) -> AttributeKey<T> {
        self.key.clone()
    }
    /// `get()`: the value, `None` for Java's null.
    pub fn get(&self) -> Option<T> {
        Self::read(&self.slot.lock())
    }
    /// `set(value)`.
    pub fn set(&self, value: Option<T>) {
        *self.slot.lock() = value.map(|v| Box::new(v) as Box<dyn Any + Send + Sync>);
    }
    pub fn get_and_set(&self, value: Option<T>) -> Option<T> {
        let mut g = self.slot.lock();
        let old = Self::read(&g);
        *g = value.map(|v| Box::new(v) as Box<dyn Any + Send + Sync>);
        old
    }
    /// `setIfAbsent(value)`: the old value, or `None` when `value` was set.
    pub fn set_if_absent(&self, value: T) -> Option<T> {
        let mut g = self.slot.lock();
        let old = Self::read(&g);
        if old.is_none() {
            *g = Some(Box::new(value));
        }
        old
    }
    pub fn get_and_remove(&self) -> Option<T> {
        self.get_and_set(None)
    }
    pub fn remove(&self) {
        self.set(None)
    }
    /// `compareAndSet(old, new)`: `same` decides Java's reference equality for `T`.
    pub fn compare_and_set(
        &self,
        old_value: Option<T>,
        new_value: Option<T>,
        same: impl Fn(&Option<T>, &Option<T>) -> bool,
    ) -> bool {
        let mut g = self.slot.lock();
        let cur = Self::read(&g);
        if same(&cur, &old_value) {
            *g = new_value.map(|v| Box::new(v) as Box<dyn Any + Send + Sync>);
            true
        } else {
            false
        }
    }
}
