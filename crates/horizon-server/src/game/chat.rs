//! Chat, the command registries and the per-world state behind commands
//! (game rules, the world clock, difficulty), bound to the game.
use super::Screen;
pub use crate::hc::world_data::{TimeMarker, WorldClock};
use crate::hc::{
    chat::{
        Chat, ChatAbilities, ChatAction, ChatScreen, ChatSource, ChatVisibility, Commands,
        SYSTEM_INDICATOR,
    },
    gamerules::GameRules,
    hud::HudBatch,
    screen_draw::Measure,
    text::{ClickEvent, Style, Text},
};
use std::collections::BTreeMap;

/// The tree the chat parses and completes with: a server's, once it has
/// sent one (ClientPacketListener.commands), else the game's own.
fn chat_tree(remote: &Option<super::remote::Remote>) -> &crate::hc::command::Dispatcher {
    match remote.as_ref().and_then(|r| r.commands.as_ref()) {
        Some(tree) => tree,
        None => crate::hc::command::vanilla::game_dispatcher(),
    }
}
use winit::{
    event::KeyEvent,
    keyboard::{KeyCode, PhysicalKey},
};

/// The local player's name (offline single player).
pub const PLAYER_NAME: &str = "Player";

/// The window's chat, death screen and options state (the world's rules and clock are in
/// `crate::game::Sim::world_data`).
#[derive(Default)]
pub struct UiState {
    pub chat: Chat,
    pub chat_screen: Option<ChatScreen>,
    /// When the chat screen opened (its input took the focus: the cursor blink).
    pub chat_opened: Option<std::time::Instant>,
    /// The chat screen is `InBedChatScreen`: the sleeper's, with a Leave Bed button.
    pub in_bed_chat: bool,
    /// F1: hide the HUD.
    pub hide_gui: bool,
    /// Death message key set by the cause of death (e.g. /kill).
    pub death_cause: Option<String>,
    /// Path of an F2 screenshot waiting for its frame.
    pub screenshot_message: Option<std::path::PathBuf>,
    /// The options screens.
    pub options: crate::hc::options_ui::OptionsUi,
    /// The prepared resources before resource packs are layered on.
    pub vanilla_root: std::path::PathBuf,
    /// Death screen cause and when the player died (buttons wait 20 ticks).
    pub death_text: Option<Text>,
    pub died_at: Option<std::time::Instant>,
    /// `DeathScreen.TitleConfirmScreen` and when it opened (its buttons wait
    /// 20 ticks, `setDelay`).
    pub death_confirm: Option<std::time::Instant>,
    /// `LivingEntity.deathTime` of the window's player: ticks since it died.
    pub death_time: u32,
}

impl UiState {
    /// The state a window starts with: the chat remembers the commands kept
    /// in the game folder (`ChatComponent`'s constructor).
    pub fn new() -> Self {
        let mut ui = Self::default();
        if let Ok(dir) = std::env::current_dir() {
            ui.chat.load_history(&dir);
        }
        ui
    }
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
    /// The block the player is looking at, for coordinate suggestions.
    fn refresh_dynamic_registries(&mut self) {
        self.world_data.registries.target_block = self.pick().map(|h| h.pos);
        // ClientPacketListener.getOnlinePlayerNames on a server.
        if let Some(names) = self.remote_online_names() {
            self.world_data.registries.players = names;
        }
    }
    pub(super) fn close_chat(&mut self) {
        // InBedChatScreen.onClose: leaving the screen means getting up; the
        // screen goes once the player has woken (`tick_in_bed_chat`).
        if self.ui.in_bed_chat && self.sleeping.is_some() {
            self.wake_up();
            return;
        }
        self.ui.chat_screen = None;
        self.ui.chat.reset_scroll();
        if self.screen == Screen::Chat {
            self.screen = Screen::Playing;
            self.capture(true);
        }
    }
    /// `Minecraft.computeChatAbilities` from the Chat option.
    pub(super) fn chat_abilities(&self) -> ChatAbilities {
        ChatAbilities::new(ChatVisibility::from_option(self.settings.chat_visibility))
    }
    /// A chat message from the game (ChatComponent.addServerSystemMessage).
    pub(super) fn system_message(&mut self, text: Text) {
        if let Some(capture) = self.chat_capture.as_mut() {
            capture.push(text);
            return;
        }
        // Connected to a server (single player's own included), the chat on
        // screen is the connection's: the game's own messages join it.
        if self.remote.is_some() {
            self.chat_line(text, ChatSource::Server);
            return;
        }
        // The Chat option's Hidden level takes the server's messages too.
        self.local_chat_line(text, Some(SYSTEM_INDICATOR), ChatSource::Server);
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
        // A server's client receives the line as a packet (PlayerChat or
        // DisguisedChat) and filters it there.
        if self.remote.is_none() {
            self.local_chat_line(text, None, ChatSource::Player);
        }
    }
    /// A line on the window's own chat when the Chat option lets `source`
    /// through.
    fn local_chat_line(&mut self, text: Text, indicator: Option<u32>, source: ChatSource) {
        if !self.ui.chat.shows(source) {
            return;
        }
        let focused = self.screen == Screen::Chat;
        self.ui
            .chat
            .add_unwrapped(text, indicator, source, self.ticks, focused);
    }
    /// ChatScreen.handleChatInput: history, then a command or a chat line.
    /// The Chat option's restrictions stop the message here too, so a
    /// control-socket `chat` cannot send what the player's setting hides.
    pub(super) fn handle_chat_input(&mut self, message: &str) {
        if !self.chat_abilities().allows(message) {
            return;
        }
        self.ui.chat.add_recent(message);
        // On a server (single player's own included) the server runs it,
        // unless a client mod owns the command.
        if self.remote.is_some() {
            self.send_chat(message);
            return;
        }
        if let Some(command) = message.strip_prefix('/') {
            self.run_command(command);
        } else {
            let line = Text::translate(
                "chat.type.text",
                &[self.player_display_name(), Text::literal(message)],
            );
            let focused = self.screen == Screen::Chat;
            self.ui
                .chat
                .add_unwrapped(line, None, ChatSource::Player, self.ticks, focused);
            self.lan_host_chat(message);
        }
    }
    /// `Screen.clickUrlAction`: nothing with Web Links off, else the link
    /// opens, after the confirmation unless Prompt on Links is off.
    pub(super) fn click_url(&mut self, url: String) {
        if !self.settings.chat_links {
            return;
        }
        if self.settings.file.bool("chatLinksPrompt") {
            self.link_prompt = Some(url);
        } else {
            super::packs::open_uri(&url);
        }
    }
    /// The prompt's answer: the link opens on yes; either way the chat is back.
    pub(super) fn answer_link_prompt(&mut self, yes: bool) {
        if let Some(url) = self.link_prompt.take()
            && yes
        {
            super::packs::open_uri(&url);
        }
    }
    /// `Screen.clickCommandAction`: run a clicked command as the player
    /// without adding it to the chat history. A client mod's command goes
    /// first, as `ClientPacketListener.sendUnattendedCommand` lets client
    /// commands take it. (Vanilla asks first when the command fails to
    /// parse; here the server answers it.)
    pub(super) fn click_command(&mut self, command: &str) {
        let command = command.strip_prefix('/').unwrap_or(command);
        if self.remote.is_some() {
            self.send_chat(&format!("/{command}"));
        } else {
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
    /// F1 hides the HUD (see `hud`); the screenshot key still works.
    pub(super) fn toggle_hide_gui(&mut self) {
        self.ui.hide_gui = !self.ui.hide_gui;
    }
    /// After a frame: report a finished F2 screenshot in chat.
    pub(super) fn finish_screenshot(&mut self, result: &std::result::Result<[u32; 2], String>) {
        let Some(path) = self.ui.screenshot_message.take() else {
            return;
        };
        let message = match result {
            Ok(_) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let link = Text::styled(
                    name,
                    Style {
                        underlined: Some(true),
                        click: Some(ClickEvent::OpenFile(path.to_string_lossy().into_owned())),
                        ..Style::default()
                    },
                );
                Text::translate("screenshot.success", &[link])
            }
            Err(e) => Text::translate("screenshot.failure", &[Text::literal(e.clone())]),
        };
        let focused = self.screen == Screen::Chat;
        self.ui
            .chat
            .add_unwrapped(message, None, ChatSource::Client, self.ticks, focused);
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
