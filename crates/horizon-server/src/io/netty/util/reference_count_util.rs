//! Port of `io.netty.util.ReferenceCountUtil`: reference counting on any message object.
use crate::io::netty::util::reference_counted::ReferenceCounted;
use crate::prelude::*;

pub struct ReferenceCountUtil;

impl ReferenceCountUtil {
    fn counted(msg: &JObjectRef) -> Option<JRef<dyn ReferenceCounted>> {
        cast::<dyn ReferenceCounted, dyn JObject>(msg)
    }
    /// `retain(msg)`: retains `msg` if it is reference counted; returns it.
    pub fn retain(msg: JObjectRef) -> JObjectRef {
        if let Some(rc) = Self::counted(&msg) {
            rc.retain();
        }
        msg
    }
    pub fn retain_2(msg: JObjectRef, increment: i32) -> JObjectRef {
        if let Some(rc) = Self::counted(&msg) {
            rc.retain_1(increment);
        }
        msg
    }
    pub fn touch(msg: JObjectRef) -> JObjectRef {
        if let Some(rc) = Self::counted(&msg) {
            rc.touch();
        }
        msg
    }
    pub fn touch_2(msg: JObjectRef, hint: JObjectRef) -> JObjectRef {
        if let Some(rc) = Self::counted(&msg) {
            rc.touch_1(hint);
        }
        msg
    }
    /// `release(msg)`: releases `msg` if it is reference counted; false otherwise.
    pub fn release(msg: &JObjectRef) -> bool {
        match Self::counted(msg) {
            Some(rc) => rc.release(),
            None => false,
        }
    }
    pub fn release_2(msg: &JObjectRef, decrement: i32) -> bool {
        match Self::counted(msg) {
            Some(rc) => rc.release_1(decrement),
            None => false,
        }
    }
    /// `safeRelease(msg)`: release, logging instead of throwing.
    pub fn safe_release(msg: &JObjectRef) {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Self::release(msg)));
        if r.is_err() {
            log::warn!("Failed to release a message: {}", msg.class_name());
        }
    }
    pub fn safe_release_2(msg: &JObjectRef, decrement: i32) {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::release_2(msg, decrement)
        }));
        if r.is_err() {
            log::warn!(
                "Failed to release a message: {} (decrement: {decrement})",
                msg.class_name()
            );
        }
    }
    /// `refCnt(msg)`: the count, or -1 when `msg` is not reference counted.
    pub fn ref_cnt(msg: &JObjectRef) -> i32 {
        match Self::counted(msg) {
            Some(rc) => rc.ref_cnt(),
            None => -1,
        }
    }
}
