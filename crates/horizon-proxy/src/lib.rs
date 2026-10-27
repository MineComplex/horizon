//! Horizon's proxy: MineComplex's Velocity fork (/new/Velocity) in Rust. Like the fork it speaks
//! only the pinned protocol (26.3), forwards players with Velocity's modern forwarding only, and
//! lets players in by the launcher's token instead of Mojang's session service.
//!
//! The proxy runs on tokio. Plugins ([`plugin::Plugin`]) get Velocity's events and register
//! commands; the Network plugin of MineComplex ([`network`]) connects the proxy to Core.
#![forbid(unsafe_code)]
pub mod auth;
pub mod commands;
pub mod config;
pub mod forwarding;
pub mod net;
pub mod network;
pub mod players;
pub mod plugin;
mod session;
pub mod text;

use auth::{AuthHandler, LauncherAuth, LauncherToken, OfflineAuth};
use config::{AuthMode, Config};
use horizon_protocol::rsa::RsaPrivateKey;
use horizon_world::nbt::Tag;
use players::{PlayerInfo, Players};
use plugin::{Command, Plugin, Tristate};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

/// One log line, `[HH:MM:SS INFO]: ...` (UTC).
pub fn log(line: &str) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    println!("[{h:02}:{m:02}:{s:02} INFO]: {line}");
}

/// What every connection of the proxy shares.
pub struct Shared {
    pub config: Config,
    /// The modern forwarding secret.
    pub secret: Vec<u8>,
    pub auth: Arc<dyn AuthHandler>,
    pub players: Arc<Players>,
    pub plugins: Vec<Arc<dyn Plugin>>,
    commands: RwLock<Vec<Arc<Command>>>,
    keys: RsaPrivateKey,
    public_der: Vec<u8>,
    /// VelocityServer.getSessionId: sent in every LoginFinished.
    session_id: u128,
    /// The last login of each address (the login rate limiter).
    logins: Mutex<HashMap<IpAddr, Instant>>,
}

impl Shared {
    /// An address for logs, unless `enable-player-address-logging` is off.
    pub fn address(&self, address: SocketAddr) -> String {
        if self.config.enable_player_address_logging {
            format!("/{address}")
        } else {
            "<ip address withheld>".into()
        }
    }

    /// Ratelimiter.attempt: one login per `login-ratelimit` milliseconds and address.
    fn login_allowed(&self, ip: IpAddr) -> bool {
        let limit = Duration::from_millis(self.config.advanced.login_ratelimit);
        if limit.is_zero() {
            return true;
        }
        let now = Instant::now();
        let mut logins = self.logins.lock().unwrap();
        if logins.len() > 4096 {
            logins.retain(|_, at| now.duration_since(*at) < limit);
        }
        match logins.insert(ip, now) {
            Some(last) => now.duration_since(last) >= limit,
            None => true,
        }
    }

    /// The server list entry as the plugins leave it.
    pub fn status_json(&self) -> String {
        session::status_json(self)
    }

    /// The permission provider: the first plugin with an answer.
    pub fn permission(&self, player: &PlayerInfo, permission: &str) -> Tristate {
        self.plugins
            .iter()
            .map(|p| p.permission(player, permission))
            .find(|t| *t != Tristate::Undefined)
            .unwrap_or(Tristate::Undefined)
    }
    pub fn has_permission(&self, player: &PlayerInfo, permission: &str) -> bool {
        self.permission(player, permission) == Tristate::True
    }
    /// Whether `player` may run `command`: its permission set to true, or for `!permission` not
    /// set to false.
    pub fn allows(&self, command: &Command, player: &PlayerInfo) -> bool {
        match command.permission.as_deref() {
            None => true,
            Some(p) => match p.strip_prefix('!') {
                Some(p) => self.permission(player, p) != Tristate::False,
                None => self.has_permission(player, p),
            },
        }
    }

    /// The proxy command with this name or alias.
    pub fn command(&self, label: &str) -> Option<Arc<Command>> {
        let label = label.to_lowercase();
        self.commands
            .read()
            .unwrap()
            .iter()
            .find(|c| c.name == label || c.aliases.contains(&label))
            .cloned()
    }
    pub fn commands(&self) -> Vec<Arc<Command>> {
        self.commands.read().unwrap().clone()
    }
}

/// The proxy as plugins see it.
#[derive(Clone)]
pub struct ProxyHandle(pub Arc<Shared>);
impl std::ops::Deref for ProxyHandle {
    type Target = Shared;
    fn deref(&self) -> &Shared {
        &self.0
    }
}
impl ProxyHandle {
    /// CommandManager.register: replaces a command of the same name.
    pub fn register_command(&self, mut command: Command) {
        command.name = command.name.to_lowercase();
        command
            .aliases
            .iter_mut()
            .for_each(|a| *a = a.to_lowercase());
        let mut commands = self.commands.write().unwrap();
        commands.retain(|c| c.name != command.name);
        commands.push(Arc::new(command));
    }
    /// ProxyServer.sendMessage: to every player.
    pub fn broadcast(&self, message: &Tag) {
        for p in self.players.all() {
            p.message(message.clone());
        }
    }
    /// A console command: the built-in ones and the plugins'. False when there is none.
    pub async fn console(&self, line: &str) -> bool {
        let mut words = line.split_whitespace();
        let Some(command) = words.next().and_then(|l| self.command(l)) else {
            return false;
        };
        let args = words.map(str::to_owned).collect();
        (command.run)(self.clone(), plugin::Source::Console, args).await;
        true
    }
}

pub struct Proxy {
    handle: ProxyHandle,
}

impl Proxy {
    /// A proxy with these plugins. The auth handler is the last plugin's, else `auth`, else the
    /// one `[auth]` asks for.
    pub fn new(
        config: Config,
        secret: Vec<u8>,
        auth: Option<Arc<dyn AuthHandler>>,
        plugins: Vec<Arc<dyn Plugin>>,
    ) -> Result<Self, String> {
        let auth = match plugins.iter().rev().find_map(|p| p.auth()).or(auth) {
            Some(auth) => auth,
            None => Self::auth_handler(&config)?,
        };
        let keys = RsaPrivateKey::generate(1024).map_err(|e| e.to_string())?;
        let public_der = keys.public_der();
        let mut session = [0u8; 16];
        horizon_protocol::crypto::random_bytes(&mut session).map_err(|e| e.to_string())?;
        let shared = Arc::new(Shared {
            config,
            secret,
            auth,
            players: Arc::new(Players::default()),
            plugins,
            commands: RwLock::new(Vec::new()),
            keys,
            public_der,
            session_id: u128::from_be_bytes(session),
            logins: Mutex::new(HashMap::new()),
        });
        let handle = ProxyHandle(shared);
        for command in commands::builtin() {
            handle.register_command(command);
        }
        Ok(Self { handle })
    }

    /// The auth handler `[auth]` asks for.
    pub fn auth_handler(config: &Config) -> Result<Arc<dyn AuthHandler>, String> {
        Ok(match config.auth.mode {
            AuthMode::Offline => Arc::new(OfflineAuth),
            AuthMode::Launcher => Arc::new(LauncherAuth {
                token: launcher_token(config)?.map(Arc::new),
                accounts: None,
            }),
        })
    }

    pub fn handle(&self) -> &ProxyHandle {
        &self.handle
    }
    pub fn shared(&self) -> &Arc<Shared> {
        &self.handle.0
    }

    /// ProxyInitializeEvent: enable the plugins in order.
    pub async fn enable(&self) -> Result<(), String> {
        for plugin in &self.handle.plugins {
            plugin
                .clone()
                .enable(self.handle.clone())
                .await
                .map_err(|e| format!("plugin {}: {e}", plugin.name()))?;
            log(&format!("Enabled plugin {}", plugin.name()));
        }
        Ok(())
    }

    /// ProxyShutdownEvent.
    pub async fn disable(&self) {
        for plugin in self.handle.plugins.iter().rev() {
            plugin.clone().disable().await;
        }
    }

    /// Accept players until the listener fails, each connection a task of its own.
    pub async fn serve(&self, listener: tokio::net::TcpListener) {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                continue;
            };
            let shared = self.handle.0.clone();
            tokio::spawn(session::serve(stream, peer, shared));
        }
    }
}

/// The launcher token verifier of `[auth]`, `None` without a key (nobody can join then).
pub fn launcher_token(config: &Config) -> Result<Option<LauncherToken>, String> {
    let key = config.auth.token_public_key.trim();
    if key.is_empty() {
        log("auth.token-public-key is not set in velocity.toml, players can not be authenticated");
        return Ok(None);
    }
    LauncherToken::new(
        key,
        &config.auth.token_issuer,
        config.auth.token_ttl_minutes,
    )
    .map(Some)
    .map_err(|e| format!("invalid auth.token-public-key: {e}"))
}
