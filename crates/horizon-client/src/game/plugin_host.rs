//! What plugins read and change of the server (docs/plugins.md): the
//! `plugin-*` host interfaces on [`PluginCtx`], the game lent to one call.
//!
//! Reads come from the game as it is. Changes a player's client must see
//! at once (messages, titles, sounds) go out at once when the player's
//! connection is in hand; the others are queued as [`Action`]s and made
//! between two ticks, mostly through the vanilla commands that already make
//! them for every client.
use super::Game;
use super::commands::Target;
use super::plugins::{
    Action, Bar, PluginCtx, json_text, mode_name, name_tag_packet, num, snbt, stack_in, stack_out,
    stacks_out, tag_json, text_tag,
};
use super::plugins::{parse_uuid, uuid_str};
use horizon_mods::plugin::{
    PluginHost, bossbars, entities, inventories, items, players, scoreboards, server, types, worlds,
};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_world::nbt::{JavaString, Tag};

/// The dimensions a server has, in `MinecraftServer.levels` order.
const DIMENSIONS: [&str; 3] = [
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

fn id_of(text: &str) -> String {
    if text.contains(':') {
        text.to_owned()
    } else {
        format!("minecraft:{text}")
    }
}

impl PluginCtx<'_, '_> {
    fn uuid(&self, id: &str) -> Option<u128> {
        parse_uuid(id)
    }
    /// A command about a player, by name, at the tick's end.
    fn player_command(&mut self, id: &str, f: impl FnOnce(&str) -> String) -> bool {
        let Some(name) = self.uuid(id).and_then(|u| self.name_of(u)) else {
            return false;
        };
        let line = f(&name);
        self.command(line);
        true
    }
    /// The world a dimension name means, or None for one the server hasn't.
    fn dim(world: &str) -> Option<super::Dim> {
        DIMENSIONS
            .contains(&world)
            .then(|| super::Dim::from_id(world))
    }
    fn guest_ref(&self, uuid: u128) -> Option<&horizon_client::lan::guest::Guest> {
        self.lan_ref()?
            .hub
            .guests
            .iter()
            .find(|g| g.uuid == uuid && g.placed)
    }
    /// A guest's inventory in plugin numbering, from wherever it is now.
    fn inventory_of(&self, uuid: u128) -> Option<Vec<Option<horizon_client::items::ItemStack>>> {
        let game = &*self.game;
        let (inv, armor, off) = if game.lan_acting_uuid == Some(uuid) && game.player.is_some() {
            (&game.inventory, &game.armor, &game.offhand)
        } else {
            let g = self.guest_ref(uuid)?;
            (&g.inventory, &g.armor, &g.offhand)
        };
        let mut out: Vec<_> = inv.iter().take(36).cloned().collect();
        out.resize(36, None);
        // Feet, legs, chest, head.
        for k in (0..4).rev() {
            out.push(armor[k].clone());
        }
        out.push(off.clone());
        Some(out)
    }
    fn max_stack(&self, item: &str) -> u32 {
        self.game
            .res
            .items()
            .get(item)
            .map_or(64, |i| i.max_stack.max(1))
    }
}

// ---------------------------------------------------------------------------
// The server.

impl PluginHost for PluginCtx<'_, '_> {
    fn caller(&mut self, plugin: &str) {
        if self.game.mods.plugin.caller != plugin {
            self.game.mods.plugin.caller = plugin.to_owned();
        }
    }
    fn server_info(&mut self) -> server::ServerInfo {
        let game = &*self.game;
        let (motd, max, port, online, dedicated) = match self.lan_ref() {
            Some(h) => (
                h.hub.lan.shared.motd(),
                h.hub.lan.shared.max_players() as u32,
                u32::from(h.hub.lan.port),
                h.hub.lan.shared.config.online.is_some(),
                h.hub.config.dedicated && !h.hub.config.integrated,
            ),
            None => (String::new(), 0, 0, false, false),
        };
        server::ServerInfo {
            name: "Horizon".into(),
            version: horizon_core::version::VERSION_ID.into(),
            protocol: horizon_protocol::PROTOCOL_VERSION as u32,
            motd: game.mods.plugin.motd.clone().unwrap_or(motd),
            max_players: game.mods.plugin.max_players.unwrap_or(max),
            online_mode: online,
            port,
            view_distance: game.settings.render_distance.max(0) as u32,
            simulation_distance: game.settings.simulation_distance.max(0) as u32,
            hardcore: game.level.hardcore,
            dedicated,
            tick: game.ticks,
        }
    }
    fn tps(&mut self) -> (f64, f64, f64) {
        let starts = &self.game.mods.plugin.tick_starts;
        let rate = |seconds: f64| -> f64 {
            let Some(last) = starts.back() else {
                return 20.0;
            };
            let window = std::time::Duration::from_secs_f64(seconds);
            let first = starts
                .iter()
                .find(|t| last.duration_since(**t) <= window)
                .copied()
                .unwrap_or(*last);
            let n = starts.iter().filter(|t| **t >= first).count();
            let span = last.duration_since(first).as_secs_f64();
            if n < 2 || span <= 0.0 {
                20.0
            } else {
                ((n - 1) as f64 / span).min(20.0)
            }
        };
        (rate(60.0), rate(300.0), rate(900.0))
    }
    fn mspt(&mut self) -> f64 {
        let l = &self.game.mods.plugin.tick_lengths;
        if l.is_empty() {
            0.0
        } else {
            l.iter().sum::<f64>() / l.len() as f64
        }
    }
    fn console(&mut self, message: &str) {
        let plain = horizon_mods::plugin::strip_codes(message);
        if self.game.options.dedicated.is_some() {
            super::dedicated::log("INFO", &plain);
        } else {
            log::info!("{plain}");
        }
    }
    fn dispatch_command(&mut self, line: String) {
        self.queue(Action::Console(line));
    }
    fn shutdown(&mut self) {
        self.queue(Action::Shutdown);
    }
    fn set_motd(&mut self, motd: String) {
        self.game.mods.plugin.motd = Some(motd.clone());
        if let Some(h) = self.lan() {
            h.hub.lan.shared.set_motd(super::plugins::amp_codes(&motd));
        }
    }
    fn set_max_players(&mut self, max: u32) {
        self.game.mods.plugin.max_players = Some(max);
        if let Some(h) = self.lan() {
            h.hub.config.max_players = max as usize;
            h.hub.lan.shared.set_max_players(max as usize);
        }
    }
    fn find_player(&mut self, name_or_id: &str) -> Option<server::OfflinePlayer> {
        if let Some(uuid) = parse_uuid(name_or_id) {
            if let Some(p) = self.player(uuid) {
                return Some(server::OfflinePlayer {
                    id: p.id,
                    name: p.name,
                    online: true,
                });
            }
            let name = super::dedicated::cached_name(uuid)?;
            return Some(server::OfflinePlayer {
                id: uuid_str(uuid),
                name,
                online: false,
            });
        }
        if let Some(p) = self
            .players()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name_or_id))
        {
            return Some(server::OfflinePlayer {
                id: p.id,
                name: p.name,
                online: true,
            });
        }
        let (uuid, name) = super::dedicated::cached_player(name_or_id)?;
        Some(server::OfflinePlayer {
            id: uuid_str(uuid),
            name,
            online: false,
        })
    }
    fn whitelist_enabled(&mut self) -> bool {
        self.game
            .mods
            .plugin
            .lists
            .as_ref()
            .and_then(|l| l.lock().ok().map(|l| l.use_whitelist))
            .unwrap_or(false)
    }
    fn set_whitelist_enabled(&mut self, on: bool) {
        self.queue(Action::Console(format!(
            "whitelist {}",
            if on { "on" } else { "off" }
        )));
    }
    fn whitelist(&mut self) -> Vec<String> {
        self.game
            .mods
            .plugin
            .lists
            .as_ref()
            .and_then(|l| {
                l.lock().ok().map(|l| {
                    l.white
                        .entries()
                        .iter()
                        .map(|e| e.user.name.clone())
                        .collect()
                })
            })
            .unwrap_or_default()
    }
    fn set_whitelisted(&mut self, name: String, on: bool) {
        self.queue(Action::Console(format!(
            "whitelist {} {name}",
            if on { "add" } else { "remove" }
        )));
    }
    fn operators(&mut self) -> Vec<String> {
        self.game
            .mods
            .plugin
            .lists
            .as_ref()
            .and_then(|l| {
                l.lock().ok().map(|l| {
                    l.ops
                        .entries()
                        .iter()
                        .map(|e| e.user.name.clone())
                        .collect()
                })
            })
            .unwrap_or_default()
    }
    fn banned_players(&mut self) -> Vec<String> {
        self.game
            .mods
            .plugin
            .lists
            .as_ref()
            .and_then(|l| {
                l.lock().ok().map(|l| {
                    l.bans
                        .entries()
                        .iter()
                        .map(|e| e.user.name.clone())
                        .collect()
                })
            })
            .unwrap_or_default()
    }
    fn ban(&mut self, name: String, reason: Option<String>) {
        let line = match reason {
            Some(r) => format!("ban {name} {r}"),
            None => format!("ban {name}"),
        };
        self.queue(Action::Console(line));
    }
    fn pardon(&mut self, name: String) {
        self.queue(Action::Console(format!("pardon {name}")));
    }
    fn ban_ip(&mut self, address: String, reason: Option<String>) {
        let line = match reason {
            Some(r) => format!("ban-ip {address} {r}"),
            None => format!("ban-ip {address}"),
        };
        self.queue(Action::Console(line));
    }
    fn pardon_ip(&mut self, address: String) {
        self.queue(Action::Console(format!("pardon-ip {address}")));
    }
    fn send_payload(&mut self, player: &str, channel: &str, data: Vec<u8>) -> bool {
        let Some(uuid) = parse_uuid(player).filter(|u| self.guest_index(*u).is_some()) else {
            return false;
        };
        self.send(
            uuid,
            C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload {
                channel: channel.to_owned(),
                data,
            }),
        );
        true
    }
}

// ---------------------------------------------------------------------------
// Players.

/// Points from one level to the next (`Player.getXpNeededForNextLevel`).
fn xp_needed(level: u32) -> u32 {
    if level >= 30 {
        112 + (level - 30) * 9
    } else if level >= 15 {
        37 + (level - 15) * 5
    } else {
        7 + level * 2
    }
}

impl players::Host for PluginCtx<'_, '_> {
    fn online(&mut self) -> Vec<types::Player> {
        self.players()
    }
    fn get(&mut self, id: String) -> Option<types::Player> {
        self.player(parse_uuid(&id)?)
    }
    fn by_name(&mut self, name: String) -> Option<types::Player> {
        self.players()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(&name))
    }
    fn send_message(&mut self, id: String, message: String) {
        if let Some(uuid) = parse_uuid(&id) {
            self.send(
                uuid,
                C::SystemChat(play::SystemChat {
                    content: text_tag(&message),
                    overlay: false,
                }),
            );
        }
    }
    fn send_action_bar(&mut self, id: String, message: String) {
        if let Some(uuid) = parse_uuid(&id) {
            self.send(
                uuid,
                C::SystemChat(play::SystemChat {
                    content: text_tag(&message),
                    overlay: true,
                }),
            );
        }
    }
    fn send_title(
        &mut self,
        id: String,
        title: String,
        subtitle: String,
        fade_in: u32,
        stay: u32,
        fade_out: u32,
    ) {
        let Some(uuid) = parse_uuid(&id) else { return };
        self.send(
            uuid,
            C::SetTitlesAnimation(play::TitlesAnimation {
                fade_in: fade_in as i32,
                stay: stay as i32,
                fade_out: fade_out as i32,
            }),
        );
        if !subtitle.is_empty() {
            self.send(
                uuid,
                C::SetSubtitleText(play::Text {
                    text: text_tag(&subtitle),
                }),
            );
        }
        self.send(
            uuid,
            C::SetTitleText(play::Text {
                text: text_tag(&title),
            }),
        );
    }
    fn clear_title(&mut self, id: String) {
        if let Some(uuid) = parse_uuid(&id) {
            self.send(uuid, C::ClearTitles(play::Bool1 { value: false }));
        }
    }
    fn set_tab_list(&mut self, id: String, header: String, footer: String) {
        if let Some(uuid) = parse_uuid(&id) {
            self.send(
                uuid,
                C::TabList(play::TabList {
                    header: text_tag(&header),
                    footer: text_tag(&footer),
                }),
            );
        }
    }
    fn set_list_name(&mut self, id: String, name: Option<String>) {
        let Some(uuid) = parse_uuid(&id) else { return };
        match &name {
            Some(n) => {
                self.game.mods.plugin.list_names.insert(uuid, n.clone());
            }
            None => {
                self.game.mods.plugin.list_names.remove(&uuid);
            }
        }
        let packet = C::PlayerInfoUpdate(play::PlayerInfoUpdate {
            actions: 32,
            entries: vec![play::PlayerInfo {
                uuid,
                display_name: Some(name.as_deref().map(text_tag)),
                ..Default::default()
            }],
        });
        let everyone: Vec<u128> = self
            .players()
            .iter()
            .filter_map(|p| parse_uuid(&p.id))
            .collect();
        for to in everyone {
            self.send(to, packet.clone());
        }
    }
    fn set_display_name(&mut self, id: String, name: Option<String>) {
        let Some(uuid) = parse_uuid(&id) else { return };
        match name {
            Some(n) => self.game.mods.plugin.display_names.insert(uuid, n),
            None => self.game.mods.plugin.display_names.remove(&uuid),
        };
    }
    fn play_sound(
        &mut self,
        id: String,
        sound: String,
        category: String,
        volume: f32,
        pitch: f32,
        at: Option<types::Location>,
    ) {
        let sound = id_of(&sound);
        let category = if category.is_empty() {
            "master".to_owned()
        } else {
            category
        };
        self.player_command(&id, |name| match &at {
            Some(l) => format!(
                "execute in {} run playsound {sound} {category} {name} {} {} {} {} {}",
                l.world,
                num(l.x),
                num(l.y),
                num(l.z),
                num(f64::from(volume)),
                num(f64::from(pitch))
            ),
            None => format!(
                "execute at {name} run playsound {sound} {category} {name} ~ ~ ~ {} {}",
                num(f64::from(volume)),
                num(f64::from(pitch))
            ),
        });
    }
    fn stop_sound(&mut self, id: String, sound: Option<String>) {
        if let Some(uuid) = parse_uuid(&id) {
            self.send(
                uuid,
                C::StopSound(play::StopSound {
                    source: None,
                    sound: sound.as_deref().map(id_of),
                }),
            );
        }
    }
    fn spawn_particle(
        &mut self,
        id: String,
        particle: String,
        at: types::Location,
        count: u32,
        offset: types::Vec3,
        speed: f32,
    ) {
        let particle = id_of(&particle);
        self.player_command(&id, |name| {
            format!(
                "execute in {} run particle {particle} {} {} {} {} {} {} {} {count} force {name}",
                at.world,
                num(at.x),
                num(at.y),
                num(at.z),
                num(offset.x),
                num(offset.y),
                num(offset.z),
                num(f64::from(speed))
            )
        });
    }
    fn send_block_change(&mut self, id: String, pos: types::BlockPos, state: String) {
        let Some(uuid) = parse_uuid(&id) else { return };
        let registry = self.game.pack.assets.registry;
        let Ok(state) = super::render_control::parse_state(registry, &state) else {
            return;
        };
        self.send(
            uuid,
            C::BlockUpdate(play::BlockUpdate {
                pos: [pos.x, pos.y, pos.z],
                state: state.0 as i32,
            }),
        );
    }
    fn kick(&mut self, id: String, reason: String) -> bool {
        let Some(uuid) = parse_uuid(&id).filter(|u| self.guest_index(*u).is_some()) else {
            return false;
        };
        self.queue(Action::Kick(uuid, text_tag(&reason)));
        true
    }
    fn transfer(&mut self, id: String, host: String, port: u32) {
        if let Some(uuid) = parse_uuid(&id) {
            self.send(
                uuid,
                C::Transfer(horizon_protocol::packets::configuration::Transfer {
                    host,
                    port: port as i32,
                }),
            );
        }
    }
    fn send_resource_pack(
        &mut self,
        id: String,
        url: String,
        hash: String,
        required: bool,
        prompt: Option<String>,
    ) {
        let Some(uuid) = parse_uuid(&id) else { return };
        let pack = horizon_protocol::crypto::offline_uuid(&url);
        self.send(
            uuid,
            C::ResourcePackPush(horizon_protocol::packets::configuration::ResourcePackPush {
                id: pack,
                url,
                // A SHA-1 in hex is 40 characters; anything else isn't one.
                hash: if hash.len() == 40 {
                    hash
                } else {
                    String::new()
                },
                required,
                prompt: prompt.as_deref().map(text_tag),
            }),
        );
    }
    fn teleport(&mut self, id: String, to: types::Location) -> bool {
        let Some(uuid) = parse_uuid(&id).filter(|u| self.guest_index(*u).is_some()) else {
            return false;
        };
        if Self::dim(&to.world).is_none() {
            return false;
        }
        self.queue(Action::Teleport(uuid, to));
        true
    }
    fn set_game_mode(&mut self, id: String, mode: types::GameMode) {
        let mode = mode_name(mode);
        self.player_command(&id, |name| format!("gamemode {mode} {name}"));
    }
    fn set_health(&mut self, id: String, health: f32) {
        if let Some(uuid) = parse_uuid(&id) {
            if health <= 0.0 {
                self.player_command(&id, |name| format!("kill {name}"));
            } else {
                self.queue(Action::Health(uuid, health));
            }
        }
    }
    fn set_max_health(&mut self, id: String, health: f64) {
        self.player_command(&id, |name| {
            format!(
                "attribute {name} minecraft:max_health base set {}",
                num(health)
            )
        });
    }
    fn set_food(&mut self, id: String, food: u32) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::Food(uuid, food));
        }
    }
    fn set_saturation(&mut self, id: String, saturation: f32) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::Saturation(uuid, saturation));
        }
    }
    fn set_level(&mut self, id: String, level: u32) {
        self.player_command(&id, |name| format!("xp set {name} {level} levels"));
    }
    fn set_exp(&mut self, id: String, progress: f32) {
        let level = self
            .uuid(&id)
            .and_then(|u| self.player(u))
            .map_or(0, |p| p.level);
        let points = (progress.clamp(0.0, 0.9999) * xp_needed(level) as f32) as u32;
        self.player_command(&id, |name| format!("xp set {name} {points} points"));
    }
    fn give_exp(&mut self, id: String, points: i32) {
        self.player_command(&id, |name| format!("xp add {name} {points} points"));
    }
    fn set_allow_flight(&mut self, id: String, allow: bool) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::Flight(uuid, Some(allow), None));
        }
    }
    fn set_flying(&mut self, id: String, flying: bool) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::Flight(uuid, None, Some(flying)));
        }
    }
    fn set_walk_speed(&mut self, id: String, speed: f32) {
        self.player_command(&id, |name| {
            format!(
                "attribute {name} minecraft:movement_speed base set {}",
                num(f64::from(speed))
            )
        });
    }
    fn set_fly_speed(&mut self, id: String, speed: f32) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::FlySpeed(uuid, speed));
        }
    }
    fn set_op(&mut self, id: String, op: bool) {
        let Some(name) = self.uuid(&id).and_then(|u| self.name_of(u)) else {
            return;
        };
        self.queue(Action::Console(format!(
            "{} {name}",
            if op { "op" } else { "deop" }
        )));
    }
    fn set_velocity(&mut self, id: String, velocity: types::Vec3) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::Velocity(uuid, [velocity.x, velocity.y, velocity.z]));
        }
    }
    fn damage(&mut self, id: String, amount: f32, cause: Option<String>) {
        let cause = id_of(cause.as_deref().unwrap_or("generic"));
        self.player_command(&id, |name| {
            format!("damage {name} {} {cause}", num(f64::from(amount)))
        });
    }
    fn kill(&mut self, id: String) {
        self.player_command(&id, |name| format!("kill {name}"));
    }
    fn set_fire_ticks(&mut self, id: String, ticks: u32) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::Fire(uuid, ticks));
        }
    }
    fn effects(&mut self, id: String) -> Vec<players::Effect> {
        let Some(uuid) = parse_uuid(&id) else {
            return Vec::new();
        };
        let list: Vec<horizon_client::effects::EffectInstance> =
            if self.game.lan_acting_uuid == Some(uuid) {
                self.game.potions.effects.list.clone()
            } else {
                self.guest_ref(uuid)
                    .map(|g| g.effects.list.clone())
                    .unwrap_or_default()
            };
        list.into_iter()
            .map(|e| players::Effect {
                kind: format!("minecraft:{}", e.id),
                duration: e.duration,
                amplifier: e.amplifier.max(0) as u32,
                ambient: e.ambient,
                particles: e.visible,
                icon: true,
            })
            .collect()
    }
    fn add_effect(&mut self, id: String, effect: players::Effect) {
        let kind = id_of(&effect.kind);
        let seconds = if effect.duration < 0 {
            "infinite".to_owned()
        } else {
            (effect.duration.max(1) as u32).div_ceil(20).to_string()
        };
        let hide = !effect.particles;
        self.player_command(&id, |name| {
            format!(
                "effect give {name} {kind} {seconds} {} {hide}",
                effect.amplifier.min(255)
            )
        });
    }
    fn remove_effect(&mut self, id: String, kind: String) {
        let kind = id_of(&kind);
        self.player_command(&id, |name| format!("effect clear {name} {kind}"));
    }
    fn clear_effects(&mut self, id: String) {
        self.player_command(&id, |name| format!("effect clear {name}"));
    }
    fn hide_player(&mut self, viewer: String, target: String) {
        let (Some(v), Some(t)) = (parse_uuid(&viewer), parse_uuid(&target)) else {
            return;
        };
        if v == t || !self.game.mods.plugin.hidden.insert((v, t)) {
            return;
        }
        let entity = self.guest_ref(t).map(|g| g.entity_id);
        self.send(
            v,
            C::PlayerInfoRemove(play::PlayerInfoRemove { uuids: vec![t] }),
        );
        if let Some(e) = entity {
            self.send(v, C::RemoveEntities(play::RemoveEntities { ids: vec![e] }));
        }
        self.queue(Action::Retrack(v, t));
    }
    fn show_player(&mut self, viewer: String, target: String) {
        let (Some(v), Some(t)) = (parse_uuid(&viewer), parse_uuid(&target)) else {
            return;
        };
        if self.game.mods.plugin.hidden.remove(&(v, t)) {
            self.queue(Action::Retrack(v, t));
        }
    }
    fn can_see(&mut self, viewer: String, target: String) -> bool {
        match (parse_uuid(&viewer), parse_uuid(&target)) {
            (Some(v), Some(t)) => !self.game.mods.plugin.hidden.contains(&(v, t)),
            _ => true,
        }
    }
    fn perform_command(&mut self, id: String, line: String) {
        if let Some(uuid) = parse_uuid(&id) {
            let line = line.trim_start_matches('/').to_owned();
            self.queue(Action::PlayerCommand(uuid, line));
        }
    }
    fn chat(&mut self, id: String, message: String) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::PlayerChat(uuid, message));
        }
    }
    fn set_respawn_location(&mut self, id: String, at: Option<types::Location>) {
        match at {
            Some(l) => {
                self.player_command(&id, |name| {
                    format!(
                        "execute in {} run spawnpoint {name} {} {} {} {}",
                        l.world,
                        l.x.floor() as i64,
                        l.y.floor() as i64,
                        l.z.floor() as i64,
                        num(f64::from(l.yaw))
                    )
                });
            }
            None => {
                if let Some(uuid) = parse_uuid(&id) {
                    self.queue(Action::ClearSpawn(uuid));
                }
            }
        }
    }
    fn inventory(&mut self, id: String) -> Vec<Option<types::ItemStack>> {
        parse_uuid(&id)
            .and_then(|u| self.inventory_of(u))
            .map(|i| stacks_out(&i))
            .unwrap_or_default()
    }
    fn get_slot(&mut self, id: String, slot: u32) -> Option<types::ItemStack> {
        let inv = self.inventory_of(parse_uuid(&id)?)?;
        inv.get(slot as usize)?.as_ref().map(stack_out)
    }
    fn set_slot(&mut self, id: String, slot: u32, item: Option<types::ItemStack>) {
        if let Some(uuid) = parse_uuid(&id)
            && slot <= 40
        {
            self.queue(Action::Slot(uuid, slot, item.as_ref().and_then(stack_in)));
        }
    }
    fn give(&mut self, id: String, items: Vec<types::ItemStack>) -> Vec<types::ItemStack> {
        let Some(uuid) = parse_uuid(&id) else {
            return items;
        };
        let Some(mut inv) = self.inventory_of(uuid) else {
            return items;
        };
        inv.truncate(36);
        // What fits now, as `Inventory.add` would place it.
        let mut fits = Vec::new();
        let mut rest = Vec::new();
        for s in items.iter().filter_map(stack_in) {
            let max = self.max_stack(&s.item);
            let mut left = s.count;
            for slot in inv.iter_mut() {
                if left == 0 {
                    break;
                }
                match slot {
                    Some(have) if have.same_item_same_components(&s) && have.count < max => {
                        let moved = (max - have.count).min(left);
                        have.count += moved;
                        left -= moved;
                    }
                    None => {
                        let moved = left.min(max);
                        *slot = Some(horizon_client::items::ItemStack {
                            count: moved,
                            ..s.clone()
                        });
                        left -= moved;
                    }
                    _ => {}
                }
            }
            if left < s.count {
                fits.push(horizon_client::items::ItemStack {
                    count: s.count - left,
                    ..s.clone()
                });
            }
            if left > 0 {
                rest.push(stack_out(&horizon_client::items::ItemStack {
                    count: left,
                    ..s
                }));
            }
        }
        if !fits.is_empty() {
            self.queue(Action::Give(uuid, fits));
        }
        rest
    }
    fn remove_item(&mut self, id: String, item: String, count: u32) -> u32 {
        let Some(uuid) = parse_uuid(&id) else {
            return 0;
        };
        let item = id_of(&item);
        let have: u32 = self
            .inventory_of(uuid)
            .unwrap_or_default()
            .iter()
            .flatten()
            .filter(|s| *s.item == *item)
            .map(|s| s.count)
            .sum();
        let removed = have.min(count);
        if removed > 0 {
            self.queue(Action::RemoveItem(uuid, item, removed));
        }
        removed
    }
    fn clear_inventory(&mut self, id: String) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::ClearInventory(uuid));
        }
    }
    fn ender_chest(&mut self, id: String) -> Vec<Option<types::ItemStack>> {
        let Some(uuid) = parse_uuid(&id) else {
            return Vec::new();
        };
        if self.game.lan_acting_uuid == Some(uuid) {
            return stacks_out(&self.game.ender_items);
        }
        self.guest_ref(uuid)
            .map(|g| stacks_out(&g.ender_items))
            .unwrap_or_default()
    }
    fn set_ender_slot(&mut self, id: String, slot: u32, item: Option<types::ItemStack>) {
        if let Some(uuid) = parse_uuid(&id)
            && slot < 27
        {
            self.queue(Action::EnderSlot(
                uuid,
                slot,
                item.as_ref().and_then(stack_in),
            ));
        }
    }
    fn set_held_slot(&mut self, id: String, slot: u32) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::HeldSlot(uuid, slot.min(8)));
        }
    }
    fn cursor(&mut self, id: String) -> Option<types::ItemStack> {
        let uuid = parse_uuid(&id)?;
        if self.game.lan_acting_uuid == Some(uuid) {
            return self.game.carried.as_ref().map(stack_out);
        }
        self.guest_ref(uuid)?.carried.as_ref().map(stack_out)
    }
    fn set_cursor(&mut self, id: String, item: Option<types::ItemStack>) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::Cursor(uuid, item.as_ref().and_then(stack_in)));
        }
    }
    fn close_inventory(&mut self, id: String) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::CloseMenu(uuid));
        }
    }
    fn update_inventory(&mut self, id: String) {
        if let Some(uuid) = parse_uuid(&id) {
            self.queue(Action::UpdateInventory(uuid));
        }
    }
}

// ---------------------------------------------------------------------------
// Worlds.

impl PluginCtx<'_, '_> {
    /// The chunks of a world: the level in place's, or a parked one's.
    fn chunks(&mut self, world: &str) -> Option<&mut horizon_client::chunk_map::ChunkMap> {
        let dim = Self::dim(world)?;
        if dim == self.game.dim {
            return Some(&mut self.game.world.chunks);
        }
        self.game
            .levels
            .parked_chunks()
            .find(|(d, _)| *d == dim)
            .map(|(_, c)| c)
    }
    fn block_state(
        &mut self,
        world: &str,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<horizon_world::registry::BlockStateId> {
        let chunks = self.chunks(world)?;
        let chunk = chunks.get(&(x >> 4, z >> 4))?;
        Some(chunk.block(x & 15, y, z & 15))
    }
    fn world_info(&mut self, name: &str) -> Option<worlds::WorldInfo> {
        let dim = Self::dim(name)?;
        let loaded =
            dim == self.game.dim || self.game.levels.parked_chunks().any(|(d, _)| d == dim);
        if !loaded {
            return None;
        }
        let game = &*self.game;
        let spawn = game
            .level
            .world_spawn
            .map_or([0.5, 64.0, 0.5], |[x, y, z]| {
                [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5]
            });
        let (environment, min_y, max_y) = match name {
            "minecraft:the_nether" => (worlds::Environment::Nether, 0, 255),
            "minecraft:the_end" => (worlds::Environment::TheEnd, 0, 255),
            _ => (worlds::Environment::Normal, -64, 319),
        };
        let players = self
            .players()
            .iter()
            .filter(|p| p.location.world == name)
            .count() as u32;
        let game = &*self.game;
        let difficulty = match game.ui.difficulty {
            0 => "peaceful",
            1 => "easy",
            3 => "hard",
            _ => "normal",
        };
        let day = game.world.day_time.max(0) as u64;
        let weather = game.world.weather;
        let mut info = worlds::WorldInfo {
            name: name.to_owned(),
            environment,
            seed: game.level.seed,
            time: day % 24000,
            full_time: game.world.game_time.max(0) as u64,
            storm: weather.raining,
            thundering: weather.thundering,
            spawn: types::Location {
                world: if name == "minecraft:overworld" {
                    name.to_owned()
                } else {
                    "minecraft:overworld".to_owned()
                },
                x: spawn[0],
                y: spawn[1],
                z: spawn[2],
                yaw: 0.0,
                pitch: 0.0,
            },
            difficulty: difficulty.into(),
            min_y,
            max_y,
            players,
            loaded_chunks: 0,
        };
        info.loaded_chunks = self.chunks(name).map_or(0, |c| c.len() as u32);
        Some(info)
    }
}

impl worlds::Host for PluginCtx<'_, '_> {
    fn worlds(&mut self) -> Vec<worlds::WorldInfo> {
        DIMENSIONS
            .iter()
            .filter_map(|d| self.world_info(d))
            .collect()
    }
    fn get(&mut self, world: String) -> Option<worlds::WorldInfo> {
        self.world_info(&world)
    }
    fn get_block(&mut self, world: String, pos: types::BlockPos) -> Option<String> {
        let state = self.block_state(&world, pos.x, pos.y, pos.z)?;
        Some(self.game.state_string(state))
    }
    fn get_biome(&mut self, world: String, pos: types::BlockPos) -> Option<String> {
        let biomes = self.game.res.biomes().clone();
        let chunks = self.chunks(&world)?;
        let chunk = chunks.get(&(pos.x >> 4, pos.z >> 4))?;
        Some(biomes.get(chunk.biome(pos.x, pos.y, pos.z)).name.clone())
    }
    fn highest_block_y(&mut self, world: String, x: i32, z: i32) -> Option<i32> {
        let air = self.game.world.air;
        let top = horizon_client::chunk::MIN_Y + horizon_client::chunk::HEIGHT;
        let column: Vec<(i32, horizon_world::registry::BlockStateId)> = {
            let chunks = self.chunks(&world)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            (horizon_client::chunk::MIN_Y..top)
                .rev()
                .map(|y| (y, chunk.block(x & 15, y, z & 15)))
                .collect()
        };
        let facts = self.game.res.facts();
        column
            .into_iter()
            .find(|&(_, s)| s != air && !facts.is_air(s))
            .map(|(y, _)| y)
    }
    fn get_block_data(&mut self, world: String, pos: types::BlockPos) -> Option<String> {
        if Self::dim(&world)? != self.game.dim {
            return None;
        }
        let tag = self
            .game
            .data_of(&super::commands::data::DataRef::Block([
                pos.x, pos.y, pos.z,
            ]))
            .ok()?;
        Some(snbt(&tag))
    }
    fn merge_block_data(&mut self, world: String, pos: types::BlockPos, snbt_text: String) {
        if Self::dim(&world).is_some() {
            self.command(format!(
                "execute in {world} run data merge block {} {} {} {snbt_text}",
                pos.x, pos.y, pos.z
            ));
        }
    }
    fn set_blocks(
        &mut self,
        world: String,
        changes: Vec<worlds::BlockChange>,
    ) -> Result<(), String> {
        if Self::dim(&world).is_none() {
            return Err(format!("no world {world}"));
        }
        if changes.len() > 32768 {
            return Err(format!("{} blocks: at most 32768 per call", changes.len()));
        }
        let registry = self.game.pack.assets.registry;
        for c in &changes {
            super::render_control::parse_state(registry, &c.state)
                .map_err(|e| format!("{}: {e}", c.state))?;
        }
        let blocks = changes
            .into_iter()
            .map(|c| horizon_mods::BlockChange {
                x: c.pos.x,
                y: c.pos.y,
                z: c.pos.z,
                state: c.state,
            })
            .collect();
        self.queue(Action::Blocks(world, blocks));
        Ok(())
    }
    fn is_chunk_loaded(&mut self, world: String, x: i32, z: i32) -> bool {
        self.chunks(&world)
            .is_some_and(|c| c.get(&(x, z)).is_some())
    }
    fn loaded_chunks(&mut self, world: String) -> Vec<(i32, i32)> {
        self.chunks(&world)
            .map(|c| c.keys().copied().collect())
            .unwrap_or_default()
    }
    fn set_chunk_forced(&mut self, world: String, x: i32, z: i32, forced: bool) {
        if Self::dim(&world).is_some() {
            self.command(format!(
                "execute in {world} run forceload {} {} {}",
                if forced { "add" } else { "remove" },
                x * 16,
                z * 16
            ));
        }
    }
    fn set_time(&mut self, _world: String, time: u64) {
        self.command(format!("time set {time}"));
    }
    fn set_storm(&mut self, _world: String, storm: bool, duration: u32) {
        let kind = if storm { "rain" } else { "clear" };
        let line = if duration > 0 {
            format!("weather {kind} {}s", duration.div_ceil(20))
        } else {
            format!("weather {kind}")
        };
        self.command(line);
    }
    fn set_thundering(&mut self, _world: String, thundering: bool) {
        self.command(format!(
            "weather {}",
            if thundering { "thunder" } else { "rain" }
        ));
    }
    fn set_spawn(&mut self, at: types::Location) {
        self.command(format!(
            "execute in {} run setworldspawn {} {} {} {}",
            at.world,
            at.x.floor() as i64,
            at.y.floor() as i64,
            at.z.floor() as i64,
            num(f64::from(at.yaw))
        ));
    }
    fn set_difficulty(&mut self, difficulty: String) {
        self.command(format!("difficulty {difficulty}"));
    }
    fn game_rule(&mut self, rule: String) -> Option<String> {
        self.game.ui.rules.get(&rule).map(|v| v.to_string())
    }
    fn set_game_rule(&mut self, rule: String, value: String) {
        self.command(format!("gamerule {rule} {value}"));
    }
    fn strike_lightning(&mut self, at: types::Location, effect_only: bool) {
        if effect_only {
            // A bolt the server never acts on: the clients' flash and thunder.
            self.queue(Action::LightningEffect(at));
        } else {
            self.command(format!(
                "execute in {} run summon minecraft:lightning_bolt {} {} {}",
                at.world,
                num(at.x),
                num(at.y),
                num(at.z)
            ));
        }
    }
    fn explode(&mut self, at: types::Location, power: f32, fire: bool, break_blocks: bool) {
        self.queue(Action::Explode(at, power, fire, break_blocks));
    }
    fn drop_item(&mut self, at: types::Location, item: types::ItemStack, naturally: bool) {
        let Some(stack) = stack_in(&item) else { return };
        let nbt = snbt(&horizon_client::save::item_to_nbt(&stack));
        let motion = if naturally {
            let r = |k: u64| {
                ((self.game.ticks.wrapping_mul(2654435761).wrapping_add(k) % 200) as f64 / 1000.0)
                    - 0.1
            };
            format!(",Motion:[{}d,0.2d,{}d]", num(r(1)), num(r(7)))
        } else {
            ",Motion:[0d,0d,0d]".to_owned()
        };
        self.command(format!(
            "execute in {} run summon minecraft:item {} {} {} {{Item:{nbt},PickupDelay:10s{motion}}}",
            at.world,
            num(at.x),
            num(at.y),
            num(at.z)
        ));
    }
    fn play_sound(
        &mut self,
        at: types::Location,
        sound: String,
        category: String,
        volume: f32,
        pitch: f32,
    ) {
        let category = if category.is_empty() {
            "master".into()
        } else {
            category
        };
        self.command(format!(
            "execute in {} run playsound {} {category} @a {} {} {} {} {}",
            at.world,
            id_of(&sound),
            num(at.x),
            num(at.y),
            num(at.z),
            num(f64::from(volume)),
            num(f64::from(pitch))
        ));
    }
    fn spawn_particle(
        &mut self,
        at: types::Location,
        particle: String,
        count: u32,
        offset: types::Vec3,
        speed: f32,
        force: bool,
    ) {
        self.command(format!(
            "execute in {} run particle {} {} {} {} {} {} {} {} {count} {}",
            at.world,
            id_of(&particle),
            num(at.x),
            num(at.y),
            num(at.z),
            num(offset.x),
            num(offset.y),
            num(offset.z),
            num(f64::from(speed)),
            if force { "force" } else { "normal" }
        ));
    }
    fn border(&mut self, _world: String) -> worlds::WorldBorder {
        let b = &self.game.border;
        let [x, z] = b.center();
        worlds::WorldBorder {
            center_x: x,
            center_z: z,
            size: b.size(),
            damage_per_block: b.damage_per_block,
            warning_blocks: b.warning_distance.max(0) as u32,
        }
    }
    fn set_border(
        &mut self,
        _world: String,
        center_x: f64,
        center_z: f64,
        size: f64,
        seconds: u32,
    ) {
        self.command(format!(
            "worldborder center {} {}",
            num(center_x),
            num(center_z)
        ));
        self.command(if seconds > 0 {
            format!("worldborder set {} {seconds}", num(size))
        } else {
            format!("worldborder set {}", num(size))
        });
    }
    fn save(&mut self) {
        self.queue(Action::Console("save-all".into()));
    }
}

// ---------------------------------------------------------------------------
// Entities.

impl Game<'_> {
    /// Every entity of the level in place a plugin may name.
    pub(super) fn plugin_targets(&self) -> Vec<Target> {
        let mut out = Vec::new();
        if let Some(h) = self.lan.as_ref() {
            out.extend(
                h.hub
                    .guests
                    .iter()
                    .filter(|g| g.placed && g.dimension == self.dim.id())
                    .map(|g| Target::Guest(g.conn)),
            );
        }
        out.extend(self.mobs.iter().map(|m| Target::Mob(m.id)));
        out.extend(self.dropped.iter().map(|i| Target::Item(i.id)));
        out.extend(self.orbs.iter().map(|o| Target::Orb(o.id)));
        out.extend(self.creatures.projectiles.iter().map(|p| Target::Projectile(p.id)));
        out.extend(self.armor_stands.iter().map(|a| Target::Stand(a.id)));
        out.extend(self.item_frames.iter().map(|f| Target::Frame(f.id)));
        out.extend(self.paintings.iter().map(|p| Target::Painting(p.id)));
        out.extend(self.crystals.iter().map(|c| Target::Crystal(c.id)));
        if let Some(d) = &self.dragon {
            out.push(Target::Dragon(d.id));
        }
        out.extend(self.boats.iter().map(|b| Target::Boat(b.id)));
        out.extend(self.carts.iter().map(|c| Target::Cart(c.id)));
        out.extend(self.others.iter().map(|o| Target::Other(o.id)));
        out
    }
    /// The network id plugins know a target by.
    pub(super) fn plugin_target_id(&self, t: Target) -> Option<i32> {
        Some(match t {
            Target::Player => {
                let conn = self.lan_acting?;
                self.lan.as_ref()?.hub.guest(conn)?.entity_id
            }
            Target::Guest(conn) => self.lan.as_ref()?.hub.guest(conn)?.entity_id,
            Target::Mob(id)
            | Target::Item(id)
            | Target::Stand(id)
            | Target::Frame(id)
            | Target::Painting(id)
            | Target::Boat(id)
            | Target::Cart(id)
            | Target::Other(id)
            | Target::Crystal(id)
            | Target::Dragon(id) => id as i32,
            Target::Orb(id) => id as i32,
            Target::Projectile(id) => id as i32,
        })
    }
    /// The target with a network id, in the level in place.
    pub(super) fn plugin_target(&self, id: i32) -> Option<Target> {
        self.plugin_targets()
            .into_iter()
            .find(|t| self.plugin_target_id(*t) == Some(id))
    }
    /// An entity as a plugin reads it.
    pub(super) fn plugin_entity(&mut self, t: Target) -> Option<entities::Entity> {
        let id = self.plugin_target_id(t)?;
        let pos = self.target_position(t)?;
        let kind = self.target_type(t);
        let player = match t {
            Target::Guest(conn) => self.lan.as_ref()?.hub.guest(conn).map(|g| g.uuid),
            Target::Player => self.lan_acting_uuid,
            _ => None,
        };
        let nbt = if player.is_some() {
            None
        } else {
            self.entity_nbt(t)
        };
        let get = |k: &str| nbt.as_ref().and_then(|n| n.get(k));
        let uuid = match (player, get("UUID")) {
            (Some(u), _) => u,
            (None, Some(Tag::IntArray(w))) if w.len() == 4 => {
                (u128::from(w[0] as u32) << 96)
                    | (u128::from(w[1] as u32) << 64)
                    | (u128::from(w[2] as u32) << 32)
                    | u128::from(w[3] as u32)
            }
            _ => super::lan::entity_uuid(id as u32),
        };
        let f = |t: Option<&Tag>| -> Option<f64> {
            match t? {
                Tag::Float(v) => Some(f64::from(f32::from_bits(*v))),
                Tag::Double(v) => Some(f64::from_bits(*v)),
                Tag::Int(v) => Some(f64::from(*v)),
                _ => None,
            }
        };
        let list = |k: &str| -> Vec<f64> {
            match get(k) {
                Some(Tag::List(items)) => items.iter().filter_map(|t| f(Some(t))).collect(),
                _ => Vec::new(),
            }
        };
        let rotation = list("Rotation");
        let motion = list("Motion");
        let tags = match get("Tags") {
            Some(Tag::List(items)) => items
                .iter()
                .filter_map(|t| match t {
                    Tag::String(s) => Some(s.to_string_lossy()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let (mut yaw, mut pitch) = (
            rotation.first().copied().unwrap_or(0.0) as f32,
            rotation.get(1).copied().unwrap_or(0.0) as f32,
        );
        let mut health = f(get("Health")).map(|h| h as f32);
        if let Some(u) = player
            && let Some(p) = self
                .lan
                .as_ref()
                .and_then(|h| h.hub.guests.iter().find(|g| g.uuid == u))
        {
            yaw = p.yaw;
            pitch = p.pitch;
            health = Some(p.health);
        }
        Some(entities::Entity {
            id,
            uuid: uuid_str(uuid),
            kind,
            location: types::Location {
                world: self.dim.id().to_owned(),
                x: pos[0],
                y: pos[1],
                z: pos[2],
                yaw,
                pitch,
            },
            velocity: types::Vec3 {
                x: motion.first().copied().unwrap_or(0.0),
                y: motion.get(1).copied().unwrap_or(0.0),
                z: motion.get(2).copied().unwrap_or(0.0),
            },
            custom_name: get("CustomName").map(|t| match t {
                Tag::String(s) => s.to_string_lossy(),
                other => tag_json(other).to_string(),
            }),
            health,
            max_health: health.map(|_| 20.0),
            dead: health.is_some_and(|h| h <= 0.0),
            on_ground: matches!(get("OnGround"), Some(Tag::Byte(1))),
            tags,
            player: player.map(uuid_str),
        })
    }
}

impl PluginCtx<'_, '_> {
    fn entity_command(&mut self, id: i32, f: impl FnOnce(&str) -> String) {
        let line = f("{e}");
        self.queue(Action::EntityCommand(id, line));
    }
    fn entity_nbt(&mut self, id: i32, snbt_text: &str) {
        if let Ok((tag, _)) = horizon_world::nbt::parse_snbt(snbt_text) {
            self.queue(Action::EntityNbt(id, tag));
        }
    }
}

impl entities::Host for PluginCtx<'_, '_> {
    fn get(&mut self, id: i32) -> Option<entities::Entity> {
        let t = self.game.plugin_target(id)?;
        self.game.plugin_entity(t)
    }
    fn by_uuid(&mut self, uuid: String) -> Option<entities::Entity> {
        let want = parse_uuid(&uuid)?;
        for t in self.game.plugin_targets() {
            if let Some(e) = self.game.plugin_entity(t)
                && parse_uuid(&e.uuid) == Some(want)
            {
                return Some(e);
            }
        }
        None
    }
    fn in_world(&mut self, world: String) -> Vec<entities::Entity> {
        if Self::dim(&world) != Some(self.game.dim) {
            return Vec::new();
        }
        self.game
            .plugin_targets()
            .into_iter()
            .filter_map(|t| self.game.plugin_entity(t))
            .collect()
    }
    fn nearby(&mut self, at: types::Location, radius: f64) -> Vec<entities::Entity> {
        if Self::dim(&at.world) != Some(self.game.dim) {
            return Vec::new();
        }
        let r2 = radius * radius;
        let near: Vec<Target> = self
            .game
            .plugin_targets()
            .into_iter()
            .filter(|t| {
                self.game.target_position(*t).is_some_and(|p| {
                    let d = [p[0] - at.x, p[1] - at.y, p[2] - at.z];
                    d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= r2
                })
            })
            .collect();
        near.into_iter()
            .filter_map(|t| self.game.plugin_entity(t))
            .collect()
    }
    fn by_tag(&mut self, tag: String) -> Vec<entities::Entity> {
        self.game
            .plugin_targets()
            .into_iter()
            .filter_map(|t| self.game.plugin_entity(t))
            .filter(|e| e.tags.contains(&tag))
            .collect()
    }
    fn spawn(&mut self, at: types::Location, kind: String, nbt: String) -> String {
        self.game.mods.plugin.spawned += 1;
        let tag = format!("hzp_spawn_{}", self.game.mods.plugin.spawned);
        let mut data =
            match horizon_world::nbt::parse_snbt(if nbt.trim().is_empty() { "{}" } else { &nbt }) {
                Ok((Tag::Compound(c), _)) => c,
                _ => Default::default(),
            };
        let mut tags = match data.remove(&JavaString::from("Tags")) {
            Some(Tag::List(l)) => l,
            _ => Vec::new(),
        };
        tags.push(Tag::String(tag.as_str().into()));
        data.insert(JavaString::from("Tags"), Tag::List(tags));
        data.entry(JavaString::from("Rotation")).or_insert_with(|| {
            Tag::List(vec![
                Tag::Float(at.yaw.to_bits()),
                Tag::Float(at.pitch.to_bits()),
            ])
        });
        self.command(format!(
            "execute in {} run summon {} {} {} {} {}",
            at.world,
            id_of(&kind),
            num(at.x),
            num(at.y),
            num(at.z),
            snbt(&Tag::Compound(data))
        ));
        tag
    }
    fn teleport(&mut self, id: i32, to: types::Location) -> bool {
        let Some(t) = self.game.plugin_target(id) else {
            return false;
        };
        if let Target::Guest(conn) = t
            && let Some(u) = self
                .lan_ref()
                .and_then(|h| h.hub.guest(conn))
                .map(|g| g.uuid)
        {
            self.queue(Action::Teleport(u, to));
            return true;
        }
        let world = to.world.clone();
        self.entity_command(id, |e| {
            format!(
                "execute in {world} run tp {e} {} {} {} {} {}",
                num(to.x),
                num(to.y),
                num(to.z),
                num(f64::from(to.yaw)),
                num(f64::from(to.pitch))
            )
        });
        true
    }
    fn remove(&mut self, id: i32) {
        self.queue(Action::EntityRemove(id));
    }
    fn set_velocity(&mut self, id: i32, v: types::Vec3) {
        self.entity_nbt(
            id,
            &format!("{{Motion:[{}d,{}d,{}d]}}", num(v.x), num(v.y), num(v.z)),
        );
    }
    fn set_custom_name(&mut self, id: i32, name: Option<String>, visible: bool) {
        match name {
            Some(n) => self.entity_nbt(
                id,
                &format!(
                    "{{CustomName:{},CustomNameVisible:{}b}}",
                    snbt(&text_tag(&n)),
                    u8::from(visible)
                ),
            ),
            None => self.entity_command(id, |e| format!("data remove entity {e} CustomName")),
        }
    }
    fn set_glowing(&mut self, id: i32, glowing: bool) {
        self.entity_nbt(id, &format!("{{Glowing:{}b}}", u8::from(glowing)));
    }
    fn set_invulnerable(&mut self, id: i32, invulnerable: bool) {
        self.entity_nbt(id, &format!("{{Invulnerable:{}b}}", u8::from(invulnerable)));
    }
    fn set_silent(&mut self, id: i32, silent: bool) {
        self.entity_nbt(id, &format!("{{Silent:{}b}}", u8::from(silent)));
    }
    fn set_gravity(&mut self, id: i32, gravity: bool) {
        self.entity_nbt(id, &format!("{{NoGravity:{}b}}", u8::from(!gravity)));
    }
    fn set_ai(&mut self, id: i32, ai: bool) {
        self.entity_nbt(id, &format!("{{NoAI:{}b}}", u8::from(!ai)));
    }
    fn damage(&mut self, id: i32, amount: f32, cause: Option<String>) {
        let cause = id_of(cause.as_deref().unwrap_or("generic"));
        self.entity_command(id, |e| {
            format!("damage {e} {} {cause}", num(f64::from(amount)))
        });
    }
    fn set_health(&mut self, id: i32, health: f32) {
        self.entity_nbt(id, &format!("{{Health:{}f}}", num(f64::from(health))));
    }
    fn add_tag(&mut self, id: i32, tag: String) {
        self.entity_command(id, |e| format!("tag {e} add {tag}"));
    }
    fn remove_tag(&mut self, id: i32, tag: String) {
        self.entity_command(id, |e| format!("tag {e} remove {tag}"));
    }
    fn add_passenger(&mut self, vehicle: i32, passenger: i32) {
        self.queue(Action::Ride(passenger, vehicle));
    }
    fn eject(&mut self, id: i32) {
        self.queue(Action::Eject(id));
    }
    fn get_nbt(&mut self, id: i32) -> Option<String> {
        let t = self.game.plugin_target(id)?;
        self.game.entity_nbt(t).map(|t| snbt(&t))
    }
    fn merge_nbt(&mut self, id: i32, snbt_text: String) {
        self.entity_nbt(id, &snbt_text);
    }
}

// ---------------------------------------------------------------------------
// Items.

/// A stack's components as a compound.
fn components(stack: &types::ItemStack) -> horizon_world::nbt::Compound {
    if stack.components.trim().is_empty() {
        return Default::default();
    }
    match horizon_world::nbt::parse_snbt(&stack.components) {
        Ok((Tag::Compound(c), _)) => c,
        _ => Default::default(),
    }
}
fn with_components(stack: &types::ItemStack, c: horizon_world::nbt::Compound) -> types::ItemStack {
    types::ItemStack {
        id: stack.id.clone(),
        count: stack.count,
        components: if c.is_empty() {
            String::new()
        } else {
            snbt(&Tag::Compound(c))
        },
    }
}
fn key(k: &str) -> JavaString {
    JavaString::from(k)
}
fn text_json(t: &Tag) -> String {
    match t {
        Tag::String(s) => s.to_string_lossy(),
        other => tag_json(other).to_string(),
    }
}

impl items::Host for PluginCtx<'_, '_> {
    fn parse(&mut self, text: String, count: u32) -> Result<types::ItemStack, String> {
        let text = text.trim();
        let (id, comps) = match text.find('[') {
            Some(at) => (&text[..at], Some(&text[at..])),
            None => (text, None),
        };
        let id = id_of(id.trim());
        if self.game.res.items().get(&id).is_none() {
            return Err(format!("Unknown item '{id}'"));
        }
        let components = match comps {
            Some(c) => snbt(&horizon_client::item_data::command_components(c)?),
            None => String::new(),
        };
        Ok(types::ItemStack {
            id,
            count: count.max(1),
            components,
        })
    }
    fn meta(&mut self, stack: types::ItemStack) -> items::ItemMeta {
        let c = components(&stack);
        let get = |k: &str| c.get(&key(k));
        let enchantments = match get("minecraft:enchantments") {
            Some(Tag::Compound(m)) => {
                let levels = match m.get(&key("levels")) {
                    Some(Tag::Compound(l)) => l,
                    _ => m,
                };
                levels
                    .iter()
                    .filter_map(|(k, v)| match v {
                        Tag::Int(l) => Some(items::Enchantment {
                            id: k.to_string_lossy(),
                            level: (*l).max(0) as u32,
                        }),
                        _ => None,
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let int = |t: Option<&Tag>| match t {
            Some(Tag::Int(v)) => Some(*v),
            Some(Tag::Short(v)) => Some(i32::from(*v)),
            Some(Tag::Byte(v)) => Some(i32::from(*v)),
            _ => None,
        };
        items::ItemMeta {
            display_name: get("minecraft:custom_name").map(text_json),
            item_name: get("minecraft:item_name").map(text_json),
            lore: match get("minecraft:lore") {
                Some(Tag::List(l)) => l.iter().map(text_json).collect(),
                _ => Vec::new(),
            },
            enchantments,
            damage: int(get("minecraft:damage")).unwrap_or(0).max(0) as u32,
            max_damage: int(get("minecraft:max_damage")).map(|v| v.max(0) as u32),
            unbreakable: get("minecraft:unbreakable").is_some(),
            custom_model_data: match get("minecraft:custom_model_data") {
                Some(Tag::Compound(m)) => match m.get(&key("floats")) {
                    Some(Tag::List(f)) => f.first().and_then(|t| match t {
                        Tag::Float(v) => Some(f32::from_bits(*v)),
                        _ => None,
                    }),
                    _ => None,
                },
                Some(Tag::Int(v)) => Some(*v as f32),
                _ => None,
            },
            max_stack_size: int(get("minecraft:max_stack_size")).map(|v| v.max(1) as u32),
            glint: match get("minecraft:enchantment_glint_override") {
                Some(Tag::Byte(v)) => Some(*v != 0),
                _ => None,
            },
            hide_tooltip: matches!(
                get("minecraft:tooltip_display").and_then(|t| t.get("hide_tooltip")),
                Some(Tag::Byte(1))
            ),
        }
    }
    fn with_meta(&mut self, stack: types::ItemStack, meta: items::ItemMeta) -> types::ItemStack {
        let mut c = components(&stack);
        let mut put = |k: &str, v: Option<Tag>| match v {
            Some(v) => {
                c.insert(key(k), v);
            }
            None => {
                c.remove(&key(k));
            }
        };
        put(
            "minecraft:custom_name",
            meta.display_name.as_deref().map(text_tag),
        );
        put(
            "minecraft:item_name",
            meta.item_name.as_deref().map(text_tag),
        );
        put(
            "minecraft:lore",
            (!meta.lore.is_empty()).then(|| {
                Tag::List(
                    meta.lore
                        .iter()
                        .map(|l| {
                            // Lore lines are italic and purple unless they say otherwise.
                            let mut v = super::plugins::plugin_component(l);
                            if let serde_json::Value::String(s) = &v {
                                v = serde_json::json!({ "text": s, "italic": false, "color": "white" });
                            } else if let serde_json::Value::Object(o) = &mut v {
                                o.entry("italic").or_insert(serde_json::Value::Bool(false));
                            }
                            horizon_client::lan::data::component_nbt(&v)
                        })
                        .collect(),
                )
            }),
        );
        put(
            "minecraft:enchantments",
            (!meta.enchantments.is_empty()).then(|| {
                Tag::compound(meta.enchantments.iter().map(|e| {
                    (
                        JavaString::from(id_of(&e.id).as_str()),
                        Tag::Int(e.level.min(255) as i32),
                    )
                }))
            }),
        );
        put(
            "minecraft:damage",
            (meta.damage > 0).then_some(Tag::Int(meta.damage as i32)),
        );
        put(
            "minecraft:max_damage",
            meta.max_damage.map(|v| Tag::Int(v as i32)),
        );
        put(
            "minecraft:unbreakable",
            meta.unbreakable
                .then(|| Tag::compound(Vec::<(&str, Tag)>::new())),
        );
        put(
            "minecraft:custom_model_data",
            meta.custom_model_data
                .map(|f| Tag::compound([("floats", Tag::List(vec![Tag::Float(f.to_bits())]))])),
        );
        put(
            "minecraft:max_stack_size",
            meta.max_stack_size.map(|v| Tag::Int(v.clamp(1, 99) as i32)),
        );
        put(
            "minecraft:enchantment_glint_override",
            meta.glint.map(|g| Tag::Byte(i8::from(g))),
        );
        put(
            "minecraft:tooltip_display",
            meta.hide_tooltip
                .then(|| Tag::compound([("hide_tooltip", Tag::Byte(1))])),
        );
        with_components(&stack, c)
    }
    fn get_tag(&mut self, stack: types::ItemStack, k: String) -> Option<String> {
        let c = components(&stack);
        let plugin = self.game.mods.plugin.caller.clone();
        let data = c.get(&key("minecraft:custom_data"))?;
        let values = data.get("PublicBukkitValues")?;
        match values.get(&format!("{plugin}:{k}"))? {
            Tag::String(s) => Some(s.to_string_lossy()),
            other => Some(snbt(other)),
        }
    }
    fn set_tag(
        &mut self,
        stack: types::ItemStack,
        k: String,
        value: Option<String>,
    ) -> types::ItemStack {
        let mut c = components(&stack);
        let plugin = self.game.mods.plugin.caller.clone();
        let mut data = match c.remove(&key("minecraft:custom_data")) {
            Some(Tag::Compound(d)) => d,
            _ => Default::default(),
        };
        let mut values = match data.remove(&key("PublicBukkitValues")) {
            Some(Tag::Compound(v)) => v,
            _ => Default::default(),
        };
        let name = key(&format!("{plugin}:{k}"));
        match value {
            Some(v) => {
                values.insert(name, Tag::String(v.as_str().into()));
            }
            None => {
                values.remove(&name);
            }
        }
        if !values.is_empty() {
            data.insert(key("PublicBukkitValues"), Tag::Compound(values));
        }
        if !data.is_empty() {
            c.insert(key("minecraft:custom_data"), Tag::Compound(data));
        }
        with_components(&stack, c)
    }
    fn similar(&mut self, a: types::ItemStack, b: types::ItemStack) -> bool {
        match (stack_in(&a), stack_in(&b)) {
            (Some(a), Some(b)) => a.same_item_same_components(&b),
            (None, None) => true,
            _ => false,
        }
    }
    fn max_stack(&mut self, id: String) -> u32 {
        PluginCtx::max_stack(self, &id_of(&id))
    }
    fn display_name(&mut self, stack: types::ItemStack) -> String {
        match stack_in(&stack) {
            Some(s) => self.game.stack_display_name(&s).plain(),
            None => "Air".into(),
        }
    }
    fn registry(&mut self, name: String) -> Vec<String> {
        let name = name.strip_prefix("minecraft:").unwrap_or(&name).to_owned();
        match name.as_str() {
            "item" => self
                .game
                .res
                .items()
                .registry_order
                .iter()
                .map(|i| i.to_string())
                .collect(),
            "block" => {
                let mut names: Vec<String> = self
                    .game
                    .res
                    .items()
                    .registry_order
                    .iter()
                    .filter(|i| self.game.pack.assets.registry.default_state(i).is_some())
                    .map(|i| i.to_string())
                    .collect();
                names.sort();
                names.dedup();
                names
            }
            _ => self
                .lan_ref()
                .and_then(|h| {
                    h.hub
                        .sync_data()
                        .builtin
                        .get(&format!("minecraft:{name}"))
                        .map(|ids| ids.names.clone())
                        .or_else(|| {
                            h.hub
                                .sync_data()
                                .dynamic
                                .registries
                                .get(&format!("minecraft:{name}"))
                                .map(|l| l.iter().map(|(n, _)| n.clone()).collect())
                        })
                })
                .unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin inventories.

impl inventories::Host for PluginCtx<'_, '_> {
    fn create(&mut self, kind: inventories::InventoryKind, rows: u32, title: String) -> u32 {
        let size = match kind {
            inventories::InventoryKind::Chest => rows.clamp(1, 6) as usize * 9,
            inventories::InventoryKind::Hopper => 5,
            inventories::InventoryKind::Dropper => 9,
        };
        let owner = self.game.mods.plugin.caller.clone();
        let dropper = matches!(kind, inventories::InventoryKind::Dropper);
        horizon_client::lan::menu::create_plugin_inventory(size, title, owner, dropper)
    }
    fn size(&mut self, inventory: u32) -> u32 {
        horizon_client::lan::menu::plugin_inventory(inventory).map_or(0, |p| p.items.len() as u32)
    }
    fn get(&mut self, inventory: u32, slot: u32) -> Option<types::ItemStack> {
        let p = horizon_client::lan::menu::plugin_inventory(inventory)?;
        p.items.get(slot as usize)?.as_ref().map(stack_out)
    }
    fn set(&mut self, inventory: u32, slot: u32, item: Option<types::ItemStack>) {
        let item = item.as_ref().and_then(stack_in);
        horizon_client::lan::menu::with_plugin_inventory(inventory, |p| {
            if let Some(s) = p.items.get_mut(slot as usize) {
                *s = item;
            }
        });
    }
    fn contents(&mut self, inventory: u32) -> Vec<Option<types::ItemStack>> {
        horizon_client::lan::menu::plugin_inventory(inventory)
            .map(|p| stacks_out(&p.items))
            .unwrap_or_default()
    }
    fn set_contents(&mut self, inventory: u32, items: Vec<Option<types::ItemStack>>) {
        let items: Vec<_> = items
            .iter()
            .map(|s| s.as_ref().and_then(stack_in))
            .collect();
        horizon_client::lan::menu::with_plugin_inventory(inventory, |p| {
            for (k, slot) in p.items.iter_mut().enumerate() {
                *slot = items.get(k).cloned().flatten();
            }
        });
    }
    fn add(&mut self, inventory: u32, items: Vec<types::ItemStack>) -> Vec<types::ItemStack> {
        let stacks: Vec<_> = items.iter().filter_map(stack_in).collect();
        let maxes: Vec<u32> = stacks.iter().map(|s| self.max_stack(&s.item)).collect();
        let mut rest = Vec::new();
        horizon_client::lan::menu::with_plugin_inventory(inventory, |p| {
            for (mut s, max) in stacks.into_iter().zip(maxes) {
                for slot in p.items.iter_mut().flatten() {
                    if s.count == 0 {
                        break;
                    }
                    if slot.same_item_same_components(&s) && slot.count < max {
                        let moved = (max - slot.count).min(s.count);
                        slot.count += moved;
                        s.count -= moved;
                    }
                }
                for slot in p.items.iter_mut() {
                    if s.count == 0 {
                        break;
                    }
                    if slot.is_none() {
                        let moved = s.count.min(max);
                        *slot = Some(horizon_client::items::ItemStack {
                            count: moved,
                            ..s.clone()
                        });
                        s.count -= moved;
                    }
                }
                if s.count > 0 {
                    rest.push(stack_out(&s));
                }
            }
        });
        rest
    }
    fn clear(&mut self, inventory: u32) {
        horizon_client::lan::menu::with_plugin_inventory(inventory, |p| {
            p.items.iter_mut().for_each(|s| *s = None);
        });
    }
    fn set_title(&mut self, inventory: u32, title: String) {
        horizon_client::lan::menu::with_plugin_inventory(inventory, |p| p.title = title);
    }
    fn open(&mut self, player: String, inventory: u32) -> bool {
        let Some(uuid) = parse_uuid(&player).filter(|u| self.guest_index(*u).is_some()) else {
            return false;
        };
        if horizon_client::lan::menu::plugin_inventory(inventory).is_none() {
            return false;
        }
        self.queue(Action::OpenInventory(uuid, inventory));
        true
    }
    fn viewers(&mut self, inventory: u32) -> Vec<String> {
        self.lan_ref()
            .map(|h| {
                h.hub
                    .guests
                    .iter()
                    .filter(|g| {
                        g.menu.as_ref().is_some_and(|m| {
                            m.slots == horizon_client::lan::menu::Slots::Plugin(inventory)
                        })
                    })
                    .map(|g| uuid_str(g.uuid))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn open_inventory(&mut self, player: String) -> Option<u32> {
        let uuid = parse_uuid(&player)?;
        let g = self.guest_ref(uuid)?;
        match g.menu.as_ref()?.slots {
            horizon_client::lan::menu::Slots::Plugin(id) => Some(id),
            _ => None,
        }
    }
    fn destroy(&mut self, inventory: u32) {
        let viewers = inventories::Host::viewers(self, inventory);
        for v in viewers {
            if let Some(u) = parse_uuid(&v) {
                self.queue(Action::CloseMenu(u));
            }
        }
        self.queue(Action::DestroyInventory(inventory));
    }
}

// ---------------------------------------------------------------------------
// Scoreboards.

/// Owners of the sidebar's lines: distinct, and invisible.
fn line_owner(i: usize) -> String {
    format!("§{:x}§r", i % 16) + &"§r".repeat(i / 16)
}
const SIDEBAR: &str = "hzp_sidebar";

/// `ChatFormatting` ids of the colour names `/team modify color` takes.
fn team_color(name: &str) -> Option<i32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "black" => 0,
        "dark_blue" => 1,
        "dark_green" => 2,
        "dark_aqua" => 3,
        "dark_red" => 4,
        "dark_purple" => 5,
        "gold" => 6,
        "gray" => 7,
        "dark_gray" => 8,
        "blue" => 9,
        "green" => 10,
        "aqua" => 11,
        "red" => 12,
        "light_purple" => 13,
        "yellow" => 14,
        "white" => 15,
        "reset" => 21,
        _ => return None,
    })
}

impl scoreboards::Host for PluginCtx<'_, '_> {
    fn set_sidebar(&mut self, player: String, title: String, lines: Vec<String>) {
        let Some(uuid) = parse_uuid(&player) else {
            return;
        };
        let lines: Vec<String> = lines.into_iter().take(15).collect();
        let old = self.game.mods.plugin.sidebars.get(&uuid).cloned();
        let mut packets = Vec::new();
        match &old {
            None => {
                packets.push(C::SetObjective(play::SetObjective {
                    name: SIDEBAR.into(),
                    method: 0,
                    display: Some((text_tag(&title), 0, Some(play::NumberFormat::Blank))),
                }));
                packets.push(C::SetDisplayObjective(play::SetDisplayObjective {
                    slot: 1,
                    objective: SIDEBAR.into(),
                }));
            }
            Some((t, _)) if *t != title => {
                packets.push(C::SetObjective(play::SetObjective {
                    name: SIDEBAR.into(),
                    method: 2,
                    display: Some((text_tag(&title), 0, Some(play::NumberFormat::Blank))),
                }));
            }
            _ => {}
        }
        let before = old.map(|(_, l)| l).unwrap_or_default();
        let n = lines.len();
        for (i, line) in lines.iter().enumerate() {
            if before.len() == n && before.get(i) == Some(line) {
                continue;
            }
            packets.push(C::SetScore(play::SetScore {
                owner: line_owner(i),
                objective: SIDEBAR.into(),
                score: (n - i) as i32,
                display: Some(text_tag(line)),
                number_format: Some(play::NumberFormat::Blank),
            }));
        }
        for i in n..before.len() {
            packets.push(C::ResetScore(play::ResetScore {
                owner: line_owner(i),
                objective: Some(SIDEBAR.into()),
            }));
        }
        for p in packets {
            self.send(uuid, p);
        }
        self.game.mods.plugin.sidebars.insert(uuid, (title, lines));
    }
    fn clear_sidebar(&mut self, player: String) {
        let Some(uuid) = parse_uuid(&player) else {
            return;
        };
        if self.game.mods.plugin.sidebars.remove(&uuid).is_some() {
            self.send(
                uuid,
                C::SetObjective(play::SetObjective {
                    name: SIDEBAR.into(),
                    method: 1,
                    display: None,
                }),
            );
        }
    }
    fn set_name_tag(
        &mut self,
        target: String,
        prefix: String,
        suffix: String,
        color: Option<String>,
    ) {
        let Some(uuid) = parse_uuid(&target) else {
            return;
        };
        let Some(name) = self.name_of(uuid) else {
            return;
        };
        let color = color.as_deref().and_then(team_color);
        let existed = self
            .game
            .mods
            .plugin
            .name_tags
            .insert(uuid, (prefix.clone(), suffix.clone(), color))
            .is_some();
        let packet = name_tag_packet(
            uuid,
            &name,
            &prefix,
            &suffix,
            color,
            if existed { 2 } else { 0 },
        );
        let everyone: Vec<u128> = self
            .players()
            .iter()
            .filter_map(|p| parse_uuid(&p.id))
            .collect();
        for to in everyone {
            self.send(to, packet.clone());
        }
    }
    fn clear_name_tag(&mut self, target: String) {
        let Some(uuid) = parse_uuid(&target) else {
            return;
        };
        if self.game.mods.plugin.name_tags.remove(&uuid).is_none() {
            return;
        }
        let name = self.name_of(uuid).unwrap_or_default();
        let packet = name_tag_packet(uuid, &name, "", "", None, 1);
        let everyone: Vec<u128> = self
            .players()
            .iter()
            .filter_map(|p| parse_uuid(&p.id))
            .collect();
        for to in everyone {
            self.send(to, packet.clone());
        }
    }
    fn objectives(&mut self) -> Vec<scoreboards::Objective> {
        self.game
            .plugin_objectives()
            .into_iter()
            .map(|(name, criteria, display)| scoreboards::Objective {
                name,
                criteria,
                display_name: display,
            })
            .collect()
    }
    fn add_objective(&mut self, name: String, criteria: String, display_name: String) {
        self.command(format!(
            "scoreboard objectives add {name} {criteria} {}",
            json_text(&display_name)
        ));
    }
    fn remove_objective(&mut self, name: String) {
        self.command(format!("scoreboard objectives remove {name}"));
    }
    fn set_display_slot(&mut self, slot: String, objective: Option<String>) {
        self.command(match objective {
            Some(o) => format!("scoreboard objectives setdisplay {slot} {o}"),
            None => format!("scoreboard objectives setdisplay {slot}"),
        });
    }
    fn get_score(&mut self, objective: String, holder: String) -> Option<i32> {
        self.game.plugin_score(&objective, &holder)
    }
    fn set_score(&mut self, objective: String, holder: String, value: i32) {
        self.command(format!(
            "scoreboard players set {holder} {objective} {value}"
        ));
    }
    fn reset_score(&mut self, objective: String, holder: String) {
        self.command(format!("scoreboard players reset {holder} {objective}"));
    }
    fn teams(&mut self) -> Vec<String> {
        self.game.plugin_teams()
    }
    fn add_team(&mut self, name: String, display_name: String) {
        self.command(if display_name.is_empty() {
            format!("team add {name}")
        } else {
            format!("team add {name} {}", json_text(&display_name))
        });
    }
    fn remove_team(&mut self, name: String) {
        self.command(format!("team remove {name}"));
    }
    fn join_team(&mut self, team: String, member: String) {
        self.command(format!("team join {team} {member}"));
    }
    fn leave_team(&mut self, member: String) {
        self.command(format!("team leave {member}"));
    }
    fn set_team_option(&mut self, team: String, key: String, value: String) {
        let value = match key.as_str() {
            "displayName" | "prefix" | "suffix" => json_text(&value),
            _ => value,
        };
        self.command(format!("team modify {team} {key} {value}"));
    }
}

// ---------------------------------------------------------------------------
// Boss bars.

fn bar_color(c: bossbars::BarColor) -> i32 {
    match c {
        bossbars::BarColor::Pink => 0,
        bossbars::BarColor::Blue => 1,
        bossbars::BarColor::Red => 2,
        bossbars::BarColor::Green => 3,
        bossbars::BarColor::Yellow => 4,
        bossbars::BarColor::Purple => 5,
        bossbars::BarColor::White => 6,
    }
}
fn bar_style(s: bossbars::BarStyle) -> i32 {
    match s {
        bossbars::BarStyle::Solid => 0,
        bossbars::BarStyle::SegmentedSix => 1,
        bossbars::BarStyle::SegmentedTen => 2,
        bossbars::BarStyle::SegmentedTwelve => 3,
        bossbars::BarStyle::SegmentedTwenty => 4,
    }
}

impl PluginCtx<'_, '_> {
    fn bar_send(&mut self, bar: u32, action: impl Fn(&Bar) -> play::BossAction) {
        let Some(b) = self.game.mods.plugin.bars.get(&bar).cloned() else {
            return;
        };
        let packet = C::BossEvent(play::BossEvent {
            id: b.uuid,
            action: action(&b),
        });
        for p in b.players {
            self.send(p, packet.clone());
        }
    }
}
fn bar_add(b: &Bar) -> play::BossAction {
    play::BossAction::Add {
        name: text_tag(&b.title),
        progress: b.progress,
        color: b.color,
        overlay: b.style,
        darken: b.flags.0,
        music: b.flags.1,
        fog: b.flags.2,
    }
}

impl bossbars::Host for PluginCtx<'_, '_> {
    fn create(
        &mut self,
        title: String,
        color: bossbars::BarColor,
        style: bossbars::BarStyle,
        progress: f32,
    ) -> u32 {
        let p = &mut self.game.mods.plugin;
        p.next_bar += 1;
        let id = p.next_bar;
        let uuid = horizon_protocol::crypto::offline_uuid(&format!(
            "horizon-plugin-bar-{id}-{}",
            p.caller
        ));
        p.bars.insert(
            id,
            Bar {
                uuid,
                title,
                progress: progress.clamp(0.0, 1.0),
                color: bar_color(color),
                style: bar_style(style),
                flags: (false, false, false),
                players: Default::default(),
            },
        );
        id
    }
    fn set_title(&mut self, bar: u32, title: String) {
        if let Some(b) = self.game.mods.plugin.bars.get_mut(&bar) {
            b.title = title;
        }
        self.bar_send(bar, |b| play::BossAction::UpdateName {
            name: text_tag(&b.title),
        });
    }
    fn set_progress(&mut self, bar: u32, progress: f32) {
        let progress = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if let Some(b) = self.game.mods.plugin.bars.get_mut(&bar) {
            b.progress = progress;
        }
        self.bar_send(bar, |b| play::BossAction::UpdateProgress {
            progress: b.progress,
        });
    }
    fn set_color(&mut self, bar: u32, color: bossbars::BarColor) {
        if let Some(b) = self.game.mods.plugin.bars.get_mut(&bar) {
            b.color = bar_color(color);
        }
        self.bar_send(bar, |b| play::BossAction::UpdateStyle {
            color: b.color,
            overlay: b.style,
        });
    }
    fn set_style(&mut self, bar: u32, style: bossbars::BarStyle) {
        if let Some(b) = self.game.mods.plugin.bars.get_mut(&bar) {
            b.style = bar_style(style);
        }
        self.bar_send(bar, |b| play::BossAction::UpdateStyle {
            color: b.color,
            overlay: b.style,
        });
    }
    fn set_flags(&mut self, bar: u32, darken_sky: bool, play_music: bool, create_fog: bool) {
        if let Some(b) = self.game.mods.plugin.bars.get_mut(&bar) {
            b.flags = (darken_sky, play_music, create_fog);
        }
        self.bar_send(bar, |b| play::BossAction::UpdateProperties {
            darken: b.flags.0,
            music: b.flags.1,
            fog: b.flags.2,
        });
    }
    fn add_player(&mut self, bar: u32, player: String) {
        let Some(uuid) = parse_uuid(&player) else {
            return;
        };
        let Some(b) = self.game.mods.plugin.bars.get_mut(&bar) else {
            return;
        };
        if b.players.insert(uuid) {
            let packet = C::BossEvent(play::BossEvent {
                id: b.uuid,
                action: bar_add(b),
            });
            self.send(uuid, packet);
        }
    }
    fn remove_player(&mut self, bar: u32, player: String) {
        let Some(uuid) = parse_uuid(&player) else {
            return;
        };
        let Some(b) = self.game.mods.plugin.bars.get_mut(&bar) else {
            return;
        };
        if b.players.remove(&uuid) {
            let packet = C::BossEvent(play::BossEvent {
                id: b.uuid,
                action: play::BossAction::Remove,
            });
            self.send(uuid, packet);
        }
    }
    fn players(&mut self, bar: u32) -> Vec<String> {
        self.game
            .mods
            .plugin
            .bars
            .get(&bar)
            .map(|b| b.players.iter().map(|u| uuid_str(*u)).collect())
            .unwrap_or_default()
    }
    fn remove(&mut self, bar: u32) {
        self.bar_send(bar, |_| play::BossAction::Remove);
        self.game.mods.plugin.bars.remove(&bar);
    }
}
