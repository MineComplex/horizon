//! Port of `com.mojang.authlib.services.ProfileResult` (a record).
use crate::prelude::*;
use crate::com::mojang::authlib::GameProfile;
use crate::com::mojang::authlib::services::ProfileActionType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileResult {
    pub profile: GameProfile,
    pub actions: HashSet<ProfileActionType>,
}
impl ProfileResult {
    pub fn new(profile: GameProfile) -> Self {
        Self::new_2(profile, HashSet::new())
    }
    pub fn new_2(profile: GameProfile, actions: HashSet<ProfileActionType>) -> Self {
        ProfileResult { profile, actions }
    }
    pub fn profile(&self) -> GameProfile {
        self.profile.clone()
    }
    pub fn actions(&self) -> HashSet<ProfileActionType> {
        self.actions.clone()
    }
}
