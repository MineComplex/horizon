//! Port of `com.mojang.authlib.services.ServicesKeyType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServicesKeyType {
    PROFILE_PROPERTY,
    PROFILE_KEY,
}
