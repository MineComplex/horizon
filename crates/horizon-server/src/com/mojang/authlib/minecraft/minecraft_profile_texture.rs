//! Port of `com.mojang.authlib.minecraft.MinecraftProfileTexture`.
use crate::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MinecraftProfileTextureType {
    SKIN,
    CAPE,
    ELYTRA,
}
impl MinecraftProfileTextureType {
    pub fn values() -> Vec<MinecraftProfileTextureType> {
        vec![Self::SKIN, Self::CAPE, Self::ELYTRA]
    }

    pub fn name(self) -> String {
        match self {
            Self::SKIN => "SKIN",
            Self::CAPE => "CAPE",
            Self::ELYTRA => "ELYTRA",
        }
        .to_string()
    }

    pub fn value_of(name: &str) -> Option<Self> {
        Self::values().into_iter().find(|t| t.name() == name)
    }
}

pub const PROFILE_TEXTURE_COUNT: i32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinecraftProfileTexture {
    pub url: String,
    pub metadata: Option<HashMap<String, String>>,
}
impl MinecraftProfileTexture {
    pub fn new(url: &str, metadata: Option<HashMap<String, String>>) -> Self {
        MinecraftProfileTexture { url: url.to_string(), metadata }
    }

    pub fn get_url(&self) -> String {
        self.url.clone()
    }

    pub fn get_metadata(&self, key: &str) -> Option<String> {
        self.metadata.as_ref()?.get(key).cloned()
    }

    /// `FilenameUtils.getBaseName(new URL(url).getPath())`.
    pub fn get_hash(&self) -> String {
        let after_scheme = match self.url.find("://") {
            Some(i) => &self.url[i + 3..],
            None => panic!("java.lang.IllegalArgumentException: Invalid profile texture url"),
        };
        let path = after_scheme.find('/').map(|i| &after_scheme[i..]).unwrap_or("");
        let path = path.split(['?', '#']).next().unwrap_or("");
        let name = path.rsplit('/').next().unwrap_or("");
        match name.rfind('.') {
            Some(i) => name[..i].to_string(),
            None => name.to_string(),
        }
    }
}
impl std::fmt::Display for MinecraftProfileTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MinecraftProfileTexture[url={},hash={}]", self.url, self.get_hash())
    }
}
