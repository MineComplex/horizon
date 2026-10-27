//! Where the server fires plugins' events (docs/plugins.md): a player's
//! packets before the game handles them, commands, chat, joins and quits,
//! deaths, damage, spawns, explosions and blocks. A cancelled action is
//! undone on the player's client (the block, the slot, the menu put back).
use super::Game;
use super::lan::LanHost;
use super::plugins::{Action, parse_uuid, stack_in, stack_out, text_tag, uuid_str};
use horizon_client::lan::hub::HostEvent;
use horizon_mods::plugin::{self, Event, EventKind, Sender, events as ev, types};
use horizon_protocol::packets::play::{self, Clientbound as C, Serverbound as S};

/// A block face as plugins name it.
fn face_name(d: u8) -> &'static str {
    match d {
        0 => "down",
        1 => "up",
        2 => "north",
        3 => "south",
        4 => "west",
        _ => "east",
    }
}
fn face_offset(d: u8) -> [i32; 3] {
    match d {
        0 => [0, -1, 0],
        1 => [0, 1, 0],
        2 => [0, 0, -1],
        3 => [0, 0, 1],
        4 => [-1, 0, 0],
        _ => [1, 0, 0],
    }
}
fn hand_of(h: i32) -> types::Hand {
    if h == 1 {
        types::Hand::Off
    } else {
        types::Hand::Main
    }
}
fn click_mode(input: i32) -> ev::ClickMode {
    match input {
        1 => ev::ClickMode::QuickMove,
        2 => ev::ClickMode::Swap,
        3 => ev::ClickMode::Clone,
        4 => ev::ClickMode::Throw,
        5 => ev::ClickMode::QuickCraft,
        6 => ev::ClickMode::PickupAll,
        _ => ev::ClickMode::Pickup,
    }
}

impl Game<'_> {
    fn plugin_block(&self, world: &str, pos: [i32; 3]) -> types::Block {
        let state = if world == self.dim.id() {
            self.state_string(self.world.block(pos[0], pos[1], pos[2]))
        } else {
            "minecraft:air".into()
        };
        types::Block {
            world: world.to_owned(),
            pos: types::BlockPos {
                x: pos[0],
                y: pos[1],
                z: pos[2],
            },
            state,
        }
    }

    /// The entity a plugin event names for a network id.
    pub(super) fn plugin_entity_ref(&self, id: i32) -> types::EntityRef {
        if let Some(g) = self
            .lan
            .as_ref()
            .and_then(|h| h.hub.guests.iter().find(|g| g.entity_id == id))
        {
            return types::EntityRef {
                id,
                kind: "minecraft:player".into(),
                player: Some(uuid_str(g.uuid)),
            };
        }
        let kind = self
            .plugin_target(id)
            .map(|t| self.target_type(t))
            .unwrap_or_default();
        types::EntityRef {
            id,
            kind,
            player: None,
        }
    }
    fn plugin_player_ref(p: &types::Player) -> types::EntityRef {
        types::EntityRef {
            id: p.entity_id,
            kind: "minecraft:player".into(),
            player: Some(p.id.clone()),
        }
    }

    /// A guest's packet, before the game handles it: the events it means.
    /// True when a plugin cancelled it (the game then drops it). Changes a
    /// plugin made to the packet (a sign's lines) are written back.
    pub(super) fn plugin_packet(&mut self, host: &mut LanHost, conn: u32, packet: &mut S) -> bool {
        if self.remote.is_some() || !self.mods.mods.runs_plugins() {
            return false;
        }
        let Some(player) = self.plugin_player(Some(host), conn) else {
            return false;
        };
        let world = player.location.world.clone();
        match packet {
            S::UseItemOn(p) if self.plugins_listen(EventKind::PlayerInteract) => {
                let block = self.plugin_block(&world, p.pos);
                let item = if p.hand == 1 {
                    host.hub
                        .guest(conn)
                        .and_then(|g| g.offhand.as_ref())
                        .map(stack_out)
                } else {
                    player.held.clone()
                };
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerInteract(ev::PlayerInteractEvent {
                        player,
                        action: ev::InteractAction::RightClickBlock,
                        hand: hand_of(p.hand),
                        item,
                        block: Some(block),
                        face: face_name(p.face as u8).into(),
                        cancelled: false,
                    }),
                );
                if plugin::is_cancelled(&back) {
                    host.hub.ack(conn, p.sequence);
                    let o = face_offset(p.face as u8);
                    let next = [p.pos[0] + o[0], p.pos[1] + o[1], p.pos[2] + o[2]];
                    self.resend_blocks(host, conn, &[p.pos, next]);
                    self.mods.plugin.resync.insert(conn);
                    self.resend_after_cancel_with(host);
                    return true;
                }
                false
            }
            S::UseItem(p) if self.plugins_listen(EventKind::PlayerInteract) => {
                let item = if p.hand == 1 {
                    host.hub
                        .guest(conn)
                        .and_then(|g| g.offhand.as_ref())
                        .map(stack_out)
                } else {
                    player.held.clone()
                };
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerInteract(ev::PlayerInteractEvent {
                        player,
                        action: ev::InteractAction::RightClickAir,
                        hand: hand_of(p.hand),
                        item,
                        block: None,
                        face: String::new(),
                        cancelled: false,
                    }),
                );
                if plugin::is_cancelled(&back) {
                    host.hub.ack(conn, p.sequence);
                    self.mods.plugin.resync.insert(conn);
                    self.resend_after_cancel_with(host);
                    return true;
                }
                false
            }
            // START_DESTROY_BLOCK: a left click on a block.
            S::PlayerAction(a)
                if a.action == 0 && self.plugins_listen(EventKind::PlayerInteract) =>
            {
                let block = self.plugin_block(&world, a.pos);
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerInteract(ev::PlayerInteractEvent {
                        item: player.held.clone(),
                        player,
                        action: ev::InteractAction::LeftClickBlock,
                        hand: types::Hand::Main,
                        block: Some(block),
                        face: face_name(a.direction).into(),
                        cancelled: false,
                    }),
                );
                if plugin::is_cancelled(&back) {
                    host.hub.ack(conn, a.sequence);
                    self.resend_blocks(host, conn, &[a.pos]);
                    return true;
                }
                false
            }
            // DROP_ALL_ITEMS (4) and DROP_ITEM (5).
            S::PlayerAction(a)
                if matches!(a.action, 4 | 5) && self.plugins_listen(EventKind::PlayerDropItem) =>
            {
                let Some(mut item) = player.held.clone() else {
                    return false;
                };
                if a.action == 5 {
                    item.count = 1;
                }
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerDropItem(ev::PlayerDropItemEvent {
                        player,
                        item,
                        cancelled: false,
                    }),
                );
                if plugin::is_cancelled(&back) {
                    self.mods.plugin.resync.insert(conn);
                    self.resend_after_cancel_with(host);
                    return true;
                }
                false
            }
            // SWAP_ITEM_WITH_OFFHAND.
            S::PlayerAction(a)
                if a.action == 7 && self.plugins_listen(EventKind::PlayerSwapHands) =>
            {
                let off = host
                    .hub
                    .guest(conn)
                    .and_then(|g| g.offhand.as_ref())
                    .map(stack_out);
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerSwapHands(ev::PlayerSwapHandsEvent {
                        main: player.held.clone(),
                        player,
                        off,
                        cancelled: false,
                    }),
                );
                if plugin::is_cancelled(&back) {
                    self.mods.plugin.resync.insert(conn);
                    self.resend_after_cancel_with(host);
                    return true;
                }
                false
            }
            // An entity used (not attacked: that is ServerboundAttack).
            S::Interact(p) if self.plugins_listen(EventKind::PlayerInteractEntity) => {
                let entity = self.plugin_entity_ref(p.id);
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerInteractEntity(ev::PlayerInteractEntityEvent {
                        player,
                        entity,
                        hand: hand_of(p.hand),
                        cancelled: false,
                    }),
                );
                if plugin::is_cancelled(&back) {
                    self.mods.plugin.resync.insert(conn);
                    self.resend_after_cancel_with(host);
                    return true;
                }
                false
            }
            S::SetCarriedItem(p) if self.plugins_listen(EventKind::PlayerItemHeld) => {
                let previous = player.selected_slot;
                let current = p.slot.clamp(0, 8) as u32;
                if previous == current {
                    return false;
                }
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerItemHeld(ev::PlayerItemHeldEvent {
                        player,
                        previous,
                        current,
                        cancelled: false,
                    }),
                );
                if plugin::is_cancelled(&back) {
                    host.hub.send_to(
                        conn,
                        &C::SetHeldSlot(play::SetHeldSlot {
                            slot: previous as i32,
                        }),
                    );
                    return true;
                }
                false
            }
            S::ContainerClick(p) if self.plugins_listen(EventKind::InventoryClick) => {
                let (kind, inventory, size) = host.hub.menu_view(conn);
                let current = (p.slot >= 0)
                    .then(|| host.hub.menu_slot_item(conn, &self.world, p.slot as usize))
                    .flatten()
                    .as_ref()
                    .map(stack_out);
                let cursor = host
                    .hub
                    .guest(conn)
                    .and_then(|g| g.carried.as_ref())
                    .map(stack_out);
                let back = self.plugin_fire(
                    Some(host),
                    Event::InventoryClick(ev::InventoryClickEvent {
                        player,
                        view: ev::InventoryView {
                            kind,
                            plugin_inventory: inventory,
                            size,
                        },
                        slot: i32::from(p.slot),
                        button: p.button.max(0) as u32,
                        mode: click_mode(p.input),
                        current,
                        cursor,
                        cancelled: false,
                    }),
                );
                if plugin::is_cancelled(&back) {
                    host.hub.resend_menu(conn, &self.world);
                    return true;
                }
                false
            }
            S::ContainerClose(_) if self.plugins_listen(EventKind::InventoryClose) => {
                let (kind, inventory, size) = host.hub.menu_view(conn);
                self.plugin_fire(
                    Some(host),
                    Event::InventoryClose(ev::InventoryCloseEvent {
                        player,
                        view: ev::InventoryView {
                            kind,
                            plugin_inventory: inventory,
                            size,
                        },
                    }),
                );
                false
            }
            S::SignUpdate(p) if self.plugins_listen(EventKind::SignChange) => {
                let block = self.plugin_block(&world, p.pos);
                let back = self.plugin_fire(
                    Some(host),
                    Event::SignChange(ev::SignChangeEvent {
                        player,
                        block,
                        lines: p.lines.to_vec(),
                        front: p.front,
                        cancelled: false,
                    }),
                );
                match back {
                    Event::SignChange(e) if e.cancelled => true,
                    Event::SignChange(e) => {
                        for (k, line) in e.lines.into_iter().take(4).enumerate() {
                            p.lines[k] = line;
                        }
                        false
                    }
                    _ => false,
                }
            }
            S::CustomPayload(p) if p.channel != horizon_protocol::quiet::CHANNEL => {
                let channel = p.channel.clone();
                let data = std::mem::take(&mut p.data);
                let heard = self
                    .with_plugins(Some(host), |mods, ctx| {
                        mods.plugin_payload(ctx, player, &channel, data.clone())
                    })
                    .unwrap_or(false);
                if !heard {
                    p.data = data;
                }
                heard
            }
            S::ResourcePack(p) if self.plugins_listen(EventKind::PlayerResourcePack) => {
                let status = match p.action {
                    0 => "loaded",
                    1 => "declined",
                    2 => "failed",
                    3 => "accepted",
                    4 => "downloaded",
                    5 => "invalid-url",
                    6 => "reload-failed",
                    7 => "discarded",
                    _ => "unknown",
                };
                self.plugin_fire(
                    Some(host),
                    Event::PlayerResourcePack(ev::PlayerResourcePackEvent {
                        player,
                        status: status.into(),
                    }),
                );
                false
            }
            S::ChangeGameMode(p) if self.plugins_listen(EventKind::PlayerGameModeChange) => {
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerGameModeChange(ev::PlayerGameModeChangeEvent {
                        player,
                        mode: super::plugins::mode_of(p.mode),
                        cancelled: false,
                    }),
                );
                plugin::is_cancelled(&back)
            }
            _ => false,
        }
    }

    /// The blocks at `positions` sent again to a guest as they are.
    fn resend_blocks(&self, host: &mut LanHost, conn: u32, positions: &[[i32; 3]]) {
        for &pos in positions {
            let state = self.world.block(pos[0], pos[1], pos[2]);
            host.hub.send_to(
                conn,
                &C::BlockUpdate(play::BlockUpdate {
                    pos,
                    state: state.0 as i32,
                }),
            );
        }
    }
    /// `resend_after_cancel` with the hub in hand.
    fn resend_after_cancel_with(&mut self, host: &mut LanHost) {
        let conns: Vec<u32> = std::mem::take(&mut self.mods.plugin.resync)
            .into_iter()
            .collect();
        for conn in conns {
            if let Some(i) = host.hub.guests.iter().position(|g| g.conn == conn) {
                host.hub.send_inventory(i);
                let selected = host.hub.guests[i].selected as i32;
                host.hub.guests[i].send(&C::SetHeldSlot(play::SetHeldSlot { slot: selected }));
            }
        }
    }

    /// A command a guest typed (without its slash): `PlayerCommandEvent`,
    /// `/plugins`, then a plugin's command. The line the game should run
    /// on, or None when a plugin took it or cancelled it.
    pub(super) fn plugin_player_command(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        line: &str,
    ) -> Option<String> {
        if self.remote.is_some() || !self.mods.mods.runs_plugins() {
            return Some(line.to_owned());
        }
        let player = self.plugin_player(Some(host), conn)?;
        let mut line = line.to_owned();
        if self.plugins_listen(EventKind::PlayerCommand) {
            let back = self.plugin_fire(
                Some(host),
                Event::PlayerCommand(ev::PlayerCommandEvent {
                    player: player.clone(),
                    line: line.clone(),
                    cancelled: false,
                }),
            );
            match back {
                Event::PlayerCommand(e) if e.cancelled => return None,
                Event::PlayerCommand(e) => line = e.line.trim_start_matches('/').to_owned(),
                _ => {}
            }
        }
        let label = line
            .split(' ')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(label.as_str(), "plugins" | "pl") && !self.mods.mods.is_plugin_command(&line) {
            let text = self.plugin_list_text();
            host.hub.send_to(
                conn,
                &C::SystemChat(play::SystemChat {
                    content: text_tag(&text),
                    overlay: false,
                }),
            );
            return None;
        }
        let sender = Sender::Player(player);
        let owned = self
            .with_plugins(Some(host), |mods, ctx| {
                mods.plugin_command(ctx, &sender, &line)
            })
            .unwrap_or(false);
        if owned { None } else { Some(line) }
    }

    /// `/plugins`: the installed plugins, green when enabled, red when not.
    pub(super) fn plugin_list_text(&self) -> String {
        let infos = self.mods.mods.plugin_infos();
        let names: Vec<String> = infos
            .iter()
            .map(|p| format!("{}{}", if p.enabled { "§a" } else { "§c" }, p.name))
            .collect();
        format!("Plugins ({}): {}", infos.len(), names.join("§f, "))
    }

    /// A console line: `ServerCommandEvent`, `plugins`, then a plugin's
    /// command as the console. The line the console should run, or None.
    pub(super) fn plugin_console_line(&mut self, line: &str) -> Option<String> {
        if !self.mods.mods.runs_plugins() {
            return Some(line.to_owned());
        }
        let mut line = line.trim_start_matches('/').to_owned();
        if self.plugins_listen(EventKind::ServerCommand) {
            let back = self.plugin_fire(
                None,
                Event::ServerCommand(ev::ServerCommandEvent {
                    line: line.clone(),
                    cancelled: false,
                }),
            );
            match back {
                Event::ServerCommand(e) if e.cancelled => return None,
                Event::ServerCommand(e) => line = e.line.trim_start_matches('/').to_owned(),
                _ => {}
            }
        }
        let label = line
            .split(' ')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(label.as_str(), "plugins" | "pl") && !self.mods.mods.is_plugin_command(&line) {
            let text = horizon_mods::plugin::strip_codes(&self.plugin_list_text());
            super::dedicated::log("INFO", &text);
            return None;
        }
        let owned = self
            .with_plugins(None, |mods, ctx| {
                mods.plugin_command(ctx, &Sender::Console, &line)
            })
            .unwrap_or(false);
        if owned {
            self.apply_plugin_actions();
            None
        } else {
            Some(line)
        }
    }

    /// Completions for a plugin's command a guest is typing: the range they
    /// replace (byte offsets into `command`) and the words.
    pub(super) fn plugin_suggestions(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        command: &str,
    ) -> Option<(usize, Vec<String>)> {
        if self.remote.is_some()
            || !self.mods.mods.runs_plugins()
            || !self.mods.mods.is_plugin_command(command)
            || !command.contains(' ')
        {
            return None;
        }
        let player = self.plugin_player(Some(host), conn)?;
        let sender = Sender::Player(player);
        let words = self
            .with_plugins(Some(host), |mods, ctx| {
                mods.plugin_tab_complete(ctx, &sender, command)
            })
            .flatten()?;
        let start = command.rfind(' ').map_or(0, |i| i + 1);
        Some((start, words))
    }

    /// A chat message held for the plugins: `PlayerChatEvent`, then the
    /// message as vanilla sends it (unchanged: signed as it came) or as
    /// the plugins made it. The hub's events for it.
    pub(super) fn plugin_chat_pending(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        message: horizon_client::lan::hub::chat::ChatMessage,
    ) -> Vec<HostEvent> {
        let Some(player) = self.plugin_player(Some(host), conn) else {
            return host.hub.relay_chat(message);
        };
        if !self.plugins_listen(EventKind::PlayerChat) {
            return host.hub.relay_chat(message);
        }
        let everyone: Vec<String> = host
            .hub
            .guests
            .iter()
            .filter(|g| g.placed)
            .map(|g| uuid_str(g.uuid))
            .collect();
        const FORMAT: &str = "<{name}> {message}";
        let back = self.plugin_fire(
            Some(host),
            Event::PlayerChat(ev::PlayerChatEvent {
                player: player.clone(),
                message: message.content.clone(),
                format: FORMAT.into(),
                recipients: everyone.clone(),
                cancelled: false,
            }),
        );
        let Event::PlayerChat(e) = back else {
            return host.hub.relay_chat(message);
        };
        if e.cancelled {
            return Vec::new();
        }
        let unchanged = e.message == message.content
            && e.format == FORMAT
            && e.recipients == everyone
            && e.player.display_name == player.name;
        if unchanged {
            return host.hub.relay_chat(message);
        }
        self.plugin_deliver_chat(host, &e);
        vec![HostEvent::PlayerChat {
            sender: message.sender,
            text: e.message,
        }]
    }

    /// After a guest joined and was placed: login and join events. Sends
    /// the join message when the hub left it to the plugins.
    pub(super) fn plugin_after_join(&mut self, host: &mut LanHost, conn: u32) {
        if self.remote.is_some() || !self.mods.mods.runs_plugins() {
            return;
        }
        let first = self.mods.plugin.newcomers.remove(&conn);
        let message = self.plugin_joined(host, conn, first);
        if !host.hub.quiet_joins {
            return;
        }
        match message {
            None => host.hub.announce_join(conn),
            Some(None) => {}
            Some(Some(text)) => {
                let content = text_tag(&text);
                for g in host.hub.guests.iter().filter(|g| g.placed) {
                    g.send(&C::SystemChat(play::SystemChat {
                        content: content.clone(),
                        overlay: false,
                    }));
                }
            }
        }
    }

    /// After a guest left: the quit event. Sends the quit message when the
    /// hub left it to the plugins.
    pub(super) fn plugin_after_leave(
        &mut self,
        host: &mut LanHost,
        g: &horizon_client::lan::guest::Guest,
    ) {
        if self.remote.is_some() || !self.mods.mods.runs_plugins() {
            return;
        }
        let message = self.plugin_left(host, g, &g.left_reason.clone());
        if !host.hub.quiet_leaves || !g.placed {
            return;
        }
        match message {
            None => host.hub.announce_leave(g.uuid, &g.name),
            Some(None) => {}
            Some(Some(text)) => {
                let content = text_tag(&text);
                for other in host.hub.guests.iter().filter(|o| o.placed) {
                    other.send(&C::SystemChat(play::SystemChat {
                        content: content.clone(),
                        overlay: false,
                    }));
                }
            }
        }
    }

    /// A guest is dying: `PlayerDeathEvent` with vanilla's message (JSON).
    /// What the plugins decided: the message (None: none), keep the
    /// inventory, keep the level.
    pub(super) fn plugin_player_death(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        message: Option<serde_json::Value>,
        keep_inventory: bool,
    ) -> Option<(Option<String>, bool, bool)> {
        if self.remote.is_some() || !self.plugins_listen(EventKind::PlayerDeath) {
            return None;
        }
        let player = self.plugin_player(Some(host), conn)?;
        let drops: Vec<types::ItemStack> = host
            .hub
            .guest(conn)
            .map(|g| {
                g.inventory
                    .iter()
                    .chain(g.armor.iter())
                    .chain(std::iter::once(&g.offhand))
                    .flatten()
                    .map(stack_out)
                    .collect()
            })
            .unwrap_or_default();
        let original = message.as_ref().map(|m| m.to_string());
        let back = self.plugin_fire(
            Some(host),
            Event::PlayerDeath(ev::PlayerDeathEvent {
                player,
                message: original.clone(),
                keep_inventory,
                keep_level: false,
                drops,
            }),
        );
        let Event::PlayerDeath(e) = back else {
            return None;
        };
        Some((e.message, e.keep_inventory, e.keep_level))
    }

    /// A guest respawned: `PlayerRespawnEvent`; a new location moves them.
    pub(super) fn plugin_respawned(&mut self, host: &mut LanHost, conn: u32) {
        if self.remote.is_some() || !self.mods.mods.runs_plugins() {
            return;
        }
        if let Some((level, progress, total)) = self.mods.plugin.kept_levels.remove(&conn)
            && let Some(g) = host.hub.guest_mut(conn)
        {
            g.xp_level = level;
            g.xp_progress = progress;
            g.xp_total = total;
        }
        if !self.plugins_listen(EventKind::PlayerRespawn) {
            return;
        }
        let Some(player) = self.plugin_player(Some(host), conn) else {
            return;
        };
        if player.dead {
            return;
        }
        let at = player.location.clone();
        let back = self.plugin_fire(
            Some(host),
            Event::PlayerRespawn(ev::PlayerRespawnEvent {
                player: player.clone(),
                location: at.clone(),
            }),
        );
        if let Event::PlayerRespawn(e) = back
            && e.location != at
            && let Some(uuid) = parse_uuid(&player.id)
        {
            self.mods
                .plugin
                .actions
                .push_back(Action::Teleport(uuid, e.location));
        }
    }

    /// Damage about to be dealt: `EntityDamageEvent`. The amount to deal,
    /// or None when a plugin cancelled it.
    pub(super) fn plugin_damage(
        &mut self,
        host: Option<&mut LanHost>,
        victim: types::EntityRef,
        cause: &str,
        amount: f32,
        damager: Option<types::EntityRef>,
    ) -> Option<f32> {
        if self.remote.is_some() || !self.plugins_listen(EventKind::EntityDamage) {
            return Some(amount);
        }
        let back = self.plugin_fire(
            host,
            Event::EntityDamage(ev::EntityDamageEvent {
                entity: victim,
                cause: cause.to_owned(),
                damage: amount,
                damager,
                cancelled: false,
            }),
        );
        match back {
            Event::EntityDamage(e) if e.cancelled => None,
            Event::EntityDamage(e) if e.damage.is_finite() => Some(e.damage.max(0.0)),
            _ => Some(amount),
        }
    }

    /// A block of this level as a plugin reads it.
    fn plugin_level_block(&self, pos: horizon_core::position::BlockPos) -> types::Block {
        self.plugin_block(self.dim.id(), [pos.x, pos.y, pos.z])
    }

    /// A piston about to move: `BlockPistonExtendEvent` or
    /// `BlockPistonRetractEvent`. False when a plugin cancelled it.
    pub(super) fn plugin_piston(&mut self, mv: &horizon_world::level::PistonMove) -> bool {
        let block = self.plugin_level_block(mv.pos);
        let e = ev::BlockPistonEvent {
            block,
            direction: face_name(mv.direction as u8).into(),
            sticky: mv.sticky,
            blocks: mv
                .blocks
                .iter()
                .map(|p| types::BlockPos {
                    x: p.x,
                    y: p.y,
                    z: p.z,
                })
                .collect(),
            cancelled: false,
        };
        let event = if mv.extending {
            Event::BlockPistonExtend(e)
        } else {
            Event::BlockPistonRetract(e)
        };
        !plugin::is_cancelled(&self.plugin_fire(None, event))
    }

    /// A fluid about to flow: `BlockFromToEvent` for each block it flows
    /// into. The blocks the plugins kept it out of.
    pub(super) fn plugin_fluid_flow(
        &mut self,
        flow: &horizon_world::level::FluidFlow,
    ) -> Vec<horizon_core::position::BlockPos> {
        let mut vetoes = Vec::new();
        let from = self.plugin_level_block(flow.from);
        let ask = |game: &mut Self, to: horizon_core::position::BlockPos| {
            let d = [to.x - flow.from.x, to.y - flow.from.y, to.z - flow.from.z];
            let face = (0..6u8).find(|&f| face_offset(f) == d).unwrap_or(0);
            let back = game.plugin_fire(
                None,
                Event::BlockFromTo(ev::BlockFromToEvent {
                    block: from.clone(),
                    to: game.plugin_level_block(to),
                    face: face_name(face).into(),
                    cancelled: false,
                }),
            );
            plugin::is_cancelled(&back)
        };
        if let Some(down) = flow.down
            && ask(self, down)
        {
            // FlowingFluid.spread stops there: no flow to the sides.
            return vec![down];
        }
        for &to in &flow.sides {
            if ask(self, to) {
                vetoes.push(to);
            }
        }
        vetoes
    }

    /// A change the level held back for the plugins: `LeavesDecayEvent`,
    /// `BlockBurnEvent`, `BlockFadeEvent` (snow melting) or `BlockFormEvent`
    /// (lava meeting water). True when it may happen.
    pub(super) fn plugin_pending(&mut self, change: horizon_world::level::PendingChange) -> bool {
        use horizon_world::level::PendingChange as P;
        let (pos, _) = change.at();
        let block = self.plugin_level_block(pos);
        let event = match change {
            P::LeavesDecay { .. } => Event::LeavesDecay(ev::LeavesDecayEvent {
                block,
                cancelled: false,
            }),
            P::Burn { source, .. } => Event::BlockBurn(ev::BlockBurnEvent {
                block,
                source: self.plugin_level_block(source),
                cancelled: false,
            }),
            P::SnowMelt { .. } => Event::BlockFade(ev::BlockGrowEvent {
                block,
                new_state: "minecraft:air".into(),
                cancelled: false,
            }),
            P::Form { into, .. } => Event::BlockForm(ev::BlockGrowEvent {
                block,
                new_state: self.state_string(into),
                cancelled: false,
            }),
        };
        !plugin::is_cancelled(&self.plugin_fire(None, event))
    }

    /// Bone meal about to be used on `target`: `BlockFertilizeEvent`, with
    /// the player whose turn it is. False when a plugin cancelled it.
    pub(super) fn plugin_fertilize(&mut self, target: horizon_core::position::BlockPos) -> bool {
        let player = self.plugin_acting_player();
        self.plugin_fertilize_by(player, target)
    }

    /// `BlockFertilizeEvent` by `player`, or a dispenser (None).
    pub(super) fn plugin_fertilize_by(
        &mut self,
        player: Option<types::Player>,
        target: horizon_core::position::BlockPos,
    ) -> bool {
        let block = self.plugin_level_block(target);
        !plugin::is_cancelled(&self.plugin_fire(
            None,
            Event::BlockFertilize(ev::BlockFertilizeEvent {
                player,
                block,
                cancelled: false,
            }),
        ))
    }

    /// Blocks changed by themselves: `BlockGrowEvent`, `BlockSpreadEvent`,
    /// `BlockFormEvent` or `BlockFadeEvent`. A cancelled change puts back the
    /// blocks it set (those nothing changed since).
    pub(super) fn plugin_block_changed(
        &mut self,
        cause: horizon_world::level::BlockChange,
        changes: &[(
            horizon_core::position::BlockPos,
            horizon_world::registry::BlockStateId,
            horizon_world::registry::BlockStateId,
        )],
    ) {
        let Some(&(pos, old, new)) = changes.first() else {
            return;
        };
        let mut block = self.plugin_level_block(pos);
        block.state = self.state_string(old);
        use horizon_world::level::BlockChange as C;
        let new_state = self.state_string(new);
        let event = match cause {
            C::Grow => Event::BlockGrow(ev::BlockGrowEvent {
                block,
                new_state,
                cancelled: false,
            }),
            C::Form => Event::BlockForm(ev::BlockGrowEvent {
                block,
                new_state,
                cancelled: false,
            }),
            C::Fade => Event::BlockFade(ev::BlockGrowEvent {
                block,
                new_state,
                cancelled: false,
            }),
            C::Spread { source } => Event::BlockSpread(ev::BlockSpreadEvent {
                block,
                source: self.plugin_level_block(source),
                new_state,
                cancelled: false,
            }),
        };
        let back = self.plugin_fire(None, event);
        if !plugin::is_cancelled(&back) {
            return;
        }
        // No events for the blocks put back.
        let was = std::mem::replace(&mut self.mods.plugin.reverting, true);
        for &(pos, old, new) in changes.iter().rev() {
            if self.world.block(pos.x, pos.y, pos.z) == new {
                let _ = self.level_set([pos.x, pos.y, pos.z], old);
            }
        }
        self.mods.plugin.reverting = was;
    }

    /// The damage the mobs' surroundings dealt them in their ticks (fire,
    /// lava, drowning, suffocation, the void, harmful effects), held back in
    /// `Mob::env_damage`: `EntityDamageEvent` for each hit, then the hit.
    /// The mobs it kills go to `dead`.
    pub(super) fn plugin_env_damage(&mut self, dead: &mut Vec<horizon_client::mobs::Mob>) {
        let held: Vec<_> = self
            .mobs
            .iter_mut()
            .filter(|m| !m.env_damage.is_empty())
            .map(|m| (m.id, std::mem::take(&mut m.env_damage)))
            .collect();
        for (id, hits) in held {
            for (amount, kind) in hits {
                let cause = super::creatures::damage_type(kind, false);
                let victim = self.plugin_entity_ref(id as i32);
                let Some(amount) = self.plugin_damage(None, victim, cause, amount, None) else {
                    continue;
                };
                // A plugin may have removed or killed it meanwhile.
                let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
                    break;
                };
                if !m.alive() || m.removed {
                    break;
                }
                m.damage(amount, horizon_client::mobs::Damage::of(kind));
                if !m.alive() && self.died.insert(id) {
                    dead.push(m.clone());
                }
            }
        }
    }

    /// The damage the surroundings dealt the player whose turn it is (fire,
    /// lava, magma, cactus, berry bushes, campfires, freezing, drowning,
    /// suffocation, the void), held back in its motion: `EntityDamageEvent`
    /// for each hit, then the hit.
    pub(super) fn plugin_player_env_damage(&mut self) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        player.defer_env_damage = false;
        let held = std::mem::take(&mut player.env_damage);
        for (cause, amount) in held {
            let Some(amount) = self.plugin_damage_acting(cause, amount, None) else {
                continue;
            };
            let (rules, health) = (self.world.health_rules, &self.motion.health);
            if let Some(player) = self.player.as_mut()
                && let Some(id) = health.damage.id(cause)
            {
                let _ = player.hurt(health, rules, id, amount);
            }
        }
    }

    /// Damage to the player whose turn it is (a guest acting), of damage
    /// type `cause`: the amount to deal, or None when cancelled.
    pub(super) fn plugin_damage_acting(
        &mut self,
        cause: &str,
        amount: f32,
        damager: Option<i32>,
    ) -> Option<f32> {
        if self.remote.is_some() || !self.plugins_listen(EventKind::EntityDamage) {
            return Some(amount);
        }
        let Some(player) = self.plugin_acting_player() else {
            return Some(amount);
        };
        let victim = Self::plugin_player_ref(&player);
        let damager = damager.map(|id| self.plugin_entity_ref(id));
        self.plugin_damage(None, victim, cause, amount, damager)
    }

    /// A guest attacks the entity `id` (a mob or another player): the
    /// damage event before the hit. False when a plugin cancelled it.
    pub(super) fn plugin_attack(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        id: i32,
        amount: f32,
    ) -> Option<f32> {
        if self.remote.is_some() || !self.plugins_listen(EventKind::EntityDamage) {
            return Some(amount);
        }
        let attacker = self.plugin_player(Some(host), conn)?;
        let victim = match host.hub.guests.iter().find(|g| g.entity_id == id) {
            Some(g) => types::EntityRef {
                id,
                kind: "minecraft:player".into(),
                player: Some(uuid_str(g.uuid)),
            },
            None => self.plugin_entity_ref(id),
        };
        let damager = Self::plugin_player_ref(&attacker);
        self.plugin_damage(
            Some(host),
            victim,
            "minecraft:player_attack",
            amount,
            Some(damager),
        )
    }

    /// A mob about to come into the world: `EntitySpawnEvent`. False when
    /// a plugin cancelled it.
    pub(super) fn plugin_spawn_allowed(
        &mut self,
        mob: &horizon_client::mobs::Mob,
        reason: &str,
    ) -> bool {
        if self.remote.is_some() || !self.plugins_listen(EventKind::EntitySpawn) {
            return true;
        }
        let kind = horizon_client::command::args::normalize(&mob.kind.id);
        let back = self.plugin_fire(
            None,
            Event::EntitySpawn(ev::EntitySpawnEvent {
                entity: types::EntityRef {
                    id: mob.id as i32,
                    kind,
                    player: None,
                },
                location: types::Location {
                    world: self.dim.id().to_owned(),
                    x: mob.position.x,
                    y: mob.position.y,
                    z: mob.position.z,
                    yaw: mob.yaw,
                    pitch: 0.0,
                },
                reason: reason.to_owned(),
                cancelled: false,
            }),
        );
        !plugin::is_cancelled(&back)
    }

    /// An entity died: `EntityDeathEvent`.
    pub(super) fn plugin_entity_died(&mut self, entity: types::EntityRef, at: [f64; 3], exp: u32) {
        if self.remote.is_some() || !self.plugins_listen(EventKind::EntityDeath) {
            return;
        }
        self.plugin_fire(
            None,
            Event::EntityDeath(ev::EntityDeathEvent {
                entity,
                location: types::Location {
                    world: self.dim.id().to_owned(),
                    x: at[0],
                    y: at[1],
                    z: at[2],
                    yaw: 0.0,
                    pitch: 0.0,
                },
                drops: Vec::new(),
                exp,
            }),
        );
    }

    /// An explosion about to happen at `center`, destroying `blocks`:
    /// `EntityExplodeEvent`. False when cancelled; `blocks` keeps only those
    /// the plugins left.
    pub(super) fn plugin_explosion(
        &mut self,
        center: [f64; 3],
        blocks: &mut Vec<[i32; 3]>,
    ) -> bool {
        if self.remote.is_some() || !self.plugins_listen(EventKind::EntityExplode) {
            return true;
        }
        let list = blocks
            .iter()
            .map(|p| types::BlockPos {
                x: p[0],
                y: p[1],
                z: p[2],
            })
            .collect();
        let back = self.plugin_fire(
            None,
            Event::EntityExplode(ev::EntityExplodeEvent {
                entity: None,
                location: types::Location {
                    world: self.dim.id().to_owned(),
                    x: center[0],
                    y: center[1],
                    z: center[2],
                    yaw: 0.0,
                    pitch: 0.0,
                },
                blocks: list,
                cancelled: false,
            }),
        );
        match back {
            Event::EntityExplode(e) if e.cancelled => false,
            Event::EntityExplode(e) => {
                let keep: std::collections::BTreeSet<[i32; 3]> =
                    e.blocks.iter().map(|b| [b.x, b.y, b.z]).collect();
                blocks.retain(|p| keep.contains(p));
                true
            }
            _ => true,
        }
    }

    /// The player whose turn it is breaks `pos`: `BlockBreakEvent`. False
    /// when cancelled (the block is sent again); a plugin that turns the
    /// drops off sets `no_drops` for the break.
    pub(super) fn plugin_block_break(&mut self, pos: [i32; 3]) -> bool {
        self.mods.plugin.no_drops = false;
        if self.remote.is_some() || !self.plugins_listen(EventKind::BlockBreak) {
            return true;
        }
        let Some(player) = self.plugin_acting_player() else {
            return true;
        };
        let world = self.dim.id().to_owned();
        let block = self.plugin_block(&world, pos);
        let back = self.plugin_fire(
            None,
            Event::BlockBreak(ev::BlockBreakEvent {
                player,
                block,
                drop_items: true,
                exp: 0,
                cancelled: false,
            }),
        );
        match back {
            Event::BlockBreak(e) if e.cancelled => {
                self.mods.plugin.resend_blocks.push((world, pos));
                if let Some(conn) = self.lan_acting {
                    self.mods.plugin.resync.insert(conn);
                }
                false
            }
            Event::BlockBreak(e) => {
                self.mods.plugin.no_drops = !e.drop_items;
                true
            }
            _ => true,
        }
    }

    /// The player whose turn it is placed `state` at `pos` over `replaced`:
    /// `BlockPlaceEvent`. False when cancelled: the old block is put back.
    pub(super) fn plugin_block_place(
        &mut self,
        pos: [i32; 3],
        replaced: horizon_world::registry::BlockStateId,
        item: Option<types::ItemStack>,
    ) -> bool {
        if self.remote.is_some() || !self.plugins_listen(EventKind::BlockPlace) {
            return true;
        }
        let Some(player) = self.plugin_acting_player() else {
            return true;
        };
        let world = self.dim.id().to_owned();
        let block = self.plugin_block(&world, pos);
        let replaced_name = self.state_string(replaced);
        let back = self.plugin_fire(
            None,
            Event::BlockPlace(ev::BlockPlaceEvent {
                player,
                block,
                replaced: replaced_name,
                item,
                hand: types::Hand::Main,
                cancelled: false,
            }),
        );
        if !plugin::is_cancelled(&back) {
            return true;
        }
        let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| b.set_block(level, ticks, at, replaced, 3));
        self.mods.plugin.resend_blocks.push((world, pos));
        if let Some(conn) = self.lan_acting {
            self.mods.plugin.resync.insert(conn);
        }
        false
    }

    /// A stack a plugin may read from a game stack.
    pub(super) fn plugin_stack(
        s: Option<&horizon_client::items::ItemStack>,
    ) -> Option<types::ItemStack> {
        s.map(stack_out)
    }
    #[allow(dead_code)]
    fn plugin_stack_in(s: &types::ItemStack) -> Option<horizon_client::items::ItemStack> {
        stack_in(s)
    }
}
