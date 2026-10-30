//! Port of `com.mojang.authlib.services.response.NameAndId` (a record).
use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NameAndId {
    pub id: Uuid,
    pub name: String,
}
impl NameAndId {
    pub fn new(id: Uuid, name: &str) -> Self {
        NameAndId { id, name: name.to_string() }
    }
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn name(&self) -> String {
        self.name.clone()
    }
}
impl std::fmt::Display for NameAndId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NameAndId[id={}, name={}]", self.id, self.name)
    }
}
