//! horizon-server's plugins (crate `horizon-server-api`, Bukkit-shaped) in
//! the game: the dedicated server loads and enables them, fires their events
//! where vanilla's server would (`PlayerList.placeNewPlayer`, chat, commands,
//! `ServerPlayerGameMode.destroyBlock`, `BlockItem.place`, `ServerPlayer.die`),
//! runs their commands and tasks, and applies what they asked for once their
//! handler returns (`apply_plugin_actions`), through the server's own commands
//! so every change reaches the players as any other does.
use super::server_net::Network;
use crate::hc::{
    server_net::{hub::HostEvent, player::ServerPlayer},
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
    game: &'a crate::game::Server<'a>,
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
            .network
            .as_ref()
            .map_or(0, |h| h.hub.network.shared.max_players())
    }
    fn time(&self) -> i64 {
        self.game.world.day_time.rem_euclid(24000)
    }
}

impl crate::game::Server<'_> {
    /// `onLoad` of the plugins `horizon-server` was built with (before the worlds).
    pub(super) fn plugins_load(&mut self) {
        let Some(plugins) = PLUGINS.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        if plugins.is_empty() {
            return;
        }
        let mut net = PluginHost::new(plugins);
        net.load();
        self.plugins = Some(Box::new(net));
        self.plugin_log();
    }
    /// `onEnable`, once the worlds are loaded and the server listens.
    pub(super) fn plugins_enable(&mut self) {
        let Some(mut net) = self.plugins.take() else {
            return;
        };
        net.enable();
        if let Some(network) = self.network.as_mut() {
            // The plugins decide the join and quit messages and see chat first.
            network.hub.quiet_joins = net.listens::<ev::PlayerJoinEvent>();
            network.hub.quiet_leaves = net.listens::<ev::PlayerQuitEvent>();
            network.hub.intercept_chat = net.listens::<ev::PlayerChatEvent>();
        }
        self.plugins = Some(net);
        self.plugin_log();
    }
    /// `onDisable` as the server stops, with what they still asked for.
    pub(super) fn plugins_disable(&mut self) {
        if let Some(net) = self.plugins.as_mut() {
            net.disable();
        }
        self.plugin_log();
    }
    fn plugin_log(&mut self) {
        let Some(net) = self.plugins.as_mut() else {
            return;
        };
        for line in net.take_log() {
            super::dedicated::log("INFO", &line);
        }
    }
    /// The online players, from the hub in hand or in place, else the last
    /// ones seen (the hub is out of its place during a player's turn).
    fn plugin_players(&mut self, net: Option<&Network>) -> Vec<PlayerRef> {
        if let Some(h) = net.or(self.network.as_ref()) {
            let players: Vec<PlayerRef> = h
                .hub
                .players
                .iter()
                .filter(|g| g.placed)
                .map(crate::game::Server::plugin_player)
                .collect();
            self.plugin_seen = players.clone();
            return players;
        }
        self.plugin_seen.clone()
    }
    /// The player whose turn it is (`as_player`).
    fn plugin_acting(&self) -> Option<PlayerRef> {
        let uuid = self.net_acting_uuid?;
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
    fn plugin_fire<E: Event>(&mut self, net: Option<&Network>, event: &mut E) {
        let players = self.plugin_players(net);
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
    fn plugin_command(&mut self, net: Option<&Network>, sender: CommandSender, line: &str) -> bool {
        if self.plugins.is_none() {
            return false;
        }
        let players = self.plugin_players(net);
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
    /// after a player's packets): messages and kicks directly, the rest
    /// through the server's own commands, quietly, as the console.
    pub(super) fn apply_plugin_actions(&mut self) {
        // Blocks a plugin kept, which their client took away already.
        if let Some(h) = self.network.as_ref() {
            for (conn, pos, state) in std::mem::take(&mut self.plugin_resend) {
                h.hub
                    .send_to(conn, &C::BlockUpdate(play::BlockUpdate { pos, state }));
            }
        }
        // Inventories a plugin kept, which their client changed already.
        if let Some(h) = self.network.as_mut() {
            for conn in std::mem::take(&mut self.plugin_resync) {
                if let Some(i) = h.hub.players.iter().position(|g| g.conn == conn) {
                    h.hub.send_inventory(i);
                }
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
            game.network.as_ref().and_then(|h| {
                h.hub
                    .players
                    .iter()
                    .find(|g| g.uuid == uuid && g.placed)
                    .map(|g| (g.conn, g.name.clone()))
            })
        };
        match action {
            Action::Message { to, text } => {
                if let (Some((conn, _)), Some(h)) = (name_of(self, to), self.network.as_ref()) {
                    h.hub.send_to(conn, &system_chat(&text));
                }
            }
            Action::Broadcast(text) => {
                super::dedicated::log("INFO", &api::strip_colors(&text));
                if let Some(h) = self.network.as_ref() {
                    h.hub.system_message(legacy_nbt(&text));
                }
            }
            Action::Log { plugin, line } => {
                super::dedicated::log("INFO", &format!("[{plugin}] {}", api::strip_colors(&line)));
            }
            Action::Kick { player, reason } => {
                if let Some((conn, _)) = name_of(self, player)
                    && let Some(h) = self.network.as_mut()
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
                    && let Some(mut net) = self.network.take()
                {
                    self.player_command(&mut net, conn, &command, None);
                    self.network = Some(net);
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
    pub(super) fn plugin_first_join(&self, net: &Network, conn: u32) -> bool {
        let Some(g) = net.hub.player(conn) else {
            return false;
        };
        !crate::hc::server_net::playerdata::exists(&self.world_root, g.uuid)
    }
    pub(super) fn plugin_joined(&mut self, net: &mut Network, conn: u32, first_join: bool) {
        if !self.plugins_listen::<ev::PlayerJoinEvent>() {
            return;
        }
        let Some(g) = net.hub.player(conn).filter(|g| g.placed) else {
            return;
        };
        let player = crate::game::Server::plugin_player(g);
        let vanilla = format!("§e{} joined the game", g.name);
        let mut event = ev::PlayerJoinEvent {
            player,
            join_message: Some(vanilla.clone()),
            first_join,
        };
        self.plugin_fire(Some(net), &mut event);
        match event.join_message {
            Some(text) if text == vanilla => net.hub.announce_join(conn),
            Some(text) => {
                super::dedicated::log("INFO", &api::strip_colors(&text));
                net.hub.system_message(legacy_nbt(&text));
            }
            None => {}
        }
    }
    /// After a player left: `PlayerQuitEvent`, and the quit message it leaves.
    pub(super) fn plugin_left(&mut self, net: &mut Network, g: &ServerPlayer) {
        if !g.placed || !self.plugins_listen::<ev::PlayerQuitEvent>() {
            return;
        }
        let vanilla = format!("§e{} left the game", g.name);
        let mut event = ev::PlayerQuitEvent {
            player: crate::game::Server::plugin_player(g),
            quit_message: Some(vanilla.clone()),
            reason: g.left_reason.clone(),
        };
        self.plugin_fire(Some(net), &mut event);
        match event.quit_message {
            Some(text) if text == vanilla => net.hub.announce_leave(g.uuid, &g.name),
            Some(text) => {
                super::dedicated::log("INFO", &api::strip_colors(&text));
                net.hub.system_message(legacy_nbt(&text));
            }
            None => {}
        }
    }
    /// A chat message the hub held for the plugins: `AsyncPlayerChatEvent`,
    /// then the message as vanilla sends it (signed as it came) when they left
    /// it alone, or as a system message as they made it.
    pub(super) fn plugin_chat(
        &mut self,
        net: &mut Network,
        message: crate::hc::server_net::hub::chat::ChatMessage,
    ) -> Vec<HostEvent> {
        let Some(g) = net.hub.players.iter().find(|g| g.uuid == message.sender) else {
            return net.hub.relay_chat(message);
        };
        let player = crate::game::Server::plugin_player(g);
        let everyone: Vec<u128> = net
            .hub
            .players
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
        self.plugin_fire(Some(net), &mut event);
        if event.cancelled {
            return Vec::new();
        }
        if event.message == message.content
            && event.format == FORMAT
            && event.recipients == everyone
        {
            return net.hub.relay_chat(message);
        }
        let line = event.formatted();
        let content = system_chat(&line);
        for g in net
            .hub
            .players
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
        net: &mut Network,
        conn: u32,
        command: &str,
    ) -> Option<String> {
        if self.plugins.is_none() {
            return Some(command.to_owned());
        }
        let Some(g) = net.hub.player(conn) else {
            return Some(command.to_owned());
        };
        let player = crate::game::Server::plugin_player(g);
        let mut line = command.trim_start_matches('/').to_owned();
        if self.plugins_listen::<ev::PlayerCommandPreprocessEvent>() {
            let mut event = ev::PlayerCommandPreprocessEvent {
                player: player.clone(),
                message: format!("/{line}"),
                cancelled: false,
            };
            self.plugin_fire(Some(net), &mut event);
            if event.cancelled {
                return None;
            }
            line = event.message.trim_start_matches('/').to_owned();
        }
        if self.plugin_command(Some(net), CommandSender::Player(player), &line) {
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
            if let Some(conn) = self.net_acting {
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
        net: &mut Network,
        conn: u32,
        message: Option<String>,
        keep_inventory: bool,
    ) -> (Option<Option<String>>, bool) {
        if !self.plugins_listen::<ev::PlayerDeathEvent>() {
            return (None, keep_inventory);
        }
        let Some(g) = net.hub.player(conn) else {
            return (None, keep_inventory);
        };
        let mut event = ev::PlayerDeathEvent {
            player: crate::game::Server::plugin_player(g),
            death_message: message.clone(),
            keep_inventory,
        };
        self.plugin_fire(Some(net), &mut event);
        let changed = (event.death_message != message).then_some(event.death_message);
        (changed, event.keep_inventory)
    }
}

impl crate::game::Server<'_> {
    /// `PlayerInteractEvent` for the player of `conn`; false when a plugin
    /// cancelled it (the caller then sends their client the truth back).
    pub(super) fn plugin_interact(
        &mut self,
        net: &mut Network,
        conn: u32,
        action: ev::InteractAction,
        hand: ev::Hand,
        block: Option<([i32; 3], i32)>,
    ) -> bool {
        if !self.plugins_listen::<ev::PlayerInteractEvent>() {
            return true;
        }
        let Some(g) = net.hub.player(conn) else {
            return true;
        };
        let held = match hand {
            ev::Hand::Main => g.inventory.get(g.selected).cloned().flatten(),
            ev::Hand::Off => g.offhand.clone(),
        };
        let mut event = ev::PlayerInteractEvent {
            player: crate::game::Server::plugin_player(g),
            action,
            hand,
            item: held.map(|s| api::ItemStack::new(&s.item, s.count)),
            world: g.dimension.clone(),
            block: block.map(|(p, face)| {
                (
                    BlockPos::new(p[0], p[1], p[2]),
                    ev::BlockFace::from_id(face),
                )
            }),
            cancelled: false,
        };
        self.plugin_fire(Some(net), &mut event);
        if event.cancelled
            && let Some(i) = net.hub.players.iter().position(|g| g.conn == conn)
        {
            net.hub.send_inventory(i);
        }
        !event.cancelled
    }
    /// `PlayerDropItemEvent` for what the player whose turn it is throws
    /// from their hand; false when a plugin keeps it there (their client,
    /// which took it out already, gets the inventory back).
    pub(super) fn plugin_drop_item(&mut self, item: &crate::hc::items::ItemStack) -> bool {
        if !self.plugins_listen::<ev::PlayerDropItemEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let mut event = ev::PlayerDropItemEvent {
            player,
            item: api::ItemStack::new(&item.item, item.count),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled
            && let Some(conn) = self.net_acting
        {
            self.plugin_resync.push(conn);
        }
        !event.cancelled
    }
    /// `PlayerGameModeChangeEvent` for the player whose turn it is; false
    /// when a plugin keeps the old mode.
    pub(super) fn plugin_game_mode_change(&mut self, mode: u8) -> bool {
        if !self.plugins_listen::<ev::PlayerGameModeChangeEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let mut event = ev::PlayerGameModeChangeEvent {
            player,
            new_game_mode: mode,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        !event.cancelled
    }
    /// `PlayerRespawnEvent` for the player at `i`, coming back at `at` in
    /// `world`. The location a plugin chose instead, if it changed it.
    pub(super) fn plugin_respawn(
        &mut self,
        net: &mut Network,
        i: usize,
        world: &str,
        at: [f64; 3],
        own_spawn: bool,
    ) -> Option<Location> {
        if !self.plugins_listen::<ev::PlayerRespawnEvent>() {
            return None;
        }
        let g = &net.hub.players[i];
        let start = Location {
            world: world.to_owned(),
            x: at[0],
            y: at[1],
            z: at[2],
            yaw: g.yaw,
            pitch: g.pitch,
        };
        let mut event = ev::PlayerRespawnEvent {
            player: crate::game::Server::plugin_player(g),
            respawn_location: start.clone(),
            own_spawn,
        };
        self.plugin_fire(Some(net), &mut event);
        (event.respawn_location != start).then_some(event.respawn_location)
    }
    /// `PlayerMoveEvent` once a tick for each player who moved or turned
    /// since the last: a cancelled move puts them back, a changed `to` moves
    /// them there.
    pub(super) fn plugin_moves(&mut self, net: &mut Network) {
        if !self.plugins_listen::<ev::PlayerMoveEvent>() {
            self.plugin_last_moves.clear();
            return;
        }
        for i in 0..net.hub.players.len() {
            let g = &net.hub.players[i];
            if !g.placed || g.dead {
                continue;
            }
            let now = crate::game::Server::plugin_player(g).location;
            let conn = g.conn;
            let Some(from) = self.plugin_last_moves.insert(conn, now.clone()) else {
                continue;
            };
            // PlayerConnection's thresholds: 1/256 of a block, 10 degrees.
            let moved = from.world == now.world
                && ((from.x - now.x).powi(2) + (from.y - now.y).powi(2) + (from.z - now.z).powi(2)
                    > 1.0 / 256.0
                    || (from.yaw - now.yaw).abs() > 10.0
                    || (from.pitch - now.pitch).abs() > 10.0);
            if !moved {
                // Below the thresholds the last place the plugins saw stays.
                self.plugin_last_moves.insert(conn, from);
                continue;
            }
            let mut event = ev::PlayerMoveEvent {
                player: crate::game::Server::plugin_player(&net.hub.players[i]),
                from: from.clone(),
                to: now.clone(),
                cancelled: false,
            };
            self.plugin_fire(Some(net), &mut event);
            let place = if event.cancelled {
                Some(from)
            } else {
                (event.to != now).then_some(event.to)
            };
            let Some(place) = place else {
                continue;
            };
            if place.world == now.world {
                let g = &mut net.hub.players[i];
                (g.yaw, g.pitch) = (place.yaw, place.pitch);
                net.hub.teleport(i, Some([place.x, place.y, place.z]));
            } else {
                let uuid = net.hub.players[i].uuid;
                self.plugin_actions.push(Action::Teleport {
                    player: uuid,
                    to: place.clone(),
                });
            }
            self.plugin_last_moves.insert(conn, place);
        }
    }
}

/// Legacy-coded text (`§a...`) as a chat component.
fn legacy_nbt(text: &str) -> horizon_world::nbt::Tag {
    crate::hc::server_net::data::component_nbt(&Text::legacy(text).component())
}
fn system_chat(text: &str) -> C {
    C::SystemChat(play::SystemChat {
        content: legacy_nbt(text),
        overlay: false,
    })
}

impl crate::game::Server<'_> {
    /// A player as plugins see them.
    pub(crate) fn plugin_player(g: &ServerPlayer) -> PlayerRef {
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
