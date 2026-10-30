//! Port of `io.netty.util.ReferenceCounted`.
//!
//! Netty counts references by hand; the port keeps the counts (Rust frees memory itself), so a buffer
//! released to 0 is inaccessible exactly when Netty's would be. Java's `retain()`/`touch()` return `this`
//! for chaining; through this trait they return nothing (types call their own inherent versions to chain).
use crate::prelude::*;

pub trait ReferenceCounted: JObject {
    /// The reference count; 0 means deallocated.
    fn ref_cnt(&self) -> i32;
    /// Increases the reference count by 1.
    fn retain(&self);
    /// Increases the reference count by `increment`.
    fn retain_1(&self, increment: i32);
    /// Records the current access location (leak detection: a no-op in the port).
    fn touch(&self) {}
    fn touch_1(&self, hint: JObjectRef) {
        let _ = hint;
    }
    /// Decreases the count by 1; true when it reached 0.
    fn release(&self) -> bool;
    /// Decreases the count by `decrement`; true when it reached 0.
    fn release_1(&self, decrement: i32) -> bool;
}
