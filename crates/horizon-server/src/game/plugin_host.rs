//! horizon-server's plugins (crate `horizon-server-api`, Bukkit-shaped) in
//! the game: the dedicated server loads and enables them, fires their events
//! where vanilla's server would (`PlayerList.placeNewPlayer`, chat, commands,
//! `ServerPlayerGameMode.destroyBlock`, `BlockItem.place`, `ServerPlayer.die`),
//! runs their commands and tasks, and applies what they asked for once their
//! handler returns (`apply_plugin_actions`), through the server's own commands
//! so every change reaches the players as any other does.
use super::{lan::LanHost};
use crate::hc::{
    lan::{guest::Guest, hub::HostEvent},
    text::Text,
};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_server_api::{
    self as api, Action, BlockPos, CommandSender, Location, PlayerRef, Plugin, PluginHost,
    event::{self as ev, Event},
};
use std::sync::Mutex;

/// The plugins `horizon-server` was built with, until the server starts.
static PLUGINS: Mutex<Option<Vec<Box<dyn Plugin>>>> = Mutex::new(None);

/// The plugins the next dedicated server runs (`horizon-server`'s, before `run_with`).
pub fn set_plugins(plugins: Vec<Box<dyn Plugin>>) {
    *PLUGINS.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugins);
}

/// What plugins read of the server.
struct View<'a> {
    game: &'a crate::game::Sim<'a>,
    players: Vec<PlayerRef>,
}
impl api::server::View for View<'_> {
    fn players(&self) -> Vec<PlayerRef> {
        self.players.clone()
    }
    fn block(&self, world: &str, pos: BlockPos) -> Option<String> {
        let g = self.game;
        if world != g.dim.id() {
            return None;
        }
        if !g.world.chunks.contains_key(&(pos.x >> 4, pos.z >> 4)) {
            return None;
        }
        let state = g.world.block(pos.x, pos.y, pos.z);
        Some(g.state_string(state))
    }
    fn tick(&self) -> u64 {
        self.game.ticks
    }
    fn world_name(&self) -> String {
        self.game.level.name.clone()
    }
    fn max_players(&self) -> usize {
        self.game
            .lan
            .as_ref()
            .map_or(0, |h| h.hub.lan.shared.max_players())
    }
    fn time(&self) -> i64 {
        self.game.world.day_time.rem_euclid(24000)
    }
}

impl crate::game::Sim<'_> {
    /// `onLoad` of the plugins `horizon-server` was built with (before the worlds).
    pub(super) fn plugins_load(&mut self) {
        let Some(plugins) = PLUGINS.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        if plugins.is_empty() {
            return;
        }
        let mut host = PluginHost::new(plugins);
        host.load();
        self.plugins = Some(Box::new(host));
        self.plugin_log();
    }
    /// `onEnable`, once the worlds are loaded and the server listens.
    pub(super) fn plugins_enable(&mut self) {
        let Some(mut host) = self.plugins.take() else {
            return;
        };
        host.enable();
        if let Some(lan) = self.lan.as_mut() {
            // The plugins decide the join and quit messages and see chat first.
            lan.hub.quiet_joins = host.listens::<ev::PlayerJoinEvent>();
            lan.hub.quiet_leaves = host.listens::<ev::PlayerQuitEvent>();
            lan.hub.intercept_chat = host.listens::<ev::PlayerChatEvent>();
        }
        self.plugins = Some(host);
        self.plugin_log();
    }
    /// `onDisable` as the server stops, with what they still asked for.
    pub(super) fn plugins_disable(&mut self) {
        if let Some(host) = self.plugins.as_mut() {
            host.disable();
        }
        self.plugin_log();
    }
    fn plugin_log(&mut self) {
        let Some(host) = self.plugins.as_mut() else {
            return;
        };
        for line in host.take_log() {
            super::dedicated::log("INFO", &line);
        }
    }
    /// The online players, from the hub in hand or in place, else the last
    /// ones seen (the hub is out of its place during a player's turn).
    fn plugin_players(&mut self, host: Option<&LanHost>) -> Vec<PlayerRef> {
        if let Some(h) = host.or(self.lan.as_ref()) {
            let players: Vec<PlayerRef> = h
                .hub
                .guests
                .iter()
                .filter(|g| g.placed)
                .map(crate::game::Sim::plugin_player)
                .collect();
            self.plugin_seen = players.clone();
            return players;
        }
        self.plugin_seen.clone()
    }
    /// The player whose turn it is (`as_guest`).
    fn plugin_acting(&self) -> Option<PlayerRef> {
        let uuid = self.lan_acting_uuid?;
        let mut p = self.plugin_seen.iter().find(|p| p.uuid == uuid)?.clone();
        // The turn's own state is in the game.
        if let Some(m) = self.player.as_ref() {
            p.location = Location {
                world: self.dim.id().to_owned(),
                x: m.position.x,
                y: m.position.y,
                z: m.position.z,
                yaw: self.yaw,
                pitch: self.pitch,
            };
            p.health = m.health.health();
        }
        Some(p)
    }
    fn plugins_listen<E: Event>(&self) -> bool {
        self.plugins.as_ref().is_some_and(|h| h.listens::<E>())
    }
    /// `callEvent`; what the listeners asked for waits for `apply_plugin_actions`.
    fn plugin_fire<E: Event>(&mut self, host: Option<&LanHost>, event: &mut E) {
        let players = self.plugin_players(host);
        let Some(mut plugins) = self.plugins.take() else {
            return;
        };
        let actions = {
            let view = View {
                game: self,
                players,
            };
            plugins.call(event, &view)
        };
        self.plugins = Some(plugins);
        self.plugin_actions.extend(actions);
        self.plugin_log();
    }
    /// The scheduler's tick (`CraftScheduler.mainThreadHeartbeat`).
    pub(super) fn plugins_tick(&mut self) {
        if self.plugins.is_none() {
            return;
        }
        let players = self.plugin_players(None);
        let Some(mut plugins) = self.plugins.take() else {
            return;
        };
        let actions = {
            let view = View {
                game: self,
                players,
            };
            plugins.tick(&view)
        };
        self.plugins = Some(plugins);
        self.plugin_actions.extend(actions);
        self.plugin_log();
    }
    /// A plugin's command or `/plugins` (`line` without its slash); false
    /// when no plugin has it.
    fn plugin_command(
        &mut self,
        host: Option<&LanHost>,
        sender: CommandSender,
        line: &str,
    ) -> bool {
        if self.plugins.is_none() {
            return false;
        }
        let players = self.plugin_players(host);
        let Some(mut plugins) = self.plugins.take() else {
            return false;
        };
        let actions = {
            let view = View {
                game: self,
                players,
            };
            plugins.command(sender, line, &view)
        };
        self.plugins = Some(plugins);
        self.plugin_log();
        match actions {
            Some(actions) => {
                self.plugin_actions.extend(actions);
                true
            }
            None => false,
        }
    }
    /// What the plugins asked for, with the hub in place (between ticks and
    /// after a guest's packets): messages and kicks directly, the rest
    /// through the server's own commands, quietly, as the console.
    pub(super) fn apply_plugin_actions(&mut self) {
        // Blocks a plugin kept, which their client took away already.
        if let Some(h) = self.lan.as_ref() {
            for (conn, pos, state) in std::mem::take(&mut self.plugin_resend) {
                h.hub
                    .send_to(conn, &C::BlockUpdate(play::BlockUpdate { pos, state }));
            }
        }
        let mut rounds = 0;
        while !self.plugin_actions.is_empty() && rounds < 8 {
            rounds += 1;
            for action in std::mem::take(&mut self.plugin_actions) {
                self.apply_plugin_action(action);
            }
        }
    }
    fn apply_plugin_action(&mut self, action: Action) {
        let name_of = |game: &Self, uuid: u128| {
            game.lan.as_ref().and_then(|h| {
                h.hub
                    .guests
                    .iter()
                    .find(|g| g.uuid == uuid && g.placed)
                    .map(|g| (g.conn, g.name.clone()))
            })
        };
        match action {
            Action::Message { to, text } => {
                if let (Some((conn, _)), Some(h)) = (name_of(self, to), self.lan.as_ref()) {
                    h.hub.send_to(conn, &system_chat(&text));
                }
            }
            Action::Broadcast(text) => {
                super::dedicated::log("INFO", &api::strip_colors(&text));
                if let Some(h) = self.lan.as_ref() {
                    h.hub.system_message(legacy_nbt(&text));
                }
            }
            Action::Log { plugin, line } => {
                super::dedicated::log("INFO", &format!("[{plugin}] {}", api::strip_colors(&line)));
            }
            Action::Kick { player, reason } => {
                if let Some((conn, _)) = name_of(self, player)
                    && let Some(h) = self.lan.as_mut()
                {
                    h.hub.kick(conn, legacy_nbt(&reason));
                }
            }
            Action::Dispatch {
                player: None,
                command,
            } => {
                if !self.plugin_command(None, CommandSender::Console, &command) {
                    self.plugin_console_run(&command);
                }
            }
            Action::Dispatch {
                player: Some(uuid),
                command,
            } => {
                if let Some((conn, _)) = name_of(self, uuid)
                    && let Some(mut host) = self.lan.take()
                {
                    self.guest_command(&mut host, conn, &command, None);
                    self.lan = Some(host);
                }
            }
            Action::SetBlock { world, pos, state } => {
                self.plugin_console_run(&format!(
                    "execute in {world} run setblock {} {} {} {state}",
                    pos.x, pos.y, pos.z
                ));
            }
            Action::Teleport { player, to } => {
                if let Some((_, name)) = name_of(self, player) {
                    self.plugin_console_run(&format!(
                        "execute in {} run tp {name} {} {} {} {} {}",
                        to.world, to.x, to.y, to.z, to.yaw, to.pitch
                    ));
                }
            }
            Action::Give { player, stack } => {
                if let Some((_, name)) = name_of(self, player) {
                    self.plugin_console_run(&format!("give {name} {} {}", stack.item, stack.count));
                }
            }
            Action::GameMode { player, mode } => {
                if let Some((_, name)) = name_of(self, player) {
                    let mode = ["survival", "creative", "adventure", "spectator"]
                        [usize::from(mode.min(3))];
                    self.plugin_console_run(&format!("gamemode {mode} {name}"));
                }
            }
        }
    }
    /// A vanilla command as the console, its feedback dropped.
    fn plugin_console_run(&mut self, command: &str) {
        self.chat_capture = Some(Vec::new());
        self.as_console(|g| g.run_command_quietly(command));
        self.chat_capture = None;
        self.admin_feedback.clear();
    }
    /// After a player joined and was placed: `PlayerJoinEvent`, and the join
    /// message it leaves (the hub left it to the plugins).
    /// Whether the player has no saved data on this server yet (checked before the join saves it).
    pub(super) fn plugin_first_join(&self, host: &LanHost, conn: u32) -> bool {
        let Some(g) = host.hub.guest(conn) else {
            return false;
        };
        !crate::hc::lan::playerdata::exists(&self.world_root, g.uuid)
    }
    pub(super) fn plugin_joined(&mut self, host: &mut LanHost, conn: u32, first_join: bool) {
        if !self.plugins_listen::<ev::PlayerJoinEvent>() {
            return;
        }
        let Some(g) = host.hub.guest(conn).filter(|g| g.placed) else {
            return;
        };
        let player = crate::game::Sim::plugin_player(g);
        let vanilla = format!("§e{} joined the game", g.name);
        let mut event = ev::PlayerJoinEvent {
            player,
            join_message: Some(vanilla.clone()),
            first_join,
        };
        self.plugin_fire(Some(host), &mut event);
        match event.join_message {
            Some(text) if text == vanilla => host.hub.announce_join(conn),
            Some(text) => {
                super::dedicated::log("INFO", &api::strip_colors(&text));
                host.hub.system_message(legacy_nbt(&text));
            }
            None => {}
        }
    }
    /// After a player left: `PlayerQuitEvent`, and the quit message it leaves.
    pub(super) fn plugin_left(&mut self, host: &mut LanHost, g: &Guest) {
        if !g.placed || !self.plugins_listen::<ev::PlayerQuitEvent>() {
            return;
        }
        let vanilla = format!("§e{} left the game", g.name);
        let mut event = ev::PlayerQuitEvent {
            player: crate::game::Sim::plugin_player(g),
            quit_message: Some(vanilla.clone()),
            reason: g.left_reason.clone(),
        };
        self.plugin_fire(Some(host), &mut event);
        match event.quit_message {
            Some(text) if text == vanilla => host.hub.announce_leave(g.uuid, &g.name),
            Some(text) => {
                super::dedicated::log("INFO", &api::strip_colors(&text));
                host.hub.system_message(legacy_nbt(&text));
            }
            None => {}
        }
    }
    /// A chat message the hub held for the plugins: `AsyncPlayerChatEvent`,
    /// then the message as vanilla sends it (signed as it came) when they left
    /// it alone, or as a system message as they made it.
    pub(super) fn plugin_chat(
        &mut self,
        host: &mut LanHost,
        message: crate::hc::lan::hub::chat::ChatMessage,
    ) -> Vec<HostEvent> {
        let Some(g) = host.hub.guests.iter().find(|g| g.uuid == message.sender) else {
            return host.hub.relay_chat(message);
        };
        let player = crate::game::Sim::plugin_player(g);
        let everyone: Vec<u128> = host
            .hub
            .guests
            .iter()
            .filter(|g| g.placed)
            .map(|g| g.uuid)
            .collect();
        const FORMAT: &str = "<{name}> {message}";
        let mut event = ev::PlayerChatEvent {
            player: player.clone(),
            message: message.content.clone(),
            format: FORMAT.into(),
            recipients: everyone.clone(),
            cancelled: false,
        };
        self.plugin_fire(Some(host), &mut event);
        if event.cancelled {
            return Vec::new();
        }
        if event.message == message.content
            && event.format == FORMAT
            && event.recipients == everyone
        {
            return host.hub.relay_chat(message);
        }
        let line = event.formatted();
        let content = system_chat(&line);
        for g in host
            .hub
            .guests
            .iter()
            .filter(|g| g.placed && event.recipients.contains(&g.uuid))
        {
            g.send(&content);
        }
        vec![HostEvent::Chat {
            line: api::strip_colors(&line),
            secure: false,
        }]
    }
    /// A command a player typed (without its slash):
    /// `PlayerCommandPreprocessEvent`, then a plugin's command or `/plugins`.
    /// The line the game should run, or None when a plugin took it.
    pub(super) fn plugin_player_command(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        command: &str,
    ) -> Option<String> {
        if self.plugins.is_none() {
            return Some(command.to_owned());
        }
        let Some(g) = host.hub.guest(conn) else {
            return Some(command.to_owned());
        };
        let player = crate::game::Sim::plugin_player(g);
        let mut line = command.trim_start_matches('/').to_owned();
        if self.plugins_listen::<ev::PlayerCommandPreprocessEvent>() {
            let mut event = ev::PlayerCommandPreprocessEvent {
                player: player.clone(),
                message: format!("/{line}"),
                cancelled: false,
            };
            self.plugin_fire(Some(host), &mut event);
            if event.cancelled {
                return None;
            }
            line = event.message.trim_start_matches('/').to_owned();
        }
        if self.plugin_command(Some(host), CommandSender::Player(player), &line) {
            return None;
        }
        Some(line)
    }
    /// A console line: `ServerCommandEvent`, then a plugin's command. The line
    /// the server should run, or None when a plugin took it.
    pub(super) fn plugin_console_command(&mut self, line: &str) -> Option<String> {
        if self.plugins.is_none() {
            return Some(line.to_owned());
        }
        let mut line = line.trim_start_matches('/').to_owned();
        if self.plugins_listen::<ev::ServerCommandEvent>() {
            let mut event = ev::ServerCommandEvent {
                command: line.clone(),
                cancelled: false,
            };
            self.plugin_fire(None, &mut event);
            if event.cancelled {
                return None;
            }
            line = event.command.trim_start_matches('/').to_owned();
        }
        if self.plugin_command(None, CommandSender::Console, &line) {
            return None;
        }
        Some(line)
    }
    /// The player whose turn it is breaks the block at `pos`:
    /// `BlockBreakEvent`. None when a plugin keeps it (the player's client,
    /// which broke it already, gets it back), else whether it drops its items.
    pub(super) fn plugin_block_break(&mut self, pos: [i32; 3]) -> Option<bool> {
        if !self.plugins_listen::<ev::BlockBreakEvent>() {
            return Some(true);
        }
        let Some(player) = self.plugin_acting() else {
            return Some(true);
        };
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let mut event = ev::BlockBreakEvent {
            player,
            world: self.dim.id().to_owned(),
            pos: BlockPos::new(pos[0], pos[1], pos[2]),
            block: self.state_string(state),
            drop_items: true,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            if let Some(conn) = self.lan_acting {
                self.plugin_resend.push((conn, pos, state.0 as i32));
            }
            return None;
        }
        Some(event.drop_items)
    }
    /// The player whose turn it is placed `state` at `pos` over `replaced`:
    /// `BlockPlaceEvent`. False when a plugin cancelled it.
    pub(super) fn plugin_block_place(
        &mut self,
        pos: [i32; 3],
        state: horizon_world::registry::BlockStateId,
        replaced: horizon_world::registry::BlockStateId,
        item: Option<api::ItemStack>,
    ) -> bool {
        if !self.plugins_listen::<ev::BlockPlaceEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let mut event = ev::BlockPlaceEvent {
            player,
            world: self.dim.id().to_owned(),
            pos: BlockPos::new(pos[0], pos[1], pos[2]),
            block: self.state_string(state),
            replaced: self.state_string(replaced),
            item,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        !event.cancelled
    }
    /// A player dies: `PlayerDeathEvent` with vanilla's message (plain text,
    /// None when `show_death_messages` is off) and `keep_inventory`. What the
    /// plugins left: the message (`Some(None)` when they removed it, `None`
    /// when they left it as it was) and whether the inventory stays.
    pub(super) fn plugin_player_death(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        message: Option<String>,
        keep_inventory: bool,
    ) -> (Option<Option<String>>, bool) {
        if !self.plugins_listen::<ev::PlayerDeathEvent>() {
            return (None, keep_inventory);
        }
        let Some(g) = host.hub.guest(conn) else {
            return (None, keep_inventory);
        };
        let mut event = ev::PlayerDeathEvent {
            player: crate::game::Sim::plugin_player(g),
            death_message: message.clone(),
            keep_inventory,
        };
        self.plugin_fire(Some(host), &mut event);
        let changed = (event.death_message != message).then_some(event.death_message);
        (changed, event.keep_inventory)
    }
}

/// Legacy-coded text (`§a...`) as a chat component.
fn legacy_nbt(text: &str) -> horizon_world::nbt::Tag {
    crate::hc::lan::data::component_nbt(&Text::legacy(text).component())
}
fn system_chat(text: &str) -> C {
    C::SystemChat(play::SystemChat {
        content: legacy_nbt(text),
        overlay: false,
    })
}

impl crate::game::Sim<'_> {
    /// A player as plugins see them.
    pub(crate) fn plugin_player(g: &Guest) -> PlayerRef {
        PlayerRef {
            name: g.name.clone(),
            uuid: g.uuid,
            location: Location {
                world: g.dimension.clone(),
                x: g.position[0],
                y: g.position[1],
                z: g.position[2],
                yaw: g.yaw,
                pitch: g.pitch,
            },
            health: g.health,
            game_mode: g.game_mode.clamp(0, 3) as u8,
            op_level: g.permission,
        }
    }
}
