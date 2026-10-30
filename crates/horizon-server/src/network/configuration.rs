//! The configuration state as Leaf runs it for a joining player (`ServerConfigurationPacketListenerImpl`
//! and `ServerCommonPacketListenerImpl` with Paper's and Leaf's patches), synchronous and free of I/O:
//! the tick thread feeds it packet bodies and ticks, and carries out the [`Action`]s it returns.
//!
//! `startConfiguration` sends the brand (Leaf's `server-mod-name`, `Leaf` by default), no server
//! links (Leaf's Cleanup empties them), the world's enabled features, then runs the tasks in order:
//! `synchronize_registries` (select known packs; on the answer every registry's data, then the tags),
//! `server_code_of_conduct` (when `enable-code-of-conduct` gave texts), Paper's
//! `paper_event_handling` (no plugins: done at once), `prepare_spawn` (the server loads the player and
//! the spawn chunks: [`Action::PrepareSpawn`], answered by [`Configuration::spawn_prepared`]) and
//! `join_world` (finish configuration). The client's finish acknowledgement hands the player over
//! to the play state ([`Action::Play`]). Leaf's Cleanup removes the server resource pack task.
//!
//! Java runs some handlers on the Netty thread and some on the main thread
//! (`ensureRunningOnSameThread`); their failures differ and are kept: a Netty-thread exception
//! disconnects with `disconnect.genericReason` ([`Fault::reason`]), a main-thread one is logged
//! and suppressed (`ServerPacketListener.onPacketError`).
//!
//! Keep-alives are Paper's (`Improve keepalive ping system`): one every second, a 5 s/1 min latency
//! average, a kick 30 s after the oldest unanswered one. Gale's `send-multiple` variant (off by
//! default) and Leaf's `async-switch-state` (off by default, a threading change) are not ported.

use std::collections::{BTreeSet, VecDeque};

use bytes::BufMut;

use super::codec::{put_var_int, read_var_int};
use super::login::GameProfile;
use super::{Fault, Reason};
use crate::registry::{KnownPack, Registries, SyncedRegistry, nbt_json};

/// Clientbound packet ids (`ConfigurationProtocols.CLIENTBOUND`).
pub mod id {
    pub const CUSTOM_PAYLOAD: u8 = 1;
    pub const DISCONNECT: u8 = 2;
    pub const FINISH_CONFIGURATION: u8 = 3;
    pub const KEEP_ALIVE: u8 = 4;
    pub const REGISTRY_DATA: u8 = 7;
    pub const UPDATE_ENABLED_FEATURES: u8 = 13;
    pub const UPDATE_TAGS: u8 = 14;
    pub const SELECT_KNOWN_PACKS: u8 = 15;
    pub const CODE_OF_CONDUCT: u8 = 20;
}

/// `KEEPALIVE_LIMIT`: Paper's `paper.playerconnection.keepalive` default, 30 s.
const KEEPALIVE_LIMIT_NS: i64 = 30_000_000_000;
const MAX_CHANNELS: usize = 128;

fn put_utf(out: &mut Vec<u8>, s: &str) {
    put_var_int(out, s.len() as i32);
    out.put_slice(s.as_bytes());
}

/// `ClientboundCustomPayloadPacket` with `BrandPayload`.
pub fn brand_packet(brand: &str) -> Vec<u8> {
    let mut out = vec![id::CUSTOM_PAYLOAD];
    put_utf(&mut out, "minecraft:brand");
    put_utf(&mut out, brand);
    out
}

/// The reason as `ComponentSerialization` writes it to NBT: a plain literal collapses to a string.
fn reason_json(reason: &Reason) -> serde_json::Value {
    match reason {
        Reason::Literal(text) => text.as_str().into(),
        Reason::Translatable(key, args) if args.is_empty() => {
            serde_json::json!({ "translate": key })
        }
        Reason::Translatable(key, args) => serde_json::json!({ "translate": key, "with": args }),
    }
}

/// `ClientboundDisconnectPacket`: the reason as network NBT.
pub fn disconnect_packet(reason: &Reason) -> Vec<u8> {
    let mut out = vec![id::DISCONNECT];
    nbt_json::write_network(&reason_json(reason), &mut out).expect("reasons are short strings");
    out
}

pub fn finish_configuration_packet() -> Vec<u8> {
    vec![id::FINISH_CONFIGURATION]
}

pub fn keep_alive_packet(challenge: i64) -> Vec<u8> {
    let mut out = vec![id::KEEP_ALIVE];
    out.put_i64(challenge);
    out
}

pub fn select_known_packs_packet(packs: &[KnownPack]) -> Vec<u8> {
    let mut out = vec![id::SELECT_KNOWN_PACKS];
    put_var_int(&mut out, packs.len() as i32);
    for p in packs {
        put_utf(&mut out, &p.namespace);
        put_utf(&mut out, &p.id);
        put_utf(&mut out, &p.version);
    }
    out
}

/// `ClientboundRegistryDataPacket` (`RegistrySynchronization.packRegistry`): every entry's id, its
/// data only when its pack is not one the client confirmed.
pub fn registry_data_packet(registry: &SyncedRegistry, negotiated: &[KnownPack]) -> Vec<u8> {
    let mut out = vec![id::REGISTRY_DATA];
    put_utf(&mut out, registry.name);
    put_var_int(&mut out, registry.entries.len() as i32);
    for entry in &registry.entries {
        put_utf(&mut out, &entry.id);
        let known = entry
            .known_pack
            .as_ref()
            .is_some_and(|p| negotiated.contains(p));
        out.push(u8::from(!known));
        if !known {
            out.extend_from_slice(&entry.nbt);
        }
    }
    out
}

/// `ClientboundUpdateTagsPacket` (`TagNetworkSerialization.serializeTagsToNetwork`): the
/// synchronized registries' tags, then the built-in ones', registries without tags left out.
/// (Java's registry order is an `IdentityHashMap`'s, different on every run.)
pub fn update_tags_packet(registries: &Registries) -> Vec<u8> {
    let synced = registries.synced.iter().map(|r| (r.name, &r.tags));
    let builtin = registries
        .builtin
        .iter()
        .map(|r| (r.name.as_str(), &r.tags));
    let all: Vec<_> = synced
        .chain(builtin)
        .filter(|(_, t)| !t.0.is_empty())
        .collect();
    let mut out = vec![id::UPDATE_TAGS];
    put_var_int(&mut out, all.len() as i32);
    for (name, tags) in all {
        put_utf(&mut out, name);
        put_var_int(&mut out, tags.0.len() as i32);
        for (tag, ids) in &tags.0 {
            put_utf(&mut out, tag);
            put_var_int(&mut out, ids.len() as i32);
            for &id in ids {
                put_var_int(&mut out, id);
            }
        }
    }
    out
}

/// `ClientboundUpdateEnabledFeaturesPacket`: the flags in the `HashSet` order Java writes them.
pub fn update_enabled_features_packet(features: &[String]) -> Vec<u8> {
    let mut features: Vec<&str> = features.iter().map(String::as_str).collect();
    nbt_json::java_hash_order(&mut features, |f| nbt_json::identifier_hash(f));
    let mut out = vec![id::UPDATE_ENABLED_FEATURES];
    put_var_int(&mut out, features.len() as i32);
    for f in features {
        put_utf(&mut out, f);
    }
    out
}

pub fn code_of_conduct_packet(text: &str) -> Vec<u8> {
    let mut out = vec![id::CODE_OF_CONDUCT];
    put_utf(&mut out, text);
    out
}

/// `ClientInformation` (enum fields as their ids, decoded as the stream codecs map them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientInformation {
    pub language: String,
    pub view_distance: i8,
    pub chat_visibility: u8,
    pub chat_colors: bool,
    pub model_customisation: u8,
    /// `HumanoidArm` id: 0 left, 1 right.
    pub main_hand: u8,
    pub text_filtering_enabled: bool,
    pub allows_listing: bool,
    pub particle_status: u8,
}

impl Default for ClientInformation {
    /// `ClientInformation.createDefault()`.
    fn default() -> Self {
        Self {
            language: "en_us".into(),
            view_distance: 2,
            chat_visibility: 0,
            chat_colors: true,
            model_customisation: 0,
            main_hand: 1,
            text_filtering_enabled: false,
            allows_listing: false,
            particle_status: 0,
        }
    }
}

/// Serverbound configuration packets (`ConfigurationProtocols.SERVERBOUND`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Serverbound {
    ClientInformation(ClientInformation),
    CookieResponse,
    /// Paper decodes every payload as raw bytes (`DiscardedPayload` with its data).
    CustomPayload {
        channel: String,
        data: Vec<u8>,
    },
    FinishConfiguration,
    KeepAlive(i64),
    Pong,
    ResourcePack,
    SelectKnownPacks(Vec<KnownPack>),
    CustomClickAction {
        id: String,
    },
    AcceptCodeOfConduct,
}

const PACKETS: [(&str, &str); 10] = [
    ("client_information", "ServerboundClientInformationPacket"),
    ("cookie_response", "ServerboundCookieResponsePacket"),
    ("custom_payload", "ServerboundCustomPayloadPacket"),
    (
        "finish_configuration",
        "ServerboundFinishConfigurationPacket",
    ),
    ("keep_alive", "ServerboundKeepAlivePacket"),
    ("pong", "ServerboundPongPacket"),
    ("resource_pack", "ServerboundResourcePackPacket"),
    ("select_known_packs", "ServerboundSelectKnownPacks"),
    ("custom_click_action", "ServerboundCustomClickActionPacket"),
    (
        "accept_code_of_conduct",
        "ServerboundAcceptCodeOfConductPacket",
    ),
];

/// A reader over a packet body; any failure becomes the decoder's "Failed to decode packet".
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = (self.0.get(..n)?, self.0.get(n..)?);
        self.0 = rest;
        Some(head)
    }
    fn var_int(&mut self) -> Option<i32> {
        let (v, n) = read_var_int(self.0).ok()??;
        self.0 = &self.0[n..];
        Some(v)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }
    fn bool(&mut self) -> Option<bool> {
        self.u8().map(|b| b != 0)
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.bytes(8)?.try_into().ok()?))
    }
    /// `Utf8String.read`: at most `max * 3` bytes and `max` UTF-16 units.
    fn utf(&mut self, max: usize) -> Option<String> {
        let len = usize::try_from(self.var_int()?)
            .ok()
            .filter(|&l| l <= max * 3)?;
        let s = String::from_utf8_lossy(self.bytes(len)?).into_owned();
        (s.encode_utf16().count() <= max).then_some(s)
    }
    fn identifier(&mut self) -> Option<String> {
        crate::registry::parse_identifier(&self.utf(32767)?).ok()
    }
    /// `ByteBufCodecs.readCount`.
    fn count(&mut self, max: usize) -> Option<usize> {
        usize::try_from(self.var_int()?).ok().filter(|&c| c <= max)
    }
}

/// `ByIdMap` with `WRAP`: `Mth.positiveModulo(id, len)`.
fn wrap(id: i32, len: i32) -> u8 {
    id.rem_euclid(len) as u8
}

pub fn decode(body: &[u8]) -> Result<Serverbound, Fault> {
    let mut r = Reader(body);
    let id = r
        .var_int()
        .ok_or_else(|| Fault::decoder("Received unknown packet id -1"))?;
    let Some(&(name, class)) = usize::try_from(id).ok().and_then(|i| PACKETS.get(i)) else {
        return Err(Fault::decoder(format!("Received unknown packet id {id}")));
    };
    let packet = (|| {
        Some(match id {
            0 => Serverbound::ClientInformation(ClientInformation {
                language: r.utf(16)?,
                view_distance: r.u8()? as i8,
                chat_visibility: wrap(r.var_int()?, 3),
                chat_colors: r.bool()?,
                model_customisation: r.u8()?,
                main_hand: match r.var_int()? {
                    1 => 1,
                    _ => 0,
                },
                text_filtering_enabled: r.bool()?,
                allows_listing: r.bool()?,
                particle_status: wrap(r.var_int()?, 3),
            }),
            1 => {
                r.identifier()?;
                if r.bool()? {
                    let len = r.count(5120)?;
                    r.bytes(len)?;
                }
                Serverbound::CookieResponse
            }
            2 => {
                let channel = r.identifier()?;
                // `DiscardedPayload.codec(id, 32767)`: every remaining byte.
                let data = r.bytes(r.0.len()).filter(|d| d.len() <= 32767)?.to_vec();
                Serverbound::CustomPayload { channel, data }
            }
            3 => Serverbound::FinishConfiguration,
            4 => Serverbound::KeepAlive(r.i64()?),
            5 => {
                r.bytes(4)?;
                Serverbound::Pong
            }
            6 => {
                r.bytes(16)?;
                (0..8)
                    .contains(&r.var_int()?)
                    .then_some(Serverbound::ResourcePack)?
            }
            7 => {
                let n = r.count(64)?;
                let packs = (0..n)
                    .map(|_| {
                        Some(KnownPack {
                            namespace: r.utf(32767)?,
                            id: r.utf(32767)?,
                            version: r.utf(32767)?,
                        })
                    })
                    .collect::<Option<_>>()?;
                Serverbound::SelectKnownPacks(packs)
            }
            8 => {
                let id = r.identifier()?;
                // An optional NBT tag, length-prefixed (at most 65536 bytes).
                let len = r.count(65536)?;
                r.bytes(len)?;
                Serverbound::CustomClickAction { id }
            }
            _ => Serverbound::AcceptCodeOfConduct,
        })
    })()
    .ok_or_else(|| {
        Fault::decoder(format!(
            "Failed to decode packet 'serverbound/minecraft:{name}'"
        ))
    })?;
    if !r.0.is_empty() {
        return Err(Fault::decoder(format!(
            "java.io.IOException: Packet configuration/serverbound/minecraft:{name} ({class}) was larger than I expected, found {} bytes extra whilst reading packet serverbound/minecraft:{name}",
            r.0.len()
        )));
    }
    Ok(packet)
}

/// Paper's `KeepAlive.PingCalculator`: the mean latency of the responses sent in the last interval.
#[derive(Clone, Debug)]
struct PingCalculator {
    interval_ns: i64,
    responses: VecDeque<(i64, i64)>,
    sum_ns: i64,
    count: i64,
    last_average_ns: i64,
}

impl PingCalculator {
    fn new(interval_ns: i64) -> Self {
        Self {
            interval_ns,
            responses: VecDeque::new(),
            sum_ns: 0,
            count: 0,
            last_average_ns: 0,
        }
    }

    fn update(&mut self, tx: i64, rx: i64) {
        self.responses.push_back((tx, rx));
        self.count += 1;
        self.sum_ns += rx - tx;
        while let Some(&(old_tx, old_rx)) = self.responses.front() {
            if tx - old_tx <= self.interval_ns {
                break;
            }
            self.responses.pop_front();
            self.count -= 1;
            self.sum_ns -= old_rx - old_tx;
        }
        self.last_average_ns = self.sum_ns / self.count;
    }
}

/// Paper's `KeepAlive`: what a connection's keep-alives carry from one state to the next.
#[derive(Clone, Debug)]
pub struct KeepAlive {
    last_tx_ns: i64,
    /// (sent at, challenge), oldest first.
    pending: VecDeque<(i64, i64)>,
    ping_1m: PingCalculator,
    ping_5s: PingCalculator,
}

impl KeepAlive {
    pub fn new(now_ns: i64) -> Self {
        Self {
            last_tx_ns: now_ns,
            pending: VecDeque::new(),
            ping_1m: PingCalculator::new(60_000_000_000),
            ping_5s: PingCalculator::new(5_000_000_000),
        }
    }

    /// `copyForListenerHandoff`: the averages, without the pending challenges.
    pub fn handoff(&self) -> Self {
        Self {
            pending: VecDeque::new(),
            ..self.clone()
        }
    }

    pub fn average_latency_1m_ms(&self) -> i64 {
        self.ping_1m.last_average_ns / 1_000_000
    }
}

/// What the server provides to the configuration of every player.
#[derive(Clone, Copy, Debug)]
pub struct Settings<'a> {
    pub registries: &'a Registries,
    /// `getServerModName()`: Leaf's `misc.rebrand.server-mod-name`, `Leaf` by default.
    pub brand: &'a str,
    /// The world's enabled feature flags (`minecraft:vanilla` for a default world).
    pub enabled_features: &'a [String],
    /// `codeofconduct/<language>.txt` texts when `enable-code-of-conduct` is on, in the order
    /// the folder listed them; empty when off.
    pub codes_of_conduct: &'a [(String, String)],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    SynchronizeRegistries,
    CodeOfConduct,
    PaperEventHandling,
    PrepareSpawn,
    JoinWorld,
}

impl Task {
    /// `ConfigurationTask.Type.id`.
    pub fn id(self) -> &'static str {
        match self {
            Task::SynchronizeRegistries => "synchronize_registries",
            Task::CodeOfConduct => "server_code_of_conduct",
            Task::PaperEventHandling => "paper_event_handling",
            Task::PrepareSpawn => "prepare_spawn",
            Task::JoinWorld => "join_world",
        }
    }
}

/// What the player brings into the play state (`CommonListenerCookie` with Paper's fields).
#[derive(Clone, Debug)]
pub struct PlayHandoff {
    pub profile: GameProfile,
    pub latency: i32,
    pub client_information: ClientInformation,
    pub transferred: bool,
    pub client_brand: Option<String>,
    pub channels: BTreeSet<String>,
    pub keep_alive: KeepAlive,
}

#[derive(Debug)]
pub enum Action {
    Send(Vec<u8>),
    /// Send [`disconnect_packet`], then close (`disconnect`).
    Disconnect(Reason),
    /// `PrepareSpawnTask.start`: load the player's data and the chunks around their spawn, then
    /// call [`Configuration::spawn_prepared`].
    PrepareSpawn,
    /// The client finished configuring: the connection is in the play state (the runtime makes
    /// the duplicate-login and `canPlayerLogin` checks and spawns the player).
    Play(Box<PlayHandoff>),
}

/// One player's configuration listener.
#[derive(Debug)]
pub struct Configuration {
    profile: GameProfile,
    transferred: bool,
    client_information: ClientInformation,
    client_brand: Option<String>,
    channels: BTreeSet<String>,
    tasks: VecDeque<Task>,
    current: Option<Task>,
    requested_packs: Vec<KnownPack>,
    spawn_ready: bool,
    keep_alive: KeepAlive,
    latency: i32,
    /// When a terminal packet (finish configuration) went out, in milliseconds.
    closed_at_ms: Option<i64>,
    disconnected: bool,
    finished: bool,
}

impl Configuration {
    /// `startConfiguration`, right after `login_acknowledged`. `now_ns` is a monotonic clock
    /// (`System.nanoTime`); its milliseconds are `Util.getMillis` (keep-alive challenges).
    pub fn start(
        profile: GameProfile,
        transferred: bool,
        settings: &Settings<'_>,
        now_ns: i64,
        out: &mut Vec<Action>,
    ) -> Self {
        let mut tasks = VecDeque::from([Task::SynchronizeRegistries]);
        if !settings.codes_of_conduct.is_empty() {
            tasks.push_back(Task::CodeOfConduct);
        }
        tasks.extend([
            Task::PaperEventHandling,
            Task::PrepareSpawn,
            Task::JoinWorld,
        ]);
        let mut config = Self {
            profile,
            transferred,
            client_information: ClientInformation::default(),
            client_brand: None,
            channels: BTreeSet::new(),
            tasks,
            current: None,
            requested_packs: settings.registries.known_packs.clone(),
            spawn_ready: false,
            keep_alive: KeepAlive::new(now_ns),
            latency: 0,
            closed_at_ms: None,
            disconnected: false,
            finished: false,
        };
        out.push(Action::Send(brand_packet(settings.brand)));
        out.push(Action::Send(update_enabled_features_packet(
            settings.enabled_features,
        )));
        config.start_next_task(settings, now_ns, out);
        config
    }

    pub fn current_task(&self) -> Option<Task> {
        self.current
    }

    /// `PrepareSpawnTask` is ready: the next tick finishes it.
    pub fn spawn_prepared(&mut self) {
        self.spawn_ready = true;
    }

    fn disconnect(&mut self, reason: Reason, out: &mut Vec<Action>) {
        if !self.disconnected {
            self.disconnected = true;
            out.push(Action::Disconnect(reason));
        }
    }

    fn start_next_task(&mut self, settings: &Settings<'_>, now_ns: i64, out: &mut Vec<Action>) {
        if self.disconnected || self.current.is_some() {
            return;
        }
        let Some(task) = self.tasks.pop_front() else {
            return;
        };
        self.current = Some(task);
        match task {
            Task::SynchronizeRegistries => out.push(Action::Send(select_known_packs_packet(
                &self.requested_packs,
            ))),
            Task::CodeOfConduct => {
                // The client's language, then `en_us`, then the first text.
                let texts = settings.codes_of_conduct;
                let language = self.client_information.language.to_lowercase();
                let text = [language.as_str(), "en_us"]
                    .iter()
                    .find_map(|l| texts.iter().find(|(k, _)| k == l))
                    .or(texts.first())
                    .map_or("", |(_, t)| t.as_str());
                out.push(Action::Send(code_of_conduct_packet(text)));
            }
            Task::PaperEventHandling => {
                // No `AsyncPlayerConnectionConfigureEvent` listeners: finished at once.
                let _ = self.finish_task(Task::PaperEventHandling, settings, now_ns, out);
            }
            Task::PrepareSpawn => out.push(Action::PrepareSpawn),
            Task::JoinWorld => {
                out.push(Action::Send(finish_configuration_packet()));
                // A terminal packet closes the listener (`ServerCommonPacketListenerImpl.close`).
                self.closed_at_ms
                    .get_or_insert(now_ns.div_euclid(1_000_000));
            }
        }
    }

    /// `finishCurrentTask`.
    fn finish_task(
        &mut self,
        task: Task,
        settings: &Settings<'_>,
        now_ns: i64,
        out: &mut Vec<Action>,
    ) -> Result<(), String> {
        if self.current != Some(task) {
            return Err(format!(
                "Unexpected request for task finish, current task: {}, requested: {}",
                self.current.map_or("null", Task::id),
                task.id()
            ));
        }
        self.current = None;
        self.start_next_task(settings, now_ns, out);
        Ok(())
    }

    /// One server tick (`tick`): keep-alives, then the current task's tick.
    pub fn tick(&mut self, settings: &Settings<'_>, now_ns: i64, out: &mut Vec<Action>) {
        if self.disconnected || self.finished {
            return;
        }
        self.keep_connection_alive(now_ns, out);
        if self.current == Some(Task::PrepareSpawn) && self.spawn_ready && !self.disconnected {
            let _ = self.finish_task(Task::PrepareSpawn, settings, now_ns, out);
        }
    }

    fn keep_connection_alive(&mut self, now_ns: i64, out: &mut Vec<Action>) {
        let now_ms = now_ns.div_euclid(1_000_000);
        if let Some(closed) = self.closed_at_ms {
            if now_ms - closed >= 15_000 {
                self.disconnect(Reason::translatable("disconnect.timeout"), out);
            }
            return;
        }
        let ka = &mut self.keep_alive;
        if now_ns - ka.last_tx_ns >= 1_000_000_000 {
            ka.last_tx_ns = now_ns;
            ka.pending.push_back((now_ns, now_ms));
            out.push(Action::Send(keep_alive_packet(now_ms)));
        }
        if ka
            .pending
            .front()
            .is_some_and(|&(tx, _)| now_ns - tx > KEEPALIVE_LIMIT_NS)
        {
            tracing::info!("{} was kicked due to keepalive timeout!", self.profile.name);
            self.disconnect(Reason::translatable("disconnect.timeout"), out);
        }
    }

    fn handle_keep_alive(&mut self, challenge: i64, now_ns: i64, out: &mut Vec<Action>) {
        let ka = &mut self.keep_alive;
        if let Some(&(tx, c)) = ka.pending.front()
            && c == challenge
        {
            ka.pending.pop_front();
            ka.ping_1m.update(tx, now_ns);
            ka.ping_5s.update(tx, now_ns);
            self.latency = (ka.ping_5s.last_average_ns / 1_000_000) as i32;
            return;
        }
        let why = match ka.pending.iter().position(|&(_, c)| c == challenge) {
            Some(i) => {
                ka.pending.remove(i);
                "out-of-order"
            }
            None => "without matching challenge",
        };
        tracing::info!(
            "Disconnecting {} for sending keepalive response ({challenge}) {why}!",
            self.profile.name
        );
        self.disconnect(Reason::translatable("disconnect.timeout"), out);
    }

    /// Paper's `handleCustomPayload`: channel (un)registrations and the client's brand.
    fn handle_custom_payload(&mut self, channel: &str, data: &[u8]) -> Result<(), String> {
        let register = channel == "minecraft:register";
        if register || channel == "minecraft:unregister" {
            // Channel names separated by zero bytes, read as US-ASCII.
            for name in data.split(|&b| b == 0).filter(|n| !n.is_empty()) {
                let name: String = name
                    .iter()
                    .map(|&b| if b < 0x80 { b as char } else { '\u{fffd}' })
                    .collect();
                if register && self.channels.len() >= MAX_CHANNELS {
                    return Err("Cannot register channel. Too many channels registered!".into());
                }
                let name = validate_channel(name)?;
                if register {
                    self.channels.insert(name);
                } else {
                    self.channels.remove(&name);
                }
            }
        } else if channel == "minecraft:brand" {
            let brand = Reader(data).utf(256).ok_or("Couldn't read the brand")?;
            self.client_brand = Some(brand);
        }
        Ok(())
    }

    /// Handles one packet body of the configuration state.
    pub fn handle(
        &mut self,
        body: &[u8],
        settings: &Settings<'_>,
        now_ns: i64,
        out: &mut Vec<Action>,
    ) {
        if self.disconnected || self.finished {
            return;
        }
        let packet = match decode(body) {
            Ok(packet) => packet,
            Err(fault) => return self.disconnect(fault.reason(), out),
        };
        // Main-thread handlers: an exception is logged and suppressed.
        let suppressed = |name: &str, e: String| {
            tracing::error!(
                "Failed to handle packet {name}, suppressing error: java.lang.IllegalStateException: {e}"
            );
        };
        match packet {
            Serverbound::ClientInformation(info) => self.client_information = info,
            Serverbound::CookieResponse => self.disconnect(
                Reason::translatable("multiplayer.disconnect.unexpected_query_response"),
                out,
            ),
            Serverbound::CustomPayload { channel, data } => {
                if let Err(e) = self.handle_custom_payload(&channel, &data) {
                    tracing::error!("Couldn't handle custom payload on channel {channel}: {e}");
                    self.disconnect(
                        Reason::Literal("Invalid custom payload payload!".into()),
                        out,
                    );
                }
            }
            Serverbound::FinishConfiguration => {
                if let Err(e) = self.finish_task(Task::JoinWorld, settings, now_ns, out) {
                    return suppressed("ServerboundFinishConfigurationPacket", e);
                }
                self.finished = true;
                out.push(Action::Play(Box::new(PlayHandoff {
                    profile: self.profile.clone(),
                    latency: self.latency,
                    client_information: self.client_information.clone(),
                    transferred: self.transferred,
                    client_brand: self.client_brand.clone(),
                    channels: std::mem::take(&mut self.channels),
                    keep_alive: self.keep_alive.handoff(),
                })));
            }
            Serverbound::KeepAlive(challenge) => self.handle_keep_alive(challenge, now_ns, out),
            Serverbound::Pong | Serverbound::ResourcePack => {}
            Serverbound::SelectKnownPacks(packs) => {
                // `SynchronizeRegistriesTask.handleResponse`: the packs count only when the client
                // confirms the whole list.
                let negotiated: &[KnownPack] = if packs == self.requested_packs {
                    &self.requested_packs
                } else {
                    &[]
                };
                for registry in &settings.registries.synced {
                    out.push(Action::Send(registry_data_packet(registry, negotiated)));
                }
                out.push(Action::Send(update_tags_packet(settings.registries)));
                if let Err(e) = self.finish_task(Task::SynchronizeRegistries, settings, now_ns, out)
                {
                    suppressed("ServerboundSelectKnownPacks", e);
                }
            }
            Serverbound::CustomClickAction { id } => {
                tracing::debug!("Received custom click action {id}");
            }
            Serverbound::AcceptCodeOfConduct => {
                // Runs on the Netty thread: a wrong state is an exception that disconnects.
                if let Err(e) = self.finish_task(Task::CodeOfConduct, settings, now_ns, out) {
                    self.disconnect(Fault::illegal_state(e).reason(), out);
                }
            }
        }
    }
}

/// Bukkit's `StandardMessenger.validateAndCorrectChannel` (without the length check: the names come
/// from a payload of at most 32767 bytes).
fn validate_channel(channel: String) -> Result<String, String> {
    let shortened = || {
        let s = channel.split_whitespace().collect::<Vec<_>>().join(" ");
        match s.char_indices().nth(32) {
            Some((i, _)) => format!("{}...", &s[..i]),
            None => s,
        }
    };
    match channel.as_str() {
        "BungeeCord" => Ok("bungeecord:main".into()),
        "bungeecord:main" => Ok("BungeeCord".into()),
        c if !c.contains(':') => Err(format!(
            "Channel must contain : separator (attempted to use {})",
            shortened()
        )),
        c if c.to_lowercase() != c => Err(format!(
            "Channel must be entirely lowercase (attempted to use {})",
            shortened()
        )),
        _ => Ok(channel),
    }
}

#[cfg(test)]
mod tests;
