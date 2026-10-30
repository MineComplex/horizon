//! Port of `com.mojang.authlib.services.ServicesKeyInfo`.
use crate::prelude::*;
use crate::com::mojang::authlib::properties::Property;
use crate::java::security::Signature;

pub trait ServicesKeyInfo: JObject {
    fn key_bit_count(&self) -> i32;
    fn signature_bit_count(&self) -> i32 {
        self.key_bit_count()
    }
    fn signature(&self) -> Signature;
    fn validate_property(&self, property: &Property) -> bool;
}
