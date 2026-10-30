//! Port of `com.mojang.authlib.services.ProfileActionType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProfileActionType {
    FORCED_NAME_CHANGE,
    USING_BANNED_SKIN,
}
