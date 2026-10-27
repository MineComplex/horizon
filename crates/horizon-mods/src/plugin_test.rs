//! A plugin run with no game: `cargo horizon-mod test` for plugins
//! (docs/plugins.md, "Testing a plugin"), and Horizon's own tests of the
//! plugin runtime.
//!
//! [`TestServer`] loads one plugin into the same sandbox the server uses and
//! plays the server's part with a [`FakeServer`]: players joining, chatting,
//! typing commands, breaking and placing blocks, clicking in menus, taking
//! damage and leaving, the console, and ticks. Everything the plugin does is
//! recorded (messages, titles, commands, kicks, teleports, sidebars,
//! inventories ...). [`PluginScript`] is a test written in TOML.
use crate::plugin::{
    self, Event, EventKind, PluginHost, Sender, bossbars, entities, events as ev, inventories,
    items, players, scoreboards, server, types, worlds,
};
use crate::{Mods, Status};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A player on the fake server.
#[derive(Clone, Debug)]
pub struct FakePlayer {
    pub record: types::Player,
    /// 41 slots in plugin numbering.
    pub inventory: Vec<Option<types::ItemStack>>,
    pub ender: Vec<Option<types::ItemStack>>,
    pub cursor: Option<types::ItemStack>,
    pub effects: Vec<players::Effect>,
    /// The plugin inventory open, if any.
    pub open: Option<u32>,
    pub list_name: Option<String>,
}

/// A plugin inventory on the fake server.
#[derive(Clone, Debug)]
pub struct FakeInventory {
    pub items: Vec<Option<types::ItemStack>>,
    pub title: String,
}

/// The server a test plays: what plugins read, and what they did.
#[derive(Default)]
pub struct FakeServer {
    pub players: Vec<FakePlayer>,
    /// Every name that joined, for its UUID.
    names: Vec<String>,
    /// Blocks set in a world; every other block is air (stone below y 64 in
    /// the Overworld).
    pub blocks: BTreeMap<(String, i32, i32, i32), String>,
    pub tick: u64,
    pub time: u64,
    pub storm: bool,
    pub thundering: bool,
    pub game_rules: BTreeMap<String, String>,
    pub motd: String,
    pub max_players: u32,
    pub whitelist: BTreeSet<String>,
    pub whitelist_on: bool,
    pub ops: BTreeSet<String>,
    pub bans: BTreeSet<String>,
    pub entities: Vec<entities::Entity>,
    next_entity: i32,
    pub inventories: BTreeMap<u32, FakeInventory>,
    next_inventory: u32,
    pub sidebars: BTreeMap<String, (String, Vec<String>)>,
    pub name_tags: BTreeMap<String, (String, String, Option<String>)>,
    pub objectives: Vec<scoreboards::Objective>,
    pub scores: BTreeMap<(String, String), i32>,
    pub teams: Vec<String>,
    pub bars: BTreeMap<u32, (String, f32, BTreeSet<String>)>,
    next_bar: u32,
    pub hidden: BTreeSet<(String, String)>,
    // What the plugin did, in order.
    /// `name: text` for every message to a player (codes stripped).
    pub told: Vec<String>,
    pub action_bars: Vec<String>,
    pub titles: Vec<String>,
    pub console: Vec<String>,
    pub dispatched: Vec<String>,
    /// Commands players ran through the plugin (`perform-command`).
    pub performed: Vec<String>,
    pub kicked: Vec<String>,
    pub sounds: Vec<String>,
    pub payloads: Vec<(String, String, Vec<u8>)>,
    pub stopped_server: bool,
    caller: String,
}

fn strip(text: &str) -> String {
    let t = text.trim_start();
    if t.starts_with('{') || t.starts_with('[') || t.starts_with('"') {
        // JSON: its text fields, in order.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            fn walk(v: &serde_json::Value, out: &mut String) {
                match v {
                    serde_json::Value::String(s) => out.push_str(s),
                    serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                    serde_json::Value::Object(o) => {
                        if let Some(t) = o.get("text") {
                            walk(t, out);
                        }
                        if let Some(e) = o.get("extra") {
                            walk(e, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut out = String::new();
            walk(&v, &mut out);
            return out;
        }
    }
    let text = if text.contains('§') {
        text.to_owned()
    } else {
        // `&` codes as the server reads them.
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '&'
                && chars
                    .peek()
                    .is_some_and(|n| "0123456789abcdefklmnorABCDEFKLMNOR".contains(*n))
            {
                out.push('§');
                out.push(chars.next().unwrap_or('r'));
            } else {
                out.push(c);
            }
        }
        out
    };
    plugin::strip_codes(&text)
}

fn offline_uuid(n: usize) -> String {
    format!("00000000-0000-4000-8000-{:012x}", n)
}

impl FakeServer {
    pub fn new() -> FakeServer {
        FakeServer {
            motd: "A Horizon Server".into(),
            max_players: 20,
            next_entity: 1000,
            next_inventory: 1,
            ..Default::default()
        }
    }
    fn index(&self, id: &str) -> Option<usize> {
        self.players.iter().position(|p| p.record.id == id)
    }
    fn name(&self, id: &str) -> String {
        self.index(id)
            .map_or_else(|| id.to_owned(), |i| self.players[i].record.name.clone())
    }
    /// A player joins: the record a plugin reads.
    pub fn add_player(&mut self, name: &str) -> types::Player {
        let n = match self.names.iter().position(|x| x == name) {
            Some(i) => i + 1,
            None => {
                self.names.push(name.to_owned());
                self.names.len()
            }
        };
        self.next_entity += 1;
        let record = types::Player {
            id: offline_uuid(n),
            name: name.to_owned(),
            display_name: name.to_owned(),
            entity_id: self.next_entity,
            location: types::Location {
                world: "minecraft:overworld".into(),
                x: 0.5,
                y: 64.0,
                z: 0.5,
                yaw: 0.0,
                pitch: 0.0,
            },
            game_mode: types::GameMode::Survival,
            health: 20.0,
            max_health: 20.0,
            food: 20,
            saturation: 5.0,
            level: 0,
            exp: 0.0,
            total_exp: 0,
            op: self.ops.contains(name),
            permission_level: if self.ops.contains(name) { 4 } else { 0 },
            address: "127.0.0.1:50000".into(),
            ping: 0,
            locale: "en_us".into(),
            client_brand: "vanilla".into(),
            view_distance: 10,
            flying: false,
            allow_flight: false,
            sneaking: false,
            sprinting: false,
            gliding: false,
            sleeping: false,
            on_ground: true,
            dead: false,
            selected_slot: 0,
            held: None,
        };
        self.players.push(FakePlayer {
            record: record.clone(),
            inventory: vec![None; 41],
            ender: vec![None; 27],
            cursor: None,
            effects: Vec::new(),
            open: None,
            list_name: None,
        });
        record
    }
    pub fn player_named(&self, name: &str) -> Option<&FakePlayer> {
        self.players
            .iter()
            .find(|p| p.record.name.eq_ignore_ascii_case(name))
    }
    fn player_mut(&mut self, id: &str) -> Option<&mut FakePlayer> {
        self.players.iter_mut().find(|p| p.record.id == id)
    }
    /// The block a plugin reads at a position.
    pub fn block(&self, world: &str, x: i32, y: i32, z: i32) -> String {
        self.blocks
            .get(&(world.to_owned(), x, y, z))
            .cloned()
            .unwrap_or_else(|| {
                if world == "minecraft:overworld" && y < 64 {
                    "minecraft:stone".into()
                } else {
                    "minecraft:air".into()
                }
            })
    }
    fn refresh(&mut self) {
        for p in &mut self.players {
            p.record.held = p.inventory[p.record.selected_slot as usize].clone();
            p.record.op = self.ops.contains(&p.record.name);
            p.record.permission_level = if p.record.op { 4 } else { 0 };
        }
    }
}

// The fake's item components: `key=value` lines, the plugin data as
// `tag.<plugin>:<key>`. They round-trip within the fake only.
fn comps(stack: &types::ItemStack) -> BTreeMap<String, String> {
    stack
        .components
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.replace("\\n", "\n")))
        .collect()
}
fn with_comps(stack: &types::ItemStack, c: BTreeMap<String, String>) -> types::ItemStack {
    types::ItemStack {
        id: stack.id.clone(),
        count: stack.count,
        components: c
            .iter()
            .map(|(k, v)| format!("{k}={}", v.replace('\n', "\\n")))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

impl PluginHost for FakeServer {
    fn caller(&mut self, plugin: &str) {
        self.caller = plugin.to_owned();
    }
    fn server_info(&mut self) -> server::ServerInfo {
        server::ServerInfo {
            name: "Horizon".into(),
            version: "26.3".into(),
            protocol: 777,
            motd: self.motd.clone(),
            max_players: self.max_players,
            online_mode: false,
            port: 25565,
            view_distance: 10,
            simulation_distance: 10,
            hardcore: false,
            dedicated: true,
            tick: self.tick,
        }
    }
    fn tps(&mut self) -> (f64, f64, f64) {
        (20.0, 20.0, 20.0)
    }
    fn mspt(&mut self) -> f64 {
        1.0
    }
    fn console(&mut self, message: &str) {
        self.console.push(strip(message));
    }
    fn dispatch_command(&mut self, line: String) {
        self.dispatched.push(line);
    }
    fn shutdown(&mut self) {
        self.stopped_server = true;
    }
    fn set_motd(&mut self, motd: String) {
        self.motd = motd;
    }
    fn set_max_players(&mut self, max: u32) {
        self.max_players = max;
    }
    fn find_player(&mut self, name_or_id: &str) -> Option<server::OfflinePlayer> {
        let online = self
            .players
            .iter()
            .find(|p| p.record.id == name_or_id || p.record.name.eq_ignore_ascii_case(name_or_id));
        if let Some(p) = online {
            return Some(server::OfflinePlayer {
                id: p.record.id.clone(),
                name: p.record.name.clone(),
                online: true,
            });
        }
        let n = self
            .names
            .iter()
            .position(|x| x.eq_ignore_ascii_case(name_or_id))?;
        Some(server::OfflinePlayer {
            id: offline_uuid(n + 1),
            name: self.names[n].clone(),
            online: false,
        })
    }
    fn whitelist_enabled(&mut self) -> bool {
        self.whitelist_on
    }
    fn set_whitelist_enabled(&mut self, on: bool) {
        self.whitelist_on = on;
    }
    fn whitelist(&mut self) -> Vec<String> {
        self.whitelist.iter().cloned().collect()
    }
    fn set_whitelisted(&mut self, name: String, on: bool) {
        if on {
            self.whitelist.insert(name);
        } else {
            self.whitelist.remove(&name);
        }
    }
    fn operators(&mut self) -> Vec<String> {
        self.ops.iter().cloned().collect()
    }
    fn banned_players(&mut self) -> Vec<String> {
        self.bans.iter().cloned().collect()
    }
    fn ban(&mut self, name: String, _reason: Option<String>) {
        self.bans.insert(name);
    }
    fn pardon(&mut self, name: String) {
        self.bans.remove(&name);
    }
    fn ban_ip(&mut self, _address: String, _reason: Option<String>) {}
    fn pardon_ip(&mut self, _address: String) {}
    fn send_payload(&mut self, player: &str, channel: &str, data: Vec<u8>) -> bool {
        let known = self.index(player).is_some();
        if known {
            self.payloads
                .push((self.name(player), channel.to_owned(), data));
        }
        known
    }
}

impl players::Host for FakeServer {
    fn online(&mut self) -> Vec<types::Player> {
        self.refresh();
        self.players.iter().map(|p| p.record.clone()).collect()
    }
    fn get(&mut self, id: String) -> Option<types::Player> {
        self.refresh();
        self.players
            .iter()
            .find(|p| p.record.id == id)
            .map(|p| p.record.clone())
    }
    fn by_name(&mut self, name: String) -> Option<types::Player> {
        self.refresh();
        self.player_named(&name).map(|p| p.record.clone())
    }
    fn send_message(&mut self, id: String, message: String) {
        let name = self.name(&id);
        for line in strip(&message).split('\n') {
            self.told.push(format!("{name}: {line}"));
        }
    }
    fn send_action_bar(&mut self, id: String, message: String) {
        let name = self.name(&id);
        self.action_bars
            .push(format!("{name}: {}", strip(&message)));
    }
    fn send_title(
        &mut self,
        id: String,
        title: String,
        subtitle: String,
        _a: u32,
        _b: u32,
        _c: u32,
    ) {
        let name = self.name(&id);
        self.titles
            .push(format!("{name}: {} / {}", strip(&title), strip(&subtitle)));
    }
    fn clear_title(&mut self, _id: String) {}
    fn set_tab_list(&mut self, _id: String, _header: String, _footer: String) {}
    fn set_list_name(&mut self, id: String, name: Option<String>) {
        if let Some(p) = self.player_mut(&id) {
            p.list_name = name;
        }
    }
    fn set_display_name(&mut self, id: String, name: Option<String>) {
        if let Some(p) = self.player_mut(&id) {
            p.record.display_name = name.unwrap_or_else(|| p.record.name.clone());
        }
    }
    fn play_sound(
        &mut self,
        id: String,
        sound: String,
        _c: String,
        _v: f32,
        _p: f32,
        _at: Option<types::Location>,
    ) {
        let name = self.name(&id);
        self.sounds.push(format!("{name}: {sound}"));
    }
    fn stop_sound(&mut self, _id: String, _sound: Option<String>) {}
    fn spawn_particle(
        &mut self,
        _id: String,
        _p: String,
        _at: types::Location,
        _c: u32,
        _o: types::Vec3,
        _s: f32,
    ) {
    }
    fn send_block_change(&mut self, _id: String, _pos: types::BlockPos, _state: String) {}
    fn kick(&mut self, id: String, reason: String) -> bool {
        let Some(i) = self.index(&id) else {
            return false;
        };
        let p = self.players.remove(i);
        self.kicked
            .push(format!("{}: {}", p.record.name, strip(&reason)));
        true
    }
    fn transfer(&mut self, _id: String, _host: String, _port: u32) {}
    fn send_resource_pack(
        &mut self,
        _id: String,
        _u: String,
        _h: String,
        _r: bool,
        _p: Option<String>,
    ) {
    }
    fn teleport(&mut self, id: String, to: types::Location) -> bool {
        match self.player_mut(&id) {
            Some(p) => {
                p.record.location = to;
                true
            }
            None => false,
        }
    }
    fn set_game_mode(&mut self, id: String, mode: types::GameMode) {
        if let Some(p) = self.player_mut(&id) {
            p.record.game_mode = mode;
        }
    }
    fn set_health(&mut self, id: String, health: f32) {
        if let Some(p) = self.player_mut(&id) {
            p.record.health = health.clamp(0.0, p.record.max_health);
            p.record.dead = health <= 0.0;
        }
    }
    fn set_max_health(&mut self, id: String, health: f64) {
        if let Some(p) = self.player_mut(&id) {
            p.record.max_health = health as f32;
        }
    }
    fn set_food(&mut self, id: String, food: u32) {
        if let Some(p) = self.player_mut(&id) {
            p.record.food = food.min(20);
        }
    }
    fn set_saturation(&mut self, id: String, saturation: f32) {
        if let Some(p) = self.player_mut(&id) {
            p.record.saturation = saturation;
        }
    }
    fn set_level(&mut self, id: String, level: u32) {
        if let Some(p) = self.player_mut(&id) {
            p.record.level = level;
        }
    }
    fn set_exp(&mut self, id: String, progress: f32) {
        if let Some(p) = self.player_mut(&id) {
            p.record.exp = progress;
        }
    }
    fn give_exp(&mut self, id: String, points: i32) {
        if let Some(p) = self.player_mut(&id) {
            p.record.total_exp = (p.record.total_exp as i32 + points).max(0) as u32;
        }
    }
    fn set_allow_flight(&mut self, id: String, allow: bool) {
        if let Some(p) = self.player_mut(&id) {
            p.record.allow_flight = allow;
            if !allow {
                p.record.flying = false;
            }
        }
    }
    fn set_flying(&mut self, id: String, flying: bool) {
        if let Some(p) = self.player_mut(&id) {
            p.record.flying = flying && p.record.allow_flight;
        }
    }
    fn set_walk_speed(&mut self, _id: String, _speed: f32) {}
    fn set_fly_speed(&mut self, _id: String, _speed: f32) {}
    fn set_op(&mut self, id: String, op: bool) {
        let name = self.name(&id);
        if op {
            self.ops.insert(name);
        } else {
            self.ops.remove(&name);
        }
        self.refresh();
    }
    fn set_velocity(&mut self, _id: String, _v: types::Vec3) {}
    fn damage(&mut self, id: String, amount: f32, _cause: Option<String>) {
        if let Some(p) = self.player_mut(&id) {
            p.record.health = (p.record.health - amount).max(0.0);
        }
    }
    fn kill(&mut self, id: String) {
        if let Some(p) = self.player_mut(&id) {
            p.record.health = 0.0;
            p.record.dead = true;
        }
    }
    fn set_fire_ticks(&mut self, _id: String, _ticks: u32) {}
    fn effects(&mut self, id: String) -> Vec<players::Effect> {
        self.player_mut(&id)
            .map(|p| p.effects.clone())
            .unwrap_or_default()
    }
    fn add_effect(&mut self, id: String, effect: players::Effect) {
        if let Some(p) = self.player_mut(&id) {
            p.effects.retain(|e| e.kind != effect.kind);
            p.effects.push(effect);
        }
    }
    fn remove_effect(&mut self, id: String, kind: String) {
        if let Some(p) = self.player_mut(&id) {
            p.effects.retain(|e| e.kind != kind);
        }
    }
    fn clear_effects(&mut self, id: String) {
        if let Some(p) = self.player_mut(&id) {
            p.effects.clear();
        }
    }
    fn hide_player(&mut self, viewer: String, target: String) {
        self.hidden.insert((viewer, target));
    }
    fn show_player(&mut self, viewer: String, target: String) {
        self.hidden.remove(&(viewer, target));
    }
    fn can_see(&mut self, viewer: String, target: String) -> bool {
        !self.hidden.contains(&(viewer, target))
    }
    fn perform_command(&mut self, id: String, line: String) {
        let name = self.name(&id);
        self.performed.push(format!("{name}: {line}"));
    }
    fn chat(&mut self, id: String, message: String) {
        let name = self.name(&id);
        self.performed.push(format!("{name} says: {message}"));
    }
    fn set_respawn_location(&mut self, _id: String, _at: Option<types::Location>) {}
    fn inventory(&mut self, id: String) -> Vec<Option<types::ItemStack>> {
        self.player_mut(&id)
            .map(|p| p.inventory.clone())
            .unwrap_or_default()
    }
    fn get_slot(&mut self, id: String, slot: u32) -> Option<types::ItemStack> {
        self.player_mut(&id)?.inventory.get(slot as usize)?.clone()
    }
    fn set_slot(&mut self, id: String, slot: u32, item: Option<types::ItemStack>) {
        if let Some(p) = self.player_mut(&id)
            && let Some(s) = p.inventory.get_mut(slot as usize)
        {
            *s = item.filter(|i| i.count > 0);
        }
    }
    fn give(&mut self, id: String, items: Vec<types::ItemStack>) -> Vec<types::ItemStack> {
        let Some(p) = self.player_mut(&id) else {
            return items;
        };
        let mut rest = Vec::new();
        for mut s in items {
            for slot in p.inventory.iter_mut().take(36) {
                if s.count == 0 {
                    break;
                }
                match slot {
                    Some(have)
                        if have.id == s.id
                            && have.components == s.components
                            && have.count < 64 =>
                    {
                        let moved = (64 - have.count).min(s.count);
                        have.count += moved;
                        s.count -= moved;
                    }
                    None => {
                        let moved = s.count.min(64);
                        *slot = Some(types::ItemStack {
                            count: moved,
                            ..s.clone()
                        });
                        s.count -= moved;
                    }
                    _ => {}
                }
            }
            if s.count > 0 {
                rest.push(s);
            }
        }
        rest
    }
    fn remove_item(&mut self, id: String, item: String, mut count: u32) -> u32 {
        let Some(p) = self.player_mut(&id) else {
            return 0;
        };
        let mut removed = 0;
        for slot in p.inventory.iter_mut() {
            if count == 0 {
                break;
            }
            if let Some(s) = slot.as_mut()
                && s.id == item
            {
                let take = s.count.min(count);
                s.count -= take;
                count -= take;
                removed += take;
                if s.count == 0 {
                    *slot = None;
                }
            }
        }
        removed
    }
    fn clear_inventory(&mut self, id: String) {
        if let Some(p) = self.player_mut(&id) {
            p.inventory.iter_mut().for_each(|s| *s = None);
        }
    }
    fn ender_chest(&mut self, id: String) -> Vec<Option<types::ItemStack>> {
        self.player_mut(&id)
            .map(|p| p.ender.clone())
            .unwrap_or_default()
    }
    fn set_ender_slot(&mut self, id: String, slot: u32, item: Option<types::ItemStack>) {
        if let Some(p) = self.player_mut(&id)
            && let Some(s) = p.ender.get_mut(slot as usize)
        {
            *s = item;
        }
    }
    fn set_held_slot(&mut self, id: String, slot: u32) {
        if let Some(p) = self.player_mut(&id) {
            p.record.selected_slot = slot.min(8);
        }
    }
    fn cursor(&mut self, id: String) -> Option<types::ItemStack> {
        self.player_mut(&id)?.cursor.clone()
    }
    fn set_cursor(&mut self, id: String, item: Option<types::ItemStack>) {
        if let Some(p) = self.player_mut(&id) {
            p.cursor = item;
        }
    }
    fn close_inventory(&mut self, id: String) {
        if let Some(p) = self.player_mut(&id) {
            p.open = None;
        }
    }
    fn update_inventory(&mut self, _id: String) {}
}

impl worlds::Host for FakeServer {
    fn worlds(&mut self) -> Vec<worlds::WorldInfo> {
        [
            "minecraft:overworld",
            "minecraft:the_nether",
            "minecraft:the_end",
        ]
        .into_iter()
        .filter_map(|w| worlds::Host::get(self, w.to_owned()))
        .collect()
    }
    fn get(&mut self, world: String) -> Option<worlds::WorldInfo> {
        let environment = match world.as_str() {
            "minecraft:overworld" => worlds::Environment::Normal,
            "minecraft:the_nether" => worlds::Environment::Nether,
            "minecraft:the_end" => worlds::Environment::TheEnd,
            _ => return None,
        };
        Some(worlds::WorldInfo {
            players: self
                .players
                .iter()
                .filter(|p| p.record.location.world == world)
                .count() as u32,
            name: world,
            environment,
            seed: 12345,
            time: self.time % 24000,
            full_time: self.tick,
            storm: self.storm,
            thundering: self.thundering,
            spawn: types::Location {
                world: "minecraft:overworld".into(),
                x: 0.5,
                y: 64.0,
                z: 0.5,
                yaw: 0.0,
                pitch: 0.0,
            },
            difficulty: "normal".into(),
            min_y: -64,
            max_y: 319,
            loaded_chunks: 441,
        })
    }
    fn get_block(&mut self, world: String, pos: types::BlockPos) -> Option<String> {
        Some(self.block(&world, pos.x, pos.y, pos.z))
    }
    fn get_biome(&mut self, _world: String, _pos: types::BlockPos) -> Option<String> {
        Some("minecraft:plains".into())
    }
    fn highest_block_y(&mut self, world: String, x: i32, z: i32) -> Option<i32> {
        let top = self
            .blocks
            .iter()
            .filter(|((w, bx, _, bz), s)| {
                *w == world && *bx == x && *bz == z && *s != "minecraft:air"
            })
            .map(|((_, _, y, _), _)| *y)
            .max();
        Some(top.unwrap_or(63).max(63))
    }
    fn get_block_data(&mut self, _w: String, _pos: types::BlockPos) -> Option<String> {
        None
    }
    fn merge_block_data(&mut self, _w: String, _pos: types::BlockPos, _snbt: String) {}
    fn set_blocks(
        &mut self,
        world: String,
        changes: Vec<worlds::BlockChange>,
    ) -> Result<(), String> {
        for c in changes {
            self.blocks
                .insert((world.clone(), c.pos.x, c.pos.y, c.pos.z), c.state);
        }
        Ok(())
    }
    fn is_chunk_loaded(&mut self, _w: String, x: i32, z: i32) -> bool {
        x.abs() <= 10 && z.abs() <= 10
    }
    fn loaded_chunks(&mut self, _w: String) -> Vec<(i32, i32)> {
        Vec::new()
    }
    fn set_chunk_forced(&mut self, _w: String, _x: i32, _z: i32, _f: bool) {}
    fn set_time(&mut self, _w: String, time: u64) {
        self.time = time;
    }
    fn set_storm(&mut self, _w: String, storm: bool, _d: u32) {
        self.storm = storm;
    }
    fn set_thundering(&mut self, _w: String, thundering: bool) {
        self.thundering = thundering;
    }
    fn set_spawn(&mut self, _at: types::Location) {}
    fn set_difficulty(&mut self, _d: String) {}
    fn game_rule(&mut self, rule: String) -> Option<String> {
        self.game_rules.get(&rule).cloned()
    }
    fn set_game_rule(&mut self, rule: String, value: String) {
        self.game_rules.insert(rule, value);
    }
    fn strike_lightning(&mut self, _at: types::Location, _e: bool) {}
    fn explode(&mut self, _at: types::Location, _p: f32, _f: bool, _b: bool) {}
    fn drop_item(&mut self, at: types::Location, item: types::ItemStack, _n: bool) {
        self.next_entity += 1;
        self.entities.push(entities::Entity {
            id: self.next_entity,
            uuid: format!("10000000-0000-4000-8000-{:012x}", self.next_entity),
            kind: "minecraft:item".into(),
            location: at,
            velocity: types::Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            custom_name: Some(item.id),
            health: None,
            max_health: None,
            dead: false,
            on_ground: true,
            tags: Vec::new(),
            player: None,
        });
    }
    fn play_sound(&mut self, _at: types::Location, sound: String, _c: String, _v: f32, _p: f32) {
        self.sounds.push(format!("world: {sound}"));
    }
    fn spawn_particle(
        &mut self,
        _at: types::Location,
        _p: String,
        _c: u32,
        _o: types::Vec3,
        _s: f32,
        _f: bool,
    ) {
    }
    fn border(&mut self, _w: String) -> worlds::WorldBorder {
        worlds::WorldBorder {
            center_x: 0.0,
            center_z: 0.0,
            size: 59_999_968.0,
            damage_per_block: 0.2,
            warning_blocks: 5,
        }
    }
    fn set_border(&mut self, _w: String, _x: f64, _z: f64, _s: f64, _t: u32) {}
    fn save(&mut self) {}
}

impl entities::Host for FakeServer {
    fn get(&mut self, id: i32) -> Option<entities::Entity> {
        self.entities.iter().find(|e| e.id == id).cloned()
    }
    fn by_uuid(&mut self, uuid: String) -> Option<entities::Entity> {
        self.entities.iter().find(|e| e.uuid == uuid).cloned()
    }
    fn in_world(&mut self, world: String) -> Vec<entities::Entity> {
        self.entities
            .iter()
            .filter(|e| e.location.world == world)
            .cloned()
            .collect()
    }
    fn nearby(&mut self, at: types::Location, radius: f64) -> Vec<entities::Entity> {
        self.entities
            .iter()
            .filter(|e| {
                let l = &e.location;
                l.world == at.world
                    && (l.x - at.x).powi(2) + (l.y - at.y).powi(2) + (l.z - at.z).powi(2)
                        <= radius * radius
            })
            .cloned()
            .collect()
    }
    fn by_tag(&mut self, tag: String) -> Vec<entities::Entity> {
        self.entities
            .iter()
            .filter(|e| e.tags.contains(&tag))
            .cloned()
            .collect()
    }
    fn spawn(&mut self, at: types::Location, kind: String, _nbt: String) -> String {
        self.next_entity += 1;
        let tag = format!("hzp_spawn_{}", self.next_entity);
        self.entities.push(entities::Entity {
            id: self.next_entity,
            uuid: format!("10000000-0000-4000-8000-{:012x}", self.next_entity),
            kind,
            location: at,
            velocity: types::Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            custom_name: None,
            health: Some(20.0),
            max_health: Some(20.0),
            dead: false,
            on_ground: true,
            tags: vec![tag.clone()],
            player: None,
        });
        tag
    }
    fn teleport(&mut self, id: i32, to: types::Location) -> bool {
        match self.entities.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                e.location = to;
                true
            }
            None => false,
        }
    }
    fn remove(&mut self, id: i32) {
        self.entities.retain(|e| e.id != id);
    }
    fn set_velocity(&mut self, id: i32, v: types::Vec3) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == id) {
            e.velocity = v;
        }
    }
    fn set_custom_name(&mut self, id: i32, name: Option<String>, _visible: bool) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == id) {
            e.custom_name = name;
        }
    }
    fn set_glowing(&mut self, _id: i32, _g: bool) {}
    fn set_invulnerable(&mut self, _id: i32, _i: bool) {}
    fn set_silent(&mut self, _id: i32, _s: bool) {}
    fn set_gravity(&mut self, _id: i32, _g: bool) {}
    fn set_ai(&mut self, _id: i32, _a: bool) {}
    fn damage(&mut self, id: i32, amount: f32, _c: Option<String>) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == id)
            && let Some(h) = e.health.as_mut()
        {
            *h = (*h - amount).max(0.0);
            e.dead = *h <= 0.0;
        }
    }
    fn set_health(&mut self, id: i32, health: f32) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == id) {
            e.health = Some(health);
        }
    }
    fn add_tag(&mut self, id: i32, tag: String) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == id)
            && !e.tags.contains(&tag)
        {
            e.tags.push(tag);
        }
    }
    fn remove_tag(&mut self, id: i32, tag: String) {
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == id) {
            e.tags.retain(|t| *t != tag);
        }
    }
    fn add_passenger(&mut self, _v: i32, _p: i32) {}
    fn eject(&mut self, _id: i32) {}
    fn get_nbt(&mut self, id: i32) -> Option<String> {
        self.entities
            .iter()
            .any(|e| e.id == id)
            .then(|| "{}".into())
    }
    fn merge_nbt(&mut self, _id: i32, _snbt: String) {}
}

impl items::Host for FakeServer {
    fn parse(&mut self, text: String, count: u32) -> Result<types::ItemStack, String> {
        let id = text.split('[').next().unwrap_or_default().trim();
        if id.is_empty() {
            return Err("no item".into());
        }
        let id = if id.contains(':') {
            id.to_owned()
        } else {
            format!("minecraft:{id}")
        };
        Ok(types::ItemStack {
            id,
            count: count.max(1),
            components: String::new(),
        })
    }
    fn meta(&mut self, stack: types::ItemStack) -> items::ItemMeta {
        let c = comps(&stack);
        items::ItemMeta {
            display_name: c.get("display_name").cloned(),
            item_name: c.get("item_name").cloned(),
            lore: c
                .get("lore")
                .map(|l| l.split('\n').map(str::to_owned).collect())
                .unwrap_or_default(),
            enchantments: c
                .get("enchantments")
                .map(|e| {
                    e.split(';')
                        .filter_map(|x| x.split_once('*'))
                        .map(|(id, l)| items::Enchantment {
                            id: id.to_owned(),
                            level: l.parse().unwrap_or(1),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            damage: c.get("damage").and_then(|d| d.parse().ok()).unwrap_or(0),
            max_damage: c.get("max_damage").and_then(|d| d.parse().ok()),
            unbreakable: c.contains_key("unbreakable"),
            custom_model_data: c.get("custom_model_data").and_then(|d| d.parse().ok()),
            max_stack_size: c.get("max_stack_size").and_then(|d| d.parse().ok()),
            glint: c.get("glint").map(|g| g == "true"),
            hide_tooltip: c.contains_key("hide_tooltip"),
        }
    }
    fn with_meta(&mut self, stack: types::ItemStack, meta: items::ItemMeta) -> types::ItemStack {
        let mut c: BTreeMap<String, String> = comps(&stack)
            .into_iter()
            .filter(|(k, _)| k.starts_with("tag."))
            .collect();
        if let Some(n) = meta.display_name {
            c.insert("display_name".into(), n);
        }
        if let Some(n) = meta.item_name {
            c.insert("item_name".into(), n);
        }
        if !meta.lore.is_empty() {
            c.insert("lore".into(), meta.lore.join("\n"));
        }
        if !meta.enchantments.is_empty() {
            let e: Vec<String> = meta
                .enchantments
                .iter()
                .map(|e| format!("{}*{}", e.id, e.level))
                .collect();
            c.insert("enchantments".into(), e.join(";"));
        }
        if meta.damage > 0 {
            c.insert("damage".into(), meta.damage.to_string());
        }
        if let Some(m) = meta.max_damage {
            c.insert("max_damage".into(), m.to_string());
        }
        if meta.unbreakable {
            c.insert("unbreakable".into(), "true".into());
        }
        if let Some(m) = meta.custom_model_data {
            c.insert("custom_model_data".into(), m.to_string());
        }
        if let Some(m) = meta.max_stack_size {
            c.insert("max_stack_size".into(), m.to_string());
        }
        if let Some(g) = meta.glint {
            c.insert("glint".into(), g.to_string());
        }
        if meta.hide_tooltip {
            c.insert("hide_tooltip".into(), "true".into());
        }
        with_comps(&stack, c)
    }
    fn get_tag(&mut self, stack: types::ItemStack, key: String) -> Option<String> {
        comps(&stack)
            .get(&format!("tag.{}:{key}", self.caller))
            .cloned()
    }
    fn set_tag(
        &mut self,
        stack: types::ItemStack,
        key: String,
        value: Option<String>,
    ) -> types::ItemStack {
        let mut c = comps(&stack);
        let k = format!("tag.{}:{key}", self.caller);
        match value {
            Some(v) => {
                c.insert(k, v);
            }
            None => {
                c.remove(&k);
            }
        }
        with_comps(&stack, c)
    }
    fn similar(&mut self, a: types::ItemStack, b: types::ItemStack) -> bool {
        a.id == b.id && a.components == b.components
    }
    fn max_stack(&mut self, id: String) -> u32 {
        if id.ends_with("_sword") || id.ends_with("_pickaxe") || id.ends_with("_axe") {
            1
        } else if id.ends_with("ender_pearl") || id.ends_with("snowball") {
            16
        } else {
            64
        }
    }
    fn display_name(&mut self, stack: types::ItemStack) -> String {
        let path = stack.id.split(':').nth(1).unwrap_or(&stack.id);
        path.split('_')
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn registry(&mut self, name: String) -> Vec<String> {
        match name.as_str() {
            "item" | "block" => vec![
                "minecraft:stone".into(),
                "minecraft:dirt".into(),
                "minecraft:diamond".into(),
            ],
            _ => Vec::new(),
        }
    }
}

impl inventories::Host for FakeServer {
    fn create(&mut self, kind: inventories::InventoryKind, rows: u32, title: String) -> u32 {
        let size = match kind {
            inventories::InventoryKind::Chest => rows.clamp(1, 6) as usize * 9,
            inventories::InventoryKind::Hopper => 5,
            inventories::InventoryKind::Dropper => 9,
        };
        let id = self.next_inventory;
        self.next_inventory += 1;
        self.inventories.insert(
            id,
            FakeInventory {
                items: vec![None; size],
                title,
            },
        );
        id
    }
    fn size(&mut self, inventory: u32) -> u32 {
        self.inventories
            .get(&inventory)
            .map_or(0, |i| i.items.len() as u32)
    }
    fn get(&mut self, inventory: u32, slot: u32) -> Option<types::ItemStack> {
        self.inventories
            .get(&inventory)?
            .items
            .get(slot as usize)?
            .clone()
    }
    fn set(&mut self, inventory: u32, slot: u32, item: Option<types::ItemStack>) {
        if let Some(s) = self
            .inventories
            .get_mut(&inventory)
            .and_then(|i| i.items.get_mut(slot as usize))
        {
            *s = item;
        }
    }
    fn contents(&mut self, inventory: u32) -> Vec<Option<types::ItemStack>> {
        self.inventories
            .get(&inventory)
            .map(|i| i.items.clone())
            .unwrap_or_default()
    }
    fn set_contents(&mut self, inventory: u32, items: Vec<Option<types::ItemStack>>) {
        if let Some(inv) = self.inventories.get_mut(&inventory) {
            for (k, slot) in inv.items.iter_mut().enumerate() {
                *slot = items.get(k).cloned().flatten();
            }
        }
    }
    fn add(&mut self, inventory: u32, items: Vec<types::ItemStack>) -> Vec<types::ItemStack> {
        let Some(inv) = self.inventories.get_mut(&inventory) else {
            return items;
        };
        let mut rest = Vec::new();
        for s in items {
            match inv.items.iter_mut().find(|x| x.is_none()) {
                Some(slot) => *slot = Some(s),
                None => rest.push(s),
            }
        }
        rest
    }
    fn clear(&mut self, inventory: u32) {
        if let Some(inv) = self.inventories.get_mut(&inventory) {
            inv.items.iter_mut().for_each(|s| *s = None);
        }
    }
    fn set_title(&mut self, inventory: u32, title: String) {
        if let Some(inv) = self.inventories.get_mut(&inventory) {
            inv.title = title;
        }
    }
    fn open(&mut self, player: String, inventory: u32) -> bool {
        if !self.inventories.contains_key(&inventory) {
            return false;
        }
        match self.player_mut(&player) {
            Some(p) => {
                p.open = Some(inventory);
                true
            }
            None => false,
        }
    }
    fn viewers(&mut self, inventory: u32) -> Vec<String> {
        self.players
            .iter()
            .filter(|p| p.open == Some(inventory))
            .map(|p| p.record.id.clone())
            .collect()
    }
    fn open_inventory(&mut self, player: String) -> Option<u32> {
        self.player_mut(&player)?.open
    }
    fn destroy(&mut self, inventory: u32) {
        self.inventories.remove(&inventory);
        for p in &mut self.players {
            if p.open == Some(inventory) {
                p.open = None;
            }
        }
    }
}

impl scoreboards::Host for FakeServer {
    fn set_sidebar(&mut self, player: String, title: String, lines: Vec<String>) {
        let name = self.name(&player);
        self.sidebars.insert(
            name,
            (
                strip(&title),
                lines.iter().take(15).map(|l| strip(l)).collect(),
            ),
        );
    }
    fn clear_sidebar(&mut self, player: String) {
        let name = self.name(&player);
        self.sidebars.remove(&name);
    }
    fn set_name_tag(
        &mut self,
        target: String,
        prefix: String,
        suffix: String,
        color: Option<String>,
    ) {
        let name = self.name(&target);
        self.name_tags
            .insert(name, (strip(&prefix), strip(&suffix), color));
    }
    fn clear_name_tag(&mut self, target: String) {
        let name = self.name(&target);
        self.name_tags.remove(&name);
    }
    fn objectives(&mut self) -> Vec<scoreboards::Objective> {
        self.objectives.clone()
    }
    fn add_objective(&mut self, name: String, criteria: String, display_name: String) {
        self.objectives.push(scoreboards::Objective {
            name,
            criteria,
            display_name,
        });
    }
    fn remove_objective(&mut self, name: String) {
        self.objectives.retain(|o| o.name != name);
    }
    fn set_display_slot(&mut self, _slot: String, _o: Option<String>) {}
    fn get_score(&mut self, objective: String, holder: String) -> Option<i32> {
        self.scores.get(&(objective, holder)).copied()
    }
    fn set_score(&mut self, objective: String, holder: String, value: i32) {
        self.scores.insert((objective, holder), value);
    }
    fn reset_score(&mut self, objective: String, holder: String) {
        self.scores.remove(&(objective, holder));
    }
    fn teams(&mut self) -> Vec<String> {
        self.teams.clone()
    }
    fn add_team(&mut self, name: String, _d: String) {
        self.teams.push(name);
    }
    fn remove_team(&mut self, name: String) {
        self.teams.retain(|t| *t != name);
    }
    fn join_team(&mut self, _team: String, _member: String) {}
    fn leave_team(&mut self, _member: String) {}
    fn set_team_option(&mut self, _t: String, _k: String, _v: String) {}
}

impl bossbars::Host for FakeServer {
    fn create(
        &mut self,
        title: String,
        _c: bossbars::BarColor,
        _s: bossbars::BarStyle,
        progress: f32,
    ) -> u32 {
        self.next_bar += 1;
        self.bars
            .insert(self.next_bar, (strip(&title), progress, BTreeSet::new()));
        self.next_bar
    }
    fn set_title(&mut self, bar: u32, title: String) {
        if let Some(b) = self.bars.get_mut(&bar) {
            b.0 = strip(&title);
        }
    }
    fn set_progress(&mut self, bar: u32, progress: f32) {
        if let Some(b) = self.bars.get_mut(&bar) {
            b.1 = progress;
        }
    }
    fn set_color(&mut self, _bar: u32, _c: bossbars::BarColor) {}
    fn set_style(&mut self, _bar: u32, _s: bossbars::BarStyle) {}
    fn set_flags(&mut self, _bar: u32, _a: bool, _b: bool, _c: bool) {}
    fn add_player(&mut self, bar: u32, player: String) {
        let name = self.name(&player);
        if let Some(b) = self.bars.get_mut(&bar) {
            b.2.insert(name);
        }
    }
    fn remove_player(&mut self, bar: u32, player: String) {
        let name = self.name(&player);
        if let Some(b) = self.bars.get_mut(&bar) {
            b.2.remove(&name);
        }
    }
    fn players(&mut self, bar: u32) -> Vec<String> {
        self.bars
            .get(&bar)
            .map(|b| b.2.iter().cloned().collect())
            .unwrap_or_default()
    }
    fn remove(&mut self, bar: u32) {
        self.bars.remove(&bar);
    }
}

// ---------------------------------------------------------------------------
// The server driving one plugin.

/// One plugin (and the plugins it depends on) on a fake server.
pub struct TestServer {
    pub mods: Mods,
    pub server: FakeServer,
    dir: PathBuf,
    /// The last event's outcome, as the plugins left it.
    pub last: Option<Event>,
    /// Chat lines as players received them (`format` applied, codes stripped).
    pub chat: Vec<String>,
    /// Join and quit messages everyone read.
    pub announcements: Vec<String>,
}

impl TestServer {
    /// Loads the plugins at `paths` (a test's own copies) and enables them.
    pub fn load(paths: &[PathBuf]) -> Result<TestServer, String> {
        static RUN: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "horizon-plugin-test-{}-{}",
            std::process::id(),
            RUN.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mods_dir = dir.join("horizon-mods");
        std::fs::create_dir_all(&mods_dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut copies = Vec::new();
        for p in paths {
            let to = mods_dir.join(p.file_name().unwrap_or("plugin.hmod".as_ref()));
            copy_tree(p, &to)?;
            copies.push(to);
        }
        let mut mods = Mods::load(&copies, |_| true, |_| false);
        if let Some(e) = mods.entries().iter().find_map(|e| match &e.status {
            Status::Failed(r) => Some(r.clone()),
            _ => None,
        }) {
            return Err(format!("the plugin didn't start: {e}"));
        }
        if !mods.runs_plugins() {
            return Err("no plugin to run (api = \"0.4\", capability plugin)".into());
        }
        let mut server = FakeServer::new();
        mods.enable_plugins(&mut server);
        let test = TestServer {
            mods,
            server,
            dir,
            last: None,
            chat: Vec::new(),
            announcements: Vec::new(),
        };
        test.check_running()?;
        Ok(test)
    }

    /// The plugins' data folder root (`plugins/<id>/` under it).
    pub fn data_dir(&self) -> PathBuf {
        self.dir.join(crate::plugin::PLUGINS_DIR)
    }

    fn check_running(&self) -> Result<(), String> {
        match self.mods.entries().iter().find_map(|e| match &e.status {
            Status::Failed(r) => Some(r.clone()),
            _ => None,
        }) {
            Some(r) => Err(format!("the plugin stopped: {r}")),
            None => Ok(()),
        }
    }

    /// Why a plugin was turned off, if one was.
    pub fn stopped(&self) -> Option<String> {
        self.mods.entries().iter().find_map(|e| match &e.status {
            Status::Failed(r) => Some(r.clone()),
            _ => None,
        })
    }

    fn fire(&mut self, event: Event) -> Event {
        let out = self.mods.fire(&mut self.server, event);
        self.mods.deliver_plugin_calls(&mut self.server);
        self.last = Some(out.clone());
        out
    }

    fn player(&mut self, name: &str) -> Result<types::Player, String> {
        self.server.refresh();
        self.server
            .player_named(name)
            .map(|p| p.record.clone())
            .ok_or_else(|| format!("{name} is not online"))
    }

    /// A player joins: login, then join; the join message is announced.
    pub fn join(&mut self, name: &str) -> Result<(), String> {
        let player = self.server.add_player(name);
        let login = self.fire(Event::PlayerLogin(ev::PlayerLoginEvent {
            player: player.clone(),
            allowed: true,
            kick_message: String::new(),
        }));
        if let Event::PlayerLogin(e) = login
            && !e.allowed
        {
            self.server.players.retain(|p| p.record.name != name);
            self.server
                .kicked
                .push(format!("{name}: {}", strip(&e.kick_message)));
            return Ok(());
        }
        let default = format!("§e{name} joined the game");
        let back = self.fire(Event::PlayerJoin(ev::PlayerJoinEvent {
            player,
            message: Some(default),
            first_join: true,
        }));
        if let Event::PlayerJoin(e) = back
            && let Some(m) = e.message
        {
            self.announcements.push(strip(&m));
        }
        Ok(())
    }

    /// A player leaves: the quit message is announced.
    pub fn leave(&mut self, name: &str) -> Result<(), String> {
        let player = self.player(name)?;
        self.server.players.retain(|p| p.record.name != name);
        let default = format!("§e{name} left the game");
        let back = self.fire(Event::PlayerQuit(ev::PlayerQuitEvent {
            player: player.clone(),
            message: Some(default),
            reason: "Disconnected".into(),
        }));
        if let Event::PlayerQuit(e) = back
            && let Some(m) = e.message
        {
            self.announcements.push(strip(&m));
        }
        self.mods.plugin_player_left(&player.id);
        Ok(())
    }

    /// A player says `text`.
    pub fn chat(&mut self, name: &str, text: &str) -> Result<(), String> {
        let player = self.player(name)?;
        let everyone: Vec<String> = self
            .server
            .players
            .iter()
            .map(|p| p.record.id.clone())
            .collect();
        let back = self.fire(Event::PlayerChat(ev::PlayerChatEvent {
            player,
            message: text.to_owned(),
            format: "<{name}> {message}".into(),
            recipients: everyone,
            cancelled: false,
        }));
        if let Event::PlayerChat(e) = back
            && !e.cancelled
        {
            let line = e
                .format
                .replace("{name}", &e.player.display_name)
                .replace("{message}", &e.message);
            self.chat.push(strip(&line));
        }
        Ok(())
    }

    /// A player types `/line`: the command event, `/plugins`, then a
    /// plugin's command. False when no plugin took it (vanilla would run it).
    pub fn command(&mut self, name: &str, line: &str) -> Result<bool, String> {
        let player = self.player(name)?;
        let back = self.fire(Event::PlayerCommand(ev::PlayerCommandEvent {
            player: player.clone(),
            line: line.trim_start_matches('/').to_owned(),
            cancelled: false,
        }));
        let line = match back {
            Event::PlayerCommand(e) if e.cancelled => return Ok(true),
            Event::PlayerCommand(e) => e.line,
            _ => line.to_owned(),
        };
        let owned = self
            .mods
            .plugin_command(&mut self.server, &Sender::Player(player), &line);
        self.mods.deliver_plugin_calls(&mut self.server);
        self.check_running()?;
        Ok(owned)
    }

    /// The console types `line`.
    pub fn console(&mut self, line: &str) -> Result<bool, String> {
        let back = self.fire(Event::ServerCommand(ev::ServerCommandEvent {
            line: line.to_owned(),
            cancelled: false,
        }));
        let line = match back {
            Event::ServerCommand(e) if e.cancelled => return Ok(true),
            Event::ServerCommand(e) => e.line,
            _ => line.to_owned(),
        };
        let owned = self
            .mods
            .plugin_command(&mut self.server, &Sender::Console, &line);
        self.check_running()?;
        Ok(owned)
    }

    /// Completions for a player typing `line`.
    pub fn complete(&mut self, name: &str, line: &str) -> Result<Vec<String>, String> {
        let player = self.player(name)?;
        Ok(self
            .mods
            .plugin_tab_complete(&mut self.server, &Sender::Player(player), line)
            .unwrap_or_default())
    }

    /// `n` server ticks.
    pub fn ticks(&mut self, n: u64) -> Result<(), String> {
        for _ in 0..n {
            self.server.tick += 1;
            self.server.time += 1;
            let tick = self.server.tick;
            self.mods.plugin_tick(&mut self.server, tick);
            self.check_running()?;
        }
        Ok(())
    }

    /// A player breaks a block; it goes unless cancelled.
    pub fn break_block(&mut self, name: &str, pos: types::BlockPos) -> Result<bool, String> {
        let player = self.player(name)?;
        let world = player.location.world.clone();
        let state = self.server.block(&world, pos.x, pos.y, pos.z);
        let back = self.fire(Event::BlockBreak(ev::BlockBreakEvent {
            player,
            block: types::Block {
                world: world.clone(),
                pos,
                state,
            },
            drop_items: true,
            exp: 0,
            cancelled: false,
        }));
        let cancelled = plugin::is_cancelled(&back);
        if !cancelled {
            self.server
                .blocks
                .insert((world, pos.x, pos.y, pos.z), "minecraft:air".into());
        }
        Ok(!cancelled)
    }

    /// A player places `state` at `pos`; it stays unless cancelled.
    pub fn place_block(
        &mut self,
        name: &str,
        pos: types::BlockPos,
        state: &str,
    ) -> Result<bool, String> {
        let player = self.player(name)?;
        let world = player.location.world.clone();
        let replaced = self.server.block(&world, pos.x, pos.y, pos.z);
        let back = self.fire(Event::BlockPlace(ev::BlockPlaceEvent {
            item: player.held.clone(),
            player,
            block: types::Block {
                world: world.clone(),
                pos,
                state: state.to_owned(),
            },
            replaced,
            hand: types::Hand::Main,
            cancelled: false,
        }));
        let cancelled = plugin::is_cancelled(&back);
        if !cancelled {
            self.server
                .blocks
                .insert((world, pos.x, pos.y, pos.z), state.to_owned());
        }
        Ok(!cancelled)
    }

    /// A player moves to `to`; the move happens unless cancelled (or where
    /// a plugin sent them).
    pub fn move_to(&mut self, name: &str, to: types::Location) -> Result<(), String> {
        let player = self.player(name)?;
        let from = player.location.clone();
        let back = self.fire(Event::PlayerMove(ev::PlayerMoveEvent {
            player: player.clone(),
            from: from.clone(),
            to: to.clone(),
            cancelled: false,
        }));
        let at = match back {
            Event::PlayerMove(e) if e.cancelled => from,
            Event::PlayerMove(e) => e.to,
            _ => to,
        };
        players::Host::teleport(&mut self.server, player.id, at);
        Ok(())
    }

    /// A player clicks slot `slot` of the plugin inventory they have open.
    pub fn click(&mut self, name: &str, slot: i32, right: bool) -> Result<bool, String> {
        let player = self.player(name)?;
        let open = self.server.player_named(name).and_then(|p| p.open);
        let (size, current) = match open.and_then(|i| self.server.inventories.get(&i)) {
            Some(inv) => (
                inv.items.len() as u32,
                usize::try_from(slot)
                    .ok()
                    .and_then(|s| inv.items.get(s).cloned().flatten()),
            ),
            None => (0, None),
        };
        let kind = match size {
            0 => "minecraft:inventory".to_owned(),
            5 => "minecraft:hopper".to_owned(),
            n => format!("minecraft:generic_9x{}", n / 9),
        };
        let back = self.fire(Event::InventoryClick(ev::InventoryClickEvent {
            player,
            view: ev::InventoryView {
                kind,
                plugin_inventory: open,
                size,
            },
            slot,
            button: u32::from(right),
            mode: ev::ClickMode::Pickup,
            current,
            cursor: None,
            cancelled: false,
        }));
        self.check_running()?;
        Ok(!plugin::is_cancelled(&back))
    }

    /// A player takes `amount` of damage of type `cause`: false when cancelled.
    pub fn damage(&mut self, name: &str, cause: &str, amount: f32) -> Result<bool, String> {
        let player = self.player(name)?;
        let back = self.fire(Event::EntityDamage(ev::EntityDamageEvent {
            entity: types::EntityRef {
                id: player.entity_id,
                kind: "minecraft:player".into(),
                player: Some(player.id.clone()),
            },
            cause: cause.to_owned(),
            damage: amount,
            damager: None,
            cancelled: false,
        }));
        match back {
            Event::EntityDamage(e) if e.cancelled => Ok(false),
            Event::EntityDamage(e) => {
                players::Host::damage(&mut self.server, player.id, e.damage, None);
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    /// Stops the plugins, as the server does when it stops.
    pub fn stop(&mut self) {
        self.mods.disable_plugins(&mut self.server);
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for e in std::fs::read_dir(from)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to)
            .map(drop)
            .map_err(|e| format!("{}: {e}", from.display()))
    }
}

// ---------------------------------------------------------------------------
// Scripts.

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
struct Pos {
    x: i32,
    y: i32,
    z: i32,
}
impl From<Pos> for types::BlockPos {
    fn from(p: Pos) -> Self {
        types::BlockPos {
            x: p.x,
            y: p.y,
            z: p.z,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
struct PlaceStep {
    x: i32,
    y: i32,
    z: i32,
    state: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
struct MoveStep {
    x: f64,
    y: f64,
    z: f64,
    #[serde(default)]
    world: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
struct ClickStep {
    slot: i32,
    #[serde(default)]
    right: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
struct DamageStep {
    amount: f32,
    #[serde(default = "generic")]
    cause: String,
}
fn generic() -> String {
    "minecraft:generic".into()
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct SlotExpect {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    empty: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct InventoryExpect {
    player: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    size: Option<u32>,
    #[serde(default)]
    slots: BTreeMap<String, SlotExpect>,
    /// No plugin inventory open.
    #[serde(default)]
    closed: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct SidebarExpect {
    player: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    lines: Option<Vec<String>>,
    #[serde(default)]
    none: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct PlayerExpect {
    name: String,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    z: Option<f64>,
    #[serde(default)]
    world: Option<String>,
    #[serde(default)]
    health: Option<f32>,
    #[serde(default)]
    food: Option<u32>,
    #[serde(default)]
    game_mode: Option<String>,
    #[serde(default)]
    flying: Option<bool>,
    #[serde(default)]
    allow_flight: Option<bool>,
    #[serde(default)]
    permission: Option<String>,
    #[serde(default)]
    has_permission: Option<bool>,
    #[serde(default)]
    online: Option<bool>,
    #[serde(default)]
    slots: BTreeMap<String, SlotExpect>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Expect {
    /// Messages to players since the step began: `Name: text`.
    told: Option<Vec<String>>,
    /// Lines that must be among them.
    told_contains: Option<Vec<String>>,
    action_bars: Option<Vec<String>>,
    titles: Option<Vec<String>>,
    console: Option<Vec<String>>,
    dispatched: Option<Vec<String>>,
    performed: Option<Vec<String>>,
    chat: Option<Vec<String>>,
    announcements: Option<Vec<String>>,
    kicked: Option<Vec<String>>,
    sounds: Option<Vec<String>>,
    /// The step's last event was cancelled (or not).
    cancelled: Option<bool>,
    /// The command was a plugin's (or not).
    handled: Option<bool>,
    completions: Option<Vec<String>>,
    blocks: Option<Vec<PlaceStep>>,
    inventory: Option<InventoryExpect>,
    sidebar: Option<SidebarExpect>,
    player: Option<PlayerExpect>,
    /// Boss bars' titles and who sees them: `title: A, B`.
    bars: Option<Vec<String>>,
    /// A file of the data folder holds this text.
    file: Option<BTreeMap<String, String>>,
    stopped: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Step {
    #[serde(default)]
    op: Vec<String>,
    #[serde(default)]
    join: Vec<String>,
    #[serde(default, rename = "as")]
    who: Option<String>,
    #[serde(default)]
    chat: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    console: Option<String>,
    #[serde(default)]
    complete: Option<String>,
    #[serde(default, rename = "break")]
    break_at: Option<Pos>,
    #[serde(default)]
    place: Option<PlaceStep>,
    #[serde(default, rename = "move")]
    move_to: Option<MoveStep>,
    #[serde(default)]
    click: Option<ClickStep>,
    #[serde(default)]
    damage: Option<DamageStep>,
    #[serde(default)]
    ticks: u64,
    #[serde(default)]
    leave: Vec<String>,
    #[serde(default)]
    stop: bool,
    #[serde(default)]
    expect: Expect,
}

/// A plugin test: `tests/<name>.toml`, steps run in order on one server.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct PluginScript {
    /// Other plugins (paths relative to the script) loaded beside it.
    #[serde(default)]
    pub with: Vec<String>,
    #[serde(rename = "step")]
    steps: Vec<Step>,
}

impl PluginScript {
    pub fn parse(text: &str) -> Result<PluginScript, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }
    pub fn steps(&self) -> usize {
        self.steps.len()
    }
    /// Runs every step on `server`; the first wrong expectation fails it.
    pub fn run(&self, server: &mut TestServer) -> Result<(), String> {
        for (i, step) in self.steps.iter().enumerate() {
            step.run(server)
                .map_err(|e| format!("step {}: {e}", i + 1))?;
        }
        Ok(())
    }
}

fn same<T: PartialEq + std::fmt::Debug>(what: &str, want: &T, got: &T) -> Result<(), String> {
    if want == got {
        Ok(())
    } else {
        Err(format!("{what}: expected {want:?}, got {got:?}"))
    }
}

fn check_slot(
    what: &str,
    want: &SlotExpect,
    got: Option<&types::ItemStack>,
    server: &mut FakeServer,
) -> Result<(), String> {
    if want.empty {
        return match got {
            None => Ok(()),
            Some(s) => Err(format!("{what}: expected empty, got {}", s.id)),
        };
    }
    let Some(s) = got else {
        return Err(format!("{what}: expected an item, got none"));
    };
    if let Some(id) = &want.id {
        let id = if id.contains(':') {
            id.clone()
        } else {
            format!("minecraft:{id}")
        };
        same(&format!("{what} item"), &id, &s.id)?;
    }
    if let Some(c) = want.count {
        same(&format!("{what} count"), &c, &s.count)?;
    }
    if let Some(n) = &want.name {
        let meta = items::Host::meta(server, s.clone());
        let name = meta
            .item_name
            .or(meta.display_name)
            .map(|n| strip(&n))
            .unwrap_or_default();
        same(&format!("{what} name"), n, &name)?;
    }
    Ok(())
}

impl Step {
    fn run(&self, t: &mut TestServer) -> Result<(), String> {
        let told = t.server.told.len();
        let bars = t.server.action_bars.len();
        let titles = t.server.titles.len();
        let console = t.server.console.len();
        let dispatched = t.server.dispatched.len();
        let performed = t.server.performed.len();
        let chat = t.chat.len();
        let announcements = t.announcements.len();
        let kicked = t.server.kicked.len();
        let sounds = t.server.sounds.len();
        t.last = None;
        for name in &self.op {
            t.server.ops.insert(name.clone());
        }
        for name in &self.join {
            t.join(name)?;
        }
        let who = || -> Result<String, String> {
            match &self.who {
                Some(w) => Ok(w.clone()),
                None => t
                    .server
                    .players
                    .first()
                    .map(|p| p.record.name.clone())
                    .ok_or_else(|| "nobody is online (join first)".to_owned()),
            }
        };
        let who = if self.chat.is_some()
            || self.command.is_some()
            || self.complete.is_some()
            || self.break_at.is_some()
            || self.place.is_some()
            || self.move_to.is_some()
            || self.click.is_some()
            || self.damage.is_some()
        {
            Some(who()?)
        } else {
            None
        };
        let name = who.as_deref().unwrap_or_default();
        let mut handled = None;
        let mut completions = None;
        if let Some(m) = &self.move_to {
            let at = t.player(name)?.location;
            t.move_to(
                name,
                types::Location {
                    world: m.world.clone().unwrap_or(at.world),
                    x: m.x,
                    y: m.y,
                    z: m.z,
                    yaw: at.yaw,
                    pitch: at.pitch,
                },
            )?;
        }
        if let Some(text) = &self.chat {
            t.chat(name, text)?;
        }
        if let Some(line) = &self.command {
            handled = Some(t.command(name, line)?);
        }
        if let Some(line) = &self.console {
            handled = Some(t.console(line)?);
        }
        if let Some(line) = &self.complete {
            completions = Some(t.complete(name, line)?);
        }
        if let Some(p) = self.break_at {
            t.break_block(name, p.into())?;
        }
        if let Some(p) = &self.place {
            t.place_block(
                name,
                types::BlockPos {
                    x: p.x,
                    y: p.y,
                    z: p.z,
                },
                &p.state,
            )?;
        }
        if let Some(c) = &self.click {
            t.click(name, c.slot, c.right)?;
        }
        if let Some(d) = &self.damage {
            t.damage(name, &d.cause, d.amount)?;
        }
        t.ticks(self.ticks)?;
        for name in &self.leave {
            t.leave(name)?;
        }
        if self.stop {
            t.stop();
        }
        let e = &self.expect;
        match (&e.stopped, t.stopped()) {
            (Some(want), Some(why)) if why.contains(want.as_str()) => {}
            (Some(want), got) => {
                return Err(format!("expected the plugin stopped ({want}), got {got:?}"));
            }
            (None, Some(why)) => return Err(format!("the plugin stopped: {why}")),
            (None, None) => {}
        }
        let since = |all: &[String], from: usize| all[from..].to_vec();
        if let Some(want) = &e.told {
            same("told", want, &since(&t.server.told, told))?;
        }
        if let Some(want) = &e.told_contains {
            let got = since(&t.server.told, told);
            for w in want {
                if !got.contains(w) {
                    return Err(format!("told: expected {w:?} among {got:?}"));
                }
            }
        }
        if let Some(want) = &e.action_bars {
            same("action bars", want, &since(&t.server.action_bars, bars))?;
        }
        if let Some(want) = &e.titles {
            same("titles", want, &since(&t.server.titles, titles))?;
        }
        if let Some(want) = &e.console {
            same("console", want, &since(&t.server.console, console))?;
        }
        if let Some(want) = &e.dispatched {
            same("dispatched", want, &since(&t.server.dispatched, dispatched))?;
        }
        if let Some(want) = &e.performed {
            same("performed", want, &since(&t.server.performed, performed))?;
        }
        if let Some(want) = &e.chat {
            same("chat", want, &since(&t.chat, chat))?;
        }
        if let Some(want) = &e.announcements {
            same(
                "announcements",
                want,
                &since(&t.announcements, announcements),
            )?;
        }
        if let Some(want) = &e.kicked {
            same("kicked", want, &since(&t.server.kicked, kicked))?;
        }
        if let Some(want) = &e.sounds {
            same("sounds", want, &since(&t.server.sounds, sounds))?;
        }
        if let Some(want) = e.cancelled {
            let got = t.last.as_ref().is_some_and(plugin::is_cancelled);
            same("cancelled", &want, &got)?;
        }
        if let Some(want) = e.handled {
            same("handled", &Some(want), &handled)?;
        }
        if let Some(want) = &e.completions {
            same("completions", &Some(want.clone()), &completions)?;
        }
        if let Some(blocks) = &e.blocks {
            for b in blocks {
                let got = t.server.block("minecraft:overworld", b.x, b.y, b.z);
                same(&format!("block {} {} {}", b.x, b.y, b.z), &b.state, &got)?;
            }
        }
        if let Some(inv) = &e.inventory {
            let open = t.server.player_named(&inv.player).and_then(|p| p.open);
            if inv.closed {
                same(&format!("{}'s open inventory", inv.player), &None, &open)?;
            } else {
                let id =
                    open.ok_or_else(|| format!("{} has no plugin inventory open", inv.player))?;
                let shown = t
                    .server
                    .inventories
                    .get(&id)
                    .cloned()
                    .ok_or("the open inventory is gone")?;
                if let Some(title) = &inv.title {
                    same("inventory title", title, &strip(&shown.title))?;
                }
                if let Some(size) = inv.size {
                    same("inventory size", &size, &(shown.items.len() as u32))?;
                }
                for (slot, want) in &inv.slots {
                    let k: usize = slot.parse().map_err(|_| format!("slot {slot:?}"))?;
                    check_slot(
                        &format!("slot {k}"),
                        want,
                        shown.items.get(k).and_then(Option::as_ref),
                        &mut t.server,
                    )?;
                }
            }
        }
        if let Some(s) = &e.sidebar {
            let got = t.server.sidebars.get(&s.player).cloned();
            if s.none {
                same(&format!("{}'s sidebar", s.player), &None, &got)?;
            } else {
                let (title, lines) = got.ok_or_else(|| format!("{} has no sidebar", s.player))?;
                if let Some(want) = &s.title {
                    same("sidebar title", want, &title)?;
                }
                if let Some(want) = &s.lines {
                    same("sidebar lines", want, &lines)?;
                }
            }
        }
        if let Some(p) = &e.player {
            let fake = t.server.player_named(&p.name).cloned();
            if let Some(online) = p.online {
                same(&format!("{} online", p.name), &online, &fake.is_some())?;
            }
            if let Some(f) = fake {
                let r = &f.record;
                let near = |what: &str, want: Option<f64>, got: f64| -> Result<(), String> {
                    match want {
                        Some(w) if (w - got).abs() > 1e-6 => {
                            Err(format!("{} {what}: expected {w}, got {got}", p.name))
                        }
                        _ => Ok(()),
                    }
                };
                near("x", p.x, r.location.x)?;
                near("y", p.y, r.location.y)?;
                near("z", p.z, r.location.z)?;
                if let Some(w) = &p.world {
                    same("world", w, &r.location.world)?;
                }
                if let Some(h) = p.health {
                    same("health", &h, &r.health)?;
                }
                if let Some(food) = p.food {
                    same("food", &food, &r.food)?;
                }
                if let Some(m) = &p.game_mode {
                    let got = match r.game_mode {
                        types::GameMode::Survival => "survival",
                        types::GameMode::Creative => "creative",
                        types::GameMode::Adventure => "adventure",
                        types::GameMode::Spectator => "spectator",
                    };
                    same("game mode", &m.as_str(), &got)?;
                }
                if let Some(v) = p.flying {
                    same("flying", &v, &r.flying)?;
                }
                if let Some(v) = p.allow_flight {
                    same("allow flight", &v, &r.allow_flight)?;
                }
                if let (Some(node), Some(want)) = (&p.permission, p.has_permission) {
                    let got = t.mods.has_permission(&mut t.server, &r.id, node);
                    same(&format!("permission {node}"), &want, &got)?;
                }
                for (slot, want) in &p.slots {
                    let k: usize = slot.parse().map_err(|_| format!("slot {slot:?}"))?;
                    check_slot(
                        &format!("{}'s slot {k}", p.name),
                        want,
                        f.inventory.get(k).and_then(Option::as_ref),
                        &mut t.server,
                    )?;
                }
            }
        }
        if let Some(want) = &e.bars {
            let got: Vec<String> = t
                .server
                .bars
                .values()
                .map(|(title, _, players)| {
                    format!(
                        "{title}: {}",
                        players.iter().cloned().collect::<Vec<_>>().join(", ")
                    )
                })
                .collect();
            same("bars", want, &got)?;
        }
        if let Some(files) = &e.file {
            for (path, want) in files {
                let full = t.data_dir().join(path);
                let text = std::fs::read_to_string(&full)
                    .map_err(|e| format!("{}: {e}", full.display()))?;
                if !text.contains(want.as_str()) {
                    return Err(format!(
                        "{path}: expected it to hold {want:?}, it holds {text:?}"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Every kind's listener fires: the kinds plugins may register for.
pub const KINDS: [EventKind; 32] = [
    EventKind::PlayerJoin,
    EventKind::PlayerQuit,
    EventKind::PlayerLogin,
    EventKind::PlayerChat,
    EventKind::PlayerCommand,
    EventKind::PlayerMove,
    EventKind::PlayerInteract,
    EventKind::PlayerInteractEntity,
    EventKind::PlayerToggleSneak,
    EventKind::PlayerToggleSprint,
    EventKind::PlayerToggleFlight,
    EventKind::PlayerItemHeld,
    EventKind::PlayerDropItem,
    EventKind::PlayerSwapHands,
    EventKind::PlayerRespawn,
    EventKind::PlayerDeath,
    EventKind::PlayerGameModeChange,
    EventKind::PlayerChangedWorld,
    EventKind::SignChange,
    EventKind::PluginMessage,
    EventKind::PlayerResourcePack,
    EventKind::BlockBreak,
    EventKind::BlockPlace,
    EventKind::EntityDamage,
    EventKind::EntityDeath,
    EventKind::EntitySpawn,
    EventKind::EntityExplode,
    EventKind::InventoryClick,
    EventKind::InventoryClose,
    EventKind::ServerCommand,
    EventKind::PluginState,
    EventKind::PluginCall,
];
