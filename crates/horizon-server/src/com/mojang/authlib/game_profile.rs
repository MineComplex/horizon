//! Port of `com.mojang.authlib.GameProfile` (a record: by value).
use crate::prelude::*;
use crate::com::mojang::authlib::properties::PropertyMap;
use crate::com::mojang::authlib::properties::property_map;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GameProfile {
    pub id: Uuid,
    pub name: String,
    pub properties: PropertyMap,
}
impl GameProfile {
    /// `new GameProfile(id, name)`: empty properties.
    pub fn new(id: Uuid, name: &str) -> Self {
        Self::new_3(id, name, property_map::EMPTY.clone())
    }

    /// The canonical constructor (`Objects.requireNonNull` on each component: the Rust types are non-null).
    pub fn new_3(id: Uuid, name: &str, properties: PropertyMap) -> Self {
        GameProfile { id, name: name.to_string(), properties }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn properties(&self) -> PropertyMap {
        self.properties.clone()
    }
}
impl std::fmt::Display for GameProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GameProfile[id={}, name={}, properties={}]", self.id, self.name, self.properties)
    }
}
crate::jobject!(GameProfile);
