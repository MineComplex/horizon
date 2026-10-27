//! Proxy plugins: Velocity's events and commands, as a Rust trait compiled into the proxy.
//!
//! Every hook has a default that changes nothing. Hooks that may wait on the network (login,
//! disconnect, commands) return futures and run on the event loop; the others are called inline
//! and must answer from memory, like a Velocity listener reading a mirror.
use crate::{
    auth::AuthHandler,
    players::{PlayerHandle, PlayerInfo},
};
use horizon_world::nbt::Tag;
use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc};

pub type Fut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub fn done<'a, T: Send + 'a>(value: T) -> Fut<'a, T> {
    Box::pin(std::future::ready(value))
}

/// Tristate: a permission set to true, to false, or not set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tristate {
    True,
    False,
    Undefined,
}

/// PreLoginEvent: the name is known, nothing is authenticated yet.
pub struct PreLogin {
    pub address: SocketAddr,
    pub name: String,
    /// Set to refuse the connection.
    pub denied: Option<Tag>,
}

/// LoginEvent: the player is authenticated and about to be let in.
pub struct Login {
    pub player: PlayerInfo,
    pub denied: Option<Tag>,
}

/// KickedFromServerEvent's results.
#[derive(Clone, Debug)]
pub enum KickResult {
    /// To another server, with a message once there.
    Redirect {
        server: String,
        notice: Option<Tag>,
    },
    Disconnect(Tag),
    /// The player stays where they are and is told.
    Notify(Tag),
}

/// KickedFromServerEvent: a server refused or dropped the player.
pub struct Kicked<'a> {
    pub player: &'a PlayerInfo,
    pub server: &'a str,
    /// The server the player was on when it happened: `server` itself for a kick, `None` while
    /// joining the proxy (KickedFromServerEvent's getCurrentServer).
    pub current: Option<&'a str>,
    /// The server's reason, when it gave one.
    pub reason: Option<&'a Tag>,
    /// True when the player was connecting to `server` and is still elsewhere.
    pub during_connect: bool,
    pub result: KickResult,
}

/// ProxyPingEvent: the server list entry.
pub struct Ping {
    pub version_name: String,
    pub protocol: i32,
    pub online: i64,
    pub max: i64,
    /// Names shown when hovering the player count.
    pub sample: Vec<(String, u128)>,
    /// A JSON chat component.
    pub description: serde_json::Value,
}

/// CommandExecuteEvent's results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandResult {
    Allowed,
    Denied,
    /// Sent to the server even when the proxy has the command.
    Forward,
}

/// Who runs a command.
#[derive(Clone)]
pub enum Source {
    Console,
    Player(PlayerHandle),
}
impl Source {
    pub fn message(&self, message: Tag) {
        match self {
            Self::Console => crate::log(&crate::text::plain(&message)),
            Self::Player(p) => p.message(message),
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Console => "CONSOLE",
            Self::Player(p) => &p.info.name,
        }
    }
}

pub type Run =
    Arc<dyn Fn(crate::ProxyHandle, Source, Vec<String>) -> Fut<'static, ()> + Send + Sync>;
pub type Suggest = Arc<dyn Fn(&crate::ProxyHandle, &Source, &[&str]) -> Vec<String> + Send + Sync>;

/// A command of the proxy (BrigadierCommand / SimpleCommand).
#[derive(Clone)]
pub struct Command {
    pub name: String,
    pub aliases: Vec<String>,
    /// Needed set to true; `!permission` lets in everyone it isn't set to false for; `None` lets
    /// everyone run it.
    pub permission: Option<String>,
    pub run: Run,
    /// Completions of the last argument, given the arguments typed so far.
    pub suggest: Option<Suggest>,
}

pub trait Plugin: Send + Sync + 'static {
    fn name(&self) -> &str;
    /// ProxyInitializeEvent: the proxy is about to accept players.
    fn enable(self: Arc<Self>, _proxy: crate::ProxyHandle) -> Fut<'static, Result<(), String>> {
        done(Ok(()))
    }
    /// ProxyShutdownEvent.
    fn disable(self: Arc<Self>) -> Fut<'static, ()> {
        done(())
    }
    /// The AuthHandler this plugin sets, if any (the last plugin's wins).
    fn auth(&self) -> Option<Arc<dyn AuthHandler>> {
        None
    }
    fn pre_login<'a>(&'a self, _event: &'a mut PreLogin) -> Fut<'a, ()> {
        done(())
    }
    fn login<'a>(&'a self, _event: &'a mut Login) -> Fut<'a, ()> {
        done(())
    }
    /// DisconnectEvent, also for a login that was denied (`logged_in` false).
    fn disconnect<'a>(&'a self, _player: &'a PlayerInfo, _logged_in: bool) -> Fut<'a, ()> {
        done(())
    }
    /// PlayerChooseInitialServerEvent.
    fn choose_initial_server(&self, _player: &PlayerInfo, _server: &mut Option<String>) {}
    /// ServerConnectedEvent.
    fn server_connected(&self, _player: &PlayerInfo, _server: &str, _previous: Option<&str>) {}
    fn kicked(&self, _event: &mut Kicked<'_>) {}
    /// CommandExecuteEvent, for every command a player types.
    fn command(&self, _player: &PlayerInfo, _line: &str) -> CommandResult {
        CommandResult::Allowed
    }
    /// After a command ran: `by_proxy` when the proxy ran it rather than the server.
    fn command_executed(&self, _player: &PlayerInfo, _line: &str, _by_proxy: bool) {}
    /// TabCompleteEvent: false hides every suggestion for this input.
    fn tab_complete(&self, _player: &PlayerInfo, _partial: &str) -> bool {
        true
    }
    fn ping(&self, _ping: &mut Ping) {}
    /// The permission provider (PermissionsSetupEvent).
    fn permission(&self, _player: &PlayerInfo, _permission: &str) -> Tristate {
        Tristate::Undefined
    }
}
