//! One connection: the status ping, or a player's login (InitialLoginSessionHandler,
//! AuthSessionHandler) and then the relay between the client and one backend at a time.
//!
//! Each player is a task on the event loop that owns both writers and the state; reader tasks
//! (the client, the current backend) hand it packets as [`Event`]s. A new server is reached by a
//! task of its own (the backend LoginSessionHandler), then the client is moved over through the
//! configuration state, as the fork does for 26.3 clients.
use crate::{
    ProxyHandle, Shared,
    auth::{AuthResult, MAX_TOKEN_BYTES, TOKEN_CHANNEL},
    forwarding, log,
    net::{self, Conn, ConnReader, ConnWriter, body},
    players::{Mailbox, Order, PlayerInfo, Ticket},
    plugin::{CommandResult, KickResult, Kicked, Login, Ping, PreLogin, Source},
    text,
};
use horizon_protocol::{
    GAME_VERSION, PROTOCOL_VERSION,
    buf::{Codec, Reader, Writer},
    crypto, ids,
    packets::{
        handshake, login,
        play::{CommandNode, Commands},
        status,
    },
};
use horizon_world::nbt::Tag;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::TcpStream,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task::JoinHandle,
};

/// How long a client may take to log in (Velocity's login timeout).
const LOGIN_TIMEOUT: Duration = Duration::from_secs(30);
/// The login query that asks for the launcher's token.
const TOKEN_QUERY: i32 = 0;
/// Failed connections in a row before the player is let go.
const MAX_ATTEMPTS: u32 = 8;
/// ArgumentTypeInfos id of `brigadier:string`.
const STRING_ARGUMENT: i32 = 5;

use ids::configuration::{clientbound as config_cb, serverbound as config_sb};
use ids::play::{clientbound as play_cb, serverbound as play_sb};

pub async fn serve(mut stream: TcpStream, peer: SocketAddr, shared: Arc<Shared>) {
    let address = if shared.config.advanced.haproxy_protocol {
        match tokio::time::timeout(LOGIN_TIMEOUT, net::read_proxy_header(&mut stream)).await {
            Ok(Ok(Some(address))) => address,
            _ => return,
        }
    } else {
        peer
    };
    let mut c = Conn::new(stream);
    let Ok((id, payload)) = c.recv_within(LOGIN_TIMEOUT).await else {
        return;
    };
    let Ok(handshake::Serverbound::Intention(intent)) =
        handshake::Serverbound::decode(id, &payload)
    else {
        return;
    };
    match intent.intent {
        handshake::STATUS => {
            if shared.config.advanced.show_ping_requests {
                log(&format!("Ping from {}", shared.address(address)));
            }
            status(c, &shared).await;
        }
        handshake::LOGIN | handshake::TRANSFER => {
            if let Err(e) = player_login(c, address, &intent, &shared).await {
                log(&format!(
                    "{} disconnected while logging in: {e}",
                    shared.address(address)
                ));
            }
        }
        _ => {}
    }
}

/// StatusSessionHandler and ProxyPingEvent: the proxy answers the server list itself.
async fn status(mut c: Conn, shared: &Shared) {
    while let Ok((id, payload)) = c.recv_within(LOGIN_TIMEOUT).await {
        match status::Serverbound::decode(id, &payload) {
            Ok(status::Serverbound::StatusRequest(_)) => {
                let json = status_json(shared);
                let packet = status::Clientbound::StatusResponse(status::StatusResponse { json });
                if c.send(&packet.encode()).await.is_err() {
                    return;
                }
            }
            Ok(status::Serverbound::PingRequest(ping)) => {
                let _ = c
                    .send(&status::Clientbound::PongResponse(ping).encode())
                    .await;
                return;
            }
            _ => return,
        }
    }
}

/// The server list entry, after the plugins.
pub fn status_json(shared: &Shared) -> String {
    let sample = if shared.config.sample_players_in_ping {
        shared
            .players
            .sample(12)
            .into_iter()
            .map(|(u, n)| (n, u))
            .collect()
    } else {
        Vec::new()
    };
    let mut ping = Ping {
        version_name: GAME_VERSION.into(),
        protocol: PROTOCOL_VERSION,
        online: shared.players.count() as i64,
        max: i64::from(shared.config.show_max_players),
        sample,
        description: text::mini_message(&shared.config.motd),
    };
    for plugin in &shared.plugins {
        plugin.ping(&mut ping);
    }
    let sample: Vec<Value> = ping
        .sample
        .iter()
        .map(|(name, id)| json!({"name": name, "id": crypto::uuid_string(*id)}))
        .collect();
    let mut players = json!({"max": ping.max, "online": ping.online});
    if !sample.is_empty() {
        players["sample"] = json!(sample);
    }
    json!({
        "version": {"name": ping.version_name, "protocol": ping.protocol},
        "players": players,
        "description": ping.description,
        "enforcesSecureChat": false,
    })
    .to_string()
}

async fn login_disconnect(c: &mut Conn, reason: &Value) {
    let packet = login::Clientbound::Disconnect(login::Disconnect {
        reason_json: reason.to_string(),
    });
    let _ = c.send(&packet.encode()).await;
}

/// StringUtil.isValidPlayerName.
fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.chars().all(|c| c > ' ' && c < '\u{7f}')
}

async fn player_login(
    mut c: Conn,
    address: SocketAddr,
    intent: &handshake::Intention,
    shared: &Arc<Shared>,
) -> Result<(), String> {
    let e = |e: horizon_protocol::Error| e.0;
    if intent.protocol != PROTOCOL_VERSION {
        let reason =
            json!({"translate": "multiplayer.disconnect.incompatible", "with": [GAME_VERSION]});
        login_disconnect(&mut c, &reason).await;
        return Ok(());
    }
    if !shared.login_allowed(address.ip()) {
        let reason = json!({"text": "Вы входите слишком часто, попробуйте позже.", "color": "red"});
        login_disconnect(&mut c, &reason).await;
        return Ok(());
    }
    let (id, payload) = c.recv_within(LOGIN_TIMEOUT).await?;
    let login::Serverbound::Hello(hello) = login::Serverbound::decode(id, &payload).map_err(e)?
    else {
        return Err("expected a login hello".into());
    };
    if !valid_name(&hello.name) {
        let reason = json!({"translate": "multiplayer.disconnect.invalid_player_data"});
        login_disconnect(&mut c, &reason).await;
        return Ok(());
    }
    let mut pre = PreLogin {
        address,
        name: hello.name.clone(),
        denied: None,
    };
    for plugin in &shared.plugins {
        plugin.pre_login(&mut pre).await;
        if pre.denied.is_some() {
            break;
        }
    }
    if let Some(reason) = pre.denied {
        login_disconnect(&mut c, &text::to_json(&reason)).await;
        return Ok(());
    }

    // generateEncryptionRequest: the client sends its token to the proxy, not to a session server.
    let mut verify = [0u8; 4];
    crypto::random_bytes(&mut verify).map_err(e)?;
    c.send(
        &login::Clientbound::Hello(login::Hello {
            server_id: String::new(),
            public_key: shared.public_der.clone(),
            challenge: verify.to_vec(),
            should_authenticate: false,
        })
        .encode(),
    )
    .await?;
    let (id, payload) = c.recv_within(LOGIN_TIMEOUT).await?;
    let login::Serverbound::Key(key) = login::Serverbound::decode(id, &payload).map_err(e)? else {
        return Err("expected an encryption response".into());
    };
    if shared.keys.decrypt(&key.challenge).ok().as_deref() != Some(&verify[..]) {
        return Err("Unable to successfully decrypt the verification token.".into());
    }
    let secret: [u8; 16] = shared
        .keys
        .decrypt(&key.key)
        .ok()
        .and_then(|s| s.try_into().ok())
        .ok_or("bad shared secret")?;
    c.enable_encryption(&secret);

    // requestToken: on the encrypted connection; a client that doesn't know the channel answers
    // without a body.
    c.send(
        &login::Clientbound::CustomQuery(login::CustomQuery {
            transaction: TOKEN_QUERY,
            channel: TOKEN_CHANNEL.into(),
            data: Vec::new(),
        })
        .encode(),
    )
    .await?;
    let token = loop {
        let (id, payload) = c.recv_within(LOGIN_TIMEOUT).await?;
        match login::Serverbound::decode(id, &payload).map_err(e)? {
            login::Serverbound::CustomQueryAnswer(a) if a.transaction == TOKEN_QUERY => {
                break a
                    .payload
                    .filter(|p| !p.is_empty() && p.len() <= MAX_TOKEN_BYTES)
                    .and_then(|p| String::from_utf8(p).ok());
            }
            login::Serverbound::CustomQueryAnswer(_) | login::Serverbound::CookieResponse(_) => {}
            _ => return Err("unexpected packet while waiting for the token".into()),
        }
    };
    let profile = match shared
        .auth
        .authenticate(address.ip(), &hello.name, token.as_deref())
        .await
    {
        AuthResult::Allowed(p) if p.name.eq_ignore_ascii_case(&hello.name) => p,
        AuthResult::Allowed(p) => {
            log(&format!(
                "The auth handler returned the profile {} for {}",
                p.name, hello.name
            ));
            login_disconnect(&mut c, &text::to_json(&crate::auth::unavailable())).await;
            return Ok(());
        }
        AuthResult::Denied(reason) => {
            log(&format!(
                "{} ({}) was refused: {}",
                hello.name,
                shared.address(address),
                text::plain(&reason)
            ));
            login_disconnect(&mut c, &text::to_json(&reason)).await;
            return Ok(());
        }
    };

    // AuthSessionHandler: register, LoginEvent, then LoginFinished.
    let info = PlayerInfo {
        uuid: profile.uuid,
        name: profile.name.clone(),
        address,
        properties: profile.properties.clone(),
    };
    let (tx, events) = unbounded_channel();
    let mailbox_tx = tx.clone();
    let mailbox: Mailbox = Arc::new(move |o| mailbox_tx.send(Event::Order(o)).is_ok());
    let kick = shared
        .config
        .kick_existing_players
        .then(|| text::from_json(&json!({"translate": "multiplayer.disconnect.duplicate_login"})));
    let Some(ticket) = shared.players.join(info.clone(), mailbox, kick) else {
        let reason = json!({"text": "Игрок с таким ником уже играет на сервере!", "color": "red"});
        login_disconnect(&mut c, &reason).await;
        return Ok(());
    };
    let mut event = Login {
        player: info.clone(),
        denied: None,
    };
    for plugin in &shared.plugins {
        plugin.login(&mut event).await;
        if event.denied.is_some() {
            break;
        }
    }
    if let Some(reason) = event.denied {
        login_disconnect(&mut c, &text::to_json(&reason)).await;
        drop(ticket);
        for plugin in &shared.plugins {
            plugin.disconnect(&info, false).await;
        }
        return Ok(());
    }
    let result = finish_login(&mut c, shared, &profile).await;
    let who = format!(
        "[connected player] {} ({})",
        profile.name,
        shared.address(address)
    );
    let log_connections = shared.config.advanced.log_player_connections;
    if result.is_ok() {
        if log_connections {
            log(&format!("{who} has connected"));
        }
        c.reader.limiter = net::Limiter::new(shared.config.packet_limiter.limits());
        let Conn { reader, writer } = c;
        let session = Session {
            shared: shared.clone(),
            tx,
            client: writer,
            client_task: None,
            info: info.clone(),
            who: who.clone(),
            client_reads: Phase::Configuration,
            client_writes: Phase::Configuration,
            brand: None,
            information: None,
            backend: None,
            pending: None,
            generation: 0,
            attempts: 0,
            tried: Vec::new(),
            boss_bars: BTreeSet::new(),
            notices: Vec::new(),
            ticket,
            closed: false,
            last_command: None,
            limited_commands: 0,
            last_tab: None,
            limited_tabs: 0,
        };
        session.run(reader, events).await;
        if log_connections {
            log(&format!("{who} has disconnected"));
        }
    } else {
        drop(ticket);
    }
    for plugin in &shared.plugins {
        plugin.disconnect(&info, true).await;
    }
    result
}

async fn finish_login(
    c: &mut Conn,
    shared: &Shared,
    profile: &login::GameProfile,
) -> Result<(), String> {
    let e = |e: horizon_protocol::Error| e.0;
    let threshold = shared.config.advanced.compression_threshold;
    if threshold >= 0 {
        c.send(&login::Clientbound::Compression(login::Compression { threshold }).encode())
            .await?;
        c.set_compression(threshold);
    }
    c.send(
        &login::Clientbound::LoginFinished(login::LoginFinished {
            profile: profile.clone(),
            session: shared.session_id,
        })
        .encode(),
    )
    .await?;
    loop {
        let (id, payload) = c.recv_within(LOGIN_TIMEOUT).await?;
        match login::Serverbound::decode(id, &payload).map_err(e)? {
            login::Serverbound::LoginAcknowledged(_) => return Ok(()),
            login::Serverbound::CookieResponse(_) | login::Serverbound::CustomQueryAnswer(_) => {}
            _ => return Err("expected the login acknowledgement".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Configuration,
    Play,
}

/// A refused connection: what went wrong, and the server's own reason when it gave one.
type Refusal = (String, Option<Tag>);

enum Event {
    Client(i32, Vec<u8>),
    ClientGone(String),
    Backend(u64, i32, Vec<u8>),
    BackendGone(u64, String),
    Connected(u64, Result<Conn, Refusal>),
    Order(Order),
}

/// Why the player is being connected to a server.
enum Reason {
    /// Joining the proxy.
    Initial,
    /// `/server`, `/send` or a plugin.
    Command,
    /// The last server kicked them or went away; the notice says so once they are in.
    Fallback { notice: Option<Tag> },
}

struct Pending {
    generation: u64,
    server: String,
    reason: Reason,
    /// Logged in to the server, waiting for the client to leave the play state.
    ready: Option<Conn>,
}

struct Backend {
    server: String,
    generation: u64,
    writer: ConnWriter,
    task: JoinHandle<()>,
    /// The state the backend reads (what we may send it) and writes (how its packets read).
    reads: Phase,
    writes: Phase,
}
impl Backend {
    async fn close(mut self) {
        self.task.abort();
        self.writer.shutdown().await;
    }
}

struct Session {
    shared: Arc<Shared>,
    tx: UnboundedSender<Event>,
    client: ConnWriter,
    client_task: Option<JoinHandle<()>>,
    info: PlayerInfo,
    who: String,
    /// The state the client reads (changes when it is sent StartConfiguration or acknowledges
    /// FinishConfiguration) and writes.
    client_reads: Phase,
    client_writes: Phase,
    /// The client's `minecraft:brand` payload and ClientInformation, sent once at login and
    /// replayed to every server it moves to.
    brand: Option<Vec<u8>>,
    information: Option<Vec<u8>>,
    backend: Option<Backend>,
    pending: Option<Pending>,
    generation: u64,
    /// Connections that failed in a row.
    attempts: u32,
    /// Servers tried since the player last got into one.
    tried: Vec<String>,
    /// Boss bars the server showed, removed on a switch (the client keeps them).
    boss_bars: BTreeSet<u128>,
    /// Messages waiting for the client to be in the play state.
    notices: Vec<Tag>,
    ticket: Ticket,
    closed: bool,
    last_command: Option<Instant>,
    limited_commands: i32,
    last_tab: Option<Instant>,
    limited_tabs: i32,
}

fn spawn_reader(
    mut reader: ConnReader,
    tx: UnboundedSender<Event>,
    backend: Option<u64>,
    timeout: Option<Duration>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let event = match (reader.recv_within(timeout).await, backend) {
                (Ok((id, payload)), None) => Event::Client(id, payload),
                (Ok((id, payload)), Some(g)) => Event::Backend(g, id, payload),
                (Err(e), None) => Event::ClientGone(e),
                (Err(e), Some(g)) => Event::BackendGone(g, e),
            };
            let last = matches!(event, Event::ClientGone(_) | Event::BackendGone(..));
            if tx.send(event).is_err() || last {
                return;
            }
        }
    })
}

impl Session {
    fn handle(&self) -> ProxyHandle {
        ProxyHandle(self.shared.clone())
    }

    async fn run(mut self, reader: ConnReader, mut events: UnboundedReceiver<Event>) {
        let timeout = self.shared.config.read_timeout();
        self.client_task = Some(spawn_reader(reader, self.tx.clone(), None, timeout));
        // connectToInitialServer and PlayerChooseInitialServerEvent.
        let mut initial = self.next_server();
        for plugin in &self.shared.plugins {
            plugin.choose_initial_server(&self.info, &mut initial);
        }
        match initial {
            Some(server) => self.connect(server, Reason::Initial),
            None => self.disconnect(no_servers()).await,
        }
        while !self.closed {
            let Some(event) = events.recv().await else {
                break;
            };
            self.on_event(event).await;
        }
        if let Some(task) = self.client_task.take() {
            task.abort();
        }
        self.client.shutdown().await;
        if let Some(b) = self.backend.take() {
            b.close().await;
        }
        if let Some(mut c) = self.pending.take().and_then(|p| p.ready) {
            c.writer.shutdown().await;
        }
    }

    async fn on_event(&mut self, event: Event) {
        let current = |s: &Self, g: u64| s.backend.as_ref().is_some_and(|b| b.generation == g);
        match event {
            Event::Client(id, payload) => self.on_client(id, payload).await,
            Event::ClientGone(error) => {
                if error.contains("too many") {
                    log(&format!("{} was disconnected: {error}", self.who));
                }
                self.closed = true;
            }
            Event::Backend(g, id, payload) if current(self, g) => {
                self.on_backend(id, payload).await
            }
            Event::BackendGone(g, error) if current(self, g) => self.backend_lost(&error).await,
            Event::Backend(..) | Event::BackendGone(..) => {}
            Event::Connected(g, result) => self.connected(g, result).await,
            Event::Order(order) => self.order(order).await,
        }
    }

    async fn order(&mut self, order: Order) {
        match order {
            Order::Kick(reason) => self.disconnect(reason).await,
            Order::Send(server) => self.switch(&server).await,
            Order::Message(message) => self.message(message).await,
            Order::Title { title, subtitle } => self.title(title, subtitle).await,
            Order::Properties(properties) => {
                self.ticket.set_properties(&properties);
                self.info.properties = properties;
            }
        }
    }

    // ---- the client ----

    async fn on_client(&mut self, id: i32, payload: Vec<u8>) {
        match self.client_writes {
            Phase::Configuration => {
                if id == config_sb::CLIENT_INFORMATION {
                    self.information = Some(payload.clone());
                } else if id == config_sb::CUSTOM_PAYLOAD
                    && Reader::new(&payload).string(32767).ok().as_deref()
                        == Some("minecraft:brand")
                {
                    self.brand = Some(payload.clone());
                }
                self.send_backend(Phase::Configuration, id, &payload).await;
                if id == config_sb::FINISH_CONFIGURATION {
                    self.client_writes = Phase::Play;
                    self.client_reads = Phase::Play;
                    if let Some(b) = &mut self.backend {
                        b.reads = Phase::Play;
                        b.writes = Phase::Play;
                    }
                }
            }
            Phase::Play => {
                match id {
                    play_sb::CONFIGURATION_ACKNOWLEDGED => {
                        self.client_writes = Phase::Configuration;
                        if let Some(mut pending) = self.pending.take_if(|p| p.ready.is_some()) {
                            let conn = pending.ready.take().expect("ready");
                            self.install(pending, conn).await;
                        } else {
                            // The server itself asked for a new configuration.
                            self.send_backend(Phase::Play, id, &payload).await;
                            if let Some(b) = &mut self.backend {
                                b.reads = Phase::Configuration;
                            }
                        }
                        return;
                    }
                    play_sb::CHAT_COMMAND | play_sb::CHAT_COMMAND_SIGNED => {
                        let line = Reader::new(&payload).string(32767).unwrap_or_default();
                        if self.command(&line).await {
                            return;
                        }
                    }
                    play_sb::COMMAND_SUGGESTION => {
                        if self.suggestions(&payload).await {
                            return;
                        }
                    }
                    play_sb::CLIENT_INFORMATION => self.information = Some(payload.clone()),
                    _ => {}
                }
                self.send_backend(Phase::Play, id, &payload).await;
            }
        }
    }

    async fn send_client(&mut self, id: i32, payload: &[u8]) {
        if self.client.send(&body(id, payload)).await.is_err() {
            self.closed = true;
        }
    }

    /// A chat message now, or once the client is back in the play state.
    async fn message(&mut self, message: Tag) {
        if self.client_reads == Phase::Play {
            let mut w = Writer::new();
            w.nbt(&message);
            w.bool(false);
            self.send_client(play_cb::SYSTEM_CHAT, &w.data).await;
        } else {
            self.notices.push(message);
        }
    }

    /// Player.showTitle with adventure's default times (0.5 s, 3.5 s, 1 s).
    async fn title(&mut self, title: Tag, subtitle: Tag) {
        if self.client_reads != Phase::Play {
            return;
        }
        let mut times = Writer::new();
        for ticks in [10, 70, 20] {
            times.i32(ticks);
        }
        self.send_client(play_cb::SET_TITLES_ANIMATION, &times.data)
            .await;
        for (id, text) in [
            (play_cb::SET_SUBTITLE_TEXT, subtitle),
            (play_cb::SET_TITLE_TEXT, title),
        ] {
            let mut w = Writer::new();
            w.nbt(&text);
            self.send_client(id, &w.data).await;
        }
    }

    async fn disconnect(&mut self, reason: Tag) {
        let id = match self.client_reads {
            Phase::Configuration => config_cb::DISCONNECT,
            Phase::Play => play_cb::DISCONNECT,
        };
        let mut w = Writer::new();
        w.nbt(&reason);
        let _ = self.client.send(&body(id, &w.data)).await;
        if self.shared.config.advanced.log_player_connections {
            log(&format!(
                "{} was disconnected: {}",
                self.who,
                text::plain(&reason)
            ));
        }
        self.closed = true;
    }

    // ---- the backend ----

    async fn send_backend(&mut self, phase: Phase, id: i32, payload: &[u8]) {
        let Some(b) = &mut self.backend else { return };
        if b.reads != phase {
            return;
        }
        if let Err(e) = b.writer.send(&body(id, payload)).await {
            self.backend_lost(&e).await;
        }
    }

    async fn on_backend(&mut self, id: i32, payload: Vec<u8>) {
        // The client is leaving the play state for another server.
        if self.pending.as_ref().is_some_and(|p| p.ready.is_some()) {
            return;
        }
        let announce = self.shared.config.advanced.announce_proxy_commands;
        let Some(b) = &mut self.backend else { return };
        let mut joined = false;
        let mut payload = payload;
        match b.writes {
            Phase::Configuration => {
                if id == config_cb::DISCONNECT {
                    let reason = Reader::new(&payload).nbt().unwrap_or(Tag::End);
                    return self.kicked(reason).await;
                }
            }
            Phase::Play => match id {
                play_cb::DISCONNECT => {
                    let reason = Reader::new(&payload).nbt().unwrap_or(Tag::End);
                    return self.kicked(reason).await;
                }
                play_cb::START_CONFIGURATION => {
                    b.writes = Phase::Configuration;
                    self.client_reads = Phase::Configuration;
                }
                play_cb::BOSS_EVENT => {
                    let mut r = Reader::new(&payload);
                    if let (Ok(uuid), Ok(operation)) = (r.uuid(), r.varint()) {
                        match operation {
                            0 => {
                                self.boss_bars.insert(uuid);
                            }
                            1 => {
                                self.boss_bars.remove(&uuid);
                            }
                            _ => {}
                        }
                    }
                }
                play_cb::COMMANDS if announce => payload = self.with_proxy_commands(payload),
                play_cb::LOGIN => joined = true,
                _ => {}
            },
        }
        self.send_client(id, &payload).await;
        if joined {
            self.joined().await;
        }
    }

    /// The server's first play packet: the player is in.
    async fn joined(&mut self) {
        self.tried.clear();
        self.attempts = 0;
        for notice in std::mem::take(&mut self.notices) {
            self.message(notice).await;
        }
    }

    /// The server disconnected the player with a reason.
    async fn kicked(&mut self, reason: Tag) {
        let Some(b) = self.backend.take() else { return };
        let server = b.server.clone();
        b.close().await;
        if self.shared.config.advanced.log_player_connections {
            log(&format!(
                "{} -> {server} has been kicked: {}",
                self.who,
                text::plain(&reason)
            ));
        }
        let friendly = reason.clone();
        let was_on = Some(server.clone());
        self.lost(server, Some(reason), friendly, false, was_on)
            .await;
    }

    /// The connection to the server dropped without a reason.
    async fn backend_lost(&mut self, error: &str) {
        let Some(b) = self.backend.take() else { return };
        let server = b.server.clone();
        b.close().await;
        log(&format!(
            "{} -> {server} lost connection: {error}",
            self.who
        ));
        let friendly = text::colored("На сервере произошла внутренняя ошибка подключения.", "red");
        if !self
            .shared
            .config
            .advanced
            .failover_on_unexpected_server_disconnect
        {
            return self.disconnect(friendly).await;
        }
        let was_on = Some(server.clone());
        self.lost(server, None, friendly, false, was_on).await;
    }

    /// handleConnectionException + KickedFromServerEvent: the player lost `server` (or could not
    /// reach it); the plugins may redirect them.
    /// `was_on` is the server the player was on when it happened (KickedFromServerEvent's
    /// getCurrentServer).
    async fn lost(
        &mut self,
        server: String,
        reason: Option<Tag>,
        friendly: Tag,
        during: bool,
        was_on: Option<String>,
    ) {
        if !self.tried.contains(&server) {
            self.tried.push(server.clone());
        }
        self.attempts += 1;
        if self.pending.is_some() {
            // A connection under way carries the player on.
            return;
        }
        let current = self.backend.as_ref().map(|b| b.server.clone());
        let shown = reason.clone().unwrap_or_else(|| friendly.clone());
        let result = if during && current.is_some() {
            KickResult::Notify(text::join(vec![
                text::colored(
                    &format!("Не удалось подключиться к серверу {server}: "),
                    "red",
                ),
                shown.clone(),
            ]))
        } else {
            match self.next_server() {
                Some(next) => KickResult::Redirect {
                    server: next,
                    notice: Some(text::join(vec![
                        text::colored(&format!("Вы были кикнуты с сервера {server}: "), "red"),
                        shown.clone(),
                    ])),
                },
                None => KickResult::Disconnect(shown.clone()),
            }
        };
        let mut event = Kicked {
            player: &self.info,
            server: &server,
            current: was_on.as_deref(),
            reason: reason.as_ref(),
            during_connect: during,
            result,
        };
        for plugin in &self.shared.plugins {
            plugin.kicked(&mut event);
        }
        match event.result {
            KickResult::Redirect { server: next, .. } if self.attempts >= MAX_ATTEMPTS => {
                log(&format!(
                    "{} could not be moved to {next}: too many attempts",
                    self.who
                ));
                self.disconnect(shown).await;
            }
            KickResult::Redirect { server: next, .. } if current.as_deref() == Some(&next) => {
                self.message(shown).await;
            }
            KickResult::Redirect {
                server: next,
                notice,
            } => {
                self.connect(next, Reason::Fallback { notice });
            }
            KickResult::Notify(message) if current.is_some() => self.message(message).await,
            KickResult::Notify(message) | KickResult::Disconnect(message) => {
                self.disconnect(message).await;
            }
        }
    }

    /// getNextServerToTry: the first of `try` not tried since the player last got in.
    fn next_server(&self) -> Option<String> {
        self.shared
            .config
            .servers
            .try_order
            .iter()
            .find(|s| !self.tried.contains(s))
            .cloned()
    }

    fn connect(&mut self, server: String, reason: Reason) {
        self.generation += 1;
        let generation = self.generation;
        self.pending = Some(Pending {
            generation,
            server: server.clone(),
            reason,
            ready: None,
        });
        let shared = self.shared.clone();
        let info = self.info.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = connect_backend(&shared, &server, &info).await;
            let _ = tx.send(Event::Connected(generation, result));
        });
    }

    async fn connected(&mut self, generation: u64, result: Result<Conn, Refusal>) {
        let Some(pending) = self
            .pending
            .take_if(|p| p.generation == generation && p.ready.is_none())
        else {
            if let Ok(mut c) = result {
                c.writer.shutdown().await;
            }
            return;
        };
        match result {
            Err((error, reason)) => {
                log(&format!(
                    "{} unable to connect to server {}: {error}",
                    self.who, pending.server
                ));
                let friendly = text::colored(
                    &format!(
                        "Не удалось подключить вас к серверу {}. Пожалуйста, попробуйте снова через некоторое время.",
                        pending.server
                    ),
                    "red",
                );
                let during = matches!(pending.reason, Reason::Command) && self.backend.is_some();
                let was_on = self.backend.as_ref().map(|b| b.server.clone());
                self.lost(pending.server, reason, friendly, during, was_on)
                    .await;
            }
            Ok(conn) if self.client_reads == Phase::Play => {
                // The client leaves the play state first; its acknowledgement installs the server.
                for uuid in std::mem::take(&mut self.boss_bars) {
                    let mut w = Writer::new();
                    w.uuid(uuid);
                    w.varint(1);
                    self.send_client(play_cb::BOSS_EVENT, &w.data).await;
                }
                self.send_client(play_cb::START_CONFIGURATION, &[]).await;
                self.client_reads = Phase::Configuration;
                self.pending = Some(Pending {
                    ready: Some(conn),
                    ..pending
                });
            }
            Ok(conn) => self.install(pending, conn).await,
        }
    }

    /// The client is in the configuration state: hand it to the new server.
    async fn install(&mut self, pending: Pending, conn: Conn) {
        let previous = match self.backend.take() {
            Some(old) => {
                let name = old.server.clone();
                old.close().await;
                Some(name)
            }
            None => None,
        };
        let Conn { reader, mut writer } = conn;
        let mut sent = writer
            .send(&body(ids::login::serverbound::LOGIN_ACKNOWLEDGED, &[]))
            .await;
        for (id, payload) in [
            (config_sb::CUSTOM_PAYLOAD, self.brand.clone()),
            (config_sb::CLIENT_INFORMATION, self.information.clone()),
        ] {
            if let (Some(payload), Ok(())) = (payload, &sent) {
                sent = writer.send(&body(id, &payload)).await;
            }
        }
        let timeout = self.shared.config.read_timeout();
        let task = spawn_reader(reader, self.tx.clone(), Some(pending.generation), timeout);
        self.ticket.set_server(&pending.server);
        if let Reason::Fallback {
            notice: Some(notice),
        } = pending.reason
        {
            self.notices.push(notice);
        }
        if self.shared.config.advanced.log_player_connections {
            log(&format!("{} -> {} has connected", self.who, pending.server));
        }
        for plugin in &self.shared.plugins {
            plugin.server_connected(&self.info, &pending.server, previous.as_deref());
        }
        self.backend = Some(Backend {
            server: pending.server,
            generation: pending.generation,
            writer,
            task,
            reads: Phase::Configuration,
            writes: Phase::Configuration,
        });
        if let Err(e) = sent {
            self.backend_lost(&e).await;
        }
    }

    /// createConnectionRequest from `/server`, `/send` or a plugin.
    async fn switch(&mut self, name: &str) {
        let servers = &self.shared.config.servers.addresses;
        let Some(server) = servers
            .keys()
            .find(|s| s.eq_ignore_ascii_case(name))
            .cloned()
        else {
            let text = format!("Указанный сервер {name} не существует.");
            return self.message(text::colored(&text, "red")).await;
        };
        if self.backend.as_ref().is_some_and(|b| b.server == server) {
            return self
                .message(text::colored("Вы уже подключены к этому серверу!", "red"))
                .await;
        }
        if self.pending.is_some() {
            return self
                .message(text::colored("Вы уже подключаетесь к серверу!", "red"))
                .await;
        }
        self.connect(server, Reason::Command);
    }

    // ---- commands ----

    /// A rate limit of `limit_ms` between actions: true when this one is allowed.
    fn rate(last: &mut Option<Instant>, limit_ms: u64) -> bool {
        let now = Instant::now();
        let allowed = last.is_none_or(|t| now.duration_since(t) >= Duration::from_millis(limit_ms));
        if allowed {
            *last = Some(now);
        }
        allowed
    }

    /// A typed command; true when the proxy took it (ran or refused it), false to forward it.
    async fn command(&mut self, line: &str) -> bool {
        let advanced = &self.shared.config.advanced;
        if !Self::rate(&mut self.last_command, advanced.command_rate_limit) {
            self.limited_commands += 1;
            let kick_after = advanced.kick_after_rate_limited_commands;
            let forward = advanced.forward_commands_if_rate_limited;
            if kick_after > 0 && self.limited_commands >= kick_after {
                let reason = text::colored("Вы отправляете команды слишком часто.", "red");
                self.disconnect(reason).await;
                return true;
            }
            return !forward;
        }
        self.limited_commands = 0;
        let mut forward = false;
        for plugin in &self.shared.plugins {
            match plugin.command(&self.info, line) {
                CommandResult::Denied => return true,
                CommandResult::Forward => forward = true,
                CommandResult::Allowed => {}
            }
        }
        let mut words = line.split_whitespace();
        let label = words.next().unwrap_or_default();
        let command = (!forward)
            .then(|| self.shared.command(label))
            .flatten()
            .filter(|c| self.shared.allows(c, &self.info));
        let Some(command) = command else {
            for plugin in &self.shared.plugins {
                plugin.command_executed(&self.info, line, false);
            }
            return false;
        };
        if self.shared.config.advanced.log_command_executions {
            log(&format!("{} -> executed command /{line}", self.who));
        }
        for plugin in &self.shared.plugins {
            plugin.command_executed(&self.info, line, true);
        }
        let Some(source) = self.shared.players.get(self.info.uuid).map(Source::Player) else {
            return true;
        };
        let args: Vec<String> = words.map(str::to_owned).collect();
        tokio::spawn((command.run)(self.handle(), source, args));
        true
    }

    /// A Tab completion request; true when the proxy answered it.
    async fn suggestions(&mut self, payload: &[u8]) -> bool {
        let mut r = Reader::new(payload);
        let (Ok(id), Ok(input)) = (r.varint(), r.string(32500)) else {
            return true;
        };
        let advanced = &self.shared.config.advanced;
        if !Self::rate(&mut self.last_tab, advanced.tab_complete_rate_limit) {
            self.limited_tabs += 1;
            let kick_after = advanced.kick_after_rate_limited_tab_completes;
            if kick_after > 0 && self.limited_tabs >= kick_after {
                let reason = text::colored("Вы отправляете запросы слишком часто.", "red");
                self.disconnect(reason).await;
            }
            return true;
        }
        self.limited_tabs = 0;
        let Some(partial) = input.strip_prefix('/') else {
            return false;
        };
        let start = input.rfind(' ').map_or(1, |i| i + 1);
        let allowed = self
            .shared
            .plugins
            .iter()
            .all(|p| p.tab_complete(&self.info, partial));
        let words: Vec<&str> = partial.split(' ').collect();
        let command = self
            .shared
            .command(words[0])
            .filter(|c| self.shared.allows(c, &self.info));
        let matches: Vec<String> = match (allowed, command) {
            (false, _) => Vec::new(),
            (true, Some(command)) if words.len() >= 2 => {
                let source = self.shared.players.get(self.info.uuid).map(Source::Player);
                let typed = words.last().copied().unwrap_or_default().to_lowercase();
                match (&command.suggest, source) {
                    (Some(suggest), Some(source)) => suggest(&self.handle(), &source, &words[1..])
                        .into_iter()
                        .filter(|s| s.to_lowercase().starts_with(&typed))
                        .collect(),
                    _ => Vec::new(),
                }
            }
            _ => return false,
        };
        let mut w = Writer::new();
        w.varint(id);
        w.varint(start as i32);
        w.varint((input.len() - start) as i32);
        w.varint(matches.len() as i32);
        for m in &matches {
            w.string(m);
            w.bool(false);
        }
        self.send_client(play_cb::COMMAND_SUGGESTIONS, &w.data)
            .await;
        true
    }

    /// The server's command tree with the proxy's commands this player may run (Velocity's
    /// AvailableCommands rewrite): `name [arguments...]`, suggested by the proxy.
    fn with_proxy_commands(&self, payload: Vec<u8>) -> Vec<u8> {
        let Ok(mut tree) = Commands::read(&mut Reader::new(&payload)) else {
            return payload;
        };
        let names: Vec<String> = self
            .shared
            .commands()
            .into_iter()
            .filter(|c| self.shared.allows(c, &self.info))
            .flat_map(|c| std::iter::once(c.name.clone()).chain(c.aliases.clone()))
            .collect();
        add_commands(&mut tree, &names);
        let mut w = Writer::new();
        Commands::write(&tree, &mut w);
        w.data
    }
}

/// Literal nodes for `names` on the root, each with a greedy string argument the proxy suggests;
/// the server's nodes of the same names are replaced.
fn add_commands(tree: &mut Commands, names: &[String]) {
    let Ok(root) = usize::try_from(tree.root) else {
        return;
    };
    if root >= tree.nodes.len() {
        return;
    }
    for name in names {
        let argument = tree.nodes.len() as i32;
        tree.nodes.push(CommandNode {
            kind: 2,
            executable: true,
            restricted: false,
            children: Vec::new(),
            redirect: None,
            name: "arguments".into(),
            parser: Some((STRING_ARGUMENT, vec![2])),
            suggestions: Some("minecraft:ask_server".into()),
        });
        let literal = tree.nodes.len() as i32;
        tree.nodes.push(CommandNode {
            kind: 1,
            executable: true,
            restricted: false,
            children: vec![argument],
            redirect: None,
            name: name.clone(),
            parser: None,
            suggestions: None,
        });
        let nodes = &tree.nodes;
        let mut children: Vec<i32> = nodes[root]
            .children
            .iter()
            .copied()
            .filter(|c| {
                usize::try_from(*c)
                    .ok()
                    .and_then(|c| nodes.get(c))
                    .is_none_or(|n| !(n.kind == 1 && n.name.eq_ignore_ascii_case(name)))
            })
            .collect();
        children.push(literal);
        tree.nodes[root].children = children;
    }
}

fn no_servers() -> Tag {
    text::colored(
        "Нет серверов, доступных для подключения. Попробуйте позже или свяжитесь с администратором.",
        "red",
    )
}

/// VelocityServerConnection.connect + backend LoginSessionHandler: log the player in to
/// `server` with modern forwarding, up to the server's LoginFinished. The login acknowledgement
/// is sent once the client is ready to be configured by the server.
async fn connect_backend(
    shared: &Shared,
    server: &str,
    player: &PlayerInfo,
) -> Result<Conn, Refusal> {
    let plain = |e: String| (e, None);
    let (host, port) = shared
        .config
        .address(server)
        .ok_or_else(|| plain("unknown server".into()))?;
    let mut c = Conn::connect(&host, port, shared.config.connection_timeout())
        .await
        .map_err(plain)?;
    let timeout = shared.config.read_timeout();
    let intention = handshake::Serverbound::Intention(handshake::Intention {
        protocol: PROTOCOL_VERSION,
        host,
        port,
        intent: handshake::LOGIN,
    });
    c.send(&intention.encode()).await.map_err(plain)?;
    let hello = login::Serverbound::Hello(login::ServerHello {
        name: player.name.clone(),
        uuid: player.uuid,
    });
    c.send(&hello.encode()).await.map_err(plain)?;
    let profile = login::GameProfile {
        uuid: player.uuid,
        name: player.name.clone(),
        properties: player.properties.clone(),
    };
    let address = player.address.ip().to_string();
    let mut forwarded = false;
    loop {
        let (id, payload) = c.reader.recv_within(timeout).await.map_err(plain)?;
        let packet = login::Clientbound::decode(id, &payload).map_err(|e| plain(e.0))?;
        let answer = match packet {
            login::Clientbound::Disconnect(d) => {
                let reason = serde_json::from_str::<Value>(&d.reason_json)
                    .map(|v| text::from_json(&v))
                    .unwrap_or_else(|_| text::colored(&d.reason_json, "red"));
                return Err((text::plain(&reason), Some(reason)));
            }
            login::Clientbound::Hello(_) => {
                return Err(plain("Backend server is online-mode!".into()));
            }
            login::Clientbound::Compression(p) => {
                c.set_compression(p.threshold);
                continue;
            }
            login::Clientbound::CustomQuery(query) => {
                let payload = (query.channel == forwarding::CHANNEL).then(|| {
                    forwarded = true;
                    let requested = forwarding::requested_version(&query.data);
                    forwarding::forwarding_data(&shared.secret, &address, &profile, requested)
                });
                login::Serverbound::CustomQueryAnswer(login::CustomQueryAnswer {
                    transaction: query.transaction,
                    payload,
                })
            }
            login::Clientbound::CookieRequest(request) => {
                login::Serverbound::CookieResponse(login::CookieResponse {
                    key: request.key,
                    payload: None,
                })
            }
            login::Clientbound::LoginFinished(_) if !forwarded => {
                let text = "Ваш сервер не посылал запрос на переадресацию на прокси-сервер. \
                     Убедитесь, что сервер настроен на переадресацию Velocity.";
                return Err(plain(text.into()));
            }
            login::Clientbound::LoginFinished(_) => return Ok(c),
            login::Clientbound::Unknown(..) => continue,
        };
        c.send(&answer.encode()).await.map_err(plain)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_commands_replace_the_servers() {
        let node = |kind, name: &str, children| CommandNode {
            kind,
            executable: kind != 0,
            restricted: false,
            children,
            redirect: None,
            name: name.into(),
            parser: None,
            suggestions: None,
        };
        let mut tree = Commands {
            nodes: vec![
                node(0, "", vec![1, 2]),
                node(1, "server", vec![]),
                node(1, "tp", vec![]),
            ],
            root: 0,
        };
        add_commands(&mut tree, &["server".into(), "glist".into()]);
        let root: Vec<&str> = tree.nodes[0]
            .children
            .iter()
            .map(|c| tree.nodes[*c as usize].name.as_str())
            .collect();
        assert_eq!(root, ["tp", "server", "glist"]);
        let server = &tree.nodes[tree.nodes[0].children[1] as usize];
        let argument = &tree.nodes[server.children[0] as usize];
        assert_eq!(argument.parser, Some((STRING_ARGUMENT, vec![2])));
        // The tree still encodes and reads back.
        let mut w = Writer::new();
        Commands::write(&tree, &mut w);
        assert_eq!(Commands::read(&mut Reader::new(&w.data)).unwrap(), tree);
    }
}
