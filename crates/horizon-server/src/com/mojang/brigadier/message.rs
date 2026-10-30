//! Port of `com.mojang.brigadier.Message` (Brigadier 1.3.11).
use crate::prelude::*;

/// `interface Message`: NMS `Component` and `LiteralMessage` implement it.
pub trait Message: JObject {
    fn get_string(&self) -> String;
}

/// Java's `Objects.equals(a, b)` on two (nullable) messages: identity, else the same text.
pub fn messages_equal(a: &Option<JRef<dyn Message>>, b: &Option<JRef<dyn Message>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same(a, b) || a.get_string() == b.get_string(),
        _ => false,
    }
}
