//! Port of `com.mojang.authlib.services.ServicesKeySet` (a functional interface). The skeleton's form is a
//! value handle: `ServicesKeySet` wraps the `keys` function; `services_key_set::EMPTY` and
//! `ServicesKeySet::lazy` as in Java, `ServicesKeySet::new(|type_| ..)` for a lambda.
use crate::prelude::*;
use crate::com::mojang::authlib::services::ServicesKeyInfo;
use crate::com::mojang::authlib::services::ServicesKeyType;

#[derive(Clone)]
pub struct ServicesKeySet(pub Arc<dyn Fn(ServicesKeyType) -> Vec<JRef<dyn ServicesKeyInfo>> + Send + Sync>);

pub static EMPTY: LazyLock<ServicesKeySet> = LazyLock::new(|| ServicesKeySet::new(|_type| Vec::new()));

impl ServicesKeySet {
    pub fn new(keys: impl Fn(ServicesKeyType) -> Vec<JRef<dyn ServicesKeyInfo>> + Send + Sync + 'static) -> Self {
        ServicesKeySet(Arc::new(keys))
    }

    pub fn lazy(supplier: impl Fn() -> ServicesKeySet + Send + Sync + 'static) -> Self {
        ServicesKeySet::new(move |type_| supplier().keys(type_))
    }

    pub fn keys(&self, type_: ServicesKeyType) -> Vec<JRef<dyn ServicesKeyInfo>> {
        (self.0)(type_)
    }
}
