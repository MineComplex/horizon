//! Hosting a world on the local network. The network side lives here: the
//! listener, the per-connection handshake/login/configuration threads and the
//! writer threads (`conn`), plus the data the configuration phase sends
//! (`data`). The play state is driven by the game thread (`hub`), which only
//! ever exchanges decoded packets with these threads through channels, so a
//! world that is not open to the network pays nothing.
mod advancements;
pub use crate::attribute_sync as attributes;
pub mod border;
pub mod chunks;
mod command_order;
pub mod command_suggestions;
pub mod commands;
pub mod conn;
pub mod containers;
pub mod data;
pub mod entity;
pub mod hub;
pub mod menu;
pub mod player;
pub mod playerdata;
mod recipes;
pub mod scoreboard;
pub use crate::entity_tracking as tracking;
pub mod velocity;
pub mod waypoints;

pub use conn::{Holding, PlayerSender, flush_all};
use horizon_protocol::packets::play;
use horizon_protocol::{packets::configuration::ClientInformation, text::translatable};
use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread::JoinHandle,
    time::Duration,
};

/// Messages from the connection threads to the game thread.
pub enum NetEvent {
    /// A client finished configuration and enters the play state.
    Joined(Box<Joined>),
    Packet(u32, Box<play::Serverbound>),
    /// The connection ended (reason for the log).
    Left(u32, String),
    /// PlayerList.disconnectAllPlayersWithProfile: a new login of this
    /// profile replaces the player already in the world.
    Duplicate(u128),
}
pub struct Joined {
    pub conn: u32,
    pub uuid: u128,
    pub name: String,
    /// The profile's properties (skin textures, in online mode).
    pub properties: Vec<horizon_protocol::packets::login::Property>,
    pub info: ClientInformation,
    pub brand: String,
    pub sender: PlayerSender,
    /// Smoothed keep-alive round trip in milliseconds.
    pub latency: Arc<AtomicI32>,
    pub address: SocketAddr,
}
/// Queued for a connection's writer thread.
pub enum Out {
    /// A packet body: VarInt id and payload.
    Packet(Vec<u8>),
    /// Packet bodies held during a tick, in order.
    Packets(Vec<Vec<u8>>),
    /// Send a play disconnect with this component, then close.
    Disconnect(horizon_world::nbt::Tag),
    Close,
}

/// PlayerList.canPlayerLogin for a dedicated server: the disconnect reason
/// (bans, whitelist) for a profile connecting from an address, if refused.
pub type LoginGate =
    Arc<dyn Fn(SocketAddr, u128, &str) -> Option<horizon_world::nbt::Tag> + Send + Sync>;

/// The dedicated server's log for the login phase: (thread, level, line)
/// written as vanilla's Log4j would, and the language that turns a
/// disconnect reason into its text (Component.getString).
#[derive(Clone)]
pub struct NetLog {
    pub write: Arc<dyn Fn(&str, &str, &str) + Send + Sync>,
    pub lang: Arc<horizon_protocol::text::Language>,
    /// log-ips: addresses in the log, or "<ip address withheld>".
    pub log_ips: bool,
}

/// How the server side of the network behaves: the defaults for tests, the
/// rest from server.properties.
#[derive(Clone)]
pub struct NetConfig {
    /// server-ip (all interfaces when absent).
    pub bind: Option<std::net::IpAddr>,
    /// 0 picks a free port.
    pub port: u16,
    pub motd: String,
    pub max_players: usize,
    /// network-compression-threshold; negative disables compression.
    pub compression_threshold: i32,
    /// server-icon.png (64x64 PNG bytes).
    pub favicon: Option<Vec<u8>>,
    pub accepts_transfers: bool,
    /// Ops bypass a full server (ServerOpList.canBypassPlayerLimit).
    pub bypasses_limit: Option<Arc<dyn Fn(u128) -> bool + Send + Sync>>,
    pub gate: Option<LoginGate>,
    /// Behind a Velocity proxy: players come with the proxy's forwarded
    /// profile (`config/paper-global.yml` `proxies.velocity`). Without it the
    /// server is offline: nobody is authenticated (this server never asks Mojang).
    pub velocity: Option<Arc<velocity::Velocity>>,
    pub log: Option<NetLog>,
    /// server.properties `resource-pack`: offered to every player during
    /// configuration (a dedicated server's; MinecraftServer.getServerResourcePack).
    pub resource_pack: Option<crate::server::properties::ResourcePackInfo>,
    /// MinecraftServer.serverLinks (a dedicated server's `bug-report-link`):
    /// sent in configuration when non-empty
    /// (ServerConfigurationPacketListenerImpl.startConfiguration).
    pub server_links: Vec<horizon_protocol::packets::configuration::ServerLink>,
}
impl NetConfig {
    /// The defaults: a port, a motd and the player limit.
    pub fn network(port: u16, motd: String, max_players: usize) -> Self {
        Self {
            bind: None,
            port,
            motd,
            max_players,
            compression_threshold: 256,
            favicon: None,
            accepts_transfers: false,
            bypasses_limit: None,
            gate: None,
            velocity: None,
            log: None,
            resource_pack: None,
            server_links: Vec::new(),
        }
    }
}

/// State shared with the connection threads.
pub struct Shared {
    pub sync: Arc<data::SyncData>,
    pub events: Sender<NetEvent>,
    pub config: NetConfig,
    motd: Mutex<String>,
    max_players: AtomicUsize,
    /// PlayerList.players in join order: profile id and name.
    online: Mutex<Vec<(u128, String)>>,
    next: AtomicU32,
    stop: AtomicBool,
}
impl Shared {
    pub fn next_id(&self) -> u32 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }
    /// PlayerList.canPlayerLogin then disconnectAllPlayersWithProfile: the
    /// refusal reason, or None once the profile holds a place. A profile
    /// already playing is kicked and its place awaited.
    pub fn try_admit(
        &self,
        address: SocketAddr,
        uuid: u128,
        name: &str,
    ) -> Option<horizon_world::nbt::Tag> {
        if let Some(gate) = &self.config.gate
            && let Some(reason) = gate(address, uuid, name)
        {
            return Some(reason);
        }
        {
            let online = self.online.lock().unwrap();
            let bypass = self.config.bypasses_limit.as_ref().is_some_and(|b| b(uuid));
            let taken = online.len();
            if taken >= self.max_players() && !bypass {
                return Some(translatable("multiplayer.disconnect.server_full", &[]));
            }
        }
        if self.is_online(uuid) {
            let _ = self.events.send(NetEvent::Duplicate(uuid));
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            while self.is_online(uuid) {
                if std::time::Instant::now() > deadline {
                    return Some(translatable("multiplayer.disconnect.slow_login", &[]));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.online.lock().unwrap().push((uuid, name.to_owned()));
        None
    }
    fn is_online(&self, uuid: u128) -> bool {
        self.online.lock().unwrap().iter().any(|(u, _)| *u == uuid)
    }
    pub fn motd(&self) -> String {
        self.motd.lock().unwrap().clone()
    }
    pub fn max_players(&self) -> usize {
        self.max_players.load(Ordering::Relaxed)
    }
    /// A new message of the day for the status list (plugins).
    pub fn set_motd(&self, motd: String) {
        *self.motd.lock().unwrap() = motd;
    }
    /// A new player limit (plugins).
    pub fn set_max_players(&self, max: usize) {
        self.max_players.store(max, Ordering::Relaxed);
    }
    pub fn release(&self, uuid: u128) {
        self.online.lock().unwrap().retain(|(u, _)| *u != uuid);
    }
    pub fn online(&self) -> usize {
        self.online.lock().unwrap().len()
    }
}

pub use crate::wire::base64;

/// The server's listener and its connections.
pub struct Listener {
    pub shared: Arc<Shared>,
    events: Receiver<NetEvent>,
    pub port: u16,
    listener: Option<JoinHandle<()>>,
    /// Stops the accept loop alone: local connections stay.
    listener_stop: Arc<AtomicBool>,
}
impl Listener {
    /// Listen on `port` (0 picks a free one).
    pub fn start(
        sync: Arc<data::SyncData>,
        port: u16,
        motd: String,
        max_players: usize,
    ) -> io::Result<Self> {
        Self::start_with(sync, NetConfig::network(port, motd, max_players))
    }
    /// ServerConnectionListener.startTcpServerListener with a configuration.
    pub fn start_with(sync: Arc<data::SyncData>, config: NetConfig) -> io::Result<Self> {
        let (tx, events) = channel();
        let port = config.port;
        let shared = Arc::new(Shared {
            sync,
            events: tx,
            motd: Mutex::new(config.motd.clone()),
            max_players: AtomicUsize::new(config.max_players),
            online: Mutex::new(Vec::new()),
            next: AtomicU32::new(1),
            stop: AtomicBool::new(false),
            config,
        });
        let mut network = Self {
            shared,
            events,
            port: 0,
            listener: None,
            listener_stop: Arc::new(AtomicBool::new(false)),
        };
        network.listen(port)?;
        Ok(network)
    }
    /// Start accepting connections on `port` (0 picks a free one). Returns
    /// the port.
    pub fn listen(&mut self, port: u16) -> io::Result<u16> {
        if self.listener.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "already listening",
            ));
        }
        let listener = Self::bind(self.shared.config.bind, port)?;
        self.listen_on(listener)
    }
    /// The socket [`Lan::listen`] accepts on, bound ahead so that a failure
    /// can be reported before the hub is at hand.
    pub fn bind(address: Option<std::net::IpAddr>, port: u16) -> io::Result<TcpListener> {
        let bind = address.unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
        let listener = TcpListener::bind((bind, port))?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    }
    /// [`Lan::listen`] on a socket already bound.
    pub fn listen_on(&mut self, listener: TcpListener) -> io::Result<u16> {
        if self.listener.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "already listening",
            ));
        }
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let (accept_shared, accept_stop) = (self.shared.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("horizon-network-accept".into())
            .spawn(move || accept_loop(listener, accept_shared, &accept_stop))?;
        self.listener = Some(thread);
        self.listener_stop = stop;
        self.port = port;
        Ok(port)
    }
    /// Stop accepting connections; connections already in
    /// (the local one included) stay.
    pub fn stop_listening(&mut self) {
        self.listener_stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.listener.take() {
            let _ = t.join();
        }
        self.port = 0;
    }
    /// Whether connections from the network are accepted.
    pub fn listening(&self) -> bool {
        self.listener.is_some()
    }
    /// Everything the connection threads reported since the last call.
    pub fn poll(&self) -> Vec<NetEvent> {
        self.events.try_iter().collect()
    }
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.stop_listening();
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>, stop: &AtomicBool) {
    while !shared.stop.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, addr)) => {
                let shared = shared.clone();
                let _ = std::thread::Builder::new()
                    .name("horizon-network-conn".into())
                    .spawn(move || handle(stream, addr, shared));
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}
fn handle(stream: TcpStream, addr: SocketAddr, shared: Arc<Shared>) {
    let _ = stream.set_nonblocking(false);
    if let Err(e) = conn::serve(stream, addr, shared)
        && std::env::var_os("HORIZON_NET_DEBUG").is_some()
    {
        log::warn!("[network] {addr}: {e}");
    }
}

/// HttpUtil.getAvailablePort: a free local port.
pub fn available_port() -> u16 {
    TcpListener::bind(("0.0.0.0", 0))
        .and_then(|l| l.local_addr())
        .map_or(25565, |a| a.port())
}
