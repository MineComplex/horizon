//! Port of `com.mojang.authlib.services.MinecraftServicesEnvironment`.
use crate::prelude::*;
use crate::com::mojang::authlib::Environment;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MinecraftServicesEnvironment {
    PROD,
    STAGING,
}
impl MinecraftServicesEnvironment {
    pub fn name(self) -> String {
        match self {
            Self::PROD => "PROD",
            Self::STAGING => "STAGING",
        }
        .to_string()
    }

    fn discovery_url(self) -> &'static str {
        match self {
            Self::PROD => "https://discovery.minecraftservices.com/minecraft/client",
            Self::STAGING => "https://discovery-staging.minecraftservices.com/minecraft/client",
        }
    }

    pub fn get_environment(self) -> Environment {
        Environment::new(self.discovery_url(), &self.name())
    }

    pub fn from_string(value: Option<&str>) -> Option<Environment> {
        let env = match value?.to_lowercase().as_str() {
            "prod" => Self::PROD,
            "staging" => Self::STAGING,
            _ => return None,
        };
        Some(env.get_environment())
    }
}
