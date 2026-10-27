//! Plugins on the server (docs/plugins.md): Bukkit-style plugins in the
//! game that simulates the world (the dedicated server, single player's
//! server thread, a LAN host).
//!
//! The runtime (`horizon_mods::plugin`) keeps the plugins' own state; this
//! file is the server's side: it lends the game to their calls
//! ([`PluginCtx`], the host traits in `plugin_host.rs`), fires events at the
//! points the game reaches, and makes the changes plugins ask for.
//!
//! Reads are live: a plugin sees the server as it is during its call. Most
//! changes are queued ([`Action`]) and made between two server ticks, where
//! no player's turn is under way and the console can run commands; messages
//! and inventory changes to a player who isn't acting go out at once.
use super::Game;

/// Where text a plugin hands over becomes a component: legacy `§` codes
/// (or `&` codes when the text holds no `§`), or JSON.
pub(super) fn plugin_component(text: &str) -> serde_json::Value {
    let t = text.trim_start();
    if (t.starts_with('{') || t.starts_with('[') || t.starts_with('"'))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(t)
    {
        return v;
    }
    let text = if text.contains('§') {
        text.to_owned()
    } else {
        amp_codes(text)
    };
    let parsed = horizon_client::text::Text::legacy(&text);
    // Plain text stays a plain string component.
    if parsed.spans.len() == 1 && parsed.spans[0].style == Default::default() {
        return serde_json::Value::String(parsed.spans[0].text.clone());
    }
    if parsed.spans.is_empty() {
        return serde_json::Value::String(String::new());
    }
    parsed.component()
}

/// `&a` as `§a` (Bukkit's `translateAlternateColorCodes`).
pub(super) fn amp_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&'
            && chars
                .peek()
                .is_some_and(|n| "0123456789abcdefklmnorABCDEFKLMNOR".contains(*n))
        {
            out.push('§');
            out.push(chars.next().unwrap_or('r').to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `UUID.toString()`.
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
pub(super) fn uuid_str(u: u128) -> String {
    let h = format!("{u:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}
/// A UUID in either form, dashed or not.
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
pub(super) fn parse_uuid(s: &str) -> Option<u128> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    (hex.len() == 32)
        .then(|| u128::from_str_radix(&hex, 16).ok())
        .flatten()
}

/// Plugin text as an NBT text component.
pub(super) fn plugin_text_tag(text: &str) -> horizon_world::nbt::Tag {
    horizon_client::lan::data::component_nbt(&plugin_component(text))
}
/// Text without its `§` codes.
pub(super) fn strip_codes(text: &str) -> String {
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

impl Game<'_> {
    /// `EntitySpawnEvent` for a mob about to come into the world: false
    /// when a plugin cancelled it.
    pub(super) fn plugin_allows_spawn(
        &mut self,
        _mob: &horizon_client::mobs::Mob,
        _reason: &str,
    ) -> bool {
        #[cfg(feature = "mods")]
        return self.plugin_spawn_allowed(_mob, _reason);
        #[cfg(not(feature = "mods"))]
        true
    }
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn plugins_before_tick(&mut self) {}
    pub(super) fn plugins_after_tick(&mut self) {}
    pub(super) fn plugins_stop(&mut self) {}
}

#[cfg(feature = "mods")]
pub(super) use run::*;

#[cfg(feature = "mods")]
mod run {
    use super::{Game, parse_uuid, plugin_component, uuid_str};
    use crate::game::commands::Target;
    use crate::game::lan::LanHost;
    use horizon_client::{
        items::ItemStack,
        lan::{data::component_nbt, guest::Guest},
    };
    use horizon_mods::{
        Mods,
        plugin::{self, Event, EventKind, types},
    };
    use horizon_protocol::packets::play::{self, Clientbound as C};
    use horizon_world::nbt::Tag;

    /// The temporary scoreboard tag that names one entity in a command.
    const TARGET_TAG: &str = "hzp_target";
    use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
    use std::time::Instant;

    /// A change a plugin asked for, made between two ticks.
    #[derive(Clone, Debug)]
    pub(in crate::game) enum Action {
        /// `dispatch-command`: as the console, its output in the log.
        Console(String),
        /// A command the API runs for a plugin, quietly.
        Quiet(String),
        /// A packet to one player.
        Packet(u128, Box<C>),
        Kick(u128, Tag),
        Teleport(u128, types::Location),
        Health(u128, f32),
        Food(u128, u32),
        Saturation(u128, f32),
        Slot(u128, u32, Option<ItemStack>),
        EnderSlot(u128, u32, Option<ItemStack>),
        Cursor(u128, Option<ItemStack>),
        HeldSlot(u128, u32),
        Give(u128, Vec<ItemStack>),
        RemoveItem(u128, String, u32),
        ClearInventory(u128),
        CloseMenu(u128),
        OpenInventory(u128, u32),
        UpdateInventory(u128),
        Flight(u128, Option<bool>, Option<bool>),
        Velocity(u128, [f64; 3]),
        FlySpeed(u128, f32),
        Blocks(String, Vec<horizon_mods::BlockChange>),
        Shutdown,
        Fire(u128, u32),
        /// The viewer's client re-learns the target (hidden or shown again).
        Retrack(u128, u128),
        PlayerCommand(u128, String),
        PlayerChat(u128, String),
        ClearSpawn(u128),
        LightningEffect(types::Location),
        Explode(types::Location, f32, bool, bool),
        /// A command with `{e}` naming the entity with this network id.
        EntityCommand(i32, String),
        EntityNbt(i32, Tag),
        EntityRemove(i32),
        Ride(i32, i32),
        Eject(i32),
        DestroyInventory(u32),
    }

    /// A boss bar a plugin shows.
    #[derive(Clone, Debug)]
    pub(in crate::game) struct Bar {
        pub uuid: u128,
        pub title: String,
        pub progress: f32,
        pub color: i32,
        pub style: i32,
        pub flags: (bool, bool, bool),
        pub players: BTreeSet<u128>,
    }

    /// The server's plugin state, beside the runtime's.
    #[derive(Default)]
    pub(in crate::game) struct PluginGame {
        pub actions: VecDeque<Action>,
        /// The players as last seen with the hub in hand, for calls made
        /// where it isn't (a player's turn deep in the gameplay code).
        pub cache: Vec<types::Player>,
        /// Each player's location at the last move check, by connection.
        pub moved_from: HashMap<u32, types::Location>,
        /// Sneaking, sprinting and flying at the last check, by connection.
        pub toggles: HashMap<u32, (bool, bool, bool)>,
        pub display_names: HashMap<u128, String>,
        pub list_names: HashMap<u128, String>,
        /// (viewer, target) pairs a plugin hid.
        pub hidden: BTreeSet<(u128, u128)>,
        pub bars: BTreeMap<u32, Bar>,
        pub next_bar: u32,
        /// Per player sidebars: title and lines, as last sent.
        pub sidebars: HashMap<u128, (String, Vec<String>)>,
        /// Name tags: prefix, suffix, colour.
        pub name_tags: HashMap<u128, (String, String, Option<i32>)>,
        pub spawned: u64,
        pub motd: Option<String>,
        pub max_players: Option<u32>,
        /// The plugin whose call is under way.
        pub caller: String,
        /// Tick start times for the TPS, and the last ticks' lengths.
        pub tick_starts: VecDeque<Instant>,
        pub tick_lengths: VecDeque<f64>,
        pub tick_started: Option<Instant>,
        /// Connections whose inventory must be sent again after a cancelled action.
        pub resync: BTreeSet<u32>,
        /// Blocks to send again after a cancelled break or place.
        pub resend_blocks: Vec<(String, [i32; 3])>,
        /// A break a plugin cancelled the drops of.
        pub no_drops: bool,
        /// Putting back blocks a plugin's cancelled block event changed: the
        /// level reports no changes meanwhile.
        pub reverting: bool,
        /// The bed each guest lies in, as last seen (`PlayerBedLeaveEvent`).
        pub beds: BTreeMap<u32, [i32; 3]>,
        pub started: bool,
        pub stop: bool,
        /// Players whose join the plugins already heard, by connection.
        pub joined: BTreeSet<u32>,
        /// The dedicated server's lists (ops, whitelist, bans), when there is one.
        pub lists: Option<std::sync::Arc<std::sync::Mutex<crate::game::dedicated::Lists>>>,
        /// Console lines for the dedicated server's console (its own
        /// commands: whitelist, ban, op, stop ...), run between ticks.
        pub actions_console: Vec<String>,
        /// Players joining for the first time, by connection, until their join event.
        pub newcomers: BTreeSet<u32>,
        /// Levels a death kept (`keep-level`), restored at the respawn.
        pub kept_levels: HashMap<u32, (i32, f32, i32)>,
        /// A damage a plugin changed for the attack under way.
        pub damage_override: Option<f32>,
    }

    /// The game lent to one plugin call.
    pub(in crate::game) struct PluginCtx<'a, 'g> {
        pub game: &'a mut Game<'g>,
        /// The hub, when the caller holds it out of `game.lan`.
        pub hub: Option<&'a mut LanHost>,
    }

    impl PluginCtx<'_, '_> {
        pub(in crate::game) fn lan(&mut self) -> Option<&mut LanHost> {
            match self.hub.as_deref_mut() {
                Some(h) => Some(h),
                None => self.game.lan.as_mut(),
            }
        }
        pub(in crate::game) fn lan_ref(&self) -> Option<&LanHost> {
            self.hub.as_deref().or(self.game.lan.as_ref())
        }
        /// The guest with this UUID: its index in the hub.
        pub(in crate::game) fn guest_index(&self, uuid: u128) -> Option<usize> {
            self.lan_ref()?
                .hub
                .guests
                .iter()
                .position(|g| g.uuid == uuid && g.placed)
        }
        /// The player with this UUID as a plugin reads them.
        pub(in crate::game) fn player(&self, uuid: u128) -> Option<types::Player> {
            match self.lan_ref() {
                Some(h) => {
                    let g = h.hub.guests.iter().find(|g| g.uuid == uuid && g.placed)?;
                    Some(self.game.plugin_player_of(h, g))
                }
                None => self
                    .game
                    .mods
                    .plugin
                    .cache
                    .iter()
                    .find(|p| parse_uuid(&p.id) == Some(uuid))
                    .cloned(),
            }
        }
        pub(in crate::game) fn players(&self) -> Vec<types::Player> {
            match self.lan_ref() {
                Some(h) => h
                    .hub
                    .guests
                    .iter()
                    .filter(|g| g.placed)
                    .map(|g| self.game.plugin_player_of(h, g))
                    .collect(),
                None => self.game.mods.plugin.cache.clone(),
            }
        }
        /// Queues a change for the end of the tick.
        pub(in crate::game) fn queue(&mut self, action: Action) {
            self.game.mods.plugin.actions.push_back(action);
        }
        /// A packet to a player: now when possible, else at the tick's end.
        pub(in crate::game) fn send(&mut self, uuid: u128, packet: C) {
            if let Some(i) = self.guest_index(uuid)
                && let Some(h) = self.lan()
            {
                h.hub.guests[i].send(&packet);
                return;
            }
            self.queue(Action::Packet(uuid, Box::new(packet)));
        }
        /// A command quietly at the tick's end.
        pub(in crate::game) fn command(&mut self, line: String) {
            self.queue(Action::Quiet(line));
        }
        /// The name of the player with this UUID, for commands.
        pub(in crate::game) fn name_of(&self, uuid: u128) -> Option<String> {
            match self.lan_ref() {
                Some(h) => h
                    .hub
                    .guests
                    .iter()
                    .find(|g| g.uuid == uuid)
                    .map(|g| g.name.clone()),
                None => self
                    .game
                    .mods
                    .plugin
                    .cache
                    .iter()
                    .find(|p| parse_uuid(&p.id) == Some(uuid))
                    .map(|p| p.name.clone()),
            }
        }
    }

    /// A plugin's item stack as the game's; None for air or an empty stack.
    pub(in crate::game) fn stack_in(s: &types::ItemStack) -> Option<ItemStack> {
        let id = if s.id.contains(':') {
            s.id.clone()
        } else {
            format!("minecraft:{}", s.id)
        };
        if s.count == 0 || id == "minecraft:air" {
            return None;
        }
        let mut stack = ItemStack::new(&id, s.count);
        if !s.components.trim().is_empty() {
            let (tag, _) = horizon_world::nbt::parse_snbt(&s.components).ok()?;
            let (damage, extra) = horizon_client::item_data::decode_components(Some(&tag));
            stack.damage = damage;
            stack.extra = extra;
        }
        Some(stack)
    }
    /// The game's item stack as a plugin's.
    pub(in crate::game) fn stack_out(s: &ItemStack) -> types::ItemStack {
        let components = horizon_client::item_data::encode_components(s)
            .map(|t| snbt(&t))
            .unwrap_or_default();
        types::ItemStack {
            id: s.item.to_string(),
            count: s.count,
            components,
        }
    }
    pub(in crate::game) fn stacks_out(s: &[Option<ItemStack>]) -> Vec<Option<types::ItemStack>> {
        s.iter().map(|s| s.as_ref().map(stack_out)).collect()
    }

    /// SNBT that `parse_snbt` reads back: keys quoted when they need it.
    pub(in crate::game) fn snbt(tag: &Tag) -> String {
        fn key(k: &str) -> String {
            let plain = !k.is_empty()
                && k.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.+".contains(&b));
            if plain {
                k.to_owned()
            } else {
                format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""))
            }
        }
        fn go(tag: &Tag, out: &mut String) {
            use std::fmt::Write as _;
            match tag {
                Tag::End => {}
                Tag::Byte(v) => write!(out, "{v}b").unwrap_or(()),
                Tag::Short(v) => write!(out, "{v}s").unwrap_or(()),
                Tag::Int(v) => write!(out, "{v}").unwrap_or(()),
                Tag::Long(v) => write!(out, "{v}L").unwrap_or(()),
                Tag::Float(v) => write!(out, "{}f", f32::from_bits(*v)).unwrap_or(()),
                Tag::Double(v) => write!(out, "{}d", f64::from_bits(*v)).unwrap_or(()),
                Tag::String(s) => write!(
                    out,
                    "\"{}\"",
                    s.to_string_lossy()
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                )
                .unwrap_or(()),
                Tag::ByteArray(v) => {
                    out.push_str("[B;");
                    out.push_str(
                        &v.iter()
                            .map(|b| format!("{}b", *b as i8))
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    out.push(']');
                }
                Tag::IntArray(v) => {
                    out.push_str("[I;");
                    out.push_str(&v.iter().map(i32::to_string).collect::<Vec<_>>().join(","));
                    out.push(']');
                }
                Tag::LongArray(v) => {
                    out.push_str("[L;");
                    out.push_str(
                        &v.iter()
                            .map(|l| format!("{l}L"))
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    out.push(']');
                }
                Tag::List(items) => {
                    out.push('[');
                    for (i, t) in items.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        go(t, out);
                    }
                    out.push(']');
                }
                Tag::Compound(map) => {
                    let mut keys: Vec<(String, &Tag)> =
                        map.iter().map(|(k, v)| (k.to_string_lossy(), v)).collect();
                    keys.sort_by(|a, b| a.0.cmp(&b.0));
                    out.push('{');
                    for (i, (k, v)) in keys.into_iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&key(&k));
                        out.push(':');
                        go(v, out);
                    }
                    out.push('}');
                }
            }
        }
        let mut out = String::new();
        go(tag, &mut out);
        out
    }

    /// NBT as the JSON a text component reads from.
    pub(in crate::game) fn tag_json(tag: &Tag) -> serde_json::Value {
        use serde_json::Value;
        match tag {
            Tag::End => Value::Null,
            Tag::Byte(v) => Value::from(*v),
            Tag::Short(v) => Value::from(*v),
            Tag::Int(v) => Value::from(*v),
            Tag::Long(v) => Value::from(*v),
            Tag::Float(v) => Value::from(f32::from_bits(*v)),
            Tag::Double(v) => Value::from(f64::from_bits(*v)),
            Tag::String(s) => Value::String(s.to_string_lossy()),
            Tag::ByteArray(v) => Value::from(v.iter().map(|b| *b as i8).collect::<Vec<_>>()),
            Tag::IntArray(v) => Value::from(v.clone()),
            Tag::LongArray(v) => Value::from(v.clone()),
            Tag::List(items) => Value::Array(items.iter().map(tag_json).collect()),
            Tag::Compound(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.to_string_lossy(), tag_json(v)))
                    .collect(),
            ),
        }
    }

    /// Plugin text as an NBT text component, for packets and items.
    pub(in crate::game) fn text_tag(text: &str) -> Tag {
        component_nbt(&plugin_component(text))
    }
    pub(in crate::game) fn mode_of(id: i32) -> types::GameMode {
        match id {
            1 => types::GameMode::Creative,
            2 => types::GameMode::Adventure,
            3 => types::GameMode::Spectator,
            _ => types::GameMode::Survival,
        }
    }
    pub(in crate::game) fn mode_name(m: types::GameMode) -> &'static str {
        match m {
            types::GameMode::Survival => "survival",
            types::GameMode::Creative => "creative",
            types::GameMode::Adventure => "adventure",
            types::GameMode::Spectator => "spectator",
        }
    }

    /// A number as a command reads it (no exponent).
    pub(in crate::game) fn num(v: f64) -> String {
        if v.is_finite() {
            let s = format!("{v:.6}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s.is_empty() || s == "-" {
                "0".into()
            } else {
                s.to_owned()
            }
        } else {
            "0".into()
        }
    }

    /// The JSON text a command takes for plugin text.
    pub(in crate::game) fn json_text(text: &str) -> String {
        plugin_component(text).to_string()
    }

    /// The menu kinds of `plugin-inventories`.
    pub(in crate::game) fn menu_kind(size: usize) -> &'static str {
        match size {
            5 => "minecraft:hopper",
            0..=17 => "minecraft:generic_9x1",
            18..=26 => "minecraft:generic_9x2",
            27..=35 => "minecraft:generic_9x3",
            36..=44 => "minecraft:generic_9x4",
            45..=53 => "minecraft:generic_9x5",
            _ => "minecraft:generic_9x6",
        }
    }

    impl Game<'_> {
        /// A guest as a plugin reads them: from the guest's record, or from
        /// the game while it is the guest's turn.
        pub(in crate::game) fn plugin_player_of(&self, host: &LanHost, g: &Guest) -> types::Player {
            let acting = self.lan_acting == Some(g.conn) && self.player.is_some();
            let id = uuid_str(g.uuid);
            let display = self
                .mods
                .plugin
                .display_names
                .get(&g.uuid)
                .cloned()
                .unwrap_or_else(|| g.name.clone());
            let (pos, yaw, pitch, health, food, saturation) = match (acting, self.player.as_ref()) {
                (true, Some(p)) => (
                    [p.position.x, p.position.y, p.position.z],
                    self.yaw,
                    self.pitch,
                    p.health.health(),
                    p.health.food.food,
                    p.health.food.saturation,
                ),
                _ => (g.position, g.yaw, g.pitch, g.health, g.food, g.saturation),
            };
            let (mode, level, exp, total, selected, held) = if acting {
                let mode = match self.mode {
                    horizon_client::command::args::GameMode::Survival => 0,
                    horizon_client::command::args::GameMode::Creative => 1,
                    horizon_client::command::args::GameMode::Adventure => 2,
                    horizon_client::command::args::GameMode::Spectator => 3,
                };
                (
                    mode,
                    self.experience.level,
                    self.experience.progress,
                    self.experience.total,
                    self.selected,
                    self.inventory.get(self.selected).cloned().flatten(),
                )
            } else {
                (
                    g.game_mode,
                    g.xp_level,
                    g.xp_progress,
                    g.xp_total,
                    g.selected,
                    g.inventory.get(g.selected).cloned().flatten(),
                )
            };
            let _ = host;
            types::Player {
                id,
                name: g.name.clone(),
                display_name: display,
                entity_id: g.entity_id,
                location: types::Location {
                    world: g.dimension.clone(),
                    x: pos[0],
                    y: pos[1],
                    z: pos[2],
                    yaw,
                    pitch,
                },
                game_mode: mode_of(mode),
                health,
                max_health: 20.0,
                food: food.max(0) as u32,
                saturation,
                level: level.max(0) as u32,
                exp: progress_clamp(exp),
                total_exp: total.max(0) as u32,
                op: g.permission > 0,
                permission_level: u32::from(g.permission),
                address: g.address.to_string(),
                ping: g.latency.load(std::sync::atomic::Ordering::Relaxed).max(0) as u32,
                locale: g.info.language.clone(),
                client_brand: g.brand.clone(),
                view_distance: g.view_distance.max(0) as u32,
                flying: g.flying,
                allow_flight: g.creative() || g.spectator(),
                sneaking: g.sneaking,
                sprinting: g.sprinting,
                gliding: g.fall_flying,
                sleeping: g.sleeping,
                on_ground: g.on_ground,
                dead: g.dead,
                selected_slot: selected as u32,
                held: held.as_ref().map(stack_out),
            }
        }

        /// Whether the plugins hear `kind` here: this game simulates the
        /// world and some plugin listens.
        pub(in crate::game) fn plugins_listen(&self, kind: EventKind) -> bool {
            self.remote.is_none() && self.mods.mods.listens(kind)
        }

        /// Runs `f` with the plugins and the game lent to them (the hub when
        /// the caller holds it). None when no plugin runs.
        pub(in crate::game) fn with_plugins<R>(
            &mut self,
            hub: Option<&mut LanHost>,
            f: impl FnOnce(&mut Mods, &mut PluginCtx) -> R,
        ) -> Option<R> {
            if self.remote.is_some() || !self.mods.mods.runs_plugins() {
                return None;
            }
            let mut mods = std::mem::take(&mut self.mods.mods);
            let out = {
                let mut ctx = PluginCtx { game: self, hub };
                f(&mut mods, &mut ctx)
            };
            self.mods.mods = mods;
            Some(out)
        }

        /// Runs `event` through the plugins; it comes back as they left it.
        pub(in crate::game) fn plugin_fire(
            &mut self,
            hub: Option<&mut LanHost>,
            event: Event,
        ) -> Event {
            if !self.plugins_listen(plugin::kind_of(&event)) {
                return event;
            }
            let back = event.clone();
            self.with_plugins(hub, |mods, ctx| mods.fire(ctx, event))
                .unwrap_or(back)
        }

        /// The player with this connection as a plugin reads them.
        pub(in crate::game) fn plugin_player(
            &self,
            host: Option<&LanHost>,
            conn: u32,
        ) -> Option<types::Player> {
            let h = host.or(self.lan.as_ref())?;
            let g = h.hub.guests.iter().find(|g| g.conn == conn)?;
            Some(self.plugin_player_of(h, g))
        }

        /// The player whose turn it is, as a plugin reads them: from the
        /// cache when the hub is out of reach.
        pub(in crate::game) fn plugin_acting_player(&self) -> Option<types::Player> {
            let uuid = self.lan_acting_uuid?;
            if let Some(h) = self.lan.as_ref()
                && let Some(g) = h.hub.guests.iter().find(|g| g.uuid == uuid)
            {
                return Some(self.plugin_player_of(h, g));
            }
            let mut p = self
                .mods
                .plugin
                .cache
                .iter()
                .find(|p| parse_uuid(&p.id) == Some(uuid))
                .cloned()?;
            // The turn's own state is in the game.
            if let Some(m) = self.player.as_ref() {
                p.location.x = m.position.x;
                p.location.y = m.position.y;
                p.location.z = m.position.z;
                p.location.yaw = self.yaw;
                p.location.pitch = self.pitch;
                p.health = m.health.health();
                p.held = self
                    .inventory
                    .get(self.selected)
                    .cloned()
                    .flatten()
                    .as_ref()
                    .map(stack_out);
                p.selected_slot = self.selected as u32;
            }
            Some(p)
        }

        /// Keeps the players' cache fresh (the hub is in `self.lan`).
        fn refresh_plugin_cache(&mut self) {
            let Some(h) = self.lan.as_ref() else {
                self.mods.plugin.cache.clear();
                return;
            };
            let players = h
                .hub
                .guests
                .iter()
                .filter(|g| g.placed)
                .map(|g| self.plugin_player_of(h, g))
                .collect();
            self.mods.plugin.cache = players;
        }

        /// Before a server tick: its start, for the TPS.
        pub(in crate::game) fn plugins_before_tick(&mut self) {
            if !self.mods.mods.runs_plugins() {
                return;
            }
            let now = Instant::now();
            let p = &mut self.mods.plugin;
            p.tick_started = Some(now);
            p.tick_starts.push_back(now);
            // 15 minutes of ticks at 20 a second.
            while p.tick_starts.len() > 18_000 {
                p.tick_starts.pop_front();
            }
        }

        /// After a server tick, with the hub in place and nobody's turn
        /// under way: the plugins are enabled the first time, players' moves
        /// and toggles become events, tasks run, and queued changes are made.
        pub(in crate::game) fn plugins_after_tick(&mut self) {
            if self.remote.is_some() || !self.mods.mods.runs_plugins() {
                return;
            }
            if let Some(started) = self.mods.plugin.tick_started.take() {
                let p = &mut self.mods.plugin;
                p.tick_lengths
                    .push_back(started.elapsed().as_secs_f64() * 1000.0);
                while p.tick_lengths.len() > 100 {
                    p.tick_lengths.pop_front();
                }
            }
            self.refresh_plugin_cache();
            if !self.mods.plugin.started {
                self.mods.plugin.started = true;
                self.with_plugins(None, |mods, ctx| mods.enable_plugins(ctx));
            }
            self.sync_plugin_hub();
            self.plugin_moves();
            let tick = self.ticks;
            self.with_plugins(None, |mods, ctx| mods.plugin_tick(ctx, tick));
            self.apply_plugin_actions();
            self.refresh_plugin_cache();
        }

        /// The server stops: plugins are disabled, their last changes made.
        pub(in crate::game) fn plugins_stop(&mut self) {
            if !self.mods.mods.runs_plugins() || !self.mods.plugin.started {
                return;
            }
            self.with_plugins(None, |mods, ctx| mods.disable_plugins(ctx));
            self.apply_plugin_actions();
            horizon_client::lan::menu::destroy_plugin_inventories(None);
            self.mods.plugin.started = false;
        }

        /// What the hub does for the plugins: leaves join, quit and chat
        /// messages to them when they listen, and adds their commands to the
        /// players' command trees.
        fn sync_plugin_hub(&mut self) {
            let joins = self.mods.mods.listens(EventKind::PlayerJoin);
            let leaves = self.mods.mods.listens(EventKind::PlayerQuit);
            let chat = self.mods.mods.listens(EventKind::PlayerChat);
            if let Some(h) = self.lan.as_mut() {
                h.hub.quiet_joins = joins;
                h.hub.quiet_leaves = leaves;
                h.hub.intercept_chat = chat;
            }
            self.sync_plugin_commands();
        }

        /// The command tree the clients get: vanilla's and the plugins'.
        pub(in crate::game) fn sync_plugin_commands(&mut self) {
            let names: Vec<String> = self
                .mods
                .mods
                .plugin_commands()
                .into_iter()
                .flat_map(|c| {
                    let plugin = c.plugin.clone();
                    let mut all = vec![c.name.clone(), format!("{plugin}:{}", c.name)];
                    all.extend(c.spec.aliases.iter().map(|a| a.to_ascii_lowercase()));
                    all
                })
                .collect();
            if let Some(h) = self.lan.as_mut()
                && h.hub.extra_commands != names
            {
                h.hub.extra_commands = names;
                h.hub.resend_commands();
            }
        }

        /// Players who moved since the last check, and their sneak, sprint
        /// and flight toggles.
        fn plugin_moves(&mut self) {
            let Some(h) = self.lan.as_ref() else {
                return;
            };
            let guests: Vec<(u32, u128, types::Location, (bool, bool, bool))> = h
                .hub
                .guests
                .iter()
                .filter(|g| g.placed && !g.dead)
                .map(|g| {
                    (
                        g.conn,
                        g.uuid,
                        types::Location {
                            world: g.dimension.clone(),
                            x: g.position[0],
                            y: g.position[1],
                            z: g.position[2],
                            yaw: g.yaw,
                            pitch: g.pitch,
                        },
                        (g.sneaking, g.sprinting, g.flying),
                    )
                })
                .collect();
            let live: BTreeSet<u32> = guests.iter().map(|g| g.0).collect();
            // PlayerBedLeaveEvent for the guests out of the bed they were in.
            let beds: BTreeMap<u32, [i32; 3]> = h
                .hub
                .guests
                .iter()
                .filter_map(|g| g.bed.map(|b| (g.conn, b)))
                .collect();
            let left: Vec<(u32, [i32; 3])> = self
                .mods
                .plugin
                .beds
                .iter()
                .filter(|(c, _)| !beds.contains_key(c))
                .map(|(&c, &b)| (c, b))
                .collect();
            self.mods.plugin.beds = beds;
            if self.plugins_listen(EventKind::PlayerBedLeave) {
                for (conn, bed) in left {
                    if let Some(player) = self.plugin_player(None, conn) {
                        let bed = self.plugin_block(&player.location.world.clone(), bed);
                        self.plugin_fire(
                            None,
                            Event::PlayerBedLeave(plugin::events::PlayerBedLeaveEvent {
                                player,
                                bed,
                            }),
                        );
                    }
                }
            }
            self.mods.plugin.moved_from.retain(|c, _| live.contains(c));
            self.mods.plugin.toggles.retain(|c, _| live.contains(c));
            for (conn, uuid, now, toggles) in guests {
                let before = self.mods.plugin.toggles.insert(conn, toggles);
                if let Some((sneak, sprint, fly)) = before {
                    if sneak != toggles.0 && self.plugins_listen(EventKind::PlayerToggleSneak) {
                        let player = self.plugin_player(None, conn);
                        if let Some(player) = player {
                            self.plugin_fire(
                                None,
                                Event::PlayerToggleSneak(plugin::events::PlayerToggleSneakEvent {
                                    player,
                                    sneaking: toggles.0,
                                    cancelled: false,
                                }),
                            );
                        }
                    }
                    if sprint != toggles.1 && self.plugins_listen(EventKind::PlayerToggleSprint) {
                        let player = self.plugin_player(None, conn);
                        if let Some(player) = player {
                            self.plugin_fire(
                                None,
                                Event::PlayerToggleSprint(
                                    plugin::events::PlayerToggleSprintEvent {
                                        player,
                                        sprinting: toggles.1,
                                        cancelled: false,
                                    },
                                ),
                            );
                        }
                    }
                    if fly != toggles.2 && self.plugins_listen(EventKind::PlayerToggleFlight) {
                        let player = self.plugin_player(None, conn);
                        if let Some(player) = player {
                            let back = self.plugin_fire(
                                None,
                                Event::PlayerToggleFlight(
                                    plugin::events::PlayerToggleFlightEvent {
                                        player,
                                        flying: toggles.2,
                                        cancelled: false,
                                    },
                                ),
                            );
                            if plugin::is_cancelled(&back) {
                                self.mods.plugin.actions.push_back(Action::Flight(
                                    uuid,
                                    None,
                                    Some(fly),
                                ));
                            }
                        }
                    }
                }
                let from = self.mods.plugin.moved_from.get(&conn).cloned();
                self.mods.plugin.moved_from.insert(conn, now.clone());
                let Some(from) = from else { continue };
                if from.world != now.world {
                    if self.plugins_listen(EventKind::PlayerChangedWorld)
                        && let Some(player) = self.plugin_player(None, conn)
                    {
                        self.plugin_fire(
                            None,
                            Event::PlayerChangedWorld(plugin::events::PlayerChangedWorldEvent {
                                player,
                                from: from.world.clone(),
                            }),
                        );
                    }
                    continue;
                }
                let moved = from.x != now.x
                    || from.y != now.y
                    || from.z != now.z
                    || from.yaw != now.yaw
                    || from.pitch != now.pitch;
                if !moved || !self.plugins_listen(EventKind::PlayerMove) {
                    continue;
                }
                let Some(player) = self.plugin_player(None, conn) else {
                    continue;
                };
                let back = self.plugin_fire(
                    None,
                    Event::PlayerMove(plugin::events::PlayerMoveEvent {
                        player,
                        from: from.clone(),
                        to: now.clone(),
                        cancelled: false,
                    }),
                );
                let Event::PlayerMove(e) = back else { continue };
                let target = if e.cancelled {
                    Some(from)
                } else if e.to != now {
                    Some(e.to)
                } else {
                    None
                };
                if let Some(to) = target {
                    self.mods.plugin.moved_from.insert(conn, to.clone());
                    self.mods
                        .plugin
                        .actions
                        .push_back(Action::Teleport(uuid, to));
                }
            }
        }

        /// Makes the changes plugins queued, in order.
        pub(in crate::game) fn apply_plugin_actions(&mut self) {
            let mut rounds = 0;
            while !self.mods.plugin.actions.is_empty() && rounds < 8 {
                rounds += 1;
                let actions: Vec<Action> = self.mods.plugin.actions.drain(..).collect();
                for a in actions {
                    self.apply_plugin_action(a);
                }
            }
            self.resend_after_cancel();
        }

        /// Inventories and blocks a cancelled action left wrong on clients.
        pub(in crate::game) fn resend_after_cancel(&mut self) {
            let conns: Vec<u32> = std::mem::take(&mut self.mods.plugin.resync)
                .into_iter()
                .collect();
            let blocks = std::mem::take(&mut self.mods.plugin.resend_blocks);
            let Some(mut host) = self.lan.take() else {
                return;
            };
            for conn in conns {
                if let Some(i) = host.hub.guests.iter().position(|g| g.conn == conn) {
                    host.hub.send_inventory(i);
                    let selected = host.hub.guests[i].selected as i32;
                    host.hub.guests[i].send(&C::SetHeldSlot(play::SetHeldSlot { slot: selected }));
                }
            }
            for (dim, pos) in blocks {
                if dim != self.dim.id() {
                    continue;
                }
                let state = self.world.block(pos[0], pos[1], pos[2]);
                let packet = C::BlockUpdate(play::BlockUpdate {
                    pos,
                    state: state.0 as i32,
                });
                for g in host.hub.guests.iter().filter(|g| g.dimension == dim) {
                    g.send(&packet);
                }
            }
            self.lan = Some(host);
        }

        fn guest_i(&self, uuid: u128) -> Option<usize> {
            self.lan
                .as_ref()?
                .hub
                .guests
                .iter()
                .position(|g| g.uuid == uuid && g.placed)
        }

        fn apply_plugin_action(&mut self, a: Action) {
            match a {
                Action::Console(line) => self.plugin_console_command(&line, true),
                Action::Quiet(line) => self.plugin_console_command(&line, false),
                Action::Shutdown => self.mods.plugin.stop = true,
                Action::Packet(uuid, packet) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_ref()
                    {
                        h.hub.guests[i].send(&packet);
                    }
                }
                Action::Kick(uuid, reason) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        let conn = h.hub.guests[i].conn;
                        h.hub.kick(conn, reason);
                    }
                }
                Action::Teleport(uuid, to) => {
                    let Some(name) = self
                        .lan
                        .as_ref()
                        .and_then(|h| h.hub.guests.iter().find(|g| g.uuid == uuid))
                        .map(|g| g.name.clone())
                    else {
                        return;
                    };
                    let line = format!(
                        "execute in {} run tp {name} {} {} {} {} {}",
                        to.world,
                        num(to.x),
                        num(to.y),
                        num(to.z),
                        num(f64::from(to.yaw)),
                        num(f64::from(to.pitch)),
                    );
                    self.plugin_console_command(&line, false);
                }
                Action::Health(uuid, health) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        let g = &mut h.hub.guests[i];
                        g.health = health.max(0.0);
                        if let Some(m) = g.motion.as_mut() {
                            let _ = m.set_health(health.max(0.0), false);
                        }
                    }
                }
                Action::Food(uuid, food) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        let g = &mut h.hub.guests[i];
                        g.food = food.min(20) as i32;
                        if let Some(m) = g.motion.as_mut() {
                            m.health.food.food = g.food;
                        }
                    }
                }
                Action::Saturation(uuid, s) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        let g = &mut h.hub.guests[i];
                        g.saturation = s.max(0.0);
                        if let Some(m) = g.motion.as_mut() {
                            m.health.food.saturation = g.saturation;
                        }
                    }
                }
                Action::Slot(uuid, slot, item) => self.plugin_set_slot(uuid, slot, item),
                Action::EnderSlot(uuid, slot, item) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                        && let Some(s) = h.hub.guests[i].ender_items.get_mut(slot as usize)
                    {
                        *s = item;
                    }
                }
                Action::Cursor(uuid, item) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        h.hub.guests[i].carried = item.clone();
                        let net = h.hub.stack(&item);
                        h.hub.guests[i].send(&C::SetCursorItem(play::SetCursorItem { item: net }));
                    }
                }
                Action::HeldSlot(uuid, slot) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        let slot = slot.min(8) as usize;
                        h.hub.guests[i].selected = slot;
                        h.hub.guests[i]
                            .send(&C::SetHeldSlot(play::SetHeldSlot { slot: slot as i32 }));
                    }
                }
                Action::Give(uuid, items) => {
                    let rest = self.plugin_give(uuid, items);
                    // What doesn't fit drops at the player's feet, as /give does.
                    if let Some(name) = self.plugin_name_of(uuid) {
                        for s in rest {
                            let line = format!(
                                "execute at {name} run summon item ~ ~ ~ {{Item:{},PickupDelay:0s}}",
                                snbt(&horizon_client::save::item_to_nbt(&s))
                            );
                            self.plugin_console_command(&line, false);
                        }
                    }
                }
                Action::RemoveItem(uuid, item, count) => {
                    self.plugin_remove_item(uuid, &item, count);
                }
                Action::ClearInventory(uuid) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        let g = &mut h.hub.guests[i];
                        g.inventory.iter_mut().for_each(|s| *s = None);
                        g.armor = Default::default();
                        g.offhand = None;
                        h.hub.send_inventory(i);
                    }
                }
                Action::CloseMenu(uuid) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(mut h) = self.lan.take()
                    {
                        let conn = h.hub.guests[i].conn;
                        self.guest_container_closed(&mut h, conn);
                        h.hub.close_container(conn);
                        self.lan = Some(h);
                    }
                }
                Action::OpenInventory(uuid, inv) => self.plugin_open_inventory(uuid, inv),
                Action::UpdateInventory(uuid) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        h.hub.send_inventory(i);
                    }
                }
                Action::Flight(uuid, allow, flying) => {
                    self.plugin_flight(uuid, allow, flying, None)
                }
                Action::FlySpeed(uuid, speed) => self.plugin_flight(uuid, None, None, Some(speed)),
                Action::Velocity(uuid, v) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_ref()
                    {
                        let g = &h.hub.guests[i];
                        g.send(&C::SetEntityMotion(play::SetEntityMotion {
                            id: g.entity_id,
                            velocity: v,
                        }));
                    }
                }
                Action::Blocks(dim, blocks) => {
                    self.mods
                        .block_writes
                        .extend(blocks.into_iter().map(|b| (dim.clone(), b)));
                    self.apply_mod_block_writes();
                }
                Action::Fire(uuid, ticks) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                        && let Some(m) = h.hub.guests[i].motion.as_mut()
                    {
                        m.set_remaining_fire_ticks(ticks as i32);
                    }
                }
                Action::Retrack(viewer, target) => self.plugin_retrack(viewer, target),
                Action::PlayerCommand(uuid, line) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(mut h) = self.lan.take()
                    {
                        let conn = h.hub.guests[i].conn;
                        self.guest_command(&mut h, conn, &line, None);
                        self.lan = Some(h);
                    }
                }
                Action::PlayerChat(uuid, message) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(mut h) = self.lan.take()
                    {
                        let conn = h.hub.guests[i].conn;
                        self.plugin_chat_as(&mut h, conn, &message);
                        self.lan = Some(h);
                    }
                }
                Action::ClearSpawn(uuid) => {
                    if let Some(i) = self.guest_i(uuid)
                        && let Some(h) = self.lan.as_mut()
                    {
                        h.hub.guests[i].spawn = None;
                        h.hub.guests[i].respawn_elsewhere = None;
                    }
                }
                Action::LightningEffect(at) => {
                    // A bolt only the clients see: an entity the server adds
                    // and removes again, never ticked.
                    self.mods.plugin.spawned += 1;
                    let id = 0x7f00_0000 + (self.mods.plugin.spawned & 0xffff) as i32;
                    let kind = self.lan.as_ref().and_then(|h| {
                        h.hub
                            .registry_id("minecraft:entity_type", "minecraft:lightning_bolt")
                    });
                    if let (Some(kind), Some(h)) = (kind, self.lan.as_ref()) {
                        let add = C::AddEntity(play::AddEntity {
                            id,
                            uuid: super::super::lan::entity_uuid(id as u32),
                            kind,
                            position: [at.x, at.y, at.z],
                            velocity: [0.0; 3],
                            pitch: 0,
                            yaw: 0,
                            head_yaw: 0,
                            data: 0,
                        });
                        for g in h.hub.guests.iter().filter(|g| g.dimension == at.world) {
                            g.send(&add);
                            g.send(&C::RemoveEntities(play::RemoveEntities { ids: vec![id] }));
                        }
                    }
                }
                Action::Explode(at, power, fire, break_blocks) => {
                    let dim = super::super::Dim::from_id(&at.world);
                    let back = self.enter_command_level(dim);
                    let interaction = if break_blocks {
                        crate::game::explosion::BlockInteraction::Destroy
                    } else {
                        crate::game::explosion::BlockInteraction::Keep
                    };
                    if let Err(e) =
                        self.server_explode([at.x, at.y, at.z], power, interaction, fire)
                    {
                        log::warn!("Horizon: a plugin's explosion failed: {e}");
                    }
                    if let Some(back) = back {
                        self.enter_command_level(back);
                    }
                }
                Action::EntityCommand(id, line) => self.plugin_entity_command(id, &line),
                Action::EntityNbt(id, tag) => {
                    if let Some(t) = self.plugin_target(id)
                        && let Err(e) = self.apply_entity_nbt(t, tag)
                    {
                        log::debug!("Horizon: a plugin's entity data: {e:?}");
                    }
                }
                Action::EntityRemove(id) => {
                    if let Some(t) = self.plugin_target(id) {
                        self.plugin_discard(t);
                    }
                }
                Action::Ride(passenger, vehicle) => {
                    self.plugin_entity_command(
                        passenger,
                        &format!(
                            "ride {{e}} mount {}",
                            self.plugin_selector(vehicle).unwrap_or_default()
                        ),
                    );
                }
                Action::Eject(id) => {
                    let riders: Vec<i32> = self
                        .plugin_targets()
                        .into_iter()
                        .filter_map(|t| self.plugin_target_id(t))
                        .collect();
                    // `ride <rider> dismount` for whoever rides it.
                    let _ = riders;
                    self.plugin_entity_command(id, "execute on passengers run ride @s dismount");
                }
                Action::DestroyInventory(inv) => {
                    horizon_client::lan::menu::destroy_plugin_inventory(inv);
                }
            }
        }

        /// A selector naming one entity for a command: a player's name, or
        /// the temporary tag `plugin_entity_command` gives it.
        fn plugin_selector(&self, id: i32) -> Option<String> {
            let t = self.plugin_target(id)?;
            Some(match t {
                Target::Guest(conn) => self.lan.as_ref()?.hub.guest(conn)?.name.clone(),
                _ => format!("@e[tag={TARGET_TAG}_{id},limit=1]"),
            })
        }

        /// Runs `line` with `{e}` naming the entity with network id `id`:
        /// the entity is tagged for the command, then untagged.
        fn plugin_entity_command(&mut self, id: i32, line: &str) {
            let Some(t) = self.plugin_target(id) else {
                return;
            };
            if let Target::Guest(conn) = t {
                let Some(name) = self
                    .lan
                    .as_ref()
                    .and_then(|h| h.hub.guest(conn))
                    .map(|g| g.name.clone())
                else {
                    return;
                };
                self.plugin_console_command(&line.replace("{e}", &name), false);
                return;
            }
            let tag = format!("{TARGET_TAG}_{id}");
            // Other entities named in the line (a vehicle) are tagged too.
            let mut tagged = vec![(t, tag.clone())];
            for word in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
                if let Some(other) = word.strip_prefix(&format!("{TARGET_TAG}_"))
                    && let Ok(other_id) = other.parse::<i32>()
                    && other_id != id
                    && let Some(ot) = self.plugin_target(other_id)
                {
                    tagged.push((ot, word.to_owned()));
                }
            }
            for (target, tag) in &tagged {
                self.plugin_tag(*target, tag, true);
            }
            let selector = format!("@e[tag={tag},limit=1]");
            self.plugin_console_command(&line.replace("{e}", &selector), false);
            for (target, tag) in &tagged {
                self.plugin_tag(*target, tag, false);
            }
        }

        /// Adds or removes a scoreboard tag of an entity.
        fn plugin_tag(&mut self, t: Target, tag: &str, add: bool) {
            let mut tags: Vec<String> =
                match self.entity_nbt(t).and_then(|n| n.get("Tags").cloned()) {
                    Some(Tag::List(items)) => items
                        .iter()
                        .filter_map(|t| match t {
                            Tag::String(s) => Some(s.to_string_lossy()),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
            tags.retain(|x| x != tag);
            if add {
                tags.push(tag.to_owned());
            }
            self.set_entity_tags(t, &tags);
        }

        /// Removes an entity from the world without drops (`Entity.discard`).
        fn plugin_discard(&mut self, t: Target) {
            match t {
                Target::Mob(id) => self.mobs.retain(|m| m.id != id),
                Target::Item(id) => self.dropped.retain(|i| i.id != id),
                Target::Orb(id) => self.orbs.retain(|o| o.id != id),
                Target::Projectile(id) => self.creatures.projectiles.retain(|p| p.id != id),
                Target::Stand(id) => self.armor_stands.retain(|a| a.id != id),
                Target::Frame(id) => self.item_frames.retain(|f| f.id != id),
                Target::Painting(id) => self.paintings.retain(|p| p.id != id),
                Target::Boat(id) => self.boats.retain(|b| b.id != id),
                Target::Cart(id) => self.carts.retain(|c| c.id != id),
                Target::Other(id) => self.others.retain(|o| o.id != id),
                Target::Crystal(id) => self.crystals.retain(|c| c.id != id),
                Target::Dragon(id) => {
                    if self.dragon.as_ref().is_some_and(|d| d.id == id) {
                        self.dragon = None;
                    }
                }
                Target::Player | Target::Guest(_) => {}
            }
        }

        /// `viewer`'s client forgets `target` and, unless hidden, learns it again.
        fn plugin_retrack(&mut self, viewer: u128, target: u128) {
            let Some(h) = self.sim.lan.as_mut() else {
                return;
            };
            let Some(vi) = h.hub.guests.iter().position(|g| g.uuid == viewer) else {
                return;
            };
            let Some(t) = h.hub.guests.iter().find(|g| g.uuid == target) else {
                return;
            };
            let (entity, name, properties, mode) = (
                t.entity_id,
                t.name.clone(),
                t.properties.clone(),
                t.game_mode,
            );
            let hidden = self.sim.mods.plugin.hidden.contains(&(viewer, target));
            let g = &mut h.hub.guests[vi];
            if hidden {
                g.tracker.block(entity);
            } else {
                g.tracker.unblock(entity);
                g.tracker.forget(entity);
            }
            if !hidden {
                g.send(&C::PlayerInfoUpdate(play::PlayerInfoUpdate {
                    actions: 1 | 4 | 8,
                    entries: vec![play::PlayerInfo {
                        uuid: target,
                        name: Some(name),
                        properties,
                        game_mode: Some(mode),
                        listed: Some(true),
                        ..Default::default()
                    }],
                }));
            }
        }

        /// Chat as a player (`plugin-players.chat`): through the chat event,
        /// then to everyone as a system message.
        fn plugin_chat_as(&mut self, host: &mut LanHost, conn: u32, message: &str) {
            let Some(player) = self.plugin_player(Some(host), conn) else {
                return;
            };
            if let Some(cmd) = message.strip_prefix('/') {
                self.guest_command(host, conn, cmd, None);
                return;
            }
            let recipients: Vec<String> = host
                .hub
                .guests
                .iter()
                .filter(|g| g.placed)
                .map(|g| uuid_str(g.uuid))
                .collect();
            let event = Event::PlayerChat(plugin::events::PlayerChatEvent {
                player: player.clone(),
                message: message.to_owned(),
                format: "<{name}> {message}".to_owned(),
                recipients,
                cancelled: false,
            });
            let back = self.plugin_fire(Some(host), event);
            if let Event::PlayerChat(e) = back
                && !e.cancelled
            {
                self.plugin_deliver_chat(host, &e);
            }
        }

        /// A chat event's outcome as system messages to its recipients, and
        /// the console's line.
        pub(in crate::game) fn plugin_deliver_chat(
            &mut self,
            host: &mut LanHost,
            e: &plugin::events::PlayerChatEvent,
        ) {
            let line = e
                .format
                .replace("{name}", &e.player.display_name)
                .replace("{message}", &e.message);
            let content = text_tag(&line);
            let recipients: BTreeSet<u128> =
                e.recipients.iter().filter_map(|r| parse_uuid(r)).collect();
            for g in host
                .hub
                .guests
                .iter()
                .filter(|g| g.placed && recipients.contains(&g.uuid))
            {
                g.send(&C::SystemChat(play::SystemChat {
                    content: content.clone(),
                    overlay: false,
                }));
            }
            let plain = horizon_mods::plugin::strip_codes(&line);
            if self.options.dedicated.is_some() {
                crate::game::dedicated::log("INFO", &format!("[Not Secure] {plain}"));
            } else {
                log::info!("{plain}");
            }
        }

        /// The server's objectives: name, criteria, display name (JSON).
        pub(in crate::game) fn plugin_objectives(&self) -> Vec<(String, String, String)> {
            self.score
                .board
                .objectives
                .iter()
                .map(|o| {
                    (
                        o.name.clone(),
                        o.criteria.clone(),
                        o.display_name.to_string(),
                    )
                })
                .collect()
        }
        pub(in crate::game) fn plugin_score(&self, objective: &str, holder: &str) -> Option<i32> {
            self.score.board.score(holder, objective).map(|s| s.value)
        }
        pub(in crate::game) fn plugin_teams(&self) -> Vec<String> {
            self.score
                .board
                .teams
                .iter()
                .map(|t| t.name.clone())
                .collect()
        }

        fn plugin_name_of(&self, uuid: u128) -> Option<String> {
            self.lan
                .as_ref()?
                .hub
                .guests
                .iter()
                .find(|g| g.uuid == uuid)
                .map(|g| g.name.clone())
        }

        /// A command as the console: logged when `log` (a plugin's own
        /// `dispatch-command`), else quiet.
        pub(in crate::game) fn plugin_console_command(&mut self, line: &str, log: bool) {
            let line = line.trim_start_matches('/');
            if line.is_empty() {
                return;
            }
            if log && self.options.dedicated.is_some() {
                // The dedicated console's own commands (whitelist, ban, op,
                // stop ...) as the console types them.
                self.mods.plugin.actions_console.push(line.to_owned());
                return;
            }
            self.chat_capture = Some(Vec::new());
            self.console_broadcast = Some(false);
            let outer = self.command_source.take();
            self.as_console(|g| g.run_command(line));
            self.command_source = outer;
            self.console_broadcast = None;
            let said = self.chat_capture.take().unwrap_or_default();
            if log {
                for t in said {
                    log::info!("{}", t.plain());
                }
            }
            if let Some(mut host) = self.lan.take() {
                self.run_level_teleports(&mut host);
                self.lan = Some(host);
            }
            self.admin_feedback.clear();
        }

        fn plugin_flight(
            &mut self,
            uuid: u128,
            allow: Option<bool>,
            flying: Option<bool>,
            speed: Option<f32>,
        ) {
            let Some(i) = self.guest_i(uuid) else { return };
            let Some(h) = self.lan.as_mut() else { return };
            let g = &mut h.hub.guests[i];
            let may = allow.unwrap_or(g.creative() || g.spectator());
            if let Some(f) = flying {
                g.flying = f && may;
            }
            if allow == Some(false) {
                g.flying = false;
            }
            let mut flags = 0i8;
            if g.creative() {
                flags |= 1 | 8;
            }
            if g.flying {
                flags |= 2;
            }
            if may {
                flags |= 4;
            }
            g.send(&C::PlayerAbilities(play::PlayerAbilities {
                flags,
                flying_speed: speed.unwrap_or(0.05),
                walking_speed: 0.1,
            }));
        }

        /// A slot of a guest's inventory, numbered as plugins number it.
        fn plugin_set_slot(&mut self, uuid: u128, slot: u32, item: Option<ItemStack>) {
            let Some(i) = self.guest_i(uuid) else { return };
            let Some(h) = self.lan.as_mut() else { return };
            let g = &mut h.hub.guests[i];
            match slot {
                0..=35 => g.inventory[slot as usize] = item,
                // Bukkit's armour order is feet first; the guest's head first.
                36..=39 => g.armor[(39 - slot) as usize] = item,
                40 => g.offhand = item,
                _ => return,
            }
            h.hub.send_inventory(i);
        }

        /// `Inventory.add` for a guest: what doesn't fit comes back.
        fn plugin_give(&mut self, uuid: u128, items: Vec<ItemStack>) -> Vec<ItemStack> {
            let Some(i) = self.guest_i(uuid) else {
                return items;
            };
            let max = |s: &ItemStack| -> u32 {
                self.res
                    .items()
                    .get(&s.item)
                    .map_or(64, |info| info.max_stack.max(1))
            };
            let maxes: Vec<u32> = items.iter().map(max).collect();
            let Some(h) = self.lan.as_mut() else {
                return items;
            };
            let g = &mut h.hub.guests[i];
            let mut rest = Vec::new();
            for (mut s, max) in items.into_iter().zip(maxes) {
                // Stacks of the same item first, hotbar first.
                for slot in g.inventory.iter_mut().flatten() {
                    if s.count == 0 {
                        break;
                    }
                    if slot.same_item_same_components(&s) && slot.count < max {
                        let moved = (max - slot.count).min(s.count);
                        slot.count += moved;
                        s.count -= moved;
                    }
                }
                for slot in g.inventory.iter_mut() {
                    if s.count == 0 {
                        break;
                    }
                    if slot.is_none() {
                        let moved = s.count.min(max);
                        *slot = Some(ItemStack {
                            count: moved,
                            ..s.clone()
                        });
                        s.count -= moved;
                    }
                }
                if s.count > 0 {
                    rest.push(s);
                }
            }
            h.hub.send_inventory(i);
            rest
        }

        fn plugin_remove_item(&mut self, uuid: u128, item: &str, mut count: u32) -> u32 {
            let Some(i) = self.guest_i(uuid) else {
                return 0;
            };
            let Some(h) = self.lan.as_mut() else { return 0 };
            let g = &mut h.hub.guests[i];
            let mut removed = 0;
            let slots = g
                .inventory
                .iter_mut()
                .chain(g.armor.iter_mut())
                .chain(std::iter::once(&mut g.offhand));
            for slot in slots {
                if count == 0 {
                    break;
                }
                if let Some(s) = slot.as_mut()
                    && &*s.item == item
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
            h.hub.send_inventory(i);
            removed
        }

        fn plugin_open_inventory(&mut self, uuid: u128, inv: u32) {
            let Some(i) = self.guest_i(uuid) else { return };
            let Some((items, title)) = horizon_client::lan::menu::plugin_inventory(inv)
                .map(|p| (p.items.clone(), p.title.clone()))
            else {
                return;
            };
            let Some(mut h) = self.lan.take() else { return };
            let conn = h.hub.guests[i].conn;
            // What was open closes first (its items go back, as vanilla's).
            if h.hub.guests[i].menu.is_some() {
                self.guest_container_closed(&mut h, conn);
                h.hub.close_container(conn);
            }
            let dropper =
                horizon_client::lan::menu::plugin_inventory(inv).is_some_and(|p| p.dropper);
            let kind = if dropper {
                "minecraft:generic_3x3"
            } else {
                menu_kind(items.len())
            };
            h.hub.open_container(
                conn,
                kind,
                text_tag(&title),
                (inv as i32, horizon_client::lan::menu::PLUGIN_KEY_Y, 0),
                horizon_client::lan::menu::Slots::Plugin(inv),
                items,
            );
            self.lan = Some(h);
        }

        // -------------------------------------------------------------------
        // Events the game fires.

        /// A player joined (after the join packets): `PlayerJoinEvent`; the
        /// message it leaves is what everyone reads. `first` is a newcomer.
        /// Returns the join message: None to send vanilla's own, Some(None)
        /// for no message.
        pub(in crate::game) fn plugin_joined(
            &mut self,
            host: &mut LanHost,
            conn: u32,
            first: bool,
        ) -> Option<Option<String>> {
            if !self.mods.mods.runs_plugins() || self.remote.is_some() {
                return None;
            }
            self.mods.plugin.joined.insert(conn);
            let player = self.plugin_player(Some(host), conn)?;
            // Name tags and boss bars plugins set are the newcomer's too.
            self.plugin_greet(host, conn);
            if self.plugins_listen(EventKind::PlayerLogin) {
                let back = self.plugin_fire(
                    Some(host),
                    Event::PlayerLogin(plugin::events::PlayerLoginEvent {
                        player: player.clone(),
                        allowed: true,
                        kick_message: String::new(),
                    }),
                );
                if let Event::PlayerLogin(e) = back
                    && !e.allowed
                {
                    let reason = if e.kick_message.is_empty() {
                        "You are not allowed on this server".to_owned()
                    } else {
                        e.kick_message
                    };
                    host.hub.kick(conn, text_tag(&reason));
                    return Some(None);
                }
            }
            if !self.plugins_listen(EventKind::PlayerJoin) {
                return None;
            }
            let default = format!("§e{} joined the game", player.name);
            let back = self.plugin_fire(
                Some(host),
                Event::PlayerJoin(plugin::events::PlayerJoinEvent {
                    player,
                    message: Some(default.clone()),
                    first_join: first,
                }),
            );
            match back {
                Event::PlayerJoin(e) if e.message.as_deref() != Some(default.as_str()) => {
                    Some(e.message)
                }
                _ => None,
            }
        }

        /// What plugins keep showing every player: name tag teams.
        fn plugin_greet(&mut self, host: &mut LanHost, conn: u32) {
            let tags: Vec<(u128, (String, String, Option<i32>))> = self
                .mods
                .plugin
                .name_tags
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect();
            for (target, (prefix, suffix, color)) in tags {
                let Some(name) = host
                    .hub
                    .guests
                    .iter()
                    .find(|g| g.uuid == target)
                    .map(|g| g.name.clone())
                else {
                    continue;
                };
                let packet = name_tag_packet(target, &name, &prefix, &suffix, color, 0);
                host.hub.send_to(conn, &packet);
            }
        }

        /// A player left: `PlayerQuitEvent`. Returns the quit message: None
        /// for vanilla's own, Some(None) for no message.
        pub(in crate::game) fn plugin_left(
            &mut self,
            host: &mut LanHost,
            g: &Guest,
            reason: &str,
        ) -> Option<Option<String>> {
            if !self.mods.mods.runs_plugins() || self.remote.is_some() {
                return None;
            }
            let heard = self.mods.plugin.joined.remove(&g.conn);
            self.mods.plugin.moved_from.remove(&g.conn);
            self.mods.plugin.toggles.remove(&g.conn);
            let id = uuid_str(g.uuid);
            for bar in self.mods.plugin.bars.values_mut() {
                bar.players.remove(&g.uuid);
            }
            self.mods.plugin.sidebars.remove(&g.uuid);
            self.mods
                .plugin
                .hidden
                .retain(|(a, b)| *a != g.uuid && *b != g.uuid);
            self.mods.mods.plugin_player_left(&id);
            if !heard || !self.plugins_listen(EventKind::PlayerQuit) {
                return None;
            }
            let player = self.plugin_player_of(host, g);
            let default = format!("§e{} left the game", g.name);
            let back = self.plugin_fire(
                Some(host),
                Event::PlayerQuit(plugin::events::PlayerQuitEvent {
                    player,
                    message: Some(default.clone()),
                    reason: reason.to_owned(),
                }),
            );
            match back {
                Event::PlayerQuit(e) if e.message.as_deref() != Some(default.as_str()) => {
                    Some(e.message)
                }
                _ => None,
            }
        }
    }

    /// `ClientboundSetPlayerTeamPacket` for a name tag: method 0 adds the
    /// team with the player, 2 changes it, 1 removes it.
    pub(in crate::game) fn name_tag_packet(
        target: u128,
        name: &str,
        prefix: &str,
        suffix: &str,
        color: Option<i32>,
        method: i8,
    ) -> C {
        let team = format!("hzp{:x}", target & 0xffff_ffff_ffff);
        let parameters = (method != 1).then(|| play::TeamParameters {
            display_name: Tag::String(team.as_str().into()),
            prefix: text_tag(prefix),
            suffix: text_tag(suffix),
            name_tag_visibility: 0,
            collision_rule: 0,
            color,
            options: 0,
        });
        C::SetPlayerTeam(play::SetPlayerTeam {
            name: team,
            method,
            parameters,
            players: if method == 0 {
                vec![name.to_owned()]
            } else {
                Vec::new()
            },
        })
    }

    fn progress_clamp(p: f32) -> f32 {
        if p.is_finite() {
            p.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}
