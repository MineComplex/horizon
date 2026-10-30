//! Port of `com.mojang.authlib.SignatureState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SignatureState {
    UNSIGNED,
    INVALID,
    SIGNED,
}
