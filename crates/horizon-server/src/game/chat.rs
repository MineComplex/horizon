//! Chat, the command registries and the per-world state behind commands
//! (game rules, the world clock, difficulty), bound to the game.
use super::Screen;
pub use crate::hc::world_data::{TimeMarker, WorldClock};
use crate::hc::{gamerules::GameRules, text::{ClickEvent, Style, Text}};
use std::collections::BTreeMap;

use winit::{
    event::KeyEvent,
    keyboard::{KeyCode, PhysicalKey},
};

/// The local player's name (offline single player).
pub const PLAYER_NAME: &str = "Player";

/// What a server keeps of the chat state: the death message key a command set.
#[derive(Default)]
pub struct UiState {
    /// Death message key set by the cause of death (e.g. /kill).
    pub death_cause: Option<String>,
}

impl crate::game::Sim<'_> {
    /// `PlayerList.isOp` for the single-player owner: the world's Allow Commands.
    pub(super) fn allow_commands(&self) -> bool {
        self.level.allow_commands == Some(true)
    }
    /// Commands the game issues itself (the server console, harnesses, the
    /// control socket) run at the owner's level whatever Allow Commands says.
    pub(super) fn as_console<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let level = std::mem::replace(&mut self.world_data.registries.permission, 4);
        let out = f(self);
        self.world_data.registries.permission = level;
        out
    }
    /// A chat message from the game (ChatComponent.addServerSystemMessage).
    pub(super) fn system_message(&mut self, text: Text) {
        if let Some(capture) = self.chat_capture.as_mut() {
            capture.push(text);
            return;
        }
        // A server has no window chat: its players get their messages as packets,
        // and the console logs what vanilla's does where it does.
        let _ = text;
    }
    /// A player's chat line on the window's own chat: a LAN guest's message
    /// while the host runs in-frame (`HostEvent::Chat`). `ChatListener`
    /// checks `canReceivePlayerMessages` for these, so Commands Only and
    /// Hidden drop them; command feedback (`/list`, `/tp`) is a system
    /// message and takes the system-message permission instead.
    pub(super) fn player_message(&mut self, text: Text) {
        if let Some(capture) = self.chat_capture.as_mut() {
            capture.push(text);
            return;
        }
        let _ = text;
    }
    /// `Screen.clickCommandAction`: run a clicked command as the player
    /// without adding it to the chat history. A client mod's command goes
    /// first, as `ClientPacketListener.sendUnattendedCommand` lets client
    /// commands take it. (Vanilla asks first when the command fails to
    /// parse; here the server answers it.)
    pub(super) fn click_command(&mut self, command: &str) {
        let command = command.strip_prefix('/').unwrap_or(command);
        {
            self.run_command(command);
        }
    }
    /// Advance the overworld clock (ServerClockManager.tick) when
    /// `advance_time` allows it.
    pub(super) fn advance_clock(&mut self) {
        if !self.world_data.rules.bool("advance_time") || self.world_data.clock.paused {
            return;
        }
        let c = &mut self.world_data.clock;
        c.partial += c.rate;
        let whole = c.partial.floor();
        c.partial -= whole;
        self.world.day_time += whole as i64;
    }
    /// `ServerClockInstance.packNetworkState`: the rate players' clients
    /// advance the overworld clock at, 0 while it is paused (`/time pause`)
    /// or `advance_time` is off.
    pub(super) fn network_clock_rate(&self) -> f32 {
        if self.world_data.rules.bool("advance_time") && !self.world_data.clock.paused {
            self.world_data.clock.rate
        } else {
            0.0
        }
    }
    /// The engine-side rule values (block behaviour and health).
    pub(super) fn level_rules(&self) -> horizon_world::level::GameRules {
        let r = &self.world_data.rules;
        horizon_world::level::GameRules {
            water_source_conversion: r.bool("water_source_conversion"),
            lava_source_conversion: r.bool("lava_source_conversion"),
            random_tick_speed: r.int("random_tick_speed"),
            block_drops: r.bool("block_drops"),
            mob_griefing: r.bool("mob_griefing"),
            max_snow_accumulation_height: r.int("max_snow_accumulation_height"),
            spawn_mobs: r.bool("spawn_mobs"),
            spawn_monsters: r.bool("spawn_monsters"),
            fire_spread_radius_around_player: r.int("fire_spread_radius_around_player"),
            spread_vines: r.bool("spread_vines"),
        }
    }
    /// Apply rules mirrored in other state (natural regeneration, difficulty).
    pub(super) fn sync_rules(&mut self) {
        self.world.health_rules.natural_regeneration =
            self.world_data.rules.bool("natural_health_regeneration");
        // Player.isInvulnerableTo: the damage-type rules.
        self.world.health_rules.drowning_damage = self.world_data.rules.bool("drowning_damage");
        self.world.health_rules.fall_damage = self.world_data.rules.bool("fall_damage");
        self.world.health_rules.fire_damage = self.world_data.rules.bool("fire_damage");
        self.world.health_rules.freeze_damage = self.world_data.rules.bool("freeze_damage");
        use horizon_world::food::Difficulty;
        self.world.health_rules.difficulty = match self.world_data.difficulty {
            0 => Difficulty::Peaceful,
            1 => Difficulty::Easy,
            3 => Difficulty::Hard,
            _ => Difficulty::Normal,
        };
    }
    /// Save the clock beside level.dat (Level.write saves the game rules: a
    /// second writer of game_rules.dat could leave it with the rules loaded
    /// when the world opened).
    pub(super) fn save_world_state(&mut self) {
        if self.world_root.as_os_str().is_empty() {
            return;
        }
        use horizon_world::nbt::Tag;
        let c = self.world_data.clock;
        // The other clocks stay as saved; the overworld one keeps the fields Horizon
        // does not model, and (like vanilla's codec) omits rate, partial tick and
        // pause while they have their defaults.
        let path = self.world_root.join("data/minecraft/world_clocks.dat");
        let existing = crate::hc::save::read_dat(&path);
        let mut data = match existing.as_ref().and_then(|r| r.get("data")) {
            Some(d @ Tag::Compound(_)) => d.clone(),
            _ => Tag::Compound(Default::default()),
        };
        let mut clock = match data.get("minecraft:overworld") {
            Some(c @ Tag::Compound(_)) => c.clone(),
            _ => Tag::Compound(Default::default()),
        };
        if let Tag::Compound(map) = &mut clock {
            map.insert("total_ticks".into(), Tag::Long(self.world.day_time));
            for (key, value, default) in [
                (
                    "partial_tick",
                    Tag::Float(c.partial.to_bits()),
                    c.partial == 0.0,
                ),
                ("rate", Tag::Float(c.rate.to_bits()), c.rate == 1.0),
                ("paused", Tag::Byte(i8::from(c.paused)), !c.paused),
            ] {
                if default {
                    map.remove(&key.into());
                } else {
                    map.insert(key.into(), value);
                }
            }
        }
        if let Tag::Compound(map) = &mut data {
            map.insert("minecraft:overworld".into(), clock);
        }
        let root = Tag::compound([
            ("data", data),
            ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
        ]);
        if let Err(e) = crate::hc::save::write_dat(&path, root) {
            log::warn!("the world clocks were not written: {e}");
        }
    }
}
/// `yyyy-MM-dd_HH.mm.ss[_n].png` (Util.getFilenameFormattedDateTime, local time
/// approximated as UTC).
pub fn screenshot_name(dir: &std::path::Path) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86400) as i64;
    let (h, m, s) = ((secs % 86400) / 3600, (secs % 3600) / 60, secs % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    let base = format!("{y}-{mo:02}-{d:02}_{h:02}.{m:02}.{s:02}");
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{base}.png")
        } else {
            format!("{base}_{n}.png")
        };
        if !dir.join(&name).exists() {
            return name;
        }
        n += 1;
    }
}

/// `IntegratedServer.getProfilePermissions` for the owner: `OWNER` (4) when
/// commands are allowed, else `ALL` (0).
fn owner_permission(allow_commands: bool) -> u8 {
    if allow_commands { 4 } else { 0 }
}

/// `ConfirmLinkScreen`'s buttons, left to right: yes, copy, no.
const LINK_YES: usize = 0;
const LINK_COPY: usize = 1;
