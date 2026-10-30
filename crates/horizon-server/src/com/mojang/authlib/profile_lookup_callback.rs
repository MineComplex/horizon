//! Port of `com.mojang.authlib.ProfileLookupCallback`.
use crate::prelude::*;
use crate::java::lang::Throwable;

pub trait ProfileLookupCallback: JObject {
    fn on_profile_lookup_succeeded(&self, profile_name: &str, profile_id: Uuid);
    fn on_profile_lookup_failed(&self, profile_name: &str, exception: Throwable);
}
