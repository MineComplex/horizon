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
//! Each tick follows `tickServer`: the empty-server pause, the tick rate, the levels (only their
//! chunk holding so far: `ServerChunkCache.tick`'s expired tickets, level updates, the players'
//! chunk loaders, unloads and chunk autosave), then `tickConnection`: new connections and logins
//! (`addPendingConnections`), each connection's tick (login verification, packets, keep-alive,
//! disconnections), then the console's lines; then Paper's incremental save (`level.dat` every
//! `autosavePeriod` ticks). While paused, the levels only unload and autosave chunks; the pause
//! starts with a full save (`autoSave`). The vanilla `ticksUntilAutosave` countdown
//! (`tick_loop::next_autosave_interval`) is never read after Paper's incremental saving, so it is
//! not kept.
//!
//! Logins (`PlayerList.canPlayerLogin` as Leaf left it, and the duplicate check): MineComplex's
//! cleanup removed the ban lists; the whitelist (`whitelist.json`) and the operators' player-limit
//! bypass (`ops.json`) are not ported yet, so nobody is refused for them; a full server refuses with
//! `multiplayer.disconnect.server_full`. Duplicates follow `disconnectAllPlayersWithProfile`
//! (Paper: also by name, ignoring case, when the proxy is in online mode): they are kicked with
//! `multiplayer.disconnect.duplicate_login` and the new login waits until they are gone.
//!
//! Joining (`network::configuration` drives each connection's configuration state, `Server`
//! carries out its actions): [`Server::start_configuration`] receives each connection that finished
//! logging in; on `PrepareSpawn` the player's file is read and the chunks around its spawn are
//! loaded with `CHUNK_LOAD` tickets (`PrepareSpawnTask`, radius 3), then kept by a `PLAYER_SPAWN`
//! ticket; on `Play`, [`Server::place_new_player`] checks for a
//! duplicate and a full server, places the player (`PlayerList.placeNewPlayer`: the join packets of
//! `network::play::join`) and switches the connection to the play state, where `server::player`
//! handles its packets and sends its chunks each tick. Players are saved when they leave and on stop.
//! Keep-alives are Paper's in both states (`configuration::KeepAlive`, carried into play).
//!
//! The overworld's chunk I/O answers are drained at the start of each tick (Paper's
//! `runAllTasksAtTickStart`), also while the server is paused, as Moonrise completes chunk loads on
//! its own executor.
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::bootstrap::ServerConfig;
use super::chunk_system::TicketType;
use super::chunk_system::ticket::{ENTITY_TICKING_LEVEL, FULL_LEVEL};
use super::level::{World, WorldError};
use super::player::{NewPlayer, Player, Spawn, chunk_of};
use super::player_data::SavedPlayer;
use super::tick_loop::{
    EmptyPause, EmptyTick, TickAction, TickClock, TickLoopConfig, TickTimeSamples,
};
use super::tick_rate::TickRateManager;
use crate::command::network::CommandsPacket;
use crate::core::position::ChunkPos;
use crate::network::configuration::{Action, Configuration, PlayHandoff, Settings};
use crate::network::listener::{self, Connection, Incoming, LoginRequest, LoginVerdict, NetEvent};
use crate::network::login::{GameProfile, LoginConfig};
use crate::network::play::chunk_sending::{ViewDistances, api_distances};
use crate::network::play::clientbound::{self as cb, GameType};
use crate::network::play::join::{self, JoinStep};
use crate::network::{ConnectionState, Reason};
use crate::registry::Registries;

/// Leaf's `misc.rebrand.server-mod-name` default (`leaf-global.yml`'s value is not read yet).
const BRAND: &str = "Leaf";
/// `PrepareSpawnTask.PREPARE_CHUNK_RADIUS`.
const PREPARE_CHUNK_RADIUS: i32 = 3;
const PREPARE_TICKET_RADIUS: u8 = 3;
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
    /// `view-distance`, `simulation-distance`: the levels' Moonrise distances.
    pub view_distance: i32,
    pub simulation_distance: i32,
    /// `enforceSecureProfile()`: the property, in online mode.
    pub enforces_secure_chat: bool,
    /// `autosavePeriod` (bukkit.yml's `ticks-per.autosave`, not read yet: 6000).
    pub autosave_period: i32,
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
            view_distance: properties.view_distance,
            simulation_distance: properties.simulation_distance,
            enforces_secure_chat: properties.enforce_secure_profile && properties.online_mode,
            autosave_period: super::config::AUTOSAVE_PERIOD,
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

/// A logged-in connection (the listener's state lives with it: [`Configuring`] or [`Joined`]).
#[derive(Debug)]
struct Peer {
    conn: Connection,
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
    fn new(conn: Connection) -> Self {
        Self {
            conn,
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

    /// One message from the reader task. `Err` hands a packet on to the state's listener.
    fn receive(&mut self, incoming: Incoming) -> Result<PeerState, listener::Packet> {
        Ok(match incoming {
            Incoming::Packet(packet) => {
                if self.closing.is_none() {
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

    /// The next message from the reader task, if any.
    fn next(&mut self) -> Option<Incoming> {
        match self.conn.incoming.try_recv() {
            Ok(incoming) => Some(incoming),
            Err(mpsc::error::TryRecvError::Empty) => None,
            Err(mpsc::error::TryRecvError::Disconnected) => Some(Incoming::Closed),
        }
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

/// `PrepareSpawnTask` between `start` and `Ready`: the chunks around the spawn are held until
/// loaded, then the position is known.
#[derive(Debug)]
struct PreparingSpawn {
    saved: Option<SavedPlayer>,
    center: ChunkPos,
    /// The `CHUNK_LOAD` tickets' identifier, and the chunks still loading.
    load_id: u64,
    loading: Vec<ChunkPos>,
    /// The saved position, else the respawn position (its height found once loaded).
    position: Option<[f64; 3]>,
    respawn: (i32, i32, i32),
    rotation: (f32, f32),
    ready: Option<Spawn>,
}

/// A connection in the configuration state.
#[derive(Debug)]
struct Configuring {
    peer: Peer,
    config: Configuration,
    spawn: Option<PreparingSpawn>,
}

/// A player in the play state.
#[derive(Debug)]
struct Joined {
    peer: Peer,
    player: Player,
}

/// Why the server could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    Bind(#[from] std::io::Error),
    #[error(transparent)]
    World(#[from] WorldError),
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
    /// Read-only once loaded; an `Arc` because the I/O workers' tables and, later, Leaf's async
    /// command-tree builder share startup's data with the tick thread.
    registries: Arc<Registries>,
    world: World,
    level_distances: ViewDistances,
    /// `Entity.ENTITY_COUNTER` (the first id is 1).
    last_entity_id: i32,
    logins: Vec<PendingLogin>,
    configuring: Vec<Configuring>,
    players: Vec<Joined>,
    /// `ServerConnectionListener.getSessionId`: made on first use, forgotten when nobody is connected.
    session_id: Option<Uuid>,
    exit_code: Option<u8>,
}

impl Server {
    /// `initServer` from "Starting Minecraft server on" to the listener running, then the world
    /// (`loadLevel`).
    pub fn start(
        config: RuntimeConfig,
        registries: Arc<Registries>,
        open_world: impl FnOnce() -> Result<World, WorldError>,
    ) -> Result<Self, StartError> {
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
                return Err(e.into());
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
        let world = open_world()?;
        let (control_sender, control) = std_mpsc::channel();
        let rate = TickRateManager::new();
        let now = Instant::now();
        Ok(Self {
            level_distances: ViewDistances::for_level(
                config.view_distance,
                config.simulation_distance,
            ),
            registries,
            world,
            last_entity_id: 0,
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
        self.world.overworld.tick_io();
        match self
            .empty_pause
            .update(self.players.len(), self.rate.is_sprinting(), false)
        {
            EmptyTick::StartPause { seconds } => {
                tracing::info!("Server empty for {seconds} seconds, pausing");
                self.auto_save();
                self.tick_paused_levels();
                self.tick_connection();
            }
            EmptyTick::Paused => {
                self.tick_paused_levels();
                self.tick_connection();
            }
            EmptyTick::Run => {
                self.tick_count = self.tick_count.wrapping_add(1);
                self.rate.tick();
                // tickChildren: the levels, then the connections.
                self.tick_levels();
                self.tick_connection();
                // Paper's incremental saving: player saves are not ported; `level.dat` on a
                // full save (the chunks autosave in the level tick).
                let period = self.config.autosave_period;
                if period > 0 && self.tick_count % period == 0 {
                    self.world.save_level_data();
                }
                let spent = i64::try_from(start.elapsed().as_nanos()).unwrap_or(i64::MAX);
                self.tick_times.record(self.tick_count, spent);
            }
        }
        self.clock.end_tick(Instant::now());
        sprinting
    }

    /// `System.nanoTime` for the listeners (from the server's start).
    fn now_ns(&self) -> i64 {
        i64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(i64::MAX)
    }

    fn settings(&self) -> Settings<'_> {
        Settings {
            registries: &self.registries,
            brand: BRAND,
            enabled_features: &self.world.data.enabled_features,
            // `enable-code-of-conduct` and `codeofconduct/` are not read yet.
            codes_of_conduct: &[],
        }
    }

    /// The overworld's `ServerChunkCache.tick` (the only part of `ServerLevel.tick` ported):
    /// expired tickets, level updates, each player's chunk loader, then unloads and autosave.
    fn tick_levels(&mut self) {
        let now = self.now_ns();
        let level = &mut self.world.overworld;
        level.tick_tickets();
        for j in &mut self.players {
            if j.peer.closing.is_none() {
                j.player.tick_chunks(level, &j.peer.conn, now);
            }
        }
        level.tick_unloads(self.world.data.game_time);
    }

    /// While paused: `getChunkSource().tick(() -> true, false)`, which only processes tickets,
    /// unloads and autosave.
    fn tick_paused_levels(&mut self) {
        let level = &mut self.world.overworld;
        level.tick_tickets();
        level.tick_unloads(self.world.data.game_time);
    }

    /// `MinecraftServer.autoSave`: `saveEverything(true, false, false)`.
    fn auto_save(&mut self) {
        tracing::debug!("Autosave started");
        for joined in &self.players {
            self.save_player(&joined.player);
        }
        self.world.save_all_chunks(true, false);
        tracing::debug!("Autosave finished");
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
        let now = self.now_ns();
        for mut c in std::mem::take(&mut self.configuring) {
            match self.tick_configuring(&mut c, now) {
                Ok(None) => self.configuring.push(c),
                Ok(Some(handoff)) => self.place_new_player(c, *handoff, now),
                Err(reason) => {
                    c.peer.log_lost(&reason);
                    self.release_spawn(&mut c);
                }
            }
        }
        for mut joined in std::mem::take(&mut self.players) {
            match self.tick_joined(&mut joined, now) {
                Ok(()) => self.players.push(joined),
                Err(reason) => self.remove_player(joined, &reason),
            }
        }
        if self.logins.is_empty() && self.configuring.is_empty() && self.players.is_empty() {
            self.session_id = None;
        }
        while let Ok(control) = self.control.try_recv() {
            self.handle_control(control);
        }
    }

    /// A configuring connection's packets, its listener's tick and its spawn preparation:
    /// `Ok(Some)` once it finished, `Err` once it is gone.
    fn tick_configuring(
        &mut self,
        c: &mut Configuring,
        now_ns: i64,
    ) -> Result<Option<Box<PlayHandoff>>, Reason> {
        let mut actions = Vec::new();
        while let Some(incoming) = c.peer.next() {
            match c.peer.receive(incoming) {
                Ok(PeerState::Open) => {}
                Ok(PeerState::Lost(reason)) => return Err(reason),
                Err(packet) => c
                    .config
                    .handle(&packet, &self.settings(), now_ns, &mut actions),
            }
        }
        if let Some(spawn) = &mut c.spawn
            && spawn.ready.is_none()
            && self.prepare_spawn_tick(spawn)
        {
            c.config.spawn_prepared();
        }
        c.config.tick(&self.settings(), now_ns, &mut actions);
        let mut handoff = None;
        for action in actions {
            match action {
                Action::Send(packet) => c.peer.conn.send(packet),
                Action::Disconnect(reason) => c.peer.disconnect(reason),
                Action::PrepareSpawn => c.spawn = Some(self.prepare_spawn(&c.peer.conn.profile)),
                Action::Play(h) => handoff = Some(h),
            }
        }
        Ok(handoff)
    }

    /// `PrepareSpawnTask.start` (with Paper's level choice): the player's file, its level (only
    /// the overworld exists: another saved dimension is Paper's "unknown dimension" case) and
    /// position (saved, else the world's respawn position), then the spawn chunks are held.
    fn prepare_spawn(&mut self, profile: &GameProfile) -> PreparingSpawn {
        let saved = self.world.players.load(profile.id, &profile.name);
        let respawn = &self.world.data.spawn;
        if let Some(dimension) = saved.as_ref().and_then(|s| s.dimension.as_deref())
            && dimension != self.world.overworld.dimension
        {
            tracing::warn!(
                "Unknown respawn dimension ResourceKey[minecraft:dimension / {dimension}], defaulting to overworld"
            );
        }
        let position = saved.as_ref().and_then(|s| s.position);
        let rotation = saved
            .as_ref()
            .and_then(|s| s.rotation)
            .unwrap_or((respawn.yaw, respawn.pitch));
        let p = respawn.pos;
        let center = match position {
            Some(position) => chunk_of(position),
            None => ChunkPos::new(p.x >> 4, p.z >> 4),
        };
        // `trackLoadWithRadius`: Moonrise's `loadChunksAsync` holds each chunk with a
        // `CHUNK_LOAD` ticket at FULL until it is loaded.
        let chunks = &mut self.world.overworld.chunks;
        let load_id = chunks.next_ticket_id();
        let r = PREPARE_CHUNK_RADIUS;
        let loading: Vec<ChunkPos> = (-r..=r)
            .flat_map(|x| (-r..=r).map(move |z| ChunkPos::new(center.x + x, center.z + z)))
            .collect();
        for &pos in &loading {
            chunks.add_ticket(TicketType::ChunkLoad, pos, FULL_LEVEL, load_id);
        }
        chunks.process_ticket_updates();
        PreparingSpawn {
            saved,
            center,
            load_id,
            loading,
            position,
            respawn: (p.x, p.y, p.z),
            rotation,
            ready: None,
        }
    }

    /// `Preparing.tick`: ready once every spawn chunk is loaded (or known missing): each chunk's
    /// `CHUNK_LOAD` ticket goes as it loads, then `PLAYER_SPAWN` keeps the area. A new player
    /// stands on the top of the respawn column (`PlayerSpawnFinder` reduced to the heightmap at
    /// the respawn position, see `Level::spawn_height`; its search over the respawn radius and
    /// its collision checks need the physics), or at the respawn position when that chunk is missing.
    fn prepare_spawn_tick(&mut self, spawn: &mut PreparingSpawn) -> bool {
        let level = &mut self.world.overworld;
        let id = spawn.load_id;
        spawn.loading.retain(|&pos| {
            let loading = level.is_loading(pos);
            if !loading {
                level
                    .chunks
                    .remove_ticket(TicketType::ChunkLoad, pos, FULL_LEVEL, id);
            }
            loading
        });
        if !spawn.loading.is_empty() {
            return false;
        }
        level.chunks.add_ticket_with_radius(
            TicketType::PlayerSpawn,
            spawn.center,
            PREPARE_TICKET_RADIUS,
        );
        let (x, y, z) = spawn.respawn;
        let position = spawn.position.unwrap_or_else(|| {
            let y = level.spawn_height(x, z).unwrap_or(y);
            [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5]
        });
        spawn.ready = Some(Spawn {
            position,
            y_rot: spawn.rotation.0,
            x_rot: spawn.rotation.1,
        });
        true
    }

    /// A preparation that ends: the `CHUNK_LOAD` tickets of chunks still loading go (Moonrise
    /// removes each when its load completes; the load still completes here).
    fn release_spawn(&mut self, c: &mut Configuring) {
        if let Some(spawn) = c.spawn.take() {
            let chunks = &mut self.world.overworld.chunks;
            for pos in spawn.loading {
                chunks.remove_ticket(TicketType::ChunkLoad, pos, FULL_LEVEL, spawn.load_id);
            }
        }
    }

    /// A player's packets, then its tick; `Err` once it is gone.
    fn tick_joined(&mut self, j: &mut Joined, now_ns: i64) -> Result<(), Reason> {
        let level = &mut self.world.overworld;
        while let Some(incoming) = j.peer.next() {
            match j.peer.receive(incoming) {
                Ok(PeerState::Open) => {}
                Ok(PeerState::Lost(reason)) => return Err(reason),
                Err(packet) => {
                    if let Err(reason) = j.player.handle(&packet, level, &j.peer.conn, now_ns) {
                        j.peer.disconnect(reason);
                    }
                }
            }
        }
        if j.peer.closing.is_none()
            && let Err(reason) = j.player.tick(&j.peer.conn, now_ns)
        {
            j.peer.disconnect(reason);
        }
        Ok(())
    }

    /// `PlayerList.remove`: saved, its chunks released.
    fn remove_player(&mut self, mut j: Joined, reason: &Reason) {
        j.peer.log_lost(reason);
        self.save_player(&j.player);
        j.player.remove(&mut self.world.overworld);
    }

    fn save_player(&self, player: &Player) {
        let data = player.saved_data(self.world.overworld.dimension);
        let profile = &player.profile;
        self.world.players.save(profile.id, &profile.name, &data);
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
            if self
                .players
                .iter()
                .any(|p| p.player.profile.id == profile.id)
            {
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
        for joined in &mut self.players {
            let same = &joined.player.profile;
            if same.id == profile.id || (by_name && same.name.eq_ignore_ascii_case(&profile.name)) {
                joined.peer.disconnect(Reason::translatable(
                    "multiplayer.disconnect.duplicate_login",
                ));
                any = true;
            }
        }
        any
    }

    /// Hand-off: a connection that finished logging in, now in the configuration state
    /// (`ServerConfigurationPacketListenerImpl.startConfiguration`).
    pub fn start_configuration(&mut self, connection: Connection) {
        let mut actions = Vec::new();
        let config = Configuration::start(
            connection.profile.clone(),
            connection.transferred,
            &self.settings(),
            self.now_ns(),
            &mut actions,
        );
        let mut peer = Peer::new(connection);
        for action in actions {
            match action {
                Action::Send(packet) => peer.conn.send(packet),
                Action::Disconnect(reason) => peer.disconnect(reason),
                other => unreachable!("not at the start: {other:?}"),
            }
        }
        self.configuring.push(Configuring {
            peer,
            config,
            spawn: None,
        });
    }

    /// `handleConfigurationFinished`, then `PrepareSpawnTask.spawnPlayer` and
    /// `PlayerList.placeNewPlayer`.
    fn place_new_player(&mut self, mut c: Configuring, handoff: PlayHandoff, now_ns: i64) {
        let profile = handoff.profile.clone();
        let refusal = if self
            .players
            .iter()
            .any(|p| p.player.profile.id == profile.id)
        {
            Some(Reason::translatable(
                "multiplayer.disconnect.duplicate_login",
            ))
        } else {
            self.can_player_login(&profile)
        };
        let ready = c.spawn.as_ref().and_then(|s| s.ready.clone());
        let (Some(spawn), None) = (ready, refusal.clone()) else {
            c.peer.disconnect(refusal.unwrap_or(Reason::translatable(
                "multiplayer.disconnect.invalid_player_data",
            )));
            self.release_spawn(&mut c);
            self.configuring.push(c);
            return;
        };
        let saved = c
            .spawn
            .as_mut()
            .and_then(|s| s.saved.take())
            .unwrap_or_default();
        // `calculateGameModeForNewPlayer`: the saved mode, else the world's (`force-gamemode` is
        // not read yet).
        let world_mode = game_type_by_id(self.world.data.game_type as u8);
        // Paper's `PLAYER_JOIN` ticket, before the level info.
        self.world.overworld.chunks.add_ticket(
            TicketType::PlayerJoin,
            chunk_of(spawn.position),
            ENTITY_TICKING_LEVEL,
            0,
        );
        self.last_entity_id += 1;
        let player = Player::new(
            NewPlayer {
                profile,
                entity_id: self.last_entity_id,
                spawn,
                game_type: saved.game_type.unwrap_or(world_mode),
                previous_game_type: saved.previous_game_type,
                client_information: handoff.client_information,
                keep_alive: handoff.keep_alive,
                latency: handoff.latency,
                saved,
                level_distances: self.level_distances,
            },
            &mut self.world.overworld,
            &c.peer.conn,
            now_ns,
        );
        // The player's chunk loader holds its chunks now.
        self.release_spawn(&mut c);
        c.peer.conn.state = ConnectionState::Play;
        let [x, y, z] = player
            .position
            .map(crate::util::number_text::double_to_string);
        tracing::info!(
            "{}[/{}] logged in with entity id {} at ([{}]{x}, {y}, {z})",
            player.profile.name,
            c.peer.conn.address,
            player.entity_id,
            self.world.overworld.dimension,
        );
        let joined = Joined {
            peer: c.peer,
            player,
        };
        match self.join_steps(&joined.player) {
            Ok(steps) => self.send_join(&joined, steps),
            Err(e) => tracing::error!("Couldn't place player in world: {e}"),
        }
        self.players.push(joined);
    }

    /// The join packets (`network::play::join`) for this server.
    fn join_steps(&self, player: &Player) -> Result<Vec<JoinStep>, crate::nbt::Error> {
        use crate::network::play::clientbound::{
            Abilities, Border, Login, PlayerInfoEntry, SpawnInfo,
        };
        let world = &self.world;
        let level = &world.overworld;
        let (view, simulation) = api_distances(self.level_distances);
        let levels = [level.dimension];
        let mode = player.game_type;
        let creative = mode == GameType::Creative;
        let spectator = mode == GameType::Spectator;
        /// `createPlayerInitializing`'s entry (display names, list order and chat sessions are
        /// not ported).
        fn entry(p: &Player) -> PlayerInfoEntry<'_> {
            PlayerInfoEntry {
                profile: &p.profile,
                listed: true,
                latency: p.latency,
                game_mode: p.game_type,
                display_name: None,
                show_hat: p.client_information.model_customisation & 0x40 != 0,
                list_order: 0,
                chat_session: None,
            }
        }
        let own = entry(player);
        let others: Vec<PlayerInfoEntry> = self.players.iter().map(|j| entry(&j.player)).collect();
        let weather = &world.weather;
        let spawn = &world.data.spawn;
        join::join_sequence(&join::JoinInputs {
            login: Login {
                player_id: player.entity_id,
                hardcore: world.data.hardcore,
                levels: &levels,
                max_players: self.config.max_players,
                chunk_radius: view,
                simulation_distance: simulation,
                // Game rules are not ported: their defaults.
                reduced_debug_info: false,
                show_death_screen: true,
                do_limited_crafting: false,
                spawn: SpawnInfo {
                    dimension_type: level.dimension_type,
                    dimension: level.dimension,
                    seed: obfuscate_seed(world.seed),
                    game_type: mode,
                    previous_game_type: player.previous_game_type,
                    is_debug: false,
                    is_flat: world.flat,
                    last_death_location: None,
                    portal_cooldown: 0,
                    // The noise settings are not read: the overworld's and flat's sea levels.
                    sea_level: if world.flat { -63 } else { 63 },
                },
                online_mode: self.config.online_mode,
                enforces_secure_chat: self.config.enforces_secure_chat,
            },
            difficulty: world.data.difficulty as u8,
            difficulty_locked: world.data.difficulty_locked,
            abilities: Abilities {
                invulnerable: creative || spectator,
                flying: spectator,
                may_fly: creative || spectator,
                instabuild: creative,
                flying_speed: 0.05,
                walking_speed: 0.1,
            },
            selected_slot: 0,
            // `ops.json` is not ported.
            permission_level: 0,
            teleport_id: 1,
            position: player.position,
            y_rot: player.y_rot,
            x_rot: player.x_rot,
            level: join::LevelInfo {
                // `world_border.dat` is not read yet: a new border.
                border: Border::default(),
                game_time: world.data.game_time,
                // The world clocks are not ported.
                clocks: &[],
                spawn_dimension: &spawn.dimension,
                spawn_pos: spawn.pos,
                spawn_yaw: spawn.yaw,
                spawn_pitch: spawn.pitch,
                raining: weather.raining,
                rain_level: if weather.raining { 1.0 } else { 0.0 },
                thunder_level: if weather.raining && weather.thundering {
                    1.0
                } else {
                    0.0
                },
                tick_rate: self.rate.tick_rate(),
                frozen: self.rate.is_frozen(),
                frozen_ticks_to_run: self.rate.frozen_ticks_to_run(),
            },
            own_info: own,
            others: &others,
            // Leaf's join message settings are not read yet: no message.
            join_message: None,
            distances: player.distances(),
            chunk: chunk_of(player.position),
        })
    }

    fn send_join(&self, joined: &Joined, steps: Vec<JoinStep>) {
        for step in steps {
            match step {
                JoinStep::Packet(packet) => joined.peer.conn.send(packet),
                JoinStep::ToOthers(packet) => {
                    for other in &self.players {
                        other.peer.conn.send(packet.clone());
                    }
                }
                // No command is ported: Paper's empty tree (only the root).
                JoinStep::Commands => joined
                    .peer
                    .conn
                    .send(cb::commands(&CommandsPacket::empty())),
                missing => tracing::debug!("Join step {missing:?} skipped: not ported yet"),
            }
        }
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
        // `saveAll`, then `removeAll` (Paper: the shutdown message; their saves already done).
        tracing::info!("Saving players");
        let message = self.config.shutdown_message.clone();
        let mut peers = Vec::new();
        for mut joined in std::mem::take(&mut self.players) {
            self.save_player(&joined.player);
            joined.player.remove(&mut self.world.overworld);
            joined.peer.disconnect(message.clone());
            peers.push(joined.peer);
        }
        for mut c in std::mem::take(&mut self.configuring) {
            self.release_spawn(&mut c);
            c.peer.conn.close();
            peers.push(c.peer);
        }
        // Logins still waiting for an answer: their tasks close when the runtime goes.
        drop(std::mem::take(&mut self.logins));
        tracing::info!("Saving worlds");
        let deadline = tokio::time::Instant::now() + CLOSE_WAIT;
        for mut peer in peers {
            let lost = self.runtime.block_on(async {
                loop {
                    let incoming =
                        tokio::time::timeout_at(deadline, peer.conn.incoming.recv()).await;
                    match incoming {
                        Err(_) => return None,
                        Ok(message) => {
                            let message = message.unwrap_or(Incoming::Closed);
                            if let Ok(PeerState::Lost(reason)) = peer.receive(message) {
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
        self.world.close();
        self.runtime.shutdown_timeout(Duration::from_secs(1));
        self.exit_code.unwrap_or(0)
    }
}

/// `BiomeManager.obfuscateSeed`: the first 8 bytes of SHA-256 over the seed, both little-endian
/// (Guava's `hashLong`/`asLong`).
fn obfuscate_seed(seed: i64) -> i64 {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(seed.to_le_bytes());
    i64::from_le_bytes(hash[..8].try_into().expect("8 bytes"))
}

/// `GameType.byId` over the network ids.
fn game_type_by_id(id: u8) -> GameType {
    match id {
        1 => GameType::Creative,
        2 => GameType::Adventure,
        3 => GameType::Spectator,
        _ => GameType::Survival,
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
