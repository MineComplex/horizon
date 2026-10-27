//! Plugins (API 0.4, docs/plugins.md): Bukkit-style server code in the same
//! sandbox as mods, run by the game that simulates the world.
//!
//! The runtime keeps what is the plugins' own: their listeners, commands,
//! tasks, permissions, data folders and the messages they send each other.
//! Everything that is the server's (players, worlds, entities, items,
//! inventories, scoreboards, boss bars) goes to a [`PluginHost`] the game
//! lends for each call, so a plugin reads the server as it is at that moment
//! and its changes happen where the game decides.
use super::{Hooks, Mods, State, Status};
use crate::manifest::{CommandSpec, Manifest, PermissionDefault};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use wasmtime::component::HasData;

pub(super) mod bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-plugin",
        additional_derives: [PartialEq],
        with: {
            "horizon:mods/log": super::super::bindings::horizon::mods::log,
            "horizon:mods/storage": super::super::bindings::horizon::mods::storage,
        },
    });
}

pub use bindings::HorizonPlugin;
pub use bindings::horizon::mods::{
    plugin_bossbars as bossbars, plugin_commands as commands, plugin_entities as entities,
    plugin_event_types as events, plugin_files as files, plugin_inventories as inventories,
    plugin_items as items, plugin_listeners as listeners, plugin_messaging as messaging,
    plugin_permissions as permissions, plugin_players as players, plugin_scheduler as scheduler,
    plugin_scoreboards as scoreboards, plugin_server as server, plugin_types as types,
    plugin_worlds as worlds,
};
pub use events::{Event, EventKind, Priority};
pub use server::{OfflinePlayer, PluginInfo, ServerInfo};
pub use types::{
    Block, BlockPos, EntityRef, GameMode, Hand, ItemStack, Location, Player, Sender, Vec3,
};

/// Fuel for a plugin's calls: enabling may load a lot, events and tasks
/// must stay quick, a command may do some work.
const FUEL_ENABLE: u64 = 10_000_000_000;
const FUEL_EVENT: u64 = 300_000_000;
const FUEL_COMMAND: u64 = 2_000_000_000;
const FUEL_TASK: u64 = 1_000_000_000;
/// A plugin's linear memory cap.
pub(super) const MEMORY: usize = 512 << 20;
/// Bytes `plugin-files` may write in one call.
const WRITE_BUDGET: u64 = 64 << 20;
/// Where plugins' data folders live, beside the server's mods folder.
pub const PLUGINS_DIR: &str = "plugins";
/// Bukkit's message for a sender without a command's permission.
pub const NO_PERMISSION: &str = "§cI'm sorry, but you do not have permission to perform this command. Please contact the server administrators if you believe that this is in error.";
/// The priorities in the order listeners run.
pub const PRIORITIES: [Priority; 6] = [
    Priority::Lowest,
    Priority::Low,
    Priority::Normal,
    Priority::High,
    Priority::Highest,
    Priority::Monitor,
];

/// The server, as the game lends it to a plugin's call. The game implements
/// the `plugin-*` host traits on its own context and these few calls that
/// the runtime's interfaces lean on.
pub trait PluginHost:
    players::Host
    + worlds::Host
    + entities::Host
    + items::Host
    + inventories::Host
    + scoreboards::Host
    + bossbars::Host
{
    /// The plugin whose code makes the calls that follow (its id).
    fn caller(&mut self, plugin: &str);
    fn server_info(&mut self) -> ServerInfo;
    /// Ticks per second over 1, 5 and 15 minutes.
    fn tps(&mut self) -> (f64, f64, f64);
    fn mspt(&mut self) -> f64;
    /// A line in the console's log, at info level.
    fn console(&mut self, message: &str);
    /// A command run as the console once the call returns.
    fn dispatch_command(&mut self, line: String);
    fn shutdown(&mut self);
    fn set_motd(&mut self, motd: String);
    fn set_max_players(&mut self, max: u32);
    fn find_player(&mut self, name_or_id: &str) -> Option<OfflinePlayer>;
    fn whitelist_enabled(&mut self) -> bool;
    fn set_whitelist_enabled(&mut self, on: bool);
    fn whitelist(&mut self) -> Vec<String>;
    fn set_whitelisted(&mut self, name: String, on: bool);
    fn operators(&mut self) -> Vec<String>;
    fn banned_players(&mut self) -> Vec<String>;
    fn ban(&mut self, name: String, reason: Option<String>);
    fn pardon(&mut self, name: String);
    fn ban_ip(&mut self, address: String, reason: Option<String>);
    fn pardon_ip(&mut self, address: String);
    /// A custom payload to a player's client; false when they aren't on.
    fn send_payload(&mut self, player: &str, channel: &str, data: Vec<u8>) -> bool;
}

/// The host as the linker hands it to the game's interfaces.
struct HostData;
impl HasData for HostData {
    type Data<'a> = &'a mut (dyn PluginHost + 'static);
}
fn host_of(state: &mut State) -> &mut (dyn PluginHost + 'static) {
    state.plugin_mut().host()
}

/// The lent host for the call under way. The game lends it for one call
/// ([`Mods::lend`]) and the pointer is cleared before the borrow ends.
#[derive(Clone, Copy)]
struct LentHost(*mut (dyn PluginHost + 'static));
// The host is only touched on the thread making the call.
unsafe impl Send for LentHost {}

/// A task of `plugin-scheduler`.
#[derive(Clone, Copy, Debug)]
struct Task {
    next: u64,
    period: Option<u64>,
}

/// A command a plugin owns.
#[derive(Clone, Debug)]
pub struct PluginCommand {
    /// The plugin's id.
    pub plugin: String,
    pub name: String,
    pub spec: CommandSpec,
}

/// What one plugin keeps in its store.
pub(super) struct PluginState {
    id: String,
    enabled: bool,
    host: Option<LentHost>,
    listeners: Vec<(EventKind, Priority, bool)>,
    listeners_changed: bool,
    commands: Vec<PluginCommand>,
    tasks: BTreeMap<u32, Task>,
    next_task: u32,
    channels: BTreeSet<String>,
    /// `send-to-plugin` calls made during the call: target, channel, data.
    calls: Vec<(String, String, Vec<u8>)>,
    data_dir: PathBuf,
    package: PathBuf,
    write_budget: u64,
    shared: Arc<Mutex<Shared>>,
}

impl PluginState {
    fn host(&mut self) -> &mut (dyn PluginHost + 'static) {
        let lent = self
            .host
            .expect("a plugin's host call outside a call the game lent a host for");
        // SAFETY: `Mods::lend` sets the pointer from a `&mut dyn PluginHost`
        // that outlives the call and clears it before returning; nothing
        // else holds the host meanwhile.
        unsafe { &mut *lent.0 }
    }
}

impl State {
    fn plugin_mut(&mut self) -> &mut PluginState {
        self.plugin
            .as_deref_mut()
            .expect("plugin interfaces are linked for plugins only")
    }
}

/// What every plugin's store shares: permissions and the plugin list.
#[derive(Default)]
pub(super) struct Shared {
    perms: Permissions,
    infos: Vec<PluginInfo>,
    tick: u64,
    /// Names and aliases taken, lower case, to the plugin owning them.
    command_names: HashMap<String, String>,
    /// Ids handed out by `run-later`, unique across plugins and reloads.
    next_task_base: u32,
}

/// The plugins' part of [`Mods`].
#[derive(Default)]
pub(super) struct Runtime {
    shared: Arc<Mutex<Shared>>,
    /// Entries in enable order.
    order: Vec<usize>,
    /// Bit `kind` is set when some running plugin listens to `kind`.
    listening: u64,
    /// `send-to-plugin` calls waiting for delivery: from, target, channel, data.
    calls: Vec<(String, String, String, Vec<u8>)>,
    enabled: bool,
    /// Plugins restarted (a dev plugin rebuilt) to enable at the next tick.
    pending: Vec<usize>,
}

// ---------------------------------------------------------------------------
// Permissions.

#[derive(Clone, Debug)]
struct Declared {
    default: PermissionDefault,
    children: Vec<(String, bool)>,
}

#[derive(Default)]
struct Permissions {
    declared: HashMap<String, Declared>,
    /// Per player: (plugin, node, value), in the order set.
    attachments: HashMap<String, Vec<(String, String, bool)>>,
}

impl Permissions {
    fn declare(&mut self, node: &str, default: PermissionDefault, children: Vec<(String, bool)>) {
        self.declared.insert(
            node.to_ascii_lowercase(),
            Declared {
                default,
                children: children
                    .into_iter()
                    .map(|(n, v)| (n.to_ascii_lowercase(), v))
                    .collect(),
            },
        );
    }
    /// Sets `node` and, through declared children, the nodes under it
    /// (Bukkit: a child's value is flipped when its parent is set false).
    fn expand(&self, node: &str, value: bool, out: &mut HashMap<String, bool>, depth: u32) {
        out.insert(node.to_owned(), value);
        if depth > 16 {
            return;
        }
        if let Some(d) = self.declared.get(node) {
            for (child, v) in &d.children {
                self.expand(child, if value { *v } else { !*v }, out, depth + 1);
            }
        }
    }
    /// Every node the player has set, defaults first, then attachments.
    fn effective(&self, player: &str, op: bool) -> HashMap<String, bool> {
        let mut out = HashMap::new();
        let mut defaults: Vec<(&String, &Declared)> = self.declared.iter().collect();
        defaults.sort_by(|a, b| a.0.cmp(b.0));
        for (node, d) in defaults {
            if d.default.grants(op) {
                self.expand(node, true, &mut out, 0);
            }
        }
        if let Some(list) = self.attachments.get(player) {
            for (_, node, value) in list {
                self.expand(node, *value, &mut out, 0);
            }
        }
        out
    }
    fn has(&self, player: &str, node: &str, op: bool) -> bool {
        let node = node.to_ascii_lowercase();
        let set = self.effective(player, op);
        if let Some(v) = set.get(&node) {
            return *v;
        }
        // The most specific wildcard: `a.b.*`, then `a.*`, then `*`.
        let mut prefix = node.as_str();
        while let Some(dot) = prefix.rfind('.') {
            prefix = &prefix[..dot];
            if let Some(v) = set.get(&format!("{prefix}.*")) {
                return *v;
            }
        }
        if let Some(v) = set.get("*") {
            return *v;
        }
        match self.declared.get(&node) {
            Some(d) => d.default.grants(op),
            // Bukkit's default for a node no one declared.
            None => op,
        }
    }
    fn is_set(&self, player: &str, node: &str) -> bool {
        let node = node.to_ascii_lowercase();
        self.attachments
            .get(player)
            .is_some_and(|l| l.iter().any(|(_, n, _)| *n == node))
    }
    fn set(&mut self, plugin: &str, player: &str, node: &str, value: Option<bool>) {
        let node = node.to_ascii_lowercase();
        let list = self.attachments.entry(player.to_owned()).or_default();
        list.retain(|(p, n, _)| !(p == plugin && *n == node));
        if let Some(v) = value {
            list.push((plugin.to_owned(), node, v));
        }
    }
    fn clear(&mut self, plugin: &str, player: Option<&str>) {
        for (who, list) in self.attachments.iter_mut() {
            if player.is_none_or(|p| p == who) {
                list.retain(|(p, _, _)| p != plugin);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Events.

/// The kind of an event.
pub fn kind_of(e: &Event) -> EventKind {
    match e {
        Event::PlayerJoin(_) => EventKind::PlayerJoin,
        Event::PlayerQuit(_) => EventKind::PlayerQuit,
        Event::PlayerLogin(_) => EventKind::PlayerLogin,
        Event::PlayerChat(_) => EventKind::PlayerChat,
        Event::PlayerCommand(_) => EventKind::PlayerCommand,
        Event::PlayerMove(_) => EventKind::PlayerMove,
        Event::PlayerInteract(_) => EventKind::PlayerInteract,
        Event::PlayerInteractEntity(_) => EventKind::PlayerInteractEntity,
        Event::PlayerToggleSneak(_) => EventKind::PlayerToggleSneak,
        Event::PlayerToggleSprint(_) => EventKind::PlayerToggleSprint,
        Event::PlayerToggleFlight(_) => EventKind::PlayerToggleFlight,
        Event::PlayerItemHeld(_) => EventKind::PlayerItemHeld,
        Event::PlayerDropItem(_) => EventKind::PlayerDropItem,
        Event::PlayerSwapHands(_) => EventKind::PlayerSwapHands,
        Event::PlayerRespawn(_) => EventKind::PlayerRespawn,
        Event::PlayerDeath(_) => EventKind::PlayerDeath,
        Event::PlayerGameModeChange(_) => EventKind::PlayerGameModeChange,
        Event::PlayerChangedWorld(_) => EventKind::PlayerChangedWorld,
        Event::SignChange(_) => EventKind::SignChange,
        Event::PluginMessage(_) => EventKind::PluginMessage,
        Event::PlayerResourcePack(_) => EventKind::PlayerResourcePack,
        Event::BlockBreak(_) => EventKind::BlockBreak,
        Event::BlockPlace(_) => EventKind::BlockPlace,
        Event::BlockPistonExtend(_) => EventKind::BlockPistonExtend,
        Event::BlockPistonRetract(_) => EventKind::BlockPistonRetract,
        Event::BlockFromTo(_) => EventKind::BlockFromTo,
        Event::BlockGrow(_) => EventKind::BlockGrow,
        Event::EntityDamage(_) => EventKind::EntityDamage,
        Event::EntityDeath(_) => EventKind::EntityDeath,
        Event::EntitySpawn(_) => EventKind::EntitySpawn,
        Event::EntityExplode(_) => EventKind::EntityExplode,
        Event::InventoryClick(_) => EventKind::InventoryClick,
        Event::InventoryClose(_) => EventKind::InventoryClose,
        Event::ServerCommand(_) => EventKind::ServerCommand,
        Event::PluginState(_) => EventKind::PluginState,
        Event::PluginCall(_) => EventKind::PluginCall,
    }
}

/// Whether a listener has cancelled the event (false for kinds that can't be).
pub fn is_cancelled(e: &Event) -> bool {
    match e {
        Event::PlayerChat(x) => x.cancelled,
        Event::PlayerCommand(x) => x.cancelled,
        Event::PlayerMove(x) => x.cancelled,
        Event::PlayerInteract(x) => x.cancelled,
        Event::PlayerInteractEntity(x) => x.cancelled,
        Event::PlayerToggleSneak(x) => x.cancelled,
        Event::PlayerToggleSprint(x) => x.cancelled,
        Event::PlayerToggleFlight(x) => x.cancelled,
        Event::PlayerItemHeld(x) => x.cancelled,
        Event::PlayerDropItem(x) => x.cancelled,
        Event::PlayerSwapHands(x) => x.cancelled,
        Event::PlayerGameModeChange(x) => x.cancelled,
        Event::SignChange(x) => x.cancelled,
        Event::BlockBreak(x) => x.cancelled,
        Event::BlockPlace(x) => x.cancelled,
        Event::BlockPistonExtend(x) | Event::BlockPistonRetract(x) => x.cancelled,
        Event::BlockFromTo(x) => x.cancelled,
        Event::BlockGrow(x) => x.cancelled,
        Event::EntityDamage(x) => x.cancelled,
        Event::EntitySpawn(x) => x.cancelled,
        Event::EntityExplode(x) => x.cancelled,
        Event::InventoryClick(x) => x.cancelled,
        Event::ServerCommand(x) => x.cancelled,
        Event::PlayerLogin(x) => !x.allowed,
        _ => false,
    }
}

fn bit(kind: EventKind) -> u64 {
    1 << (kind as u64)
}

// ---------------------------------------------------------------------------
// The game's side.

impl Mods {
    /// Some plugin runs: the game skips its plugin work otherwise.
    pub fn runs_plugins(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.hooks.plugin().is_some()))
    }
    /// The plugins have been enabled ([`Mods::enable_plugins`]).
    pub fn plugins_enabled(&self) -> bool {
        self.plugins.enabled
    }
    /// Some running plugin listens to `kind`: the game builds the event only then.
    pub fn listens(&self, kind: EventKind) -> bool {
        self.plugins.listening & bit(kind) != 0
    }

    /// Runs `f` on plugin `i` with `host` lent for the call.
    fn plugin_call<R>(
        &mut self,
        i: usize,
        host: &mut dyn PluginHost,
        fuel: u64,
        f: impl FnOnce(&HorizonPlugin, &mut wasmtime::Store<State>) -> wasmtime::Result<R>,
    ) -> Option<R> {
        // SAFETY: only the lifetime is erased; the pointer is cleared below,
        // before `host`'s borrow ends.
        let lent: *mut (dyn PluginHost + 'static) = unsafe {
            std::mem::transmute::<*mut dyn PluginHost, *mut (dyn PluginHost + 'static)>(
                host as *mut dyn PluginHost,
            )
        };
        let id = {
            let run = self.entries.get_mut(i)?.run.as_mut()?;
            let p = run.store.data_mut().plugin.as_deref_mut()?;
            p.host = Some(LentHost(lent));
            p.write_budget = WRITE_BUDGET;
            p.id.clone()
        };
        host.caller(&id);
        let result = self.call(i, fuel, |run| match &run.hooks {
            Hooks::Plugin(p) => f(p, &mut run.store),
            _ => Err(wasmtime::Error::msg("not a plugin")),
        });
        self.after_plugin_call(i, &id);
        result
    }

    /// Clears the lent host and collects what the call left for others.
    fn after_plugin_call(&mut self, i: usize, id: &str) {
        let mut changed = false;
        if let Some(run) = self.entries.get_mut(i).and_then(|e| e.run.as_mut())
            && let Some(p) = run.store.data_mut().plugin.as_deref_mut()
        {
            p.host = None;
            for (target, channel, data) in std::mem::take(&mut p.calls) {
                self.plugins
                    .calls
                    .push((id.to_owned(), target, channel, data));
            }
            changed = std::mem::take(&mut p.listeners_changed);
        }
        let stopped = self.entries.get(i).is_some_and(|e| e.run.is_none());
        if stopped {
            // A trap or an overrun: its tasks, attachments and commands go with it.
            if let Ok(mut s) = self.plugins.shared.lock() {
                s.perms.clear(id, None);
                s.command_names.retain(|_, owner| owner != id);
            }
            changed = true;
        }
        if changed {
            self.refresh_listening();
        }
        self.refresh_infos();
    }

    fn refresh_listening(&mut self) {
        let mut mask = 0;
        for e in &self.entries {
            if let Some(p) = e
                .run
                .as_ref()
                .and_then(|r| r.store.data().plugin.as_deref())
                && p.enabled
            {
                for (kind, _, _) in &p.listeners {
                    mask |= bit(*kind);
                }
            }
        }
        // Plugin calls reach their target without a listener.
        self.plugins.listening = mask;
    }

    fn refresh_infos(&mut self) {
        let infos: Vec<PluginInfo> = self
            .entries
            .iter()
            .filter_map(|e| {
                let m = e.manifest.as_ref().filter(|m| m.is_plugin())?;
                let enabled = e
                    .run
                    .as_ref()
                    .and_then(|r| r.store.data().plugin.as_deref())
                    .is_some_and(|p| p.enabled);
                Some(PluginInfo {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    version: m.version.clone(),
                    enabled,
                    authors: m.authors.clone(),
                    description: m.description.clone(),
                    depend: m.depend.clone(),
                })
            })
            .collect();
        if let Ok(mut s) = self.plugins.shared.lock() {
            s.infos = infos;
        }
    }

    fn plugin_index(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.run
                .as_ref()
                .and_then(|r| r.store.data().plugin.as_deref())
                .is_some_and(|p| p.id == id)
        })
    }

    /// Enables every plugin that is loaded, dependencies first: `depend`
    /// must be running, `softdepend` comes first when there, `loadbefore`
    /// after. A plugin whose dependency is missing or failed stays off.
    pub fn enable_plugins(&mut self, host: &mut dyn PluginHost) {
        if self.plugins.enabled {
            return;
        }
        self.plugins.enabled = true;
        let order = self.plugin_order();
        self.plugins.order.clear();
        for i in order {
            self.enable_plugin(i, host);
        }
        self.refresh_listening();
        self.refresh_infos();
    }

    /// The loaded plugins in enable order.
    fn plugin_order(&self) -> Vec<usize> {
        let plugins: Vec<(usize, &Manifest)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.run.as_ref().is_some_and(|r| r.hooks.plugin().is_some()))
            .filter_map(|(i, e)| Some((i, e.manifest.as_ref()?)))
            .collect();
        let index = |id: &str| plugins.iter().find(|(_, m)| m.id == id).map(|(i, _)| *i);
        // Edges: before -> after.
        let mut after: HashMap<usize, BTreeSet<usize>> = HashMap::new();
        for &(i, m) in &plugins {
            for dep in m.depend.iter().chain(&m.softdepend) {
                if let Some(d) = index(dep) {
                    after.entry(i).or_default().insert(d);
                }
            }
            for later in &m.loadbefore {
                if let Some(l) = index(later) {
                    after.entry(l).or_default().insert(i);
                }
            }
        }
        // Kahn's algorithm, keeping the load order among equals; a cycle
        // falls back to load order.
        let mut done: Vec<usize> = Vec::new();
        let mut left: Vec<usize> = plugins.iter().map(|(i, _)| *i).collect();
        while !left.is_empty() {
            let ready = left.iter().position(|i| {
                after
                    .get(i)
                    .is_none_or(|deps| deps.iter().all(|d| done.contains(d) || !left.contains(d)))
            });
            let k = ready.unwrap_or(0);
            done.push(left.remove(k));
        }
        done
    }

    fn enable_plugin(&mut self, i: usize, host: &mut dyn PluginHost) {
        let Some(m) = self.entries[i].manifest.clone() else {
            return;
        };
        let missing = m.depend.iter().find(|d| {
            self.plugin_index(d).is_none_or(|k| {
                !self.entries[k]
                    .run
                    .as_ref()
                    .and_then(|r| r.store.data().plugin.as_deref())
                    .is_some_and(|p| p.enabled)
            })
        });
        if let Some(dep) = missing {
            let reason = format!("it depends on {dep}, which isn't running");
            log::warn!("Horizon: plugin {} not enabled: {reason}", m.id);
            self.entries[i].status = Status::Failed(reason);
            self.entries[i].run = None;
            return;
        }
        // The manifest's permissions and commands.
        if let Ok(mut s) = self.plugins.shared.lock() {
            for (node, spec) in &m.permissions {
                let children = spec.children.iter().map(|(k, v)| (k.clone(), *v)).collect();
                s.perms.declare(node, spec.default, children);
            }
        }
        let commands: Vec<PluginCommand> = m
            .commands
            .iter()
            .map(|(name, spec)| PluginCommand {
                plugin: m.id.clone(),
                name: name.to_ascii_lowercase(),
                spec: spec.clone(),
            })
            .collect();
        let mut taken = Vec::new();
        if let Ok(mut s) = self.plugins.shared.lock() {
            for c in &commands {
                for label in std::iter::once(&c.name).chain(&c.spec.aliases) {
                    let label = label.to_ascii_lowercase();
                    match s.command_names.entry(label) {
                        std::collections::hash_map::Entry::Occupied(e) => {
                            taken.push(e.key().clone())
                        }
                        std::collections::hash_map::Entry::Vacant(e) => {
                            e.insert(m.id.clone());
                        }
                    }
                }
            }
        }
        for label in taken {
            log::warn!(
                "Horizon: plugin {}'s /{label} is another plugin's; use /{}:{label}",
                m.id,
                m.id
            );
        }
        if let Some(p) = self.entries[i]
            .run
            .as_mut()
            .and_then(|r| r.store.data_mut().plugin.as_deref_mut())
        {
            p.commands = commands;
            p.enabled = true;
        }
        log::info!("[{}] Enabling {} v{}", m.display_name(), m.name, m.version);
        let result = self.plugin_call(i, host, FUEL_ENABLE, |p, s| {
            p.horizon_mods_plugin().call_enable(s)
        });
        match result {
            Some(Ok(())) => {
                self.plugins.order.push(i);
                self.refresh_listening();
                self.refresh_infos();
                let event = Event::PluginState(events::PluginStateEvent {
                    plugin: m.id.clone(),
                    enabled: true,
                });
                self.fire(host, event);
            }
            Some(Err(reason)) => {
                log::warn!("Horizon: plugin {} did not enable: {reason}", m.id);
                self.forget_plugin(i, &m.id);
                self.entries[i].status = Status::Failed(reason);
                self.entries[i].run = None;
            }
            // Trapped: `call` turned it off already.
            None => self.forget_plugin(i, &m.id),
        }
    }

    /// Drops what plugin `id` registered server-wide.
    fn forget_plugin(&mut self, i: usize, id: &str) {
        if let Ok(mut s) = self.plugins.shared.lock() {
            s.perms.clear(id, None);
            s.command_names.retain(|_, owner| owner != id);
        }
        self.plugins.order.retain(|&k| k != i);
        if let Some(p) = self.entries[i]
            .run
            .as_mut()
            .and_then(|r| r.store.data_mut().plugin.as_deref_mut())
        {
            p.enabled = false;
            p.tasks.clear();
            p.listeners.clear();
            p.commands.clear();
            p.channels.clear();
        }
        self.refresh_listening();
        self.refresh_infos();
    }

    /// Disables every plugin, the last enabled first (the server stops or
    /// the world closes).
    pub fn disable_plugins(&mut self, host: &mut dyn PluginHost) {
        if !self.plugins.enabled {
            return;
        }
        let order: Vec<usize> = self.plugins.order.iter().rev().copied().collect();
        for i in order {
            let Some(id) = self.plugin_id(i) else {
                continue;
            };
            let name = self.entries[i]
                .manifest
                .as_ref()
                .map(|m| format!("{} v{}", m.name, m.version))
                .unwrap_or_default();
            log::info!("[{id}] Disabling {name}");
            self.plugin_call(i, host, FUEL_EVENT, |p, s| {
                p.horizon_mods_plugin().call_disable(s)
            });
            self.forget_plugin(i, &id);
            let event = Event::PluginState(events::PluginStateEvent {
                plugin: id,
                enabled: false,
            });
            self.fire(host, event);
        }
        self.plugins.enabled = false;
    }

    fn plugin_id(&self, i: usize) -> Option<String> {
        let p = self
            .entries
            .get(i)?
            .run
            .as_ref()?
            .store
            .data()
            .plugin
            .as_deref()?;
        Some(p.id.clone())
    }

    /// Runs `event` through every listener, lowest priority first, and
    /// returns it as they left it. A listener that traps is turned off and
    /// the event goes on as it was.
    pub fn fire(&mut self, host: &mut dyn PluginHost, mut event: Event) -> Event {
        let kind = kind_of(&event);
        if !self.listens(kind) {
            return event;
        }
        let order = self.plugins.order.clone();
        for priority in PRIORITIES {
            for &i in &order {
                let wants = self.entries[i]
                    .run
                    .as_ref()
                    .and_then(|r| r.store.data().plugin.as_deref())
                    .and_then(|p| {
                        p.listeners
                            .iter()
                            .find(|(k, pr, _)| *k == kind && *pr == priority)
                            .map(|(_, _, ignore)| *ignore)
                    });
                let Some(ignore_cancelled) = wants else {
                    continue;
                };
                if ignore_cancelled && is_cancelled(&event) {
                    continue;
                }
                let sent = event.clone();
                let back = self.plugin_call(i, host, FUEL_EVENT, |p, s| {
                    p.horizon_mods_plugin().call_on_event(s, priority, &sent)
                });
                if let Some(back) = back
                    && priority != Priority::Monitor
                    && kind_of(&back) == kind
                {
                    event = back;
                }
            }
        }
        event
    }

    /// The commands plugins own, for the command tree and `/help`.
    pub fn plugin_commands(&self) -> Vec<PluginCommand> {
        self.plugins
            .order
            .iter()
            .filter_map(|&i| self.entries[i].run.as_ref()?.store.data().plugin.as_deref())
            .flat_map(|p| p.commands.iter().cloned())
            .collect()
    }

    /// The plugin and command a label (`spawn`, `essentials:spawn`) names.
    fn command_for(&self, label: &str) -> Option<(usize, PluginCommand)> {
        let label = label.to_ascii_lowercase();
        let (only, label) = match label.split_once(':') {
            Some((plugin, rest)) => (Some(plugin.to_owned()), rest.to_owned()),
            None => (None, label),
        };
        let owner = match &only {
            Some(p) => Some(p.clone()),
            None => self
                .plugins
                .shared
                .lock()
                .ok()
                .and_then(|s| s.command_names.get(&label).cloned()),
        };
        for &i in &self.plugins.order {
            let Some(p) = self.entries[i]
                .run
                .as_ref()
                .and_then(|r| r.store.data().plugin.as_deref())
            else {
                continue;
            };
            if owner.as_ref().is_some_and(|o| *o != p.id) {
                continue;
            }
            let found = p.commands.iter().find(|c| {
                c.name == label
                    || c.spec
                        .aliases
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(&label))
            });
            if let Some(c) = found {
                return Some((i, c.clone()));
            }
        }
        None
    }

    /// Whether a plugin owns the command `line` (without its slash) starts with.
    pub fn is_plugin_command(&self, line: &str) -> bool {
        let label = line.split(' ').next().unwrap_or_default();
        self.command_for(label).is_some()
    }

    /// Whether the player with this UUID has `node`.
    pub fn has_permission(&self, host: &mut dyn PluginHost, player: &str, node: &str) -> bool {
        let op = players::Host::get(host, player.to_owned()).is_some_and(|p| p.op);
        self.plugins
            .shared
            .lock()
            .is_ok_and(|s| s.perms.has(player, node, op))
    }

    /// Runs `line` (without its slash) if a plugin owns its command: the
    /// permission check, the plugin's handler, and its usage when the
    /// handler says so. False when no plugin owns it.
    pub fn plugin_command(
        &mut self,
        host: &mut dyn PluginHost,
        sender: &Sender,
        line: &str,
    ) -> bool {
        let mut words = line.split(' ');
        let label = words.next().unwrap_or_default().to_owned();
        let args: Vec<String> = words.map(str::to_owned).collect();
        let Some((i, command)) = self.command_for(&label) else {
            return false;
        };
        let reply = |host: &mut dyn PluginHost, text: &str| match sender {
            Sender::Console => host.console(text),
            Sender::Player(p) => players::Host::send_message(host, p.id.clone(), text.to_owned()),
        };
        if let (Some(node), Sender::Player(p)) = (&command.spec.permission, sender)
            && !self.has_permission(host, &p.id, node)
        {
            let message = command
                .spec
                .permission_message
                .clone()
                .unwrap_or_else(|| NO_PERMISSION.to_owned());
            if !message.is_empty() {
                reply(host, &message);
            }
            return true;
        }
        let label = label
            .split_once(':')
            .map_or(label.clone(), |(_, l)| l.to_owned());
        let name = command.name.clone();
        let result = self.plugin_call(i, host, FUEL_COMMAND, |p, s| {
            p.horizon_mods_plugin()
                .call_on_command(s, sender, &name, &label, &args)
        });
        match result {
            Some(true) => {}
            Some(false) => {
                for line in command.spec.usage.replace("<command>", &label).lines() {
                    reply(host, line);
                }
            }
            None => reply(
                host,
                "§cAn internal error occurred while attempting to perform this command",
            ),
        }
        true
    }

    /// Completions for `line` (without its slash, the last word being typed)
    /// when a plugin owns its command: the plugin's, or else the names of the
    /// players online that start with the last word. None when no plugin
    /// owns it.
    pub fn plugin_tab_complete(
        &mut self,
        host: &mut dyn PluginHost,
        sender: &Sender,
        line: &str,
    ) -> Option<Vec<String>> {
        let mut words = line.split(' ');
        let label = words.next().unwrap_or_default().to_owned();
        let args: Vec<String> = words.map(str::to_owned).collect();
        if args.is_empty() {
            return None;
        }
        let (i, command) = self.command_for(&label)?;
        if let (Some(node), Sender::Player(p)) = (&command.spec.permission, sender)
            && !self.has_permission(host, &p.id, node)
        {
            return Some(Vec::new());
        }
        let name = command.name.clone();
        let offered = self
            .plugin_call(i, host, FUEL_EVENT, |p, s| {
                p.horizon_mods_plugin()
                    .call_on_tab_complete(s, sender, &name, &args)
            })
            .unwrap_or_default();
        if !offered.is_empty() {
            return Some(offered);
        }
        let last = args
            .last()
            .map(|a| a.to_ascii_lowercase())
            .unwrap_or_default();
        let names = players::Host::online(host)
            .into_iter()
            .map(|p| p.name)
            .filter(|n| n.to_ascii_lowercase().starts_with(&last))
            .collect();
        Some(names)
    }

    /// The end of a server tick: due tasks run, then plugins' calls to
    /// each other are delivered.
    pub fn plugin_tick(&mut self, host: &mut dyn PluginHost, tick: u64) {
        if let Ok(mut s) = self.plugins.shared.lock() {
            s.tick = tick;
        }
        for i in std::mem::take(&mut self.plugins.pending) {
            if self.plugins.enabled && self.entries.get(i).is_some_and(|e| e.run.is_some()) {
                self.enable_plugin(i, host);
            }
        }
        let order = self.plugins.order.clone();
        for i in order {
            let due: Vec<u32> = match self.entries[i]
                .run
                .as_mut()
                .and_then(|r| r.store.data_mut().plugin.as_deref_mut())
            {
                Some(p) => {
                    let due: Vec<u32> = p
                        .tasks
                        .iter()
                        .filter(|(_, t)| t.next <= tick)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in &due {
                        let t = p.tasks.get(id).copied();
                        match t.and_then(|t| t.period) {
                            Some(period) => {
                                if let Some(t) = p.tasks.get_mut(id) {
                                    t.next = tick + period.max(1);
                                }
                            }
                            None => {
                                p.tasks.remove(id);
                            }
                        }
                    }
                    due
                }
                None => continue,
            };
            for id in due {
                if self.entries[i].run.is_none() {
                    break;
                }
                self.plugin_call(i, host, FUEL_TASK, |p, s| {
                    p.horizon_mods_plugin().call_on_task(s, id)
                });
            }
        }
        self.deliver_plugin_calls(host);
    }

    /// Delivers the `send-to-plugin` calls made so far, as `plugin-call`
    /// events to their targets (those calls' own calls wait for the next round).
    pub fn deliver_plugin_calls(&mut self, host: &mut dyn PluginHost) {
        for _ in 0..4 {
            let calls = std::mem::take(&mut self.plugins.calls);
            if calls.is_empty() {
                return;
            }
            for (from, target, channel, data) in calls {
                let Some(i) = self.plugin_index(&target) else {
                    continue;
                };
                let event = Event::PluginCall(events::PluginCallEvent {
                    from,
                    channel,
                    data,
                });
                self.plugin_call(i, host, FUEL_EVENT, |p, s| {
                    p.horizon_mods_plugin()
                        .call_on_event(s, Priority::Normal, &event)
                });
            }
        }
    }

    /// The channels some plugin hears.
    pub fn plugin_channels(&self) -> BTreeSet<String> {
        self.plugins
            .order
            .iter()
            .filter_map(|&i| self.entries[i].run.as_ref()?.store.data().plugin.as_deref())
            .flat_map(|p| p.channels.iter().cloned())
            .collect()
    }

    /// A payload a player's client sent: to each plugin that registered the
    /// channel, as a `plugin-message` event.
    pub fn plugin_payload(
        &mut self,
        host: &mut dyn PluginHost,
        player: Player,
        channel: &str,
        data: Vec<u8>,
    ) -> bool {
        let order = self.plugins.order.clone();
        let mut heard = false;
        for i in order {
            let hears = self.entries[i]
                .run
                .as_ref()
                .and_then(|r| r.store.data().plugin.as_deref())
                .is_some_and(|p| p.channels.contains(channel));
            if !hears {
                continue;
            }
            heard = true;
            let event = Event::PluginMessage(events::PluginMessageEvent {
                player: player.clone(),
                channel: channel.to_owned(),
                data: data.clone(),
            });
            self.plugin_call(i, host, FUEL_EVENT, |p, s| {
                p.horizon_mods_plugin()
                    .call_on_event(s, Priority::Normal, &event)
            });
        }
        heard
    }

    /// The plugins, for `/plugins`.
    pub fn plugin_infos(&self) -> Vec<PluginInfo> {
        self.plugins
            .shared
            .lock()
            .map(|s| s.infos.clone())
            .unwrap_or_default()
    }

    /// A plugin entry was started again (its file changed): its old
    /// registrations go, and it is enabled at the next tick.
    pub(super) fn plugin_restarted(&mut self, i: usize, id: &str) {
        if let Ok(mut s) = self.plugins.shared.lock() {
            s.perms.clear(id, None);
            s.command_names.retain(|_, owner| owner != id);
        }
        self.plugins.order.retain(|&k| k != i);
        self.plugins.pending.push(i);
        self.refresh_listening();
    }

    /// A player left: what plugins set for them goes.
    pub fn plugin_player_left(&mut self, player: &str) {
        if let Ok(mut s) = self.plugins.shared.lock() {
            s.perms.attachments.remove(player);
        }
    }
}

// ---------------------------------------------------------------------------
// Starting a plugin (called from `Mods::start`).

/// The parts of a plugin's store [`Mods::start`] makes.
pub(super) fn new_state(mods: &Mods, path: &Path, m: &Manifest) -> Box<PluginState> {
    let root = path
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let base = mods
        .plugins
        .shared
        .lock()
        .map(|mut s| {
            s.next_task_base = s.next_task_base.wrapping_add(1 << 20);
            s.next_task_base
        })
        .unwrap_or(1 << 20);
    Box::new(PluginState {
        id: m.id.clone(),
        enabled: false,
        host: None,
        listeners: Vec::new(),
        listeners_changed: false,
        commands: Vec::new(),
        tasks: BTreeMap::new(),
        next_task: base,
        channels: BTreeSet::new(),
        calls: Vec::new(),
        data_dir: root.join(PLUGINS_DIR).join(&m.id),
        package: path.to_path_buf(),
        write_budget: WRITE_BUDGET,
        shared: mods.plugins.shared.clone(),
    })
}

/// Instantiates a plugin's exports (its `enable` waits for
/// [`Mods::enable_plugins`]).
pub(super) fn instantiate(
    store: &mut wasmtime::Store<State>,
    instance: &wasmtime::component::Instance,
) -> wasmtime::Result<Hooks> {
    let exports = HorizonPlugin::new(store, instance)?;
    Ok(Hooks::Plugin(Box::new(exports)))
}

/// Adds the plugin interfaces to the linker every mod shares.
pub(super) fn add_to_linker(
    linker: &mut wasmtime::component::Linker<State>,
) -> wasmtime::Result<()> {
    use wasmtime::component::HasSelf;
    types::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    events::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    server::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    messaging::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    permissions::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    scheduler::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    files::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    listeners::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    commands::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    players::add_to_linker::<State, HostData>(linker, host_of)?;
    worlds::add_to_linker::<State, HostData>(linker, host_of)?;
    entities::add_to_linker::<State, HostData>(linker, host_of)?;
    items::add_to_linker::<State, HostData>(linker, host_of)?;
    inventories::add_to_linker::<State, HostData>(linker, host_of)?;
    scoreboards::add_to_linker::<State, HostData>(linker, host_of)?;
    bossbars::add_to_linker::<State, HostData>(linker, host_of)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The interfaces the runtime answers itself.

impl types::Host for State {}
impl events::Host for State {}

impl server::Host for State {
    fn info(&mut self) -> ServerInfo {
        self.plugin_mut().host().server_info()
    }
    fn tps(&mut self) -> (f64, f64, f64) {
        self.plugin_mut().host().tps()
    }
    fn mspt(&mut self) -> f64 {
        self.plugin_mut().host().mspt()
    }
    fn now_millis(&mut self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    }
    fn random(&mut self) -> u64 {
        random_u64()
    }
    fn broadcast(&mut self, message: String, permission: Option<String>) -> u32 {
        let p = self.plugin_mut();
        let shared = p.shared.clone();
        let host = p.host();
        let mut sent = 0;
        for player in players::Host::online(host) {
            let allowed = permission.as_ref().is_none_or(|node| {
                shared
                    .lock()
                    .is_ok_and(|s| s.perms.has(&player.id, node, player.op))
            });
            if allowed {
                players::Host::send_message(host, player.id, message.clone());
                sent += 1;
            }
        }
        host.console(&strip_codes(&message));
        sent
    }
    fn console(&mut self, message: String) {
        self.plugin_mut().host().console(&message);
    }
    fn dispatch_command(&mut self, line: String) {
        let line = line.strip_prefix('/').map(str::to_owned).unwrap_or(line);
        self.plugin_mut().host().dispatch_command(line);
    }
    fn shutdown(&mut self) {
        self.plugin_mut().host().shutdown();
    }
    fn set_motd(&mut self, motd: String) {
        self.plugin_mut().host().set_motd(motd);
    }
    fn set_max_players(&mut self, max: u32) {
        self.plugin_mut().host().set_max_players(max);
    }
    fn plugins(&mut self) -> Vec<PluginInfo> {
        self.plugin_mut()
            .shared
            .lock()
            .map(|s| s.infos.clone())
            .unwrap_or_default()
    }
    fn plugin_enabled(&mut self, id: String) -> bool {
        self.plugin_mut()
            .shared
            .lock()
            .is_ok_and(|s| s.infos.iter().any(|p| p.id == id && p.enabled))
    }
    fn send_to_plugin(&mut self, target: String, channel: String, data: Vec<u8>) -> bool {
        let p = self.plugin_mut();
        let running = p
            .shared
            .lock()
            .is_ok_and(|s| s.infos.iter().any(|i| i.id == target && i.enabled));
        if running {
            p.calls.push((target, channel, data));
        }
        running
    }
    fn find_player(&mut self, name_or_id: String) -> Option<OfflinePlayer> {
        self.plugin_mut().host().find_player(&name_or_id)
    }
    fn whitelist_enabled(&mut self) -> bool {
        self.plugin_mut().host().whitelist_enabled()
    }
    fn set_whitelist_enabled(&mut self, on: bool) {
        self.plugin_mut().host().set_whitelist_enabled(on);
    }
    fn whitelist(&mut self) -> Vec<String> {
        self.plugin_mut().host().whitelist()
    }
    fn set_whitelisted(&mut self, name: String, on: bool) {
        self.plugin_mut().host().set_whitelisted(name, on);
    }
    fn operators(&mut self) -> Vec<String> {
        self.plugin_mut().host().operators()
    }
    fn banned_players(&mut self) -> Vec<String> {
        self.plugin_mut().host().banned_players()
    }
    fn ban(&mut self, name: String, reason: Option<String>) {
        self.plugin_mut().host().ban(name, reason);
    }
    fn pardon(&mut self, name: String) {
        self.plugin_mut().host().pardon(name);
    }
    fn ban_ip(&mut self, address: String, reason: Option<String>) {
        self.plugin_mut().host().ban_ip(address, reason);
    }
    fn pardon_ip(&mut self, address: String) {
        self.plugin_mut().host().pardon_ip(address);
    }
}

impl messaging::Host for State {
    fn register_incoming(&mut self, channel: String) {
        if channel_ok(&channel) {
            self.plugin_mut().channels.insert(channel);
        }
    }
    fn unregister_incoming(&mut self, channel: String) {
        self.plugin_mut().channels.remove(&channel);
    }
    fn send(&mut self, player: String, channel: String, data: Vec<u8>) -> bool {
        channel_ok(&channel)
            && self
                .plugin_mut()
                .host()
                .send_payload(&player, &channel, data)
    }
}

/// A channel name: `namespace:path`, as a resource location.
fn channel_ok(channel: &str) -> bool {
    channel.len() <= 128
        && channel.split_once(':').is_some_and(|(ns, path)| {
            !ns.is_empty()
                && !path.is_empty()
                && ns
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
                && path
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-/".contains(&b))
        })
}

impl permissions::Host for State {
    fn has(&mut self, player: String, node: String) -> bool {
        let p = self.plugin_mut();
        let shared = p.shared.clone();
        let op = players::Host::get(p.host(), player.clone()).is_some_and(|p| p.op);
        shared.lock().is_ok_and(|s| s.perms.has(&player, &node, op))
    }
    fn is_set(&mut self, player: String, node: String) -> bool {
        self.plugin_mut()
            .shared
            .lock()
            .is_ok_and(|s| s.perms.is_set(&player, &node))
    }
    fn set(&mut self, player: String, node: String, value: bool) {
        let p = self.plugin_mut();
        let id = p.id.clone();
        if let Ok(mut s) = p.shared.lock() {
            s.perms.set(&id, &player, &node, Some(value));
        }
    }
    fn unset(&mut self, player: String, node: String) {
        let p = self.plugin_mut();
        let id = p.id.clone();
        if let Ok(mut s) = p.shared.lock() {
            s.perms.set(&id, &player, &node, None);
        }
    }
    fn clear(&mut self, player: String) {
        let p = self.plugin_mut();
        let id = p.id.clone();
        if let Ok(mut s) = p.shared.lock() {
            s.perms.clear(&id, Some(&player));
        }
    }
    fn effective(&mut self, player: String) -> Vec<(String, bool)> {
        let p = self.plugin_mut();
        let shared = p.shared.clone();
        let op = players::Host::get(p.host(), player.clone()).is_some_and(|p| p.op);
        let mut set: Vec<(String, bool)> = shared
            .lock()
            .map(|s| s.perms.effective(&player, op).into_iter().collect())
            .unwrap_or_default();
        set.sort();
        set
    }
    fn declare(
        &mut self,
        node: String,
        default: permissions::PermissionDefault,
        _description: String,
        children: Vec<(String, bool)>,
    ) {
        let default = match default {
            permissions::PermissionDefault::Always => PermissionDefault::True,
            permissions::PermissionDefault::Never => PermissionDefault::False,
            permissions::PermissionDefault::Op => PermissionDefault::Op,
            permissions::PermissionDefault::NotOp => PermissionDefault::NotOp,
        };
        if let Ok(mut s) = self.plugin_mut().shared.lock() {
            s.perms.declare(&node, default, children);
        }
    }
}

impl scheduler::Host for State {
    fn run_later(&mut self, delay: u64, period: Option<u64>) -> u32 {
        let p = self.plugin_mut();
        let tick = p.shared.lock().map_or(0, |s| s.tick);
        p.next_task = p.next_task.wrapping_add(1).max(1);
        let id = p.next_task;
        p.tasks.insert(
            id,
            Task {
                next: tick + delay,
                period: period.map(|p| p.max(1)),
            },
        );
        id
    }
    fn cancel(&mut self, task: u32) -> bool {
        self.plugin_mut().tasks.remove(&task).is_some()
    }
    fn cancel_all(&mut self) {
        self.plugin_mut().tasks.clear();
    }
    fn is_queued(&mut self, task: u32) -> bool {
        self.plugin_mut().tasks.contains_key(&task)
    }
    fn current_tick(&mut self) -> u64 {
        self.plugin_mut().shared.lock().map_or(0, |s| s.tick)
    }
}

impl listeners::Host for State {
    fn listen(&mut self, kind: EventKind, priority: Priority, ignore_cancelled: bool) {
        let p = self.plugin_mut();
        p.listeners
            .retain(|(k, pr, _)| !(*k == kind && *pr == priority));
        p.listeners.push((kind, priority, ignore_cancelled));
        p.listeners_changed = true;
    }
    fn unlisten(&mut self, kind: EventKind) {
        let p = self.plugin_mut();
        p.listeners.retain(|(k, _, _)| *k != kind);
        p.listeners_changed = true;
    }
}

impl commands::Host for State {
    fn register(&mut self, spec: commands::CommandSpec) -> Result<(), String> {
        let p = self.plugin_mut();
        let name = spec.name.to_ascii_lowercase();
        let labels: Vec<String> = std::iter::once(name.clone())
            .chain(spec.aliases.iter().map(|a| a.to_ascii_lowercase()))
            .collect();
        if let Some(bad) = labels.iter().find(|l| !crate::manifest::command_name_ok(l)) {
            return Err(format!("{bad:?} is not a command name"));
        }
        let mut s = p.shared.lock().map_err(|_| "the server is busy")?;
        if let Some(taken) = labels
            .iter()
            .find(|l| s.command_names.get(*l).is_some_and(|owner| *owner != p.id))
        {
            return Err(format!("/{taken} is another plugin's"));
        }
        for l in &labels {
            s.command_names.insert(l.clone(), p.id.clone());
        }
        drop(s);
        p.commands.retain(|c| c.name != name);
        p.commands.push(PluginCommand {
            plugin: p.id.clone(),
            name,
            spec: CommandSpec {
                description: spec.description,
                usage: spec.usage,
                aliases: spec.aliases,
                permission: spec.permission,
                permission_message: spec.permission_message,
            },
        });
        Ok(())
    }
    fn unregister(&mut self, name: String) {
        let p = self.plugin_mut();
        let name = name.to_ascii_lowercase();
        let id = p.id.clone();
        if let Some(c) = p.commands.iter().find(|c| c.name == name) {
            let labels: Vec<String> = std::iter::once(c.name.clone())
                .chain(c.spec.aliases.iter().map(|a| a.to_ascii_lowercase()))
                .collect();
            if let Ok(mut s) = p.shared.lock() {
                for l in labels {
                    if s.command_names.get(&l) == Some(&id) {
                        s.command_names.remove(&l);
                    }
                }
            }
        }
        p.commands.retain(|c| c.name != name);
    }
}

/// A relative path in a data folder: `/`-separated `[A-Za-z0-9_.-]`
/// segments, none `.` or `..`.
fn path_ok(path: &str) -> bool {
    path.len() <= 512
        && path.split('/').count() <= 16
        && path.split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
}

impl PluginState {
    fn file(&self, path: &str) -> Option<PathBuf> {
        path_ok(path).then(|| self.data_dir.join(path))
    }
    fn spend(&mut self, bytes: usize) -> Result<(), String> {
        let bytes = bytes as u64;
        if bytes > self.write_budget {
            return Err("too much written in one call".into());
        }
        self.write_budget -= bytes;
        Ok(())
    }
    /// A file of the package's `defaults/` folder.
    fn bundled(&self, path: &str) -> Option<Vec<u8>> {
        if !path_ok(path) {
            return None;
        }
        if self.package.is_dir() {
            return std::fs::read(self.package.join("defaults").join(path)).ok();
        }
        let file = std::fs::File::open(&self.package).ok()?;
        let mut zip = zip::ZipArchive::new(file).ok()?;
        let entry = zip.by_name(&format!("defaults/{path}")).ok()?;
        let mut out = Vec::new();
        entry.take(64 << 20).read_to_end(&mut out).ok()?;
        Some(out)
    }
}

impl files::Host for State {
    fn read(&mut self, path: String) -> Option<Vec<u8>> {
        std::fs::read(self.plugin_mut().file(&path)?).ok()
    }
    fn write(&mut self, path: String, data: Vec<u8>) -> Result<(), String> {
        let p = self.plugin_mut();
        let file = p
            .file(&path)
            .ok_or_else(|| format!("{path:?} is not a data path"))?;
        p.spend(data.len())?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Written beside, then moved: a crash never leaves half a file.
        let tmp = file.with_extension("horizon-tmp");
        std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &file).map_err(|e| e.to_string())
    }
    fn append(&mut self, path: String, data: Vec<u8>) -> Result<(), String> {
        let p = self.plugin_mut();
        let file = p
            .file(&path)
            .ok_or_else(|| format!("{path:?} is not a data path"))?;
        p.spend(data.len())?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .map_err(|e| e.to_string())?;
        f.write_all(&data).map_err(|e| e.to_string())
    }
    fn delete(&mut self, path: String) -> bool {
        let Some(file) = self.plugin_mut().file(&path) else {
            return false;
        };
        std::fs::remove_file(&file).is_ok() || std::fs::remove_dir(&file).is_ok()
    }
    fn exists(&mut self, path: String) -> bool {
        self.plugin_mut().file(&path).is_some_and(|f| f.exists())
    }
    fn list(&mut self, dir: String) -> Vec<String> {
        let p = self.plugin_mut();
        let folder = if dir.is_empty() {
            Some(p.data_dir.clone())
        } else {
            p.file(dir.trim_end_matches('/'))
        };
        let Some(folder) = folder else {
            return Vec::new();
        };
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.ends_with(".horizon-tmp"))
            .collect();
        names.sort();
        names
    }
    fn bundled(&mut self, path: String) -> Option<Vec<u8>> {
        self.plugin_mut().bundled(&path)
    }
    fn save_default(&mut self, path: String) -> bool {
        let p = self.plugin_mut();
        let Some(file) = p.file(&path) else {
            return false;
        };
        if file.exists() {
            return false;
        }
        let Some(bytes) = p.bundled(&path) else {
            return false;
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&file, bytes).is_ok()
    }
}

/// Text without its `§` codes, for the console.
pub fn strip_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// 64 random bits: the OS's hasher keys, mixed with the time and a counter.
fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_follow_defaults_children_and_wildcards() {
        let mut p = Permissions::default();
        p.declare("ess.spawn", PermissionDefault::True, vec![]);
        p.declare(
            "ess.admin",
            PermissionDefault::Op,
            vec![("ess.kick".into(), true), ("ess.spawn.others".into(), true)],
        );
        assert!(p.has("a", "ess.spawn", false));
        assert!(!p.has("a", "ess.kick", false));
        assert!(
            p.has("a", "ess.kick", true),
            "an op gets ess.admin's children"
        );
        assert!(!p.has("a", "undeclared.node", false));
        assert!(p.has("a", "undeclared.node", true));
        p.set("perms", "a", "ess.*", Some(true));
        assert!(p.has("a", "ess.anything", false));
        p.set("perms", "a", "ess.spawn", Some(false));
        assert!(
            !p.has("a", "ess.spawn", false),
            "an exact node beats a wildcard"
        );
        p.set("perms", "a", "ess.admin", Some(false));
        assert!(
            !p.has("a", "ess.kick", true),
            "a parent set false flips its children"
        );
        assert!(p.is_set("a", "ess.admin"));
        p.clear("perms", Some("a"));
        assert!(!p.is_set("a", "ess.admin"));
        p.set("perms", "b", "*", Some(true));
        assert!(p.has("b", "x.y.z", false));
    }

    #[test]
    fn data_paths_stay_in_the_folder() {
        assert!(path_ok("config.yml"));
        assert!(path_ok("players/abc-def.json"));
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "a/./b",
            "a\\b",
            "C:x",
        ] {
            assert!(!path_ok(bad), "{bad}");
        }
    }

    #[test]
    fn channels_are_resource_locations() {
        assert!(channel_ok("bungeecord:main"));
        assert!(channel_ok("my_plugin:sync/data"));
        assert!(!channel_ok("main"));
        assert!(!channel_ok("Bungee:Main"));
    }

    #[test]
    fn codes_are_stripped_for_the_console() {
        assert_eq!(strip_codes("§aHello §lworld"), "Hello world");
    }
}
