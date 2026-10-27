//! The Network plugin of MineComplex (Network/Proxy) on the Rust proxy: the proxy as a PROXY node
//! of Core. It lets players in by the launcher token and their Core account
//! (LauncherAuthHandler), checks bans and maintenance, reports presence, balances lobbies,
//! answers the server list, gives permissions from Core's groups, delivers Core's messages,
//! kicks and moves, and has `/maintenance` and `/proxy`.
//!
//! Enabled by `plugins/network/config.toml` beside velocity.toml.
pub mod config;
pub mod core;
pub mod permissions;

use self::{
    config::NetworkConfig,
    core::{Core, SessionEvent, pb},
    permissions::{Group, Node},
};
use crate::{
    ProxyHandle,
    auth::{
        Account, Accounts, AuthHandler, LauncherAuth, LauncherToken, base64_encode, parse_uuid,
    },
    log,
    players::{Order, PlayerHandle, PlayerInfo},
    plugin::{
        Command, CommandResult, Fut, KickResult, Kicked, Login, Ping, Plugin, PreLogin, Source,
        Tristate,
    },
    text::{self, CUBE, ERROR, SOMETHING, SUCCESS},
};
use horizon_protocol::{crypto::uuid_string, packets::login::Property};
use horizon_world::nbt::Tag;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

/// Network.WEBSITE.
pub const WEBSITE: &str = "https://minecomplex.net";
/// AccountManager.LINK_GROUP: the group of accounts with a linked social network.
const LINK_GROUP: &str = "verified";
/// PermissionData.STAFF.
const STAFF: &str = "network.staff";
/// Shared.SECONDARY.
const SECONDARY: &str = "#FFC218";
const LOBBY: &str = "LOBBY";

/// The mirrors of Core's data and the plugin's own state.
struct State {
    config: NetworkConfig,
    servers: BTreeMap<String, pb::ServerView>,
    maintenance: pb::MaintenanceView,
    groups: HashMap<String, Group>,
    /// Permission nodes of the players on this proxy.
    users: HashMap<u128, Vec<Node>>,
    accounts: HashMap<u128, pb::AccountView>,
    /// Joins Core let in, sent again after a reconnection (PlayerManager.joins).
    joined: HashMap<u128, pb::JoinRequest>,
    /// Players sent to a lobby who have not arrived yet.
    pending: HashMap<String, i32>,
    lobby_cooldown: HashMap<u128, Instant>,
    command_cooldown: HashMap<u128, Instant>,
    motd: usize,
    broadcast: usize,
    /// Bytes per second in and out, sampled every second (TrafficManager).
    traffic: (u64, u64),
    /// Process and system CPU use, sampled every second (LagUtil).
    cpu: Option<(f64, f64)>,
}

pub struct Network {
    core: Core,
    path: PathBuf,
    token: Option<Arc<LauncherToken>>,
    state: Mutex<State>,
    proxy: OnceLock<ProxyHandle>,
    me: OnceLock<Weak<Network>>,
}

/// AccountManager.checkLogin over Core.
struct CoreAccounts(Core);
impl Accounts for CoreAccounts {
    fn check_login(
        &self,
        account: u128,
        session: u128,
    ) -> Fut<'_, Result<Option<Account>, String>> {
        Box::pin(async move {
            let request = self.0.request(pb::LoginCheck {
                session: uuid_string(session),
                player: uuid_string(account),
            });
            let response = self
                .0
                .accounts()
                .check_login(request)
                .await
                .map_err(|s| s.message().to_owned())?;
            Ok(response.into_inner().account.and_then(|a| {
                let uuid = parse_uuid(&a.id)?;
                Some((uuid, a.name.clone(), textures(&a)))
            }))
        })
    }
}

/// SkinUtil.encode: the account's skin as an unsigned `textures` property.
pub fn textures(account: &pb::AccountView) -> Vec<Property> {
    let Some(skin) = &account.skin else {
        return Vec::new();
    };
    let value = serde_json::json!({
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        "profileId": account.id.replace('-', ""),
        "profileName": account.name,
        "textures": {"SKIN": {"url": skin.url}},
    });
    vec![Property {
        name: "textures".into(),
        value: base64_encode(value.to_string().as_bytes()),
        signature: Some(String::new()),
    }]
}

/// SkinUtil.getUrl of a `textures` property.
fn skin_url(properties: &[Property]) -> Option<String> {
    let value = &properties.iter().find(|p| p.name == "textures")?.value;
    let json: serde_json::Value =
        serde_json::from_slice(&crate::auth::base64_decode(value)?).ok()?;
    Some(json["textures"]["SKIN"]["url"].as_str()?.to_owned())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// NumberUtil.plurals.
fn plurals(n: i64, one: &str, few: &str, many: &str) -> String {
    let m = n.abs() % 100;
    let form = if n == 0 || (11..20).contains(&m) {
        many
    } else {
        match m % 10 {
            1 => one,
            2..=4 => few,
            _ => many,
        }
    };
    format!("{n} {form}")
}

/// TimeUtil.formatDetailed.
pub fn format_detailed(millis: i64) -> String {
    let secs = millis.max(0) / 1000;
    let (days, hours, minutes, seconds) =
        (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let mut parts = Vec::new();
    if days != 0 {
        parts.push(plurals(days, "день", "дня", "дней"));
    }
    if hours != 0 {
        parts.push(plurals(hours, "час", "часа", "часов"));
    }
    if minutes != 0 {
        parts.push(plurals(minutes, "минута", "минуты", "минут"));
    }
    if days == 0 && seconds != 0 {
        parts.push(plurals(seconds, "секунда", "секунды", "секунд"));
    }
    if parts.is_empty() {
        "сейчас".into()
    } else {
        parts.join(" ")
    }
}

/// PunishmentUtil.getText of a ban.
pub fn ban_text(ban: &pb::PunishmentView, issuer: Option<&str>, now: i64) -> String {
    let reason = ban.reason.as_deref().unwrap_or("не указана");
    let mut lines = vec![
        "§cВы были заблокированы на MINECOMPLEX.".to_owned(),
        String::new(),
        format!("§cПричина: §f{reason}"),
    ];
    match ban.expires_at {
        None => lines.push("§cДлительность: §fнавсегда.".into()),
        Some(at) => lines.push(format!("§cОсталось: §f{}", format_detailed(at - now))),
    }
    lines.push(format!("§cЗабанил: §f{}", issuer.unwrap_or("Консоль")));
    lines.push(String::new());
    lines.push("§cПриобрести разбан можно на сайте:".into());
    lines.push(format!("§7{WEBSITE}"));
    lines.join("\n")
}

/// Punishment.isActive.
fn is_active(p: &pb::PunishmentView, now: i64) -> bool {
    p.removed_at.is_none() && p.remover.is_none() && p.expires_at.is_none_or(|at| at > now)
}

/// SocialType: (display color, name).
fn social(kind: &str) -> (&'static str, String) {
    match kind.to_uppercase().as_str() {
        "DISCORD" => ("§9", "Дискорд".into()),
        "TELEGRAM" => ("§b", "Телеграм".into()),
        "VK" => ("§b", "ВКонтакте".into()),
        other => ("§b", other.to_owned()),
    }
}

/// A blocked or excluded command: its label, without a namespace, in `list`
/// (CommandUtil.containsCommand).
fn contains_command(line: &str, list: &[String]) -> bool {
    let label = line
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let label = label.rsplit(':').next().unwrap_or_default();
    list.iter().any(|c| c.eq_ignore_ascii_case(label))
}

fn unavailable(status: &tonic::Status) -> bool {
    matches!(
        status.code(),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded
    )
}

/// LagUtil.formatMemory.
fn format_memory(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
}

/// `/proc` readings on Linux: (process CPU ticks, system busy ticks, system total ticks).
fn cpu_ticks() -> Option<(u64, u64, u64)> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let fields: Vec<&str> = stat.rsplit(')').next()?.split_whitespace().collect();
    let process = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    let system = std::fs::read_to_string("/proc/stat").ok()?;
    let cpu: Vec<u64> = system
        .lines()
        .next()?
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    let total: u64 = cpu.iter().sum();
    let idle = cpu.get(3).copied().unwrap_or(0) + cpu.get(4).copied().unwrap_or(0);
    Some((process, total - idle, total))
}

/// A `/proc/meminfo` value in bytes.
fn meminfo(key: &str) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with(key))?;
    Some(line.split_whitespace().nth(1)?.parse::<u64>().ok()? * 1024)
}

fn resident_memory() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|l| l.starts_with("VmRSS:"))?;
    Some(line.split_whitespace().nth(1)?.parse::<u64>().ok()? * 1024)
}

impl Network {
    /// The plugin, reading `path`; `token` checks the launcher's tokens (`[auth]` of velocity.toml).
    pub fn new(path: PathBuf, token: Option<Arc<LauncherToken>>) -> Result<Arc<Self>, String> {
        let config = NetworkConfig::load(&path)?;
        let core = Core::new(&config.core.host, config.core.port, &config.core.token)?;
        Ok(Arc::new(Self {
            core,
            path,
            token,
            state: Mutex::new(State {
                config,
                servers: BTreeMap::new(),
                maintenance: pb::MaintenanceView::default(),
                groups: HashMap::new(),
                users: HashMap::new(),
                accounts: HashMap::new(),
                joined: HashMap::new(),
                pending: HashMap::new(),
                lobby_cooldown: HashMap::new(),
                command_cooldown: HashMap::new(),
                motd: 0,
                broadcast: 0,
                traffic: (0, 0),
                cpu: None,
            }),
            proxy: OnceLock::new(),
            me: OnceLock::new(),
        }))
    }

    fn proxy(&self) -> &ProxyHandle {
        self.proxy.get().expect("the plugin is enabled")
    }
    fn me(&self) -> Option<Arc<Self>> {
        self.me.get().and_then(Weak::upgrade)
    }
    fn player(&self, uuid: u128) -> Option<PlayerHandle> {
        self.proxy.get()?.players.get(uuid)
    }
    fn has(&self, player: &PlayerInfo, permission: &str) -> bool {
        self.proxy
            .get()
            .is_some_and(|p| p.has_permission(player, permission))
    }

    // ---- lobbies (LobbyManager) ----

    /// NetworkUtil.canAccess.
    fn can_access(&self, player: &PlayerInfo, server: &pb::ServerView) -> bool {
        match server.state.as_str() {
            "ONLINE" => true,
            "WHITELIST" => {
                server
                    .whitelist
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(&player.name))
                    || self.has(player, "network.server.whitelist")
                    || self.has(
                        player,
                        &format!("network.server.{}.whitelist", server.r#type.to_lowercase()),
                    )
            }
            _ => false,
        }
    }

    /// getOptimalServer: the accessible lobby of this proxy with the fewest players.
    fn optimal_lobby(&self, player: &PlayerInfo, exclude: Option<&str>) -> Option<String> {
        let proxy = self.proxy.get()?;
        let lobbies: Vec<pb::ServerView> = {
            let st = self.state.lock().unwrap();
            st.servers
                .values()
                .filter(|s| s.r#type == LOBBY)
                .cloned()
                .collect()
        };
        let pending = self.state.lock().unwrap().pending.clone();
        lobbies
            .iter()
            .filter(|s| self.can_access(player, s))
            .filter(|s| proxy.config.servers.addresses.contains_key(&s.id))
            .filter(|s| Some(s.id.as_str()) != exclude)
            .min_by_key(|s| {
                proxy.players.count_on(&s.id) as i32 + pending.get(&s.id).copied().unwrap_or(0)
            })
            .map(|s| s.id.clone())
    }

    fn has_running_lobby(&self) -> bool {
        let Some(proxy) = self.proxy.get() else {
            return false;
        };
        let st = self.state.lock().unwrap();
        st.servers.values().any(|s| {
            s.r#type == LOBBY
                && matches!(s.state.as_str(), "ONLINE" | "WHITELIST")
                && proxy.config.servers.addresses.contains_key(&s.id)
        })
    }

    /// LobbyManager.toLobby.
    fn to_lobby(&self, player: &PlayerHandle) {
        match self.optimal_lobby(&player.info, None) {
            Some(lobby) if player.server.as_deref() != Some(&lobby) => player.connect(&lobby),
            _ => player.message(text::legacy(&format!(
                "{ERROR}Извините, в данный момент нет для вас доступного Лобби. Попробуйте позже!"
            ))),
        }
    }

    /// checkCooldown: true while the player has to wait.
    fn lobby_cooldown(&self, uuid: u128) -> bool {
        let mut st = self.state.lock().unwrap();
        let now = Instant::now();
        if st
            .lobby_cooldown
            .get(&uuid)
            .is_some_and(|until| *until > now)
        {
            return true;
        }
        let until = now + st.config.lobby_cooldown();
        st.lobby_cooldown.insert(uuid, until);
        false
    }

    /// Counted until the player arrives, at most a second.
    fn add_pending(&self, server: &str) {
        *self
            .state
            .lock()
            .unwrap()
            .pending
            .entry(server.to_owned())
            .or_default() += 1;
        let (me, server) = (self.me(), server.to_owned());
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if let Some(me) = me {
                if let Some(n) = me.state.lock().unwrap().pending.get_mut(&server) {
                    *n -= 1;
                }
            }
        });
    }

    // ---- joining (PlayerListener, PlayerManager) ----

    async fn track(&self, uuid: u128) -> Result<(), tonic::Status> {
        let request = self.core.request(pb::PlayerRequest {
            player: uuid_string(uuid),
        });
        let user = self
            .core
            .permissions()
            .get_user(request)
            .await?
            .into_inner();
        let nodes = user.nodes.iter().map(Node::from_view).collect();
        self.state.lock().unwrap().users.insert(uuid, nodes);
        Ok(())
    }

    /// PunishmentUtil.checkBan: the ban's text, or `None`; a bought unban lifts the ban.
    async fn check_ban(&self, player: &PlayerInfo) -> Result<Option<String>, tonic::Status> {
        let query = pb::PunishmentTargetQuery {
            target: Some(uuid_string(player.uuid)),
            name: Some(player.name.clone()),
            ip: Some(player.address.ip().to_string()),
            r#type: "BAN".into(),
        };
        let response = self
            .core
            .punishments()
            .get_active(self.core.request(query))
            .await?
            .into_inner();
        let now = now_millis();
        let Some(ban) = response.punishment.filter(|p| is_active(p, now)) else {
            return Ok(None);
        };
        let unclaimed = pb::UnclaimedQuery {
            player: uuid_string(player.uuid),
            server: None,
            r#type: Some("UNBAN".into()),
        };
        let unbans = self
            .core
            .store()
            .list_unclaimed(self.core.request(unclaimed))
            .await?
            .into_inner()
            .orders;
        let Some(unban) = unbans.first() else {
            let issuer = match &ban.issuer {
                Some(id) => {
                    let query = pb::AccountQuery {
                        key: Some(pb::account_query::Key::Id(id.clone())),
                        with_history: false,
                    };
                    self.core
                        .accounts()
                        .get_account(self.core.request(query))
                        .await
                        .ok()
                        .and_then(|r| r.into_inner().account)
                        .map(|a| a.name)
                }
                None => None,
            };
            return Ok(Some(ban_text(&ban, issuer.as_deref(), now)));
        };
        let cancel = pb::PunishmentCancel {
            punishment: ban.id.clone(),
            remover: None,
            remover_ip: None,
            reason: None,
        };
        self.core
            .punishments()
            .cancel(self.core.request(cancel))
            .await?;
        let claim = pb::OrderLookup {
            order: unban.id.clone(),
        };
        self.core
            .store()
            .claim_order(self.core.request(claim))
            .await?;
        Ok(None)
    }

    /// PlayerListener.join: the refusal's text, or `None` when Core let the player in.
    async fn join(&self, player: &PlayerInfo) -> Result<Option<String>, tonic::Status> {
        // Mirrored before the join: the staff flag of the join is a permission.
        self.track(player.uuid).await?;
        if let Some(ban) = self.check_ban(player).await? {
            return Ok(Some(ban));
        }
        let request = pb::JoinRequest {
            player: uuid_string(player.uuid),
            name: player.name.clone(),
            ip: player.address.ip().to_string(),
            staff: self.has(player, STAFF),
        };
        let response = self
            .core
            .presence()
            .join(self.core.request(request.clone()))
            .await?
            .into_inner();
        Ok(match pb::JoinResult::try_from(response.result) {
            Ok(pb::JoinResult::JoinOk) => {
                let mut st = self.state.lock().unwrap();
                st.joined.insert(player.uuid, request);
                if let Some(account) = response.account {
                    st.accounts.insert(player.uuid, account);
                }
                None
            }
            Ok(pb::JoinResult::JoinAlreadyOnline) => {
                Some(format!("{ERROR}Этот аккаунт уже в игре."))
            }
            Ok(pb::JoinResult::JoinNoAccount) => {
                log(&format!(
                    "Launcher player {} has no account in the core",
                    player.name
                ));
                Some(format!(
                    "{ERROR}Аккаунт не найден. Войдите в лаунчер заново."
                ))
            }
            _ => return Err(tonic::Status::unknown("unknown join result")),
        })
    }

    fn is_joined(&self, uuid: u128) -> bool {
        self.state.lock().unwrap().joined.contains_key(&uuid)
    }

    /// AuthSubscriber.updateLinkGroup: the verified group follows the account's linked socials.
    async fn update_link_group(&self, uuid: u128) {
        let (linked, has_group) = {
            let st = self.state.lock().unwrap();
            let Some(account) = st.accounts.get(&uuid) else {
                return;
            };
            let group = Node::group(LINK_GROUP);
            let has = st
                .users
                .get(&uuid)
                .is_some_and(|nodes| nodes.iter().any(|n| n.value && n.same_as(&group)));
            (!account.socials.is_empty(), has)
        };
        if linked == has_group {
            return;
        }
        let change = pb::PermissionNodeChange {
            actor: Some(pb::PermissionActor {
                id: String::new(),
                name: "Система".into(),
            }),
            holder: Some(pb::PermissionHolderRef {
                holder: Some(pb::permission_holder_ref::Holder::Player(uuid_string(uuid))),
            }),
            node: Some(Node::group(LINK_GROUP).view()),
        };
        let request = self.core.request(change);
        let result = if linked {
            self.core.permissions().add_node(request).await
        } else {
            self.core.permissions().remove_node(request).await
        };
        match result {
            Ok(response) => {
                if let Some(user) = response.into_inner().user {
                    let nodes = user.nodes.iter().map(Node::from_view).collect();
                    self.state.lock().unwrap().users.insert(uuid, nodes);
                }
                let action = if linked { "Added" } else { "Removed" };
                log(&format!(
                    "{action} verified group {LINK_GROUP} for {}",
                    uuid_string(uuid)
                ));
            }
            Err(e) => log(&format!(
                "Failed to update the verified group: {}",
                e.message()
            )),
        }
    }

    // ---- Core's session and events (ProxySubscriber, MaintenanceManager, ...) ----

    /// After every (re)connection: the resync Core asks for, then fresh mirrors.
    async fn connected(&self, resync: bool) {
        if resync {
            let players = {
                let st = self.state.lock().unwrap();
                st.joined
                    .iter()
                    .map(|(uuid, join)| pb::ResyncPlayer {
                        join: Some(join.clone()),
                        server: self
                            .player(*uuid)
                            .and_then(|p| p.server)
                            .unwrap_or_default(),
                    })
                    .collect()
            };
            self.core
                .send(pb::node_message::Message::ProxyResync(pb::ProxyResync {
                    players,
                }));
        }
        if let Err(e) = self.refresh().await {
            log(&format!(
                "Failed to load the mirrors of the core: {}",
                e.message()
            ));
        }
    }

    async fn refresh(&self) -> Result<(), tonic::Status> {
        let maintenance = self
            .core
            .maintenance()
            .get(self.core.request(pb::Empty {}))
            .await?
            .into_inner();
        let groups = self
            .core
            .permissions()
            .list_groups(self.core.request(pb::Empty {}))
            .await?
            .into_inner()
            .groups;
        let query = pb::ServerListQuery {
            r#type: None,
            active_only: false,
        };
        let servers = self
            .core
            .servers()
            .list(self.core.request(query))
            .await?
            .into_inner()
            .servers;
        let tracked: Vec<u128> = {
            let mut st = self.state.lock().unwrap();
            st.maintenance = maintenance;
            st.groups = groups
                .iter()
                .map(Group::from_view)
                .map(|g| (g.name.clone(), g))
                .collect();
            st.servers = servers.into_iter().map(|s| (s.id.clone(), s)).collect();
            st.users.keys().copied().collect()
        };
        for uuid in tracked {
            self.track(uuid).await?;
        }
        Ok(())
    }

    async fn event(&self, event: pb::Event) {
        use pb::event::Payload;
        let Some(payload) = event.payload else {
            return;
        };
        match payload {
            Payload::Account(account) => self.account(account),
            Payload::Message(m) => {
                let message = text::message(&m.text);
                for player in m.players.iter().filter_map(|p| parse_uuid(p)) {
                    if self.is_joined(player) {
                        if let Some(p) = self.player(player) {
                            p.message(message.clone());
                        }
                    }
                }
            }
            Payload::Alert(alert) => self.proxy().broadcast(&text::message(&alert.text)),
            Payload::Kick(kick) => {
                if let Some(p) = parse_uuid(&kick.player).and_then(|u| self.player(u)) {
                    p.disconnect(text::message(&kick.reason));
                }
            }
            Payload::Connect(connect) => self.connect(connect),
            Payload::Signal(signal) if signal.r#type == "reload" => self.reload(),
            Payload::Maintenance(view) => self.maintenance(view),
            Payload::Servers(list) => {
                self.state.lock().unwrap().servers = list
                    .servers
                    .into_iter()
                    .map(|s| (s.id.clone(), s))
                    .collect();
            }
            Payload::PermissionGroup(group) => {
                let group = Group::from_view(&group);
                self.state
                    .lock()
                    .unwrap()
                    .groups
                    .insert(group.name.clone(), group);
            }
            Payload::PermissionGroupRemoved(removed) => {
                self.state
                    .lock()
                    .unwrap()
                    .groups
                    .remove(&removed.name.to_lowercase());
            }
            Payload::PermissionUser(user) => {
                if let Some(uuid) = parse_uuid(&user.player) {
                    let mut st = self.state.lock().unwrap();
                    if let Some(nodes) = st.users.get_mut(&uuid) {
                        *nodes = user.nodes.iter().map(Node::from_view).collect();
                    }
                }
            }
            Payload::Auth(notice) => self.auth_notice(notice).await,
            _ => {}
        }
    }

    /// SkinSubscriber: a new skin goes to the profile used from the next server on.
    fn account(&self, account: pb::AccountView) {
        let Some(uuid) = parse_uuid(&account.id) else {
            return;
        };
        let Some(player) = self.player(uuid) else {
            return;
        };
        self.state
            .lock()
            .unwrap()
            .accounts
            .insert(uuid, account.clone());
        let url = account.skin.as_ref().map(|s| s.url.clone());
        if skin_url(&player.info.properties) == url {
            return;
        }
        let mut properties: Vec<Property> = player
            .info
            .properties
            .iter()
            .filter(|p| p.name != "textures")
            .cloned()
            .collect();
        properties.extend(textures(&account));
        player.order(Order::Properties(properties));
    }

    fn connect(&self, connect: pb::PlayerConnect) {
        let Some(uuid) = parse_uuid(&connect.player) else {
            return;
        };
        let Some(player) = self.player(uuid).filter(|_| self.is_joined(uuid)) else {
            return;
        };
        match pb::ConnectTarget::try_from(connect.target) {
            Ok(pb::ConnectTarget::ConnectServer) => {
                if let Some(server) = connect
                    .server
                    .filter(|s| self.proxy().config.servers.addresses.contains_key(s))
                {
                    player.connect(&server);
                }
            }
            Ok(pb::ConnectTarget::ConnectLobby) => {
                if connect.command && self.lobby_cooldown(uuid) {
                    player.message(text::legacy(&format!(
                        "{ERROR}Пожалуйста, подождите немного..."
                    )));
                    return;
                }
                self.to_lobby(&player);
            }
            _ => {}
        }
    }

    fn reload(&self) {
        match NetworkConfig::load(&self.path) {
            Ok(config) => {
                self.state.lock().unwrap().config = config;
                log("Reloaded config");
            }
            Err(e) => log(&format!("Failed to reload the config: {e}")),
        }
    }

    fn maintenance_message(&self) -> Tag {
        text::lines(&self.state.lock().unwrap().config.maintenance.kick_message)
    }

    fn is_whitelisted(&self, ip: &str) -> bool {
        let st = self.state.lock().unwrap();
        st.maintenance.whitelist.iter().any(|w| w == ip)
    }

    /// MaintenanceManager.changed.
    fn maintenance(&self, view: pb::MaintenanceView) {
        let was = {
            let mut st = self.state.lock().unwrap();
            if view.version < st.maintenance.version {
                return;
            }
            let was = st.maintenance.enabled;
            st.maintenance = view.clone();
            was
        };
        if view.enabled {
            let reason = self.maintenance_message();
            for p in self.proxy().players.all() {
                if !self.is_whitelisted(&p.ip().to_string()) {
                    p.disconnect(reason.clone());
                }
            }
        }
        if view.enabled != was {
            let status = if view.enabled {
                "§aвкл."
            } else {
                "§cвыкл."
            };
            self.proxy().broadcast(&text::legacy(&format!(
                "{SOMETHING}Режим тех. работ был {status}"
            )));
        }
    }

    /// AuthSubscriber.handle.
    async fn auth_notice(&self, notice: pb::AuthNotice) {
        let Some(player) = parse_uuid(&notice.player).and_then(|u| self.player(u)) else {
            return;
        };
        match pb::AuthNoticeType::try_from(notice.r#type) {
            Ok(pb::AuthNoticeType::AuthPasswordChanged) => {
                // Sessions on other ips are closed, the one which has changed the password stays.
                if notice.ip.is_some_and(|ip| ip != player.ip().to_string()) {
                    player.disconnect(text::legacy(&format!(
                        "{ERROR}Ваш пароль был изменён в личном кабинете."
                    )));
                }
            }
            Ok(pb::AuthNoticeType::AuthSocialLinked) => {
                let Some(kind) = notice.social else { return };
                let (color, name) = social(&kind);
                linked_message(
                    &player,
                    SUCCESS,
                    "§f",
                    &format!("Вы успешно привязали свой аккаунт к {color}{name}"),
                    "Привязка",
                );
                if notice.totp {
                    player.message(text::legacy(&format!(
                        "{SOMETHING}Ваш аккаунт теперь защищён через {name}§f!"
                    )));
                    player.message(text::legacy(&format!(
                        "{CUBE}§7При входе в лаунчер бот пришлёт код подтверждения."
                    )));
                } else {
                    player.message(text::legacy(&format!(
                        "{CUBE}§7Через бота можно восстановить доступ к аккаунту. Включить защиту входа: §b/link"
                    )));
                }
                self.update_link_group(player.info.uuid).await;
            }
            Ok(pb::AuthNoticeType::AuthSocialUnlinked) => {
                let Some(kind) = notice.social else { return };
                let (_, name) = social(&kind);
                linked_message(
                    &player,
                    ERROR,
                    "§c",
                    &format!("Вы отвязали свой аккаунт от {name}"),
                    "Отвязка",
                );
                let linked = self
                    .state
                    .lock()
                    .unwrap()
                    .accounts
                    .get(&player.info.uuid)
                    .map(|a| !a.socials.is_empty());
                if linked == Some(false) {
                    player.message(text::legacy(&format!(
                        "{CUBE}§7У аккаунта не осталось привязанных соц. сетей. Привяжите новую для защиты: §b/link"
                    )));
                }
                self.update_link_group(player.info.uuid).await;
            }
            _ => {}
        }
    }

    // ---- timers ----

    fn start_timers(self: &Arc<Self>) {
        // PingManager: the next MOTD every motd-delay seconds.
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                let delay = match me.upgrade() {
                    Some(me) => {
                        let mut st = me.state.lock().unwrap();
                        let n = st.config.bungee.motds.len();
                        st.motd = if n == 0 { 0 } else { (st.motd + 1) % n };
                        st.config.bungee.motd_delay.max(1)
                    }
                    None => return,
                };
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        });
        // BroadcastManager: an announcement every `delay` seconds, after 5 s.
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            loop {
                let delay = match me.upgrade() {
                    Some(me) => {
                        me.announce();
                        me.state.lock().unwrap().config.broadcast.delay.max(1)
                    }
                    None => return,
                };
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        });
        // TrafficManager and LagUtil: samples every second.
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            let load =
                |a: &std::sync::atomic::AtomicU64| a.load(std::sync::atomic::Ordering::Relaxed);
            let (mut last_in, mut last_out) =
                (load(&crate::net::BYTES_IN), load(&crate::net::BYTES_OUT));
            let mut last_cpu = cpu_ticks();
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some(me) = me.upgrade() else { return };
                let (now_in, now_out) = (load(&crate::net::BYTES_IN), load(&crate::net::BYTES_OUT));
                let cpu = cpu_ticks();
                let mut st = me.state.lock().unwrap();
                st.traffic = (now_in - last_in, now_out - last_out);
                // Clock ticks are 1/100 s on Linux.
                st.cpu = match (last_cpu, cpu) {
                    (Some((p0, b0, t0)), Some((p1, b1, t1))) if t1 > t0 => Some((
                        (p1 - p0) as f64 / (100.0 * cores) * 100.0,
                        (b1 - b0) as f64 / (t1 - t0) as f64 * 100.0,
                    )),
                    _ => None,
                };
                (last_in, last_out, last_cpu) = (now_in, now_out, cpu);
            }
        });
    }

    fn announce(&self) {
        let message = {
            let mut st = self.state.lock().unwrap();
            let messages = &st.config.broadcast.messages;
            if messages.is_empty() || self.proxy().players.count() == 0 {
                return;
            }
            let index = st.broadcast % messages.len();
            let announcement = messages[index].clone();
            st.broadcast = index + 1;
            let lines: Vec<String> = announcement
                .lines
                .iter()
                .map(|l| {
                    if announcement.center {
                        text::center(l)
                    } else {
                        l.clone()
                    }
                })
                .collect();
            text::lines(&lines)
        };
        self.proxy().broadcast(&message);
    }

    // ---- commands ----

    fn commands(self: &Arc<Self>) -> Vec<Command> {
        let me = Arc::downgrade(self);
        let maintenance = Command {
            name: "maintenance".into(),
            aliases: Vec::new(),
            permission: Some("network.maintenance".into()),
            run: Arc::new(move |_, source, args| {
                let me = me.clone();
                Box::pin(async move {
                    if let Some(me) = me.upgrade() {
                        me.maintenance_command(source, args).await;
                    }
                })
            }),
            suggest: Some(Arc::new(|_, _, args| match args.len() {
                1 => vec!["add".into(), "remove".into(), "toggle".into()],
                _ => Vec::new(),
            })),
        };
        let me = Arc::downgrade(self);
        let proxy = Command {
            name: "proxy".into(),
            aliases: Vec::new(),
            permission: Some("network.admin".into()),
            run: Arc::new(move |_, source, args| {
                let me = me.clone();
                Box::pin(async move {
                    if let Some(me) = me.upgrade() {
                        me.proxy_command(&source, &args);
                    }
                })
            }),
            suggest: Some(Arc::new(|_, _, args| match args.len() {
                1 => vec!["cpu".into(), "ram".into(), "network".into()],
                _ => Vec::new(),
            })),
        };
        vec![maintenance, proxy]
    }

    /// MaintenanceCommand.
    async fn maintenance_command(&self, source: Source, args: Vec<String>) {
        let say = |s: String| source.message(text::legacy(&s));
        let failed = |e: tonic::Status| {
            say(if unavailable(&e) {
                format!("{ERROR}Ядро недоступно, попробуйте позже.")
            } else {
                format!("{ERROR}Не удалось: {}", e.message())
            })
        };
        match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["add", ip] => {
                if self.is_whitelisted(ip) {
                    return say(format!("{ERROR}{ip} уже находится в белом списке!"));
                }
                let request = self
                    .core
                    .request(pb::MaintenanceAddress { ip: (*ip).into() });
                match self.core.maintenance().add_address(request).await {
                    Ok(view) => {
                        self.maintenance(view.into_inner());
                        say(format!("{SUCCESS}Вы добавили {ip} в белый список."));
                    }
                    Err(e) => failed(e),
                }
            }
            ["remove", ip] => {
                if !self.is_whitelisted(ip) {
                    return say(format!("{ERROR}{ip} не находится в белом списке!"));
                }
                let request = self
                    .core
                    .request(pb::MaintenanceAddress { ip: (*ip).into() });
                match self.core.maintenance().remove_address(request).await {
                    Ok(view) => {
                        self.maintenance(view.into_inner());
                        say(format!("{SUCCESS}Вы удалили {ip} из белого списка."));
                    }
                    Err(e) => failed(e),
                }
            }
            ["toggle"] => {
                // Every proxy announces the change itself, from Core's event.
                let enabled = !self.state.lock().unwrap().maintenance.enabled;
                let request = self.core.request(pb::MaintenanceToggle { enabled });
                if let Err(e) = self.core.maintenance().set_enabled(request).await {
                    failed(e);
                }
            }
            _ => say(format!("{ERROR}/maintenance <add|remove|toggle> [ip]")),
        }
    }

    /// ProxyCommand.
    fn proxy_command(&self, source: &Source, args: &[String]) {
        let lines: Vec<String> = match args.first().map(String::as_str) {
            Some("cpu") => {
                let (cpu, cores) = (
                    self.state.lock().unwrap().cpu,
                    std::thread::available_parallelism().map_or(1, |n| n.get()),
                );
                let pct = |v: Option<f64>| v.map_or("н/д".into(), |v| format!("{v:.2}%"));
                vec![
                    format!(
                        "Использование CPU сервером в данный момент: §e{}",
                        pct(cpu.map(|c| c.0))
                    ),
                    format!(
                        "Использование CPU системой прямо сейчас: §e{}",
                        pct(cpu.map(|c| c.1))
                    ),
                    format!("Общее количество виртуальных ядер: §e{cores}"),
                ]
            }
            Some("ram") => {
                let mem = |v: Option<u64>| v.map_or("н/д".into(), format_memory);
                vec![
                    format!("Используемая память прокси: §e{}", mem(resident_memory())),
                    format!(
                        "Свободная память системы: §e{}",
                        mem(meminfo("MemAvailable:"))
                    ),
                    format!(
                        "Общий объем памяти системы: §e{}",
                        mem(meminfo("MemTotal:"))
                    ),
                ]
            }
            Some("network") => {
                let (incoming, outgoing) = self.state.lock().unwrap().traffic;
                let total = |a: &std::sync::atomic::AtomicU64| {
                    format_memory(a.load(std::sync::atomic::Ordering::Relaxed))
                };
                vec![
                    format!("Входящий трафик сейчас: §e{}/с", format_memory(incoming)),
                    format!("Исходящий трафик сейчас: §e{}/с", format_memory(outgoing)),
                    format!(
                        "Всего входящего трафика: §e{}",
                        total(&crate::net::BYTES_IN)
                    ),
                    format!(
                        "Всего исходящего трафика: §e{}",
                        total(&crate::net::BYTES_OUT)
                    ),
                ]
            }
            _ => return source.message(text::legacy(&format!("{ERROR}/proxy <cpu|ram|network>"))),
        };
        source.message(Tag::String("".into()));
        for line in lines {
            source.message(text::legacy(&format!("{CUBE}{line}")));
        }
        source.message(Tag::String("".into()));
    }
}

/// AuthSubscriber.sendMessage: the message and a title.
fn linked_message(player: &PlayerHandle, prefix: &str, color: &str, message: &str, title: &str) {
    player.message(text::legacy(&format!("{prefix}{color}{message}{color}!")));
    player.order(Order::Title {
        title: text::legacy(&format!("{SECONDARY}§l{title}")),
        subtitle: text::legacy("Приятной игры!"),
    });
}

impl Plugin for Network {
    fn name(&self) -> &str {
        "network"
    }

    fn enable(self: Arc<Self>, proxy: ProxyHandle) -> Fut<'static, Result<(), String>> {
        Box::pin(async move {
            let _ = self.me.set(Arc::downgrade(&self));
            for command in self.commands() {
                proxy.register_command(command);
            }
            let _ = self.proxy.set(proxy);
            let hello = pb::Hello {
                node_id: self.state.lock().unwrap().config.node(),
                kind: pb::NodeKind::Proxy as i32,
                protocol_version: core::PROTOCOL_VERSION,
                software: format!("Horizon Proxy {}", horizon_protocol::GAME_VERSION),
                topics: vec!["servers".into(), "signals".into(), "permissions".into()],
            };
            let (tx, mut rx) = mpsc::unbounded_channel();
            tokio::spawn(self.core.clone().run_session(hello, tx));
            let me = Arc::downgrade(&self);
            tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    let Some(me) = me.upgrade() else { return };
                    match event {
                        SessionEvent::Connected { resync } => {
                            tokio::spawn(async move { me.connected(resync).await });
                        }
                        SessionEvent::Event(event) => me.event(event).await,
                        SessionEvent::Lost(_) => {}
                    }
                }
            });
            self.start_timers();
            Ok(())
        })
    }

    fn auth(&self) -> Option<Arc<dyn AuthHandler>> {
        Some(Arc::new(LauncherAuth {
            token: self.token.clone(),
            accounts: Some(Arc::new(CoreAccounts(self.core.clone()))),
        }))
    }

    /// LoginListener: maintenance and the connections of one address.
    fn pre_login<'a>(&'a self, event: &'a mut PreLogin) -> Fut<'a, ()> {
        Box::pin(async move {
            let ip = event.address.ip();
            let enabled = self.state.lock().unwrap().maintenance.enabled;
            if enabled && !self.is_whitelisted(&ip.to_string()) {
                event.denied = Some(self.maintenance_message());
                return;
            }
            let limit = self
                .state
                .lock()
                .unwrap()
                .config
                .settings
                .ip_connections_limit;
            if self.proxy().players.count_ip(ip) >= limit {
                event.denied = Some(text::legacy(&format!(
                    "{ERROR}Вы имеете слишком много активных подключений."
                )));
            }
        })
    }

    /// PlayerListener.onLogin.
    fn login<'a>(&'a self, event: &'a mut Login) -> Fut<'a, ()> {
        Box::pin(async move {
            if !self.has_running_lobby() {
                event.denied = Some(text::legacy(&format!(
                    "{ERROR}Лобби сервера перезапускаются. Попробуйте через пару секунд!"
                )));
                return;
            }
            match self.join(&event.player).await {
                Ok(None) => {}
                Ok(Some(reason)) => event.denied = Some(text::legacy(&reason)),
                Err(e) => {
                    log(&format!(
                        "Failed to join {}: {}",
                        event.player.name,
                        e.message()
                    ));
                    event.denied = Some(text::legacy(&format!(
                        "{ERROR}Сервер временно недоступен, попробуйте позже."
                    )));
                }
            }
        })
    }

    /// PlayerListener.onDisconnect and the cooldowns of the managers.
    fn disconnect<'a>(&'a self, player: &'a PlayerInfo, _logged_in: bool) -> Fut<'a, ()> {
        Box::pin(async move {
            let joined = {
                let mut st = self.state.lock().unwrap();
                st.users.remove(&player.uuid);
                st.accounts.remove(&player.uuid);
                st.lobby_cooldown.remove(&player.uuid);
                st.command_cooldown.remove(&player.uuid);
                st.joined.remove(&player.uuid).is_some()
            };
            if joined {
                let request = self.core.request(pb::PlayerRequest {
                    player: uuid_string(player.uuid),
                });
                if let Err(e) = self.core.presence().leave(request).await {
                    // Core drops the player itself: it is not in the next resync.
                    log(&format!(
                        "Failed to report the leave of {}: {}",
                        player.name,
                        e.message()
                    ));
                }
            }
        })
    }

    /// LobbyListener.onInitial.
    fn choose_initial_server(&self, player: &PlayerInfo, server: &mut Option<String>) {
        *server = self.optimal_lobby(player, None);
    }

    /// ConnectListener and AuthSubscriber.onConnected.
    fn server_connected(&self, player: &PlayerInfo, server: &str, previous: Option<&str>) {
        if !self.is_joined(player.uuid) {
            return;
        }
        let (core, uuid, server, name) = (
            self.core.clone(),
            player.uuid,
            server.to_owned(),
            player.name.clone(),
        );
        tokio::spawn(async move {
            let request = core.request(pb::SwitchRequest {
                player: uuid_string(uuid),
                server,
            });
            if let Err(e) = core.presence().switch(request).await {
                // Sent with the players of this proxy after the reconnection.
                log(&format!(
                    "Failed to report the server of {name}: {}",
                    e.message()
                ));
            }
        });
        if previous.is_none() {
            if let Some(me) = self.me() {
                tokio::spawn(async move { me.update_link_group(uuid).await });
            }
        }
    }

    /// LobbyListener.onFallback.
    fn kicked(&self, event: &mut Kicked<'_>) {
        if event.current.is_none() {
            return;
        }
        let reason = event.reason.map(text::plain).unwrap_or_default();
        if !reason.is_empty() {
            let ignored = self
                .state
                .lock()
                .unwrap()
                .config
                .fallback
                .ignore_reasons
                .clone();
            if ignored.iter().any(|w| reason.contains(w.as_str())) {
                return;
            }
        }
        match self.optimal_lobby(event.player, Some(event.server)) {
            Some(lobby) => {
                let notice = match &event.result {
                    KickResult::Redirect { notice, .. } => notice.clone(),
                    _ => event.reason.cloned(),
                };
                self.add_pending(&lobby);
                event.result = KickResult::Redirect {
                    server: lobby,
                    notice,
                };
            }
            None => {
                let message = text::legacy(&format!(
                    "{ERROR}Лобби сервера перезапускаются. Попробуйте через пару секунд!"
                ));
                event.result = if event.during_connect {
                    KickResult::Notify(message)
                } else {
                    KickResult::Disconnect(message)
                };
            }
        }
    }

    /// CommandListener.onCommand: blocked commands and the cooldown.
    fn command(&self, player: &PlayerInfo, line: &str) -> CommandResult {
        let (blocked, excluded, cooldown) = {
            let st = self.state.lock().unwrap();
            (
                contains_command(line, &st.config.commands.blocked),
                contains_command(line, &st.config.commands.cooldown_exclusions),
                st.config.command_cooldown(),
            )
        };
        let say = |s: &str| {
            if let Some(p) = self.player(player.uuid) {
                p.message(text::legacy(&format!("{ERROR}{s}")));
            }
        };
        if blocked && !self.has(player, "network.command.block") {
            say("У вас нет доступа к этой команде.");
            return CommandResult::Denied;
        }
        if !excluded && !self.has(player, "network.command.cooldown") {
            let mut st = self.state.lock().unwrap();
            let now = Instant::now();
            if st
                .command_cooldown
                .get(&player.uuid)
                .is_some_and(|until| *until > now)
            {
                drop(st);
                say("Подождите немного перед следующей командой...");
                return CommandResult::Denied;
            }
            st.command_cooldown.insert(player.uuid, now + cooldown);
        }
        CommandResult::Allowed
    }

    /// CommandListener.onLog: the commands the proxy ran go to Core's action log.
    fn command_executed(&self, player: &PlayerInfo, line: &str, by_proxy: bool) {
        if !by_proxy || !self.is_joined(player.uuid) {
            return;
        }
        let command: String = format!("/{line}").chars().take(512).collect();
        let request = self.core.request(pb::ActionLog {
            player: uuid_string(player.uuid),
            ip: Some(player.address.ip().to_string()),
            server: self
                .player(player.uuid)
                .and_then(|p| p.server)
                .unwrap_or_default(),
            action: Some(format!("command: {command}")),
        });
        let core = self.core.clone();
        tokio::spawn(async move {
            let _ = core.actions().log(request).await;
        });
    }

    /// CommandListener.onTab.
    fn tab_complete(&self, player: &PlayerInfo, partial: &str) -> bool {
        let blocked = {
            let st = self.state.lock().unwrap();
            contains_command(partial, &st.config.commands.blocked)
        };
        !blocked || self.has(player, "network.command.block")
    }

    /// PingListener.
    fn ping(&self, ping: &mut Ping) {
        let st = self.state.lock().unwrap();
        ping.online = st.servers.values().map(|s| i64::from(s.online)).sum();
        ping.max = st.config.bungee.max_players;
        ping.sample = st
            .config
            .bungee
            .description
            .iter()
            .map(|l| (text::translate(l), 0))
            .collect();
        if st.maintenance.enabled {
            ping.protocol = 999;
            ping.version_name = "Тех. работы".into();
        } else {
            ping.version_name = st.config.bungee.name.clone();
        }
        if let Some(motd) = st.config.bungee.motds.get(st.motd) {
            ping.description = text::to_json(&text::lines(&[&motd.first, &motd.second]));
        }
    }

    /// PermissionListener: Core's permissions in the global context.
    fn permission(&self, player: &PlayerInfo, permission: &str) -> Tristate {
        let st = self.state.lock().unwrap();
        let Some(nodes) = st.users.get(&player.uuid) else {
            return Tristate::Undefined;
        };
        permissions::calculate(nodes, &st.groups, &BTreeMap::new(), now_millis()).check(permission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ban_texts_and_durations() {
        assert_eq!(format_detailed(0), "сейчас");
        assert_eq!(format_detailed(61_000), "1 минута 1 секунда");
        assert_eq!(
            format_detailed(2 * 86_400_000 + 5 * 3_600_000 + 30_000),
            "2 дня 5 часов"
        );
        assert_eq!(plurals(11, "день", "дня", "дней"), "11 дней");
        assert_eq!(plurals(22, "день", "дня", "дней"), "22 дня");
        let ban = pb::PunishmentView {
            id: "1".into(),
            r#type: "BAN".into(),
            reason: Some("читы".into()),
            expires_at: Some(3_600_000),
            ..Default::default()
        };
        let text = ban_text(&ban, Some("Admin"), 0);
        assert!(text.contains("§cПричина: §fчиты"));
        assert!(text.contains("§cОсталось: §f1 час"));
        assert!(text.contains("§cЗабанил: §fAdmin"));
        assert!(text.ends_with(WEBSITE));
        assert!(is_active(&ban, 0));
        assert!(!is_active(&ban, 3_600_000));
        let permanent = pb::PunishmentView {
            reason: None,
            expires_at: None,
            ..ban
        };
        assert!(ban_text(&permanent, None, 0).contains("навсегда"));
        assert!(ban_text(&pb::PunishmentView::default(), None, 0).contains("Консоль"));
    }

    #[test]
    fn commands_and_skins() {
        let blocked = vec!["plugins".to_owned(), "pl".to_owned()];
        assert!(contains_command("pl", &blocked));
        assert!(contains_command("bukkit:plugins x", &blocked));
        assert!(!contains_command("plot", &blocked));
        let account = pb::AccountView {
            id: "11111111-2222-3333-4444-555555555555".into(),
            name: "Steve".into(),
            skin: Some(pb::AccountSkinView {
                url: "https://cdn.minecomplex.net/objects/ab/x".into(),
                sha256: String::new(),
            }),
            ..Default::default()
        };
        let properties = textures(&account);
        assert_eq!(
            skin_url(&properties).as_deref(),
            Some("https://cdn.minecomplex.net/objects/ab/x")
        );
        assert_eq!(properties[0].signature.as_deref(), Some(""));
        assert!(textures(&pb::AccountView::default()).is_empty());
    }
}
