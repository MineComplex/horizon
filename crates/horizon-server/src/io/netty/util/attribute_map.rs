//! Port of `io.netty.util.AttributeMap` / `DefaultAttributeMap`: the attributes of a channel (or a
//! handler context, which Netty forwards to its channel).
use crate::io::netty::util::attribute::{Attribute, Slot};
use crate::io::netty::util::attribute_key::AttributeKey;
use crate::prelude::*;

#[derive(Default)]
pub struct DefaultAttributeMap {
    slots: parking_lot::Mutex<HashMap<i32, Slot>>,
}

impl DefaultAttributeMap {
    pub fn new() -> Self {
        Self::default()
    }
    /// `attr(key)`: the attribute for `key`, created empty on first use.
    pub fn attr<T: JGeneric>(&self, key: &AttributeKey<T>) -> Attribute<T> {
        let slot = self
            .slots
            .lock()
            .entry(key.id())
            .or_insert_with(|| Arc::new(parking_lot::Mutex::new(None)))
            .clone();
        Attribute::new(key.clone(), slot)
    }
    /// `hasAttr(key)`: whether the attribute exists and holds a value.
    pub fn has_attr<T: JGeneric>(&self, key: &AttributeKey<T>) -> bool {
        self.slots
            .lock()
            .get(&key.id())
            .is_some_and(|s| s.lock().is_some())
    }
}
