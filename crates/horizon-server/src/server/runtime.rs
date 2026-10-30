//! The running server after [`super::bootstrap`]: `DedicatedServer.initServer`'s network part,
//! `MinecraftServer.runServer`'s loop and `stopServer`, as Leaf runs them.
//!
//! Threads and ownership:
//! - **The tick thread** (the caller of [`Server::run`]) owns [`Server`]: the tick clock and rate,
//!   every joined connection, the logins waiting for an answer. Nothing in it is shared.
//! - **The network** is a tokio multi-thread runtime owned by `Server` (Netty's event loops): the
//!   accept task and the tasks of each connection (`network::listener`). They reach the tick thread
//!   through one queue of [`NetEvent`]s and, once joined, a queue per connection each way.
//! - **The console** is a plain thread reading stdin lines into the control queue
//!   ([`spawn_console`]). Paper's jline console (`TerminalConsoleAppender`) brings history and
//!   completion; no commands are ported yet, so lines on stdin are what the console needs today
//!   (`rustyline` is the replacement once completion has commands to complete).
//! - **Signals** (Ctrl-C, SIGTERM) are a task on the runtime that posts to the same control queue,
//!   in place of Paper's `ServerShutdownThread` (which halts the loop and lets it stop the server).
//!
//! Each tick follows `tickServer`: the empty-server pause, the tick rate, the levels (none yet),
//! then `tickConnection`: new connections and logins (`addPendingConnections`), each connection's
//! tick (login verification, packets, keep-alive, disconnections), then the console's lines.
//!
//! Logins (`PlayerList.canPlayerLogin` as Leaf left it, and the duplicate check): MineComplex's
//! cleanup removed the ban lists; the whitelist (`whitelist.json`) and the operators' player-limit
//! bypass (`ops.json`) are not ported yet, so nobody is refused for them; a full server refuses with
//! `multiplayer.disconnect.server_full`. Duplicates follow `disconnectAllPlayersWithProfile`
//! (Paper: also by name, ignoring case, when the proxy is in online mode): they are kicked with
//! `multiplayer.disconnect.duplicate_login` and the new login waits until they are gone.
//!
//! Hand-off to the configuration and play states (other modules): [`Server::start_configuration`]
//! receives each connection that finished logging in, [`Server::configuration_packet`] each of its
//! packets but keep-alives, and [`Server::place_new_player`] moves it into the player list.
use std::net::SocketAddr;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::bootstrap::ServerConfig;
use super::tick_loop::{
    EmptyPause, EmptyTick, TickAction, TickClock, TickLoopConfig, TickTimeSamples,
};
use super::tick_rate::TickRateManager;
use crate::network::listener::{self, Connection, Incoming, LoginRequest, LoginVerdict, NetEvent};
use crate::network::login::{GameProfile, LoginConfig};
use crate::network::{ConnectionState, Reason};

/// `ServerCommonPacketListenerImpl`: a keep-alive every 15 s; Paper kicks once one has been
/// unanswered for 30 s (`paper.playerconnection.keepalive`).
const KEEP_ALIVE_INTERVAL_MS: i64 = 15_000;
const KEEP_ALIVE_LIMIT_MS: i64 = 30_000;
/// Paper's `handleAllDisconnections`: how long stopping waits for the connections to close.
const CLOSE_WAIT: Duration = Duration::from_secs(30);

/// What the running server reads from the settings.
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// `server-ip` (empty: every address) and `server-port`, after the command line's overrides.
    pub server_ip: String,
    pub server_port: i32,
    pub login: LoginConfig,
    pub tick_loop: TickLoopConfig,
    pub max_players: i32,
    pub online_mode: bool,
    /// Paper's `isProxyOnlineMode`: duplicates are also found by name.
    pub proxy_online_mode: bool,
    pub white_list: bool,
    pub pause_when_empty_seconds: i32,
    /// `settings.shutdown-message` of `bukkit.yml` (Leaf's default), sent to players on stop.
    /// `bukkit.yml` is not read yet.
    pub shutdown_message: Reason,
    /// When the process started (Paper's `BOOT_TIME`, for "Done (...)!").
    pub boot: Instant,
}

impl RuntimeConfig {
    pub fn from_bootstrap(config: &ServerConfig, boot: Instant) -> Self {
        let properties = config.properties.properties();
        let velocity = &config.yaml.paper.velocity;
        Self {
            server_ip: properties.server_ip.clone(),
            server_port: properties.server_port,
            login: LoginConfig {
                accepts_transfers: properties.accepts_transfers,
                compression_threshold: properties.network_compression_threshold,
                velocity_secret: velocity.enabled.then(|| velocity.secret.clone()),
                // Gale's `misc.premium-account-slow-login-timeout` and Spigot's `messages.outdated-*`
                // are not read yet: their defaults.
                ..LoginConfig::default()
            },
            tick_loop: config.yaml.tick_loop(),
            max_players: properties.max_players,
            online_mode: properties.online_mode,
            proxy_online_mode: properties.online_mode || (velocity.enabled && velocity.online_mode),
            white_list: properties.white_list,
            pause_when_empty_seconds: properties.pause_when_empty_seconds,
            shutdown_message: Reason::Literal("Сервер перезапускается".into()),
            boot,
        }
    }
}

/// What reaches the tick thread from outside the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    /// A console line.
    Console(String),
    Signal(Signal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// SIGINT (Ctrl-C): the JVM exits with 130 after its shutdown hooks.
    Interrupt,
    /// SIGTERM: 143.
    Terminate,
}

/// A logged-in connection, with `ServerCommonPacketListenerImpl`'s keep-alive state.
#[derive(Debug)]
struct Peer {
    conn: Connection,
    keep_alive_time: i64,
    keep_alive_pending: bool,
    keep_alive_challenge: i64,
    latency: i32,
    /// Set by a disconnect: what the log says once the connection is closed; packets are ignored.
    closing: Option<Reason>,
}

/// What a peer's tick decided.
#[derive(Debug, PartialEq, Eq)]
enum PeerState {
    Open,
    Lost(Reason),
}

impl Peer {
    fn new(conn: Connection, now_ms: i64) -> Self {
        Self {
            conn,
            keep_alive_time: now_ms,
            keep_alive_pending: false,
            keep_alive_challenge: 0,
            latency: 0,
            closing: None,
        }
    }

    /// `ServerCommonPacketListenerImpl.disconnect`: the packet, then close; read-only meanwhile.
    fn disconnect(&mut self, reason: Reason) {
        if self.closing.is_none() {
            self.conn.disconnect(&reason);
            self.closing = Some(reason);
        }
    }

    /// `keepConnectionAlive` with Paper's longer limit.
    fn keep_alive(&mut self, now_ms: i64) {
        let elapsed = now_ms - self.keep_alive_time;
        if self.closing.is_some() || elapsed < KEEP_ALIVE_INTERVAL_MS {
            return;
        }
        if self.keep_alive_pending {
            if elapsed >= KEEP_ALIVE_LIMIT_MS {
                self.disconnect(Reason::translatable("disconnect.timeout"));
            }
        } else {
            self.keep_alive_pending = true;
            self.keep_alive_time = now_ms;
            self.keep_alive_challenge = now_ms;
            let id = match self.conn.state {
                ConnectionState::Play => 45,
                _ => 4,
            };
            let mut packet = vec![id];
            packet.extend(now_ms.to_be_bytes());
            self.conn.send(packet);
        }
    }

    /// `handleKeepAlive`: `Some` when the body is a keep-alive of this state.
    fn handle_keep_alive(&mut self, body: &[u8], now_ms: i64) -> bool {
        let id = match self.conn.state {
            ConnectionState::Configuration => 4,
            ConnectionState::Play => 28,
            _ => return false,
        };
        let Some((&first, rest)) = body.split_first() else {
            return false;
        };
        if first != id {
            return false;
        }
        let answer = <[u8; 8]>::try_from(rest).ok().map(i64::from_be_bytes);
        if self.keep_alive_pending && answer == Some(self.keep_alive_challenge) {
            let time = (now_ms - self.keep_alive_time) as i32;
            self.latency = (self.latency * 3 + time) / 4;
            self.keep_alive_pending = false;
        } else {
            self.disconnect(Reason::translatable("disconnect.timeout"));
        }
        true
    }

    /// One message from the reader task. `Err` hands a packet on to the state's listener.
    fn receive(&mut self, incoming: Incoming, now_ms: i64) -> Result<PeerState, listener::Packet> {
        Ok(match incoming {
            Incoming::Packet(packet) => {
                if self.closing.is_none() && !self.handle_keep_alive(&packet, now_ms) {
                    return Err(packet);
                }
                PeerState::Open
            }
            // `exceptionCaught`: the reason in the state's disconnect packet, then close.
            Incoming::Fault(fault) => {
                tracing::debug!("Failed to sent packet: {fault}");
                self.disconnect(fault.reason());
                PeerState::Lost(self.closing.take().expect("just set"))
            }
            Incoming::TimedOut => {
                self.conn.close();
                PeerState::Lost(Reason::translatable("disconnect.timeout"))
            }
            // `channelInactive`: end of stream, unless the server closed it with a reason.
            Incoming::Closed => PeerState::Lost(
                self.closing
                    .take()
                    .unwrap_or_else(|| Reason::translatable("disconnect.endOfStream")),
            ),
        })
    }

    /// `onDisconnect`'s log line.
    fn log_lost(&self, reason: &Reason) {
        let text = listener::reason_component(reason).to_string();
        let profile = &self.conn.profile;
        match self.conn.state {
            ConnectionState::Play => tracing::info!("{} lost connection: {text}", profile.name),
            _ => tracing::info!("{} ({}) lost connection: {text}", profile.name, profile.id),
        }
    }
}

struct PendingLogin {
    request: LoginRequest,
    /// `WAITING_FOR_DUPE_DISCONNECT`.
    waiting: bool,
}

/// The tick thread's state.
pub struct Server {
    config: RuntimeConfig,
    runtime: tokio::runtime::Runtime,
    local_addr: SocketAddr,
    accept: JoinHandle<()>,
    events: mpsc::UnboundedReceiver<NetEvent>,
    control: std_mpsc::Receiver<Control>,
    control_sender: std_mpsc::Sender<Control>,
    rate: TickRateManager,
    clock: TickClock,
    empty_pause: EmptyPause,
    tick_count: i32,
    tick_times: TickTimeSamples,
    epoch: Instant,
    logins: Vec<PendingLogin>,
    configuring: Vec<Peer>,
    players: Vec<Peer>,
    /// `ServerConnectionListener.getSessionId`: made on first use, forgotten when nobody is connected.
    session_id: Option<Uuid>,
    exit_code: Option<u8>,
    #[cfg(test)]
    place_on_join: bool,
}

impl Server {
    /// `initServer` from "Starting Minecraft server on" to the listener running.
    pub fn start(config: RuntimeConfig) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .thread_name("Netty Epoll Server IO")
            .enable_all()
            .build()?;
        let ip = if config.server_ip.is_empty() {
            "*"
        } else {
            &config.server_ip
        };
        tracing::info!("Starting Minecraft server on {ip}:{}", config.server_port);
        let bound = u16::try_from(config.server_port)
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("port out of range:{}", config.server_port),
                )
            })
            .and_then(|port| {
                let host = if config.server_ip.is_empty() {
                    "0.0.0.0"
                } else {
                    &config.server_ip
                };
                runtime.block_on(TcpListener::bind((host, port)))
            });
        let tcp = match bound {
            Ok(tcp) => tcp,
            Err(e) => {
                tracing::warn!("**** FAILED TO BIND TO PORT!");
                tracing::warn!("The exception was: {e}");
                tracing::warn!("Perhaps a server is already running on that port?");
                return Err(e);
            }
        };
        let local_addr = tcp.local_addr()?;
        if !config.online_mode {
            tracing::warn!("**** SERVER IS RUNNING IN OFFLINE/INSECURE MODE!");
        }
        if config.white_list {
            tracing::warn!(
                "white-list is on, but whitelist.json is not ported yet: nobody is refused"
            );
        }
        let (event_sender, events) = mpsc::unbounded_channel();
        let accept = runtime.spawn(listener::listen(
            tcp,
            std::sync::Arc::new(config.login.clone()),
            event_sender,
        ));
        let (control_sender, control) = std_mpsc::channel();
        let rate = TickRateManager::new();
        let now = Instant::now();
        Ok(Self {
            clock: TickClock::new(now, config.tick_loop, &rate, false),
            empty_pause: EmptyPause::new(config.pause_when_empty_seconds),
            config,
            runtime,
            local_addr,
            accept,
            events,
            control,
            control_sender,
            rate,
            tick_count: 0,
            tick_times: TickTimeSamples::default(),
            epoch: now,
            logins: Vec::new(),
            configuring: Vec::new(),
            players: Vec::new(),
            session_id: None,
            exit_code: None,
            #[cfg(test)]
            place_on_join: false,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Where the console and signals post.
    pub fn control(&self) -> std_mpsc::Sender<Control> {
        self.control_sender.clone()
    }

    /// Ctrl-C and SIGTERM stop the server (Paper's shutdown hook: `halt`, then the loop stops it).
    pub fn listen_for_signals(&self) {
        let control = self.control();
        self.runtime.spawn(async move {
            #[cfg(unix)]
            let signal = {
                use tokio::signal::unix::{SignalKind, signal};
                let Ok(mut term) = signal(SignalKind::terminate()) else {
                    return;
                };
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => Signal::Interrupt,
                    _ = term.recv() => Signal::Terminate,
                }
            };
            #[cfg(not(unix))]
            let signal = match tokio::signal::ctrl_c().await {
                Ok(()) => Signal::Interrupt,
                Err(_) => return,
            };
            let _ = control.send(Control::Signal(signal));
        });
    }

    /// `runServer`: ticks until halted, then stops; returns the process's exit code.
    pub fn run(mut self) -> u8 {
        tracing::info!("Running delayed init tasks");
        tracing::info!(
            "Done ({:.3}s)! For help, type \"help\"",
            self.config.boot.elapsed().as_secs_f64()
        );
        self.clock = TickClock::new(Instant::now(), self.config.tick_loop, &self.rate, false);
        while self.exit_code.is_none() {
            let sprinting = self.tick();
            // Paper never sleeps past the next tick; tasks between ticks come with the levels.
            while let TickAction::Wait(left) = self.clock.next_action(Instant::now()) {
                std::thread::sleep(left);
            }
            if sprinting {
                self.rate.end_tick_work(Instant::now());
            }
        }
        self.stop()
    }

    /// One `processPacketsAndTick`; true for a sprint tick.
    pub fn tick(&mut self) -> bool {
        let start = Instant::now();
        let sprinting = self.clock.start_tick(start, &mut self.rate, false);
        match self
            .empty_pause
            .update(self.players.len(), self.rate.is_sprinting(), false)
        {
            EmptyTick::StartPause { seconds } => {
                tracing::info!("Server empty for {seconds} seconds, pausing");
                self.tick_connection();
            }
            EmptyTick::Paused => self.tick_connection(),
            EmptyTick::Run => {
                self.tick_count = self.tick_count.wrapping_add(1);
                self.rate.tick();
                // tickChildren: the levels are not ported yet; then the connections.
                self.tick_connection();
                let spent = i64::try_from(start.elapsed().as_nanos()).unwrap_or(i64::MAX);
                self.tick_times.record(self.tick_count, spent);
            }
        }
        self.clock.end_tick(Instant::now());
        sprinting
    }

    fn now_ms(&self) -> i64 {
        i64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(i64::MAX)
    }

    /// `DedicatedServer.tickConnection`: the connections, then the console.
    fn tick_connection(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                NetEvent::Login(request) => self.logins.push(PendingLogin {
                    request,
                    waiting: false,
                }),
                NetEvent::Joined(connection) => self.start_configuration(connection),
            }
        }
        self.tick_logins();
        let now = self.now_ms();
        for players in [false, true] {
            let mut peers = std::mem::take(self.peers(players));
            peers.retain_mut(|peer| self.tick_peer(peer, now));
            let list = self.peers(players);
            peers.append(list);
            *list = peers;
        }
        if self.logins.is_empty() && self.configuring.is_empty() && self.players.is_empty() {
            self.session_id = None;
        }
        while let Ok(control) = self.control.try_recv() {
            self.handle_control(control);
        }
    }

    fn peers(&mut self, players: bool) -> &mut Vec<Peer> {
        if players {
            &mut self.players
        } else {
            &mut self.configuring
        }
    }

    /// Drains a peer's packets and runs its keep-alive; false once it is gone.
    fn tick_peer(&mut self, peer: &mut Peer, now_ms: i64) -> bool {
        loop {
            let incoming = match peer.conn.incoming.try_recv() {
                Ok(incoming) => incoming,
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => Incoming::Closed,
            };
            match peer.receive(incoming, now_ms) {
                Ok(PeerState::Open) => {}
                Ok(PeerState::Lost(reason)) => {
                    peer.log_lost(&reason);
                    return false;
                }
                Err(packet) => match peer.conn.state {
                    ConnectionState::Configuration => self.configuration_packet(peer, packet),
                    _ => tracing::debug!(
                        "{}: play packets are not handled yet",
                        peer.conn.profile.name
                    ),
                },
            }
        }
        peer.keep_alive(now_ms);
        true
    }

    /// The login listener's tick for each login the tick thread was asked about.
    fn tick_logins(&mut self) {
        for mut login in std::mem::take(&mut self.logins) {
            if login.request.reply.is_closed() {
                continue; // the client left, or `slow_login`
            }
            let profile = &login.request.forwarded.profile;
            if !login.waiting {
                if let Some(reason) = self.can_player_login(profile) {
                    let _ = login.request.reply.send(LoginVerdict::Deny(reason));
                    continue;
                }
                login.waiting = self.disconnect_all_players_with_profile(&profile.clone());
            }
            let profile = &login.request.forwarded.profile;
            if self.players.iter().any(|p| p.conn.profile.id == profile.id) {
                self.logins.push(login);
                continue;
            }
            let session_id = *self.session_id.get_or_insert_with(Uuid::new_v4);
            let _ = login.request.reply.send(LoginVerdict::Allow { session_id });
        }
    }

    /// `PlayerList.canPlayerLogin` (see the module's notes).
    fn can_player_login(&self, _profile: &GameProfile) -> Option<Reason> {
        let full =
            i64::try_from(self.players.len()).unwrap_or(i64::MAX) >= self.config.max_players.into();
        full.then(|| Reason::translatable("multiplayer.disconnect.server_full"))
    }

    /// `disconnectAllPlayersWithProfile`: true when some had to go.
    fn disconnect_all_players_with_profile(&mut self, profile: &GameProfile) -> bool {
        let by_name = self.config.proxy_online_mode;
        let mut any = false;
        for player in &mut self.players {
            let same = &player.conn.profile;
            if same.id == profile.id || (by_name && same.name.eq_ignore_ascii_case(&profile.name)) {
                player.disconnect(Reason::translatable(
                    "multiplayer.disconnect.duplicate_login",
                ));
                any = true;
            }
        }
        any
    }

    /// Hand-off: a connection that finished logging in, now in the configuration state
    /// (`ServerConfigurationPacketListenerImpl.startConfiguration` goes here).
    pub fn start_configuration(&mut self, connection: Connection) {
        let peer = Peer::new(connection, self.now_ms());
        #[cfg(test)]
        if self.place_on_join {
            self.players.push(peer);
            return;
        }
        self.configuring.push(peer);
    }

    /// Hand-off: a configuration packet other than a keep-alive.
    fn configuration_packet(&mut self, peer: &mut Peer, packet: listener::Packet) {
        tracing::debug!(
            "{}: configuration packet {:#04x} is not handled yet",
            peer.conn.profile.name,
            packet.first().copied().unwrap_or_default()
        );
    }

    /// Hand-off: `PlayerList.placeNewPlayer`, the configuration connection at `index` joins the
    /// player list (counted for `max-players`, checked for duplicates).
    pub fn place_new_player(&mut self, index: usize) {
        let peer = self.configuring.remove(index);
        self.players.push(peer);
    }

    fn handle_control(&mut self, control: Control) {
        match control {
            Control::Signal(signal) => self.halt(match signal {
                Signal::Interrupt => 130,
                Signal::Terminate => 143,
            }),
            Control::Console(line) => {
                // `performPrefixedCommand`: one leading slash is dropped.
                let command = line.trim();
                let command = command.strip_prefix('/').unwrap_or(command);
                match command {
                    "" => {}
                    "stop" => {
                        tracing::info!("Stopping the server");
                        self.halt(0);
                    }
                    _ => tracing::warn!("Unknown command (commands are not ported yet): {command}"),
                }
            }
        }
    }

    /// `halt(false)`: the loop stops after this tick.
    fn halt(&mut self, exit_code: u8) {
        self.exit_code.get_or_insert(exit_code);
    }

    /// `stopServer` as far as there is something to stop: the listener, the players (with
    /// `bukkit.yml`'s shutdown message), then every connection closed (Paper's
    /// `handleAllDisconnections`, which waits up to 30 s for them).
    fn stop(mut self) -> u8 {
        tracing::info!("Stopping server");
        self.accept.abort();
        tracing::info!("Saving players");
        let message = self.config.shutdown_message.clone();
        for player in &mut self.players {
            player.disconnect(message.clone());
        }
        for peer in &self.configuring {
            peer.conn.close();
        }
        // Logins still waiting for an answer: their tasks close when the runtime goes.
        drop(std::mem::take(&mut self.logins));
        let deadline = tokio::time::Instant::now() + CLOSE_WAIT;
        let now = self.now_ms();
        let mut peers = std::mem::take(&mut self.configuring);
        peers.append(&mut self.players);
        for mut peer in peers {
            let lost = self.runtime.block_on(async {
                loop {
                    let incoming =
                        tokio::time::timeout_at(deadline, peer.conn.incoming.recv()).await;
                    match incoming {
                        Err(_) => return None,
                        Ok(message) => {
                            let message = message.unwrap_or(Incoming::Closed);
                            if let Ok(PeerState::Lost(reason)) = peer.receive(message, now) {
                                return Some(reason);
                            }
                        }
                    }
                }
            });
            match lost {
                Some(reason) => peer.log_lost(&reason),
                None => tracing::error!(
                    "Timed out whilst waiting for player connection channel to close"
                ),
            }
        }
        self.runtime.shutdown_timeout(Duration::from_secs(1));
        self.exit_code.unwrap_or(0)
    }
}

/// The console: one thread reading stdin lines into `control` until stdin ends.
pub fn spawn_console(control: std_mpsc::Sender<Control>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("Server console handler".into())
        .spawn(move || {
            for line in std::io::stdin().lines() {
                let sent = match line {
                    Ok(line) => control.send(Control::Console(line)).is_ok(),
                    Err(e) => {
                        tracing::error!("Exception handling console input: {e}");
                        false
                    }
                };
                if !sent {
                    return;
                }
            }
        })
        .map(drop)
}

#[cfg(test)]
mod tests;
