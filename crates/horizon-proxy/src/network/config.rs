//! `plugins/network/config.toml`: ProxyConfig and the `core` keys of the Network plugin.
use serde::Deserialize;
use std::{path::Path, time::Duration};

pub const DEFAULT: &str = include_str!("default.toml");

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkConfig {
    pub core: Core,
    pub bungee: Bungee,
    pub commands: Commands,
    pub settings: Settings,
    pub maintenance: Maintenance,
    pub broadcast: Broadcast,
    pub fallback: Fallback,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Core {
    pub host: String,
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub node: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Bungee {
    pub name: String,
    pub max_players: i64,
    pub description: Vec<String>,
    pub motd_delay: u64,
    #[serde(default)]
    pub motds: Vec<Motd>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Motd {
    pub first: String,
    pub second: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Commands {
    pub cooldown: f64,
    pub cooldown_exclusions: Vec<String>,
    pub blocked: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Settings {
    pub lobby_cooldown: f64,
    pub ip_connections_limit: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Maintenance {
    pub kick_message: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Broadcast {
    pub delay: u64,
    #[serde(default)]
    pub messages: Vec<Announcement>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Announcement {
    pub lines: Vec<String>,
    #[serde(default)]
    pub center: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Fallback {
    pub ignore_reasons: Vec<String>,
}

impl NetworkConfig {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
    pub fn node(&self) -> String {
        match self.core.node.trim() {
            "" => "proxy".into(),
            node => node.into(),
        }
    }
    pub fn command_cooldown(&self) -> Duration {
        Duration::from_secs_f64(self.commands.cooldown.max(0.0))
    }
    pub fn lobby_cooldown(&self) -> Duration {
        Duration::from_secs_f64(self.settings.lobby_cooldown.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parses() {
        let c = NetworkConfig::parse(DEFAULT).unwrap();
        assert_eq!(c.core.port, 7500);
        assert_eq!(c.node(), "proxy");
        assert_eq!(c.bungee.description.len(), 6);
        assert!(c.commands.blocked.contains(&"plugins".to_owned()));
        assert_eq!(c.command_cooldown(), Duration::from_secs(1));
    }
}
