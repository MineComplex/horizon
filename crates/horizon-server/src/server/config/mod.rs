//! The YAML settings the ported code reads, from the files and keys Leaf reads them from, with Leaf's
//! defaults (Paper's, Spigot's and Purpur's as Leaf and MineComplex's patches change them).
//!
//! | Setting | File | Key | Default |
//! |---|---|---|---|
//! | Velocity forwarding | `config/paper-global.yml` | `proxies.velocity.{enabled, online-mode, secret}` | `false`, `true`, `''` |
//! | Late ticks run back to back | `config/paper-global.yml` | `misc.catch-up-ticks` | `default` (5) |
//! | Commands sent to clients | `spigot.yml` | `commands.tab-complete`, `commands.send-namespaced` | `0`, `false` (Leaf; Spigot `true`) |
//! | Lagging flag | `purpur.yml` | `settings.lagging-threshold` | `19.0` |
//! | 5 s TPS in `getTPS()` | `config/leaf-global.yml` | `misc.including-5s-in-get-tps` | `true` |
//!
//! Not here: `proxies.proxy-protocol` and BungeeCord (removed by MineComplex's cleanup patches);
//! `gale-global.yml` (its `misc.last-tick-time-in-tps-command` no longer reaches `/tps` after
//! MineComplex's patch 0071, and nothing else in it touches the tick loop yet).
//!
//! Each file is read and written back the way its library does it (all three write a missing file with
//! the defaults read and a header): Configurate for Paper (values normalized and written back, keys
//! sorted, `_version`), Bukkit's `YamlConfiguration` for Spigot and Purpur (missing keys filled in,
//! other values left as they are, `config-version`), ConfigurationMaster for Leaf. Keys this port does
//! not read yet are kept. A file written from scratch holds only the keys above (plus each file's version
//! and, for Purpur, `verbose`), where Leaf writes all of its settings.
pub mod yaml;

use std::path::{Path, PathBuf};

use yaml::{KeyOrder, Value, YamlDocument, YamlError};

use super::tick_loop::TickLoopConfig;

/// `GlobalConfiguration.CURRENT_VERSION`.
pub const PAPER_GLOBAL_VERSION: i32 = 31;
const PAPER_GLOBAL_HEADER: &str = "This is the global configuration file for Paper.
As you can see, there's a lot to configure. Some options may impact gameplay, so use
with caution, and make sure you know what each option does before configuring.

If you need help with the configuration or have any questions related to Paper,
join us in our Discord or check the docs page.

The world configuration options have been moved inside
their respective world folder. The files are named paper-world.yml

File Reference: https://docs.papermc.io/paper/reference/global-configuration/
Docs: https://docs.papermc.io/
Discord: https://discord.gg/papermc
Website: https://papermc.io/";
const SPIGOT_HEADER: &str = "This is the Spigot configuration file for Paper.
As you can see, there's tons to configure. Some options may impact gameplay, so use
with caution, and make sure you know what each option does before configuring.

If you need help with the configuration or have any questions related to Paper,
join us in our Discord or check the docs page.

File Reference: https://docs.papermc.io/paper/reference/spigot-configuration/
Docs: https://docs.papermc.io/
Discord: https://discord.gg/papermc
Website: https://papermc.io/";
const PURPUR_HEADER: &str = "This is the main configuration file for Purpur.
As you can see, there's tons to configure. Some options may impact gameplay, so use
with caution, and make sure you know what each option does before configuring.

If you need help with the configuration or have any questions related to Purpur,
join us in our Discord guild.

Website: https://purpurmc.org
Docs: https://purpurmc.org/docs ";
/// The comment ConfigurationMaster writes above `config-version`, the file's first key.
const LEAF_HEADER: &str = "Leaf Config

Website: https://www.leafmc.one/
Docs: https://www.leafmc.one/docs/getting-started
GitHub Repo: https://github.com/Winds-Studio/Leaf
Discord: https://discord.com/invite/gfgAwdSEuM";
/// `ConfigCategory`'s base paths, created in this order before any module reads (`structureConfig`).
const LEAF_CATEGORIES: [&str; 6] = [
    "async",
    "performance",
    "fixes",
    "gameplay-mechanisms",
    "network",
    "misc",
];

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Bukkit's message for spigot.yml and purpur.yml (the server does not start); also used for
    /// paper-global.yml, whose loader fails the start the same way.
    #[error("Could not load {file}, please correct your syntax errors: {source}")]
    Syntax { file: String, source: YamlError },
    #[error("Could not load {file}: {source}")]
    Io {
        file: String,
        source: std::io::Error,
    },
    /// A value Configurate can't deserialize (`SerializationException`): the server does not start.
    #[error("Could not load {file}: [{path}]: {message}")]
    Invalid {
        file: String,
        path: &'static str,
        message: String,
    },
}

/// `paper-global.yml`'s `proxies.velocity`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VelocityConfig {
    pub enabled: bool,
    pub online_mode: bool,
    pub secret: String,
}

/// The `paper-global.yml` settings read so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperGlobalConfig {
    pub velocity: VelocityConfig,
    /// `misc.catch-up-ticks`: `None` is `default`, which the tick loop reads as 5.
    pub catchup_ticks: Option<i32>,
}

/// The `spigot.yml` settings read so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpigotConfig {
    /// `commands.tab-complete`: below 0 no commands are sent; otherwise the command packet is sent and
    /// completion starts at that many characters.
    pub tab_complete: i32,
    pub send_namespaced: bool,
}

/// The `purpur.yml` settings read so far.
#[derive(Clone, Debug, PartialEq)]
pub struct PurpurConfig {
    pub verbose: bool,
    pub lagging_threshold: f64,
}

/// The `leaf-global.yml` settings read so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeafGlobalConfig {
    pub including_5s_in_get_tps: bool,
}

impl Default for LeafGlobalConfig {
    fn default() -> Self {
        Self {
            including_5s_in_get_tps: true,
        }
    }
}

/// The YAML configs, loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct YamlConfigs {
    pub leaf: LeafGlobalConfig,
    pub spigot: SpigotConfig,
    pub paper: PaperGlobalConfig,
    pub purpur: PurpurConfig,
}

impl YamlConfigs {
    /// What the tick loop reads (`catchupTicks.or(5)`, `PurpurConfig.laggingThreshold`).
    pub fn tick_loop(&self) -> TickLoopConfig {
        TickLoopConfig {
            catchup_ticks: self.paper.catchup_ticks.unwrap_or(5),
            lagging_threshold: self.purpur.lagging_threshold,
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

/// Bukkit's `YamlConfiguration` with `copyDefaults(true)`, read through Spigot's and Purpur's
/// `getInt`/`getBoolean`/`getDouble`: a missing (or null) key gets the default, written back; a value of
/// the wrong type reads as the default and stays in the file.
struct BukkitFile {
    path: PathBuf,
    doc: YamlDocument,
}

impl BukkitFile {
    /// `config.load(file)`: a file that can't be read is an empty configuration; bad YAML stops the start.
    fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = YamlDocument::read(path).ok().flatten().unwrap_or_default();
        let doc = YamlDocument::parse(&text).map_err(|source| {
            let file = file_name(path);
            tracing::error!("Could not load {file}, please correct your syntax errors");
            ConfigError::Syntax { file, source }
        })?;
        Ok(Self {
            path: path.to_owned(),
            doc,
        })
    }

    fn get(&mut self, path: &str, default: Value<'static>) -> Value<'_> {
        if self.doc.get(path) == Value::Null {
            self.doc.set(path, default);
        }
        self.doc.get(path)
    }

    fn int(&mut self, path: &str, default: i32) -> i32 {
        match self.get(path, Value::Int(default.into())) {
            Value::Int(i) => i as i32, // Number.intValue: the low 32 bits
            Value::Float(f) => f as i32,
            _ => default,
        }
    }

    fn bool(&mut self, path: &str, default: bool) -> bool {
        match self.get(path, Value::Bool(default)) {
            Value::Bool(b) => b,
            _ => default,
        }
    }

    fn double(&mut self, path: &str, default: f64) -> f64 {
        match self.get(path, Value::Float(default)) {
            Value::Int(i) => i as f64,
            Value::Float(f) => f,
            _ => default,
        }
    }

    /// `config.save(file)`: failures are logged.
    fn save(&self, header: &str) {
        if let Err(e) = write(
            &self.path,
            &self.doc.to_text(header, "#", KeyOrder::AsLoaded),
        ) {
            tracing::error!("Could not save {}: {e}", self.path.display());
        }
    }
}

/// `SpigotConfig.init` (the settings above only).
pub fn load_spigot(path: &Path) -> Result<SpigotConfig, ConfigError> {
    let mut file = BukkitFile::load(path)?;
    let version = file.int("config-version", 13);
    file.doc.set("config-version", Value::Int(13));
    if version < 6 {
        let old = file.bool("commands.tab-complete", true);
        file.doc.set(
            "commands.tab-complete",
            Value::Int(if old { 0 } else { -1 }),
        );
    }
    let config = SpigotConfig {
        tab_complete: file.int("commands.tab-complete", 0),
        send_namespaced: file.bool("commands.send-namespaced", false),
    };
    file.save(SPIGOT_HEADER);
    Ok(config)
}

/// `PurpurConfig.init` (the settings above only).
pub fn load_purpur(path: &Path) -> Result<PurpurConfig, ConfigError> {
    let mut file = BukkitFile::load(path)?;
    let verbose = file.bool("verbose", false);
    file.int("config-version", 49);
    file.doc.set("config-version", Value::Int(49));
    let config = PurpurConfig {
        verbose,
        lagging_threshold: file.double("settings.lagging-threshold", 19.0),
    };
    file.save(PURPUR_HEADER);
    Ok(config)
}

/// `Configurations.initializeGlobalConfiguration` for `paper-global.yml` in `dir` (Paper's
/// `--paper-settings-directory`, `config` by default). `velocity_secret_env` is `PAPER_VELOCITY_SECRET`.
pub fn load_paper_global(
    dir: &Path,
    velocity_secret_env: Option<&str>,
) -> Result<PaperGlobalConfig, ConfigError> {
    let path = dir.join("paper-global.yml");
    let file = file_name(&path);
    let mut doc = match YamlDocument::read(&path) {
        Ok(None) => {
            let mut doc = YamlDocument::default();
            doc.set("_version", Value::Int(PAPER_GLOBAL_VERSION.into()));
            doc
        }
        Ok(Some(text)) => {
            let doc = YamlDocument::parse(&text).map_err(|source| ConfigError::Syntax {
                file: file.clone(),
                source,
            })?;
            verify_paper_version(doc)
        }
        Err(source) => return Err(ConfigError::Io { file, source }),
    };
    let invalid = |path, message| ConfigError::Invalid {
        file: file.clone(),
        path,
        message,
    };

    let mut velocity = VelocityConfig {
        enabled: configurate_bool(&doc, "proxies.velocity.enabled")
            .map_err(|m| invalid("proxies, velocity, enabled", m))?
            .unwrap_or(false),
        online_mode: configurate_bool(&doc, "proxies.velocity.online-mode")
            .map_err(|m| invalid("proxies, velocity, online-mode", m))?
            .unwrap_or(true),
        secret: match doc.get("proxies.velocity.secret") {
            Value::Collection => {
                return Err(invalid(
                    "proxies, velocity, secret",
                    "Expected a scalar, got a collection".into(),
                ));
            }
            value => value.java_string().unwrap_or_default(),
        },
    };
    let catchup_ticks = int_or_default(&doc, "misc.catch-up-ticks")
        .map_err(|m| invalid("misc, catch-up-ticks", m))?;

    // Velocity's @PostProcess, before the values are written back (so both changes reach the file).
    if velocity.enabled {
        if let Some(secret) = velocity_secret_env.filter(|s| !s.is_empty()) {
            velocity.secret = secret.to_owned();
        }
        if velocity.secret.is_empty() {
            tracing::error!(
                "Velocity is enabled, but no secret key was specified. A secret key is required. Disabling velocity..."
            );
            velocity.enabled = false;
        }
    }

    doc.set("proxies.velocity.enabled", Value::Bool(velocity.enabled));
    doc.set(
        "proxies.velocity.online-mode",
        Value::Bool(velocity.online_mode),
    );
    doc.set(
        "proxies.velocity.secret",
        Value::Str(velocity.secret.as_str().into()),
    );
    doc.set(
        "misc.catch-up-ticks",
        catchup_ticks.map_or(Value::Str("default".into()), |t| Value::Int(t.into())),
    );

    match write(
        &path,
        &doc.to_text(PAPER_GLOBAL_HEADER, "# ", KeyOrder::Sorted),
    ) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => tracing::warn!(
            "Could not save {}: Paper could not persist the full set of configuration settings in the configuration file. Any setting missing from the configuration file will be set with its default value in memory. Admins should make sure to review the configuration documentation at https://docs.papermc.io/paper/configuration for more details.",
            path.display()
        ),
        Err(source) => return Err(ConfigError::Io { file, source }),
    }
    Ok(PaperGlobalConfig {
        velocity,
        catchup_ticks,
    })
}

/// `verifyGlobalConfigVersion`.
fn verify_paper_version(mut doc: YamlDocument) -> YamlDocument {
    match doc.get("_version") {
        Value::Null => {
            tracing::warn!("The global config file didn't have a version set, assuming latest");
            doc.set("_version", Value::Int(PAPER_GLOBAL_VERSION.into()));
        }
        Value::Int(v) if v > PAPER_GLOBAL_VERSION.into() => tracing::error!(
            "Loading a newer configuration than is supported ({v} > {PAPER_GLOBAL_VERSION})! You may have to backup & delete your global config file to start the server."
        ),
        _ => {}
    }
    doc
}

/// Configurate's `Scalars.BOOLEAN` (`None`: missing, the field keeps its default).
fn configurate_bool(doc: &YamlDocument, path: &str) -> Result<Option<bool>, String> {
    Ok(Some(match doc.get(path) {
        Value::Null => return Ok(None),
        Value::Bool(b) => b,
        // `!number.equals(0)`: only an `Integer` 0 equals the boxed 0.
        Value::Int(i) => i32::try_from(i) != Ok(0),
        Value::Float(_) => true,
        Value::Str(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "1" => true,
            "false" | "f" | "no" | "n" | "0" => false,
            _ => {
                return Err(format!(
                    "Failed to coerce input value of type class java.lang.String to boolean: {s}"
                ));
            }
        },
        Value::Collection => {
            return Err("Failed to coerce input value of a collection to boolean".into());
        }
    }))
}

/// Paper's `IntOr.Default` serializer: `default` (any case), a number, or an error.
fn int_or_default(doc: &YamlDocument, path: &str) -> Result<Option<i32>, String> {
    let lossy = |n: String| {
        tracing::error!("{n} cannot be converted to an integer without losing information")
    };
    match doc.get(path) {
        Value::Null => Ok(None),
        Value::Str(s) if s.eq_ignore_ascii_case("default") => Ok(None),
        Value::Str(s) if is_parsable(&s) => s
            .parse()
            .map(Some)
            .map_err(|_| format!("For input string: \"{s}\"")),
        Value::Int(i) => {
            if i32::try_from(i).is_err() {
                lossy(i.to_string());
            }
            Ok(Some(i as i32))
        }
        Value::Float(f) => {
            if f.fract() != 0.0 || f as i32 as f64 != f {
                lossy(crate::util::number_text::double_to_string(f));
            }
            Ok(Some(f as i32))
        }
        other => Err(format!(
            "{} is not a(n) int or 'default'",
            other.java_string().unwrap_or_default()
        )),
    }
}

/// Commons Lang's `NumberUtils.isParsable`: an optional leading `-`, digits, at most one `.` inside.
fn is_parsable(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
    !body.is_empty()
        && !body.ends_with('.')
        && digits(int)
        && digits(frac)
        && !(int.is_empty() && frac.is_empty())
}

/// `LeafConfig.loadConfig` and `ConfigModule.loadAfterBootstrap` for `config/leaf-global.yml`: a file
/// that can't be loaded is logged, and the modules keep their defaults.
pub fn load_leaf_global(path: &Path) -> LeafGlobalConfig {
    let doc = YamlDocument::read(path)
        .map_err(|e| e.to_string())
        .and_then(|text| YamlDocument::parse(&text.unwrap_or_default()).map_err(|e| e.to_string()));
    let mut doc = match doc {
        Ok(doc) => doc,
        Err(e) => {
            tracing::error!("Failed to load config modules! {e}");
            return LeafGlobalConfig::default();
        }
    };
    doc.set("config-version", Value::Str("3.0".into()));
    for category in LEAF_CATEGORIES {
        if doc.get(category) == Value::Null {
            doc.set(category, Value::Collection);
        }
    }
    let path_5s = "misc.including-5s-in-get-tps";
    // ConfigurationMaster's `getBoolean`: the value's text, `true`/`false` in any case, else the default.
    let including_5s = match doc.get(path_5s).java_string() {
        None => {
            doc.set(path_5s, Value::Bool(true));
            true
        }
        Some(text) if text.eq_ignore_ascii_case("false") => false,
        Some(_) => true,
    };
    if let Err(e) = write(path, &doc.to_text(LEAF_HEADER, "# ", KeyOrder::AsLoaded)) {
        tracing::error!("Failed to save config file! {e}");
    }
    LeafGlobalConfig {
        including_5s_in_get_tps: including_5s,
    }
}

#[cfg(test)]
mod tests;
