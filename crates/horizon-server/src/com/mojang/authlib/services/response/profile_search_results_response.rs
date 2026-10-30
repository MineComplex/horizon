//! Port of `com.mojang.authlib.services.response.ProfileSearchResultsResponse` (a record).
use crate::com::mojang::authlib::services::response::NameAndId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileSearchResultsResponse {
    pub profiles: Vec<NameAndId>,
}
impl ProfileSearchResultsResponse {
    pub fn profiles(&self) -> Vec<NameAndId> {
        self.profiles.clone()
    }
}
