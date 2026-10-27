//! `velocity.toml`: the same file as MineComplex's Velocity fork (VelocityConfiguration), so a
//! proxy moves over with its config. Keys the port does not use yet are accepted and ignored.
//! The fork's auth handler was a plugin (LauncherAuthHandler); here it is the `[auth]` table.
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, time::Duration};

/// The file a new proxy starts with (default-velocity.toml of the fork, plus `[auth]`).
pub const DEFAULT: &str = include_str!("default-velocity.toml");

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub bind: String,
    /// MiniMessage (the subset in [`crate::text::mini_message`]).
    pub motd: String,
    pub show_max_players: i32,
    pub forwarding_secret_file: String,
    #[serde(default)]
    pub kick_existing_players: bool,
    #[serde(default)]
    pub sample_players_in_ping: bool,
    #[serde(default = "yes")]
    pub enable_player_address_logging: bool,
    pub servers: Servers,
    #[serde(default)]
    pub packet_limiter: PacketLimiter,
    #[serde(default)]
    pub advanced: Advanced,
    #[serde(default)]
    pub auth: Auth,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Servers {
    /// In what order servers are tried when a player logs in or is kicked.
    #[serde(rename = "try", default)]
    pub try_order: Vec<String>,
    /// Server name to `host:port`.
    #[serde(flatten)]
    pub addresses: BTreeMap<String, String>,
}

/// `[packet-limiter]`: averages a client may send over a moving window; -1 turns a check off.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", default)]
pub struct PacketLimiter {
    pub interval: u32,
    pub packets_per_second: i64,
    pub bytes_per_second: i64,
    pub decompressed_bytes_per_second: i64,
}
impl Default for PacketLimiter {
    fn default() -> Self {
        Self {
            interval: 7,
            packets_per_second: -1,
            bytes_per_second: -1,
            decompressed_bytes_per_second: 5_242_880,
        }
    }
}
impl PacketLimiter {
    pub fn limits(&self) -> crate::net::Limits {
        crate::net::Limits {
            interval: self.interval,
            packets_per_second: self.packets_per_second,
            bytes_per_second: self.bytes_per_second,
            decompressed_bytes_per_second: self.decompressed_bytes_per_second,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", default)]
pub struct Advanced {
    pub compression_threshold: i32,
    pub login_ratelimit: u64,
    pub connection_timeout: u64,
    pub read_timeout: u64,
    pub haproxy_protocol: bool,
    pub show_ping_requests: bool,
    pub failover_on_unexpected_server_disconnect: bool,
    pub announce_proxy_commands: bool,
    pub log_command_executions: bool,
    pub log_player_connections: bool,
    pub command_rate_limit: u64,
    pub forward_commands_if_rate_limited: bool,
    pub kick_after_rate_limited_commands: i32,
    pub tab_complete_rate_limit: u64,
    pub kick_after_rate_limited_tab_completes: i32,
}
impl Default for Advanced {
    fn default() -> Self {
        Self {
            compression_threshold: 256,
            login_ratelimit: 3000,
            connection_timeout: 5000,
            read_timeout: 30000,
            haproxy_protocol: false,
            show_ping_requests: false,
            failover_on_unexpected_server_disconnect: true,
            announce_proxy_commands: true,
            log_command_executions: false,
            log_player_connections: true,
            command_rate_limit: 50,
            forward_commands_if_rate_limited: true,
            kick_after_rate_limited_commands: 0,
            tab_complete_rate_limit: 10,
            kick_after_rate_limited_tab_completes: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    /// The launcher's token (LauncherAuthHandler of the Network plugin).
    #[default]
    Launcher,
    /// Anyone with any name, offline UUIDs: for tests on a closed machine only.
    Offline,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", default)]
pub struct Auth {
    pub mode: AuthMode,
    /// tokenPublicKey of Rest's `GET /api/meta`: base64 X.509 (PEM armour allowed).
    pub token_public_key: String,
    pub token_issuer: String,
    pub token_ttl_minutes: u64,
}
impl Default for Auth {
    fn default() -> Self {
        Self {
            mode: AuthMode::Launcher,
            token_public_key: String::new(),
            token_issuer: "minecomplex".into(),
            token_ttl_minutes: 720,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        config.validate()?;
        Ok(config)
    }
    /// Read `path`, writing the default file first when there is none.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            std::fs::write(path, DEFAULT).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn validate(&self) -> Result<(), String> {
        if self.servers.addresses.is_empty() {
            return Err("no servers are configured".into());
        }
        for (name, address) in &self.servers.addresses {
            parse_address(address).map_err(|e| format!("server {name}: {e}"))?;
        }
        for name in &self.servers.try_order {
            if !self.servers.addresses.contains_key(name) {
                return Err(format!(
                    "fallback/initial server list names unknown server {name}"
                ));
            }
        }
        Ok(())
    }

    /// The configured address of a server.
    pub fn address(&self, server: &str) -> Option<(String, u16)> {
        parse_address(self.servers.addresses.get(server)?).ok()
    }
    pub fn connection_timeout(&self) -> Duration {
        Duration::from_millis(self.advanced.connection_timeout.max(1))
    }
    pub fn read_timeout(&self) -> Option<Duration> {
        (self.advanced.read_timeout > 0).then(|| Duration::from_millis(self.advanced.read_timeout))
    }
}

/// `host:port`, `host` (port 25565) or `[v6]:port`.
pub fn parse_address(text: &str) -> Result<(String, u16), String> {
    let text = text.trim();
    let (host, port) = match text.strip_prefix('[') {
        Some(rest) => {
            let (host, rest) = rest.split_once(']').ok_or("unclosed [")?;
            (host, rest.strip_prefix(':'))
        }
        None => match text.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            _ => (text, None),
        },
    };
    if host.is_empty() {
        return Err(format!("no host in {text:?}"));
    }
    let port = match port {
        Some(p) => p.parse().map_err(|_| format!("bad port in {text:?}"))?,
        None => 25565,
    };
    Ok((host.to_owned(), port))
}

/// The modern forwarding secret: `VELOCITY_FORWARDING_SECRET`, else the file, which is created
/// with a random secret when missing (VelocityConfiguration.read).
pub fn forwarding_secret(config: &Config, dir: &Path) -> Result<Vec<u8>, String> {
    match std::env::var("VELOCITY_FORWARDING_SECRET") {
        Ok(secret) if !secret.is_empty() => return Ok(secret.into_bytes()),
        _ => {}
    }
    let path = dir.join(&config.forwarding_secret_file);
    if !path.exists() {
        let secret = random_secret()?;
        std::fs::write(&path, &secret).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let secret = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(format!("{} is empty", path.display()));
    }
    Ok(secret.as_bytes().to_vec())
}

/// 12 random letters and digits, as Velocity generates.
fn random_secret() -> Result<String, String> {
    const ABC: &[u8] = b"AaBbCcDdEeFfGgHhIiJjKkLlMmNnOoPpQqRrSsTtUuVvWwXxYyZz1234567890";
    let mut bytes = [0u8; 12];
    horizon_protocol::crypto::random_bytes(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes
        .iter()
        .map(|b| ABC[usize::from(*b) % ABC.len()] as char)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_parses() {
        let c = Config::parse(DEFAULT).unwrap();
        assert_eq!(c.bind, "0.0.0.0:25565");
        assert_eq!(c.servers.try_order, ["lobby"]);
        assert_eq!(c.address("lobby"), Some(("127.0.0.1".into(), 30066)));
        assert_eq!(c.advanced, Advanced::default());
        assert_eq!(c.packet_limiter, PacketLimiter::default());
        assert_eq!(c.auth, Auth::default());
    }

    #[test]
    fn rejects_unknown_try_servers() {
        let text = DEFAULT.replace("try = [\n    \"lobby\"", "try = [\n    \"nope\"");
        assert!(Config::parse(&text).unwrap_err().contains("nope"));
    }

    #[test]
    fn addresses() {
        assert_eq!(parse_address("a:1"), Ok(("a".into(), 1)));
        assert_eq!(parse_address("a"), Ok(("a".into(), 25565)));
        assert_eq!(parse_address("[::1]:7"), Ok(("::1".into(), 7)));
        assert_eq!(parse_address("::1"), Ok(("::1".into(), 25565)));
        assert!(parse_address("a:x").is_err());
        assert!(parse_address(":1").is_err());
    }
}
