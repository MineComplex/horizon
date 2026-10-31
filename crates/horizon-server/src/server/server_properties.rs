//! `server.properties` as Leaf 26.3 reads it: vanilla's `DedicatedServerProperties` and `Settings`, with
//! CraftBukkit's command-line overrides, Paper's defaults and flags, Purpur's `server-name`, and Leaf's
//! changes (LZ4 region files, and MineComplex's cleanup, which removed RCON, Query, the management
//! server, the status settings, resource packs, `motd`, `allow-flight`, `prevent-proxy-connections`,
//! `rate-limit`, `text-filtering-*`, `bug-report-link`, `debug`, and changed several defaults).
//!
//! Reading a value writes its normalized text back (`Settings.get` does `properties.put(key,
//! serialize(value))`), so storing the file after reading rewrites it as Leaf does on start: every key,
//! defaults filled in, bad values replaced by defaults, legacy keys dropped. Unknown keys are kept.
//!
//! Left out: `feature-level-seed` (Leaf reads it only with SecureSeed on; secure seed is not ported),
//! and the world preset lookup of `level-type` (`WorldDimensionData.create`, which needs the registries).
//! Known difference: `generator-settings` is written back by `serde_json`, which prints numbers from
//! their value (`1e5` -> `100000.0`) where Gson keeps their text.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};

use super::properties_file::{PropertiesError, PropertiesFile, java_date_now};
use crate::util::random::java_string_hash;

const COMMENT: &str = "Minecraft server properties";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    const ALL: [Self; 4] = [Self::Peaceful, Self::Easy, Self::Normal, Self::Hard];

    pub fn id(self) -> i32 {
        self as i32
    }

    /// `Difficulty.byId`: out-of-range ids wrap around.
    pub fn by_id(id: i32) -> Self {
        Self::ALL[id.rem_euclid(4) as usize]
    }

    pub fn name(self) -> &'static str {
        ["peaceful", "easy", "normal", "hard"][self as usize]
    }

    /// `Difficulty.byName`: exact, case-sensitive.
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameType {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameType {
    const ALL: [Self; 4] = [
        Self::Survival,
        Self::Creative,
        Self::Adventure,
        Self::Spectator,
    ];

    pub fn id(self) -> i32 {
        self as i32
    }

    /// `GameType.byId`: out-of-range ids give survival.
    pub fn by_id(id: i32) -> Self {
        usize::try_from(id)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or(Self::Survival)
    }

    pub fn name(self) -> &'static str {
        ["survival", "creative", "adventure", "spectator"][self as usize]
    }

    /// `GameType.byName(name, null)`: exact, case-sensitive.
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.name() == name)
    }
}

/// `PermissionLevel` (`op-permission-level`, `function-permission-level`: vanilla stores the
/// `LevelBasedPermissionSet` for it, which is one per level).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionLevel {
    All,
    Moderators,
    Gamemasters,
    Admins,
    Owners,
}

impl PermissionLevel {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Moderators,
        Self::Gamemasters,
        Self::Admins,
        Self::Owners,
    ];

    pub fn id(self) -> i32 {
        self as i32
    }

    /// `PermissionLevel.byId`: out-of-range ids are clamped.
    pub fn by_id(id: i32) -> Self {
        Self::ALL[id.clamp(0, 4) as usize]
    }
}

/// What starts the server with besides the file: CraftBukkit's command-line options that replace a
/// property (`Settings.getOverride`, matched by the option's name, which is the property's key), and
/// Paper's JVM system properties. These are passed in, never read from the environment here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartupOptions {
    pub overrides: CommandLineOverrides,
    /// `-DPaper.enable-sync-chunk-writes=true`: without it `sync-chunk-writes` is always off.
    pub enable_sync_chunk_writes: bool,
    /// `-DPaper.skipServerPropertiesComments=true`: the stored file has no comment lines.
    pub skip_comments: bool,
}

/// The options that are also property keys. An override is read in place of the file's value and,
/// like any value read, written back to the file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandLineOverrides {
    /// `-h`, `--host`, `--server-ip`.
    pub server_ip: Option<String>,
    /// `-w`, `--world`, `--level-name`.
    pub level_name: Option<String>,
    /// `-p`, `--port`, `--server-port`.
    pub server_port: Option<i32>,
    /// `-o`, `--online-mode` (converted with `Boolean.valueOf`: anything but `true` is false).
    pub online_mode: Option<bool>,
    /// `-s`, `--size`, `--max-players`.
    pub max_players: Option<i32>,
    /// `--server-name` (Purpur).
    pub server_name: Option<String>,
}

impl CommandLineOverrides {
    fn get(&self, key: &str) -> Option<String> {
        match key {
            "server-ip" => self.server_ip.clone(),
            "level-name" => self.level_name.clone(),
            "server-port" => self.server_port.map(|v| v.to_string()),
            "online-mode" => self.online_mode.map(|v| v.to_string()),
            "max-players" => self.max_players.map(|v| v.to_string()),
            "server-name" => self.server_name.clone(),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServerPropertiesError {
    /// A malformed `\uxxxx` escape in the file: the server does not start.
    #[error(transparent)]
    File(PropertiesError),
    /// A value whose parsing throws (Paper: "Could not load invalidly configured property"): the
    /// server does not start.
    #[error("Could not load invalidly configured property '{key}': {reason}")]
    Invalid { key: &'static str, reason: String },
}

/// Leaf's `server.properties` values, in the order Leaf reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct DedicatedServerProperties {
    pub online_mode: bool,
    pub server_ip: String,
    pub server_name: String,
    pub code_of_conduct: bool,
    pub force_game_mode: bool,
    pub enforce_whitelist: bool,
    pub difficulty: Difficulty,
    pub game_mode: GameType,
    pub level_name: String,
    pub server_port: i32,
    /// Legacy key: read once and removed from the file.
    pub announce_player_achievements: Option<bool>,
    pub hardcore: bool,
    pub use_native_transport: bool,
    pub spawn_protection: i32,
    pub op_permission_level: PermissionLevel,
    pub function_permission_level: PermissionLevel,
    pub max_tick_time: i64,
    pub max_chained_neighbor_updates: i32,
    pub command_spam_threshold_seconds: i32,
    pub chat_spam_threshold_seconds: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub max_players: i32,
    pub network_compression_threshold: i32,
    pub broadcast_console_to_ops: bool,
    /// Clamped to `1..=29999984`.
    pub max_world_size: i32,
    /// The file's value and Paper's `enable_sync_chunk_writes` flag.
    pub sync_chunk_writes: bool,
    pub region_file_compression: String,
    pub hide_online_players: bool,
    /// Clamped to `10..=1000`.
    pub entity_broadcast_range_percentage: i32,
    pub player_idle_timeout: i32,
    pub white_list: bool,
    pub enforce_secure_profile: bool,
    pub log_ips: bool,
    pub pause_when_empty_seconds: i32,
    pub accepts_transfers: bool,
    /// `WorldOptions.parseSeed(level-seed)`; `None` means a random seed for a new world.
    pub level_seed: Option<i64>,
    pub generate_structures: bool,
    pub generator_settings: Map<String, Value>,
    /// Lowercased.
    pub level_type: String,
    pub initial_enabled_packs: Vec<String>,
    pub initial_disabled_packs: Vec<String>,
}

/// A property `Settings.MutableValue` lets the server change while running (commands, the API).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyUpdate {
    ForceGameMode(bool),
    EnforceWhitelist(bool),
    Difficulty(Difficulty),
    GameMode(GameType),
    SpawnProtection(i32),
    OpPermissionLevel(PermissionLevel),
    ViewDistance(i32),
    SimulationDistance(i32),
    MaxPlayers(i32),
    HideOnlinePlayers(bool),
    EntityBroadcastRangePercentage(i32),
    PlayerIdleTimeout(i32),
    WhiteList(bool),
    PauseWhenEmptySeconds(i32),
    AcceptsTransfers(bool),
}

impl PropertyUpdate {
    /// The key and the text its serializer writes.
    fn entry(&self) -> (&'static str, String) {
        match *self {
            Self::ForceGameMode(v) => ("force-gamemode", v.to_string()),
            Self::EnforceWhitelist(v) => ("enforce-whitelist", v.to_string()),
            Self::Difficulty(v) => ("difficulty", v.name().to_owned()),
            Self::GameMode(v) => ("gamemode", v.name().to_owned()),
            Self::SpawnProtection(v) => ("spawn-protection", v.to_string()),
            Self::OpPermissionLevel(v) => ("op-permission-level", v.id().to_string()),
            Self::ViewDistance(v) => ("view-distance", v.to_string()),
            Self::SimulationDistance(v) => ("simulation-distance", v.to_string()),
            Self::MaxPlayers(v) => ("max-players", v.to_string()),
            Self::HideOnlinePlayers(v) => ("hide-online-players", v.to_string()),
            Self::EntityBroadcastRangePercentage(v) => {
                ("entity-broadcast-range-percentage", v.to_string())
            }
            Self::PlayerIdleTimeout(v) => ("player-idle-timeout", v.to_string()),
            Self::WhiteList(v) => ("white-list", v.to_string()),
            Self::PauseWhenEmptySeconds(v) => ("pause-when-empty-seconds", v.to_string()),
            Self::AcceptsTransfers(v) => ("accepts-transfers", v.to_string()),
        }
    }
}

/// Reads values the way `Settings` does: the override or the file's text, deserialized (a `None`
/// keeps the default), the result serialized back into the file.
struct Reader<'a> {
    file: PropertiesFile,
    overrides: &'a CommandLineOverrides,
}

type Parsed<V> = Result<Option<V>, String>;

impl Reader<'_> {
    fn raw(&self, key: &str) -> Option<String> {
        self.overrides
            .get(key)
            .or_else(|| self.file.get(key).map(str::to_owned))
    }

    fn get<V>(
        &mut self,
        key: &'static str,
        parse: impl FnOnce(&str) -> Parsed<V>,
        write: impl FnOnce(&V) -> String,
        default: V,
    ) -> Result<V, ServerPropertiesError> {
        let parsed = match self.raw(key) {
            Some(text) => {
                parse(&text).map_err(|reason| ServerPropertiesError::Invalid { key, reason })?
            }
            None => None,
        };
        let value = parsed.unwrap_or(default);
        self.file.insert(key, write(&value));
        Ok(value)
    }

    fn string(&mut self, key: &'static str, default: &str) -> String {
        self.infallible(key, |s| Some(s.to_owned()), default.to_owned())
    }

    /// `Boolean.valueOf`: `true` in any case is true, anything else false.
    fn bool(&mut self, key: &'static str, default: bool) -> bool {
        self.infallible(key, |s| Some(s.eq_ignore_ascii_case("true")), default)
    }

    /// `Integer.parseInt`, a bad number keeping the default.
    fn int(&mut self, key: &'static str, default: i32) -> i32 {
        self.infallible(key, |s| s.parse().ok(), default)
    }

    fn infallible<V: ToString>(
        &mut self,
        key: &'static str,
        parse: impl FnOnce(&str) -> Option<V>,
        default: V,
    ) -> V {
        let value = self.raw(key).and_then(|s| parse(&s)).unwrap_or(default);
        self.file.insert(key, value.to_string());
        value
    }

    /// `Settings.getLegacyBoolean`: read and removed from the file.
    fn legacy_bool(&mut self, key: &str) -> Option<bool> {
        let value = self.raw(key)?;
        self.file.remove(key);
        Some(value.eq_ignore_ascii_case("true"))
    }
}

impl DedicatedServerProperties {
    /// The constructor: reads every value in Leaf's order and returns them with the file as it is to
    /// be stored (normalized values written back, legacy keys removed).
    pub fn read(
        file: PropertiesFile,
        options: &StartupOptions,
    ) -> Result<(Self, PropertiesFile), ServerPropertiesError> {
        let mut r = Reader {
            file,
            overrides: &options.overrides,
        };
        let online_mode = r.bool("online-mode", false);
        let server_ip = r.string("server-ip", "");
        let server_name = r.string("server-name", "Unknown Server");
        let code_of_conduct = r.bool("enable-code-of-conduct", false);
        let force_game_mode = r.bool("force-gamemode", false);
        let enforce_whitelist = r.bool("enforce-whitelist", false);
        let difficulty = r.by_id_or_name(
            "difficulty",
            Difficulty::by_id,
            Difficulty::by_name,
            Difficulty::name,
            Difficulty::Easy,
        );
        let game_mode = r.by_id_or_name(
            "gamemode",
            GameType::by_id,
            GameType::by_name,
            GameType::name,
            GameType::Survival,
        );
        let level_name = r.string("level-name", "world");
        let server_port = r.int("server-port", 25565);
        let announce_player_achievements = r.legacy_bool("announce-player-achievements");
        let hardcore = r.bool("hardcore", false);
        let use_native_transport = r.bool("use-native-transport", true);
        let spawn_protection = r.int("spawn-protection", 0);
        let op_permission_level = r.permission("op-permission-level", PermissionLevel::Owners);
        let function_permission_level =
            r.permission("function-permission-level", PermissionLevel::Gamemasters);
        let max_tick_time = r.infallible("max-tick-time", |s| s.parse::<i64>().ok(), 60_000);
        let max_chained_neighbor_updates = r.int("max-chained-neighbor-updates", 1_000_000);
        let command_spam_threshold_seconds = r.int("command-spam-threshold-seconds", 10);
        let chat_spam_threshold_seconds = r.int("chat-spam-threshold-seconds", 10);
        let view_distance = r.int("view-distance", 5);
        let simulation_distance = r.int("simulation-distance", 7);
        let max_players = r.int("max-players", 250);
        let network_compression_threshold = r.int("network-compression-threshold", 256);
        let broadcast_console_to_ops = r.bool("broadcast-console-to-ops", true);
        let max_world_size = r.infallible(
            "max-world-size",
            |s| s.parse::<i32>().ok().map(|v| v.clamp(1, 29_999_984)),
            29_999_984,
        );
        let sync_chunk_writes =
            r.bool("sync-chunk-writes", true) && options.enable_sync_chunk_writes;
        let region_file_compression = r.string("region-file-compression", "lz4");
        let hide_online_players = r.bool("hide-online-players", false);
        // Not wrapped like the other numbers: a bad number stops the server.
        let entity_broadcast_range_percentage = r.get(
            "entity-broadcast-range-percentage",
            |s| match s.parse::<i32>() {
                Ok(v) => Ok(Some(v.clamp(10, 1000))),
                Err(_) => Err(format!("For input string: \"{s}\"")),
            },
            i32::to_string,
            100,
        )?;
        let player_idle_timeout = r.int("player-idle-timeout", 0);
        let white_list = r.bool("white-list", false);
        let enforce_secure_profile = r.bool("enforce-secure-profile", false);
        let log_ips = r.bool("log-ips", true);
        let pause_when_empty_seconds = r.int("pause-when-empty-seconds", -1);
        let accepts_transfers = r.bool("accepts-transfers", false);
        // The constructor's body.
        let level_seed = parse_seed(&r.string("level-seed", ""));
        let generate_structures = r.bool("generate-structures", true);
        let generator_settings = r.get(
            "generator-settings",
            |s| parse_json_object(if s.is_empty() { "{}" } else { s }).map(Some),
            write_json,
            Map::new(),
        )?;
        let level_type = r.infallible(
            "level-type",
            |s| Some(s.to_lowercase()),
            "minecraft:normal".to_owned(),
        );
        let initial_enabled_packs = split_packs(&r.string("initial-enabled-packs", "vanilla"));
        let initial_disabled_packs = split_packs(&r.string("initial-disabled-packs", ""));
        let properties = Self {
            online_mode,
            server_ip,
            server_name,
            code_of_conduct,
            force_game_mode,
            enforce_whitelist,
            difficulty,
            game_mode,
            level_name,
            server_port,
            announce_player_achievements,
            hardcore,
            use_native_transport,
            spawn_protection,
            op_permission_level,
            function_permission_level,
            max_tick_time,
            max_chained_neighbor_updates,
            command_spam_threshold_seconds,
            chat_spam_threshold_seconds,
            view_distance,
            simulation_distance,
            max_players,
            network_compression_threshold,
            broadcast_console_to_ops,
            max_world_size,
            sync_chunk_writes,
            region_file_compression,
            hide_online_players,
            entity_broadcast_range_percentage,
            player_idle_timeout,
            white_list,
            enforce_secure_profile,
            log_ips,
            pause_when_empty_seconds,
            accepts_transfers,
            level_seed,
            generate_structures,
            generator_settings,
            level_type,
            initial_enabled_packs,
            initial_disabled_packs,
        };
        Ok((properties, r.file))
    }
}

impl Reader<'_> {
    /// `dispatchNumberOrString`: an int is an id, anything else a name.
    fn by_id_or_name<V: Copy>(
        &mut self,
        key: &'static str,
        by_id: fn(i32) -> V,
        by_name: fn(&str) -> Option<V>,
        name: fn(V) -> &'static str,
        default: V,
    ) -> V {
        let value = self
            .raw(key)
            .and_then(|s| s.parse().map_or_else(|_| by_name(&s), |id| Some(by_id(id))))
            .unwrap_or(default);
        self.file.insert(key, name(value));
        value
    }

    /// `deserializePermission`: an int, clamped to a level; written as the level's id.
    fn permission(&mut self, key: &'static str, default: PermissionLevel) -> PermissionLevel {
        let value = self
            .raw(key)
            .and_then(|s| s.parse::<i32>().ok())
            .map_or(default, PermissionLevel::by_id);
        self.file.insert(key, value.id().to_string());
        value
    }
}

/// `WorldOptions.parseSeed`: trimmed (`String.trim`), a `long`, else the text's `String.hashCode`.
fn parse_seed(text: &str) -> Option<i64> {
    let text = text.trim_matches(|c| c <= ' ');
    if text.is_empty() {
        return None;
    }
    Some(
        text.parse::<i64>()
            .unwrap_or_else(|_| i64::from(java_string_hash(text.encode_utf16()))),
    )
}

/// `GsonHelper.parse`: strict JSON, the first value read (anything after it ignored), which must be
/// an object.
fn parse_json_object(text: &str) -> Result<Map<String, Value>, String> {
    let mut de = serde_json::Deserializer::from_str(text);
    match Value::deserialize(&mut de) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(Value::Null) => Err("JSON data was null or empty".to_owned()),
        Ok(_) => Err("Expected a JSON object".to_owned()),
        Err(e) => Err(e.to_string()),
    }
}

/// `JsonObject.toString`: compact; Gson also escapes U+2028 and U+2029.
fn write_json(map: &Map<String, Value>) -> String {
    let text = serde_json::to_string(map).expect("a JSON map always serializes");
    text.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Guava's `Splitter.on(',').trimResults()` (its whitespace is Unicode `White_Space`, as `str::trim`).
fn split_packs(text: &str) -> Vec<String> {
    text.split(',').map(|s| s.trim().to_owned()).collect()
}

/// `DedicatedServerSettings`: the file, its values, and the startup options they were read with.
#[derive(Clone, Debug)]
pub struct DedicatedServerSettings {
    source: PathBuf,
    options: StartupOptions,
    file: PropertiesFile,
    properties: DedicatedServerProperties,
}

impl DedicatedServerSettings {
    /// `DedicatedServerProperties.fromFile`: a missing file reads as empty, and so does one that can't
    /// be read (logged); a malformed escape or an invalid value is an error.
    pub fn load(source: &Path, options: StartupOptions) -> Result<Self, ServerPropertiesError> {
        let file = match PropertiesFile::load(source) {
            Ok(file) => file,
            Err(PropertiesError::Io { path, source }) => {
                tracing::error!("Failed to load properties from file: {path}: {source}");
                PropertiesFile::default()
            }
            Err(e) => return Err(ServerPropertiesError::File(e)),
        };
        Self::from_file(source.to_owned(), file, options)
    }

    fn from_file(
        source: PathBuf,
        file: PropertiesFile,
        options: StartupOptions,
    ) -> Result<Self, ServerPropertiesError> {
        let (properties, file) = DedicatedServerProperties::read(file, &options)?;
        Ok(Self {
            source,
            options,
            file,
            properties,
        })
    }

    pub fn properties(&self) -> &DedicatedServerProperties {
        &self.properties
    }

    /// `MutableValue.update` then `DedicatedServerSettings.update`: the stored entries with this
    /// key replaced are read again from the start (overrides included), and the file is stored.
    pub fn update(&mut self, update: PropertyUpdate) -> Result<(), ServerPropertiesError> {
        let mut file = self.file.clone();
        let (key, value) = update.entry();
        file.insert(key, value);
        let (properties, file) = DedicatedServerProperties::read(file, &self.options)?;
        self.properties = properties;
        self.file = file;
        self.force_save();
        Ok(())
    }

    /// The text `store` writes, dated `date` (`Date.toString`).
    pub fn to_text(&self, date: &str) -> String {
        let text = self.file.to_text(COMMENT, date);
        if self.options.skip_comments {
            strip_comment_lines(&text)
        } else {
            text
        }
    }

    /// `Settings.store` as Paper has it: a file that exists but can't be written is skipped with a
    /// warning; other failures are logged.
    pub fn force_save(&self) {
        let path = &self.source;
        let text = self.to_text(&java_date_now());
        match std::fs::write(path, text) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && path.exists() => {
                tracing::warn!("Can not write to file {}, skipping.", path.display());
            }
            Err(e) => {
                tracing::error!(
                    "Failed to store properties to file: {}: {e}",
                    path.display()
                );
            }
        }
    }
}

/// Paper's comment-skipping stream: a `#` right after a line break (or at the start) begins a
/// comment, which runs to and including the next `\r` or `\n`.
fn strip_comment_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut after_newline, mut in_comment) = (true, false);
    for c in text.chars() {
        let newline = c == '\n' || c == '\r';
        if in_comment {
            in_comment = !newline;
        } else if c == '#' && after_newline {
            in_comment = true;
        } else {
            out.push(c);
        }
        after_newline = newline;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATE: &str = "Thu Jan 01 00:00:00 UTC 1970";

    fn read(text: &str, options: StartupOptions) -> DedicatedServerSettings {
        let file = PropertiesFile::parse(text.as_bytes()).unwrap();
        DedicatedServerSettings::from_file(PathBuf::new(), file, options).unwrap()
    }

    fn stored(settings: &DedicatedServerSettings) -> String {
        settings.to_text(DATE).replace("\r\n", "\n")
    }

    /// The file Leaf writes for an empty server.properties. Produced on Java 25 by running Leaf's final
    /// field list (transcribed from the patches) on the real 26.3 `Settings`, `Difficulty`, `GameType`,
    /// `DedicatedServerProperties.deserializePermission` and Gson, stored with
    /// `-Djava.properties.date="Thu Jan 01 00:00:00 UTC 1970"`.
    const LEAF_DEFAULTS: &str = "#Minecraft server properties\n#Thu Jan 01 00:00:00 UTC 1970\n\
        accepts-transfers=false\nbroadcast-console-to-ops=true\nchat-spam-threshold-seconds=10\n\
        command-spam-threshold-seconds=10\ndifficulty=easy\nenable-code-of-conduct=false\n\
        enforce-secure-profile=false\nenforce-whitelist=false\nentity-broadcast-range-percentage=100\n\
        force-gamemode=false\nfunction-permission-level=2\ngamemode=survival\ngenerate-structures=true\n\
        generator-settings={}\nhardcore=false\nhide-online-players=false\ninitial-disabled-packs=\n\
        initial-enabled-packs=vanilla\nlevel-name=world\nlevel-seed=\nlevel-type=minecraft\\:normal\n\
        log-ips=true\nmax-chained-neighbor-updates=1000000\nmax-players=250\nmax-tick-time=60000\n\
        max-world-size=29999984\nnetwork-compression-threshold=256\nonline-mode=false\n\
        op-permission-level=4\npause-when-empty-seconds=-1\nplayer-idle-timeout=0\n\
        region-file-compression=lz4\nserver-ip=\nserver-name=Unknown Server\nserver-port=25565\n\
        simulation-distance=7\nspawn-protection=0\nsync-chunk-writes=true\nuse-native-transport=true\n\
        view-distance=5\nwhite-list=false\n";

    #[test]
    fn empty_file_stores_leaf_defaults() {
        let settings = read("", StartupOptions::default());
        assert_eq!(stored(&settings), LEAF_DEFAULTS);
        let p = settings.properties();
        assert_eq!(p.difficulty, Difficulty::Easy);
        assert_eq!(p.op_permission_level, PermissionLevel::Owners);
        assert_eq!(p.function_permission_level, PermissionLevel::Gamemasters);
        assert_eq!(p.initial_enabled_packs, ["vanilla"]);
        assert_eq!(p.initial_disabled_packs, [""]);
        assert_eq!(p.level_seed, None);
        assert!(!p.sync_chunk_writes, "off without Paper's flag");
        // Stored text reads back to the same values and the same file.
        let again = read(&stored(&settings), StartupOptions::default());
        assert_eq!(again.properties(), p);
        assert_eq!(stored(&again), LEAF_DEFAULTS);
    }

    /// Each case's values and stored lines from the same Java program.
    #[test]
    fn edge_cases_like_java() {
        let settings = read(
            "difficulty=5\ngamemode=9\nop-permission-level=7\nfunction-permission-level=-3\n\
             entity-broadcast-range-percentage=5000\nmax-world-size=0\nannounce-player-achievements=TRUE\n\
             level-seed=\\u0020-12\\u0020\n\
             generator-settings={\"a\": 1, \"b\":[\"x\\u2028<\\\\u0001\"]} junk\n\
             level-type=MINECRAFT:FLAT\nhardcore=yes\nmax-tick-time=9999999999999\n\
             initial-enabled-packs= vanilla , ,x\nunknown-key=kept\n",
            StartupOptions::default(),
        );
        let p = settings.properties();
        assert_eq!(p.difficulty, Difficulty::Easy, "5 wraps to 1");
        assert_eq!(p.game_mode, GameType::Survival);
        assert_eq!(p.op_permission_level, PermissionLevel::Owners);
        assert_eq!(p.function_permission_level, PermissionLevel::All);
        assert_eq!(p.entity_broadcast_range_percentage, 1000);
        assert_eq!(p.max_world_size, 1);
        assert_eq!(p.announce_player_achievements, Some(true));
        assert_eq!(p.level_seed, Some(-12));
        assert!(!p.hardcore);
        assert_eq!(p.max_tick_time, 9_999_999_999_999);
        assert_eq!(p.initial_enabled_packs, ["vanilla", "", "x"]);
        let text = stored(&settings);
        for line in [
            "difficulty=easy\n",
            "entity-broadcast-range-percentage=1000\n",
            "function-permission-level=0\n",
            "generator-settings={\"a\"\\:1,\"b\"\\:[\"x\\\\u2028<\\\\u0001\"]}\n",
            "hardcore=false\n",
            "initial-enabled-packs=vanilla , ,x\n",
            "level-seed=\\ -12 \n",
            "level-type=minecraft\\:flat\n",
            "max-world-size=1\n",
            "op-permission-level=4\n",
            "unknown-key=kept\n",
        ] {
            assert!(text.contains(line), "{line:?} in\n{text}");
        }
        assert!(!text.contains("announce-player-achievements"));

        let settings = read(
            "generator-settings={\"layers\":[1],\"b\":2,\"layers\":\"x\"}\ndifficulty=HARD\n\
             gamemode=Creative\nlevel-type=default\nhardcore=TrUe\nserver-port=+25566\n\
             max-players=12x\nview-distance=99999999999\nspawn-protection=-0\nwhite-list=\n",
            StartupOptions::default(),
        );
        let p = settings.properties();
        assert_eq!(p.difficulty, Difficulty::Easy, "names are case-sensitive");
        assert_eq!(p.game_mode, GameType::Survival);
        assert!(p.hardcore);
        assert_eq!(
            (p.server_port, p.max_players, p.view_distance),
            (25566, 250, 5)
        );
        assert_eq!((p.spawn_protection, p.white_list), (0, false));
        let text = stored(&settings);
        assert!(text.contains("generator-settings={\"layers\"\\:\"x\",\"b\"\\:2}\n"));
        assert!(text.contains("level-type=default\n"));
    }

    #[test]
    fn names_and_ids() {
        let p = read(
            "difficulty=hard\ngamemode=2\nop-permission-level=1",
            StartupOptions::default(),
        );
        let p = p.properties();
        assert_eq!(p.difficulty, Difficulty::Hard);
        assert_eq!(p.game_mode, GameType::Adventure);
        assert_eq!(p.op_permission_level, PermissionLevel::Moderators);
        assert_eq!(Difficulty::by_id(-1), Difficulty::Hard);
        assert_eq!(GameType::by_name("spectator"), Some(GameType::Spectator));
    }

    #[test]
    fn invalid_values_stop_the_server() {
        for text in [
            "entity-broadcast-range-percentage=lots",
            "generator-settings=[1]",
            "generator-settings={'a':1}",
            "generator-settings=null",
        ] {
            let file = PropertiesFile::parse(text.as_bytes()).unwrap();
            let result = DedicatedServerProperties::read(file, &StartupOptions::default());
            assert!(
                matches!(result, Err(ServerPropertiesError::Invalid { .. })),
                "{text}"
            );
        }
    }

    #[test]
    fn overrides_and_flags() {
        let options = StartupOptions {
            overrides: CommandLineOverrides {
                server_port: Some(25570),
                online_mode: Some(true),
                max_players: Some(3),
                server_name: Some("Lobby".into()),
                ..Default::default()
            },
            enable_sync_chunk_writes: true,
            skip_comments: true,
        };
        let mut settings = read("server-port=1\nmax-players=9\n", options);
        let p = settings.properties();
        assert_eq!(
            (p.server_port, p.online_mode, p.max_players),
            (25570, true, 3)
        );
        assert!(p.sync_chunk_writes);
        let text = stored(&settings);
        assert!(!text.contains('#'), "Paper's flag drops the comment lines");
        assert!(text.starts_with("accepts-transfers=false\n"));
        assert!(text.contains("server-port=25570\n") && text.contains("server-name=Lobby\n"));

        // An update reads everything again: the override still wins over the new value.
        settings.source = std::env::temp_dir().join(format!(
            "horizon-server-properties-{}.properties",
            std::process::id()
        ));
        settings.update(PropertyUpdate::MaxPlayers(40)).unwrap();
        settings.update(PropertyUpdate::ViewDistance(12)).unwrap();
        settings
            .update(PropertyUpdate::Difficulty(Difficulty::Hard))
            .unwrap();
        let p = settings.properties();
        assert_eq!((p.max_players, p.view_distance), (3, 12));
        assert_eq!(p.difficulty, Difficulty::Hard);
        let on_disk = std::fs::read_to_string(&settings.source).unwrap();
        std::fs::remove_file(&settings.source).unwrap();
        assert!(on_disk.contains("difficulty=hard") && on_disk.contains("view-distance=12"));
    }

    #[test]
    fn comment_filter() {
        assert_eq!(strip_comment_lines("#a\n#b\nk=v\n"), "k=v\n");
        assert_eq!(strip_comment_lines("#a\r\nk=\\#v\n"), "\nk=\\#v\n");
    }

    /// Values from `WorldOptions.parseSeed` on Java 25.
    #[test]
    fn seeds() {
        assert_eq!(parse_seed("  "), None);
        assert_eq!(parse_seed("42"), Some(42));
        assert_eq!(parse_seed("hello"), Some(99_162_322));
        assert_eq!(parse_seed("Minecraft"), Some(-1_595_926_131));
        assert_eq!(parse_seed(" 7\t"), Some(7));
        assert_eq!(parse_seed("+5"), Some(5));
        assert_eq!(parse_seed("9223372036854775808"), Some(-1_773_151_197));
        assert_eq!(
            parse_seed("\u{a0}7"),
            Some(5015),
            "String.trim leaves U+00A0"
        );
    }
}
