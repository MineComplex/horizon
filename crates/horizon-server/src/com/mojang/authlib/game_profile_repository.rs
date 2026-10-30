//! Port of `com.mojang.authlib.GameProfileRepository`.
use crate::prelude::*;
use crate::com::mojang::authlib::ProfileLookupCallback;
use crate::com::mojang::authlib::services::response::NameAndId;

pub trait GameProfileRepository: JObject {
    fn find_profiles_by_names(&self, names: &Vec<String>, callback: JRef<dyn ProfileLookupCallback>);
    fn find_profile_by_name(&self, name: &str) -> Option<NameAndId>;
}
