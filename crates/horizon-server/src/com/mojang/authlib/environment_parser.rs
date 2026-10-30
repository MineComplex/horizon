//! Port of `com.mojang.authlib.EnvironmentParser`.
use crate::prelude::*;
use crate::com::mojang::authlib::Environment;
use crate::com::mojang::authlib::services::MinecraftServicesEnvironment;

pub struct EnvironmentParser;

pub static ENVIRONMENT_OVERRIDE: LazyLock<Mut<Option<String>>> = LazyLock::new(|| Mut::new(None));
pub const PROP_PREFIX: &str = "minecraft.api.";
pub const PROP_ENV: &str = "minecraft.api.env";
pub const PROP_DISCOVERY_HOST: &str = "minecraft.api.discovery.host";

impl EnvironmentParser {
    pub fn set_environment_override(override_: Option<String>) {
        ENVIRONMENT_OVERRIDE.set(override_);
    }

    pub fn get_environment_from_properties() -> Option<Environment> {
        // System properties are read from the environment (`-Dminecraft.api.env=` has no equivalent).
        let env_name = ENVIRONMENT_OVERRIDE.get().or_else(|| std::env::var(PROP_ENV).ok());
        let env = MinecraftServicesEnvironment::from_string(env_name.as_deref());
        if env.is_some() { env } else { Self::from_host_names() }
    }

    fn from_host_names() -> Option<Environment> {
        let discovery = std::env::var(PROP_DISCOVERY_HOST).ok();
        discovery.map(|d| Environment::new(&d, "properties"))
    }
}
