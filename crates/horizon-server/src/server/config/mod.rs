//! The YAML settings the ported code reads, from the files and keys Leaf reads them from, with Leaf's
//! defaults (Paper's, Spigot's and Purpur's as Leaf and MineComplex's patches change them).
//!
//! | Setting | File | Key | Default |
//! |---|---|---|---|
//! | Velocity forwarding | `config/paper-global.yml` | `proxies.velocity.{enabled, online-mode, secret}` | `false`, `true`, `''` |
//! | Late ticks run back to back | `config/paper-global.yml` | `misc.catch-up-ticks` | `default` (5) |
//! | Commands sent to clients | `spigot.yml` | `commands.tab-complete`, `commands.send-namespaced` | `0`, `false` (Leaf; Spigot `true`) |
//! | Structure salts | `spigot.yml` | `world-settings.<world or default>.seed-*` ([`STRUCTURE_SALTS`], `seed-buriedtreasure`, `seed-mineshaft`, `seed-stronghold`) | the 26.3 jar's salts, `10387320`, `default`, `default` |
//! | End void rings | `purpur.yml` | `settings.generate-end-void-rings` | `false` |
//! | Lagging flag | `purpur.yml` | `settings.lagging-threshold` | `19.0` |
//! | 5 s TPS in `getTPS()` | `config/leaf-global.yml` | `misc.including-5s-in-get-tps` | `true` |
//! | Chunk autosave, unloads | `config/paper-world-defaults.yml` | `chunks.{auto-save-interval, max-auto-save-chunks-per-tick, min-chunk-unload-count, min-chunk-unload-fraction, delay-chunk-unloads-by}` | `default` (6000), `24`, `50`, `0.05`, `10s` |
//!
//! The per-world `paper-world.yml` overrides are not read yet (one world, the defaults apply).
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

use super::chunk_system::ChunkSettings;
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
/// `PaperConfigurations.WORLD_DEFAULTS_HEADER`.
const PAPER_WORLD_DEFAULTS_HEADER: &str = "This is the world defaults configuration file for Paper.
As you can see, there's a lot to configure. Some options may impact gameplay, so use
with caution, and make sure you know what each option does before configuring.

If you need help with the configuration or have any questions related to Paper,
join us in our Discord or check the docs page.

Configuration options here apply to all worlds, unless you specify overrides inside
the world-specific config file inside each world folder.

File Reference: https://docs.papermc.io/paper/reference/world-configuration/
Docs: https://docs.papermc.io/
Discord: https://discord.gg/papermc
Website: https://papermc.io/";
/// `WorldConfiguration.CURRENT_VERSION`.
pub const PAPER_WORLD_VERSION: i32 = 31;
/// bukkit.yml's `ticks-per.autosave` default (`autosavePeriod`; bukkit.yml is not read yet).
pub const AUTOSAVE_PERIOD: i32 = 6000;
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
#[derive(Clone, Debug, PartialEq)]
pub struct SpigotConfig {
    /// `commands.tab-complete`: below 0 no commands are sent; otherwise the command packet is sent and
    /// completion starts at that many characters.
    pub tab_complete: i32,
    pub send_namespaced: bool,
    /// `world-settings`' structure seeds, resolved per level by [`SpigotConfig::world_seeds`].
    pub seed_sections: SeedSections,
}

/// `SpigotWorldConfig.initWorldGenSeeds`' integer keys that replace a `minecraft:` random-spread
/// structure set's salt (Paper's `ChunkGeneratorStructureState.injectSpigot`): key, the set's path,
/// default (each the 26.3 jar's salt).
pub const STRUCTURE_SALTS: [(&str, &str, i32); 18] = [
    ("seed-village", "villages", 10387312),
    ("seed-desert", "desert_pyramids", 14357617),
    ("seed-igloo", "igloos", 14357618),
    ("seed-jungle", "jungle_temples", 14357619),
    ("seed-swamp", "swamp_huts", 14357620),
    ("seed-monument", "ocean_monuments", 10387313),
    ("seed-shipwreck", "shipwrecks", 165745295),
    ("seed-ocean", "ocean_ruins", 14357621),
    ("seed-outpost", "pillager_outposts", 165745296),
    ("seed-endcity", "end_cities", 10387313),
    ("seed-nether", "nether_complexes", 30084232),
    ("seed-mansion", "woodland_mansions", 10387319),
    ("seed-fossil", "nether_fossils", 14357921),
    ("seed-portal", "ruined_portals", 34222645),
    ("seed-ancientcity", "ancient_cities", 20083232),
    ("seed-trailruins", "trail_ruins", 83469867),
    ("seed-trialchambers", "trial_chambers", 94251327),
    ("seed-abandonedcamp", "abandoned_camp", 91231127),
];
const BURIED_TREASURE_SEED: (&str, i32) = ("seed-buriedtreasure", 10387320);
/// Read with `getString(path, "default")`, a number only when `NumberUtils.isParsable`.
const MINESHAFT_SEED: &str = "seed-mineshaft";
const STRONGHOLD_SEED: &str = "seed-stronghold";

/// One `world-settings` section's seed values as Bukkit reads them: an integer key's value when it
/// is a `Number` (`intValue`), a string key's value as `toString` when present.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeedSection {
    salts: [Option<i32>; 18],
    buried_treasure: Option<i32>,
    mineshaft: Option<String>,
    stronghold: Option<String>,
}

impl SeedSection {
    fn read(doc: &YamlDocument, section: &str) -> Self {
        let int = |key: &str| match doc.get(&format!("{section}.{key}")) {
            Value::Int(i) => Some(i as i32), // Number.intValue: the low 32 bits
            Value::Float(f) => Some(f as i32),
            _ => None,
        };
        let text = |key: &str| match doc.get(&format!("{section}.{key}")) {
            Value::Null => None,
            // A section's `toString` is never a number.
            Value::Collection => Some(String::new()),
            value => value.java_string(),
        };
        Self {
            salts: STRUCTURE_SALTS.map(|(key, ..)| int(key)),
            buried_treasure: int(BURIED_TREASURE_SEED.0),
            mineshaft: text(MINESHAFT_SEED),
            stronghold: text(STRONGHOLD_SEED),
        }
    }
}

/// The `spigot.yml` document, read per level at `world-settings.<world>.<key>` as
/// `SpigotWorldConfig` does: Bukkit splits a path on `.`, so a world name with dots nests.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeedSections {
    doc: YamlDocument,
}

/// A level's structure seeds (`SpigotWorldConfig`'s seed fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpigotWorldSeeds {
    /// By [`STRUCTURE_SALTS`].
    pub salts: [i32; 18],
    pub buried_treasure: i32,
    /// `None` is `default`: no override.
    pub mineshaft: Option<i32>,
    pub stronghold: Option<i64>,
}

impl Default for SpigotWorldSeeds {
    fn default() -> Self {
        Self {
            salts: STRUCTURE_SALTS.map(|(.., salt)| salt),
            buried_treasure: BURIED_TREASURE_SEED.1,
            mineshaft: None,
            stronghold: None,
        }
    }
}

impl SpigotWorldSeeds {
    /// The salt Paper gives a `minecraft:` random-spread set (`injectSpigot`); `None` for a set
    /// without a key, which keeps its own.
    pub fn salt(&self, set_path: &str) -> Option<i32> {
        STRUCTURE_SALTS
            .iter()
            .position(|(_, set, _)| *set == set_path)
            .map(|i| self.salts[i])
    }
}

impl SpigotConfig {
    /// `SpigotWorldConfig.initWorldGenSeeds` for the level `world` (its dimension key, e.g.
    /// `minecraft:overworld`): the world's section, else `world-settings.default`, else the default.
    /// A parsable but non-integer `seed-mineshaft`/`seed-stronghold` (`1.5`, or out of range) fails
    /// as `Integer.parseInt`/`Long.parseLong` does, which stops the level's creation in Leaf.
    pub fn world_seeds(&self, world: &str) -> Result<SpigotWorldSeeds, ConfigError> {
        let doc = &self.seed_sections.doc;
        let own_section = SeedSection::read(doc, &format!("world-settings.{world}"));
        let own = Some(&own_section);
        let default_section = SeedSection::read(doc, "world-settings.default");
        let default = &default_section;
        let mut seeds = SpigotWorldSeeds::default();
        for (i, salt) in seeds.salts.iter_mut().enumerate() {
            *salt = own
                .and_then(|s| s.salts[i])
                .or(default.salts[i])
                .unwrap_or(*salt);
        }
        seeds.buried_treasure = own
            .and_then(|s| s.buried_treasure)
            .or(default.buried_treasure)
            .unwrap_or(seeds.buried_treasure);
        let text = |pick: fn(&SeedSection) -> &Option<String>| {
            own.and_then(|s| pick(s).clone())
                .or_else(|| pick(default).clone())
                .unwrap_or_else(|| "default".into())
        };
        let invalid = |key: &'static str, text: &str| ConfigError::Invalid {
            file: "spigot.yml".into(),
            path: key,
            message: format!("For input string: \"{text}\""),
        };
        let mineshaft = text(|s| &s.mineshaft);
        if is_parsable(&mineshaft) {
            seeds.mineshaft = Some(
                mineshaft
                    .parse()
                    .map_err(|_| invalid(MINESHAFT_SEED, &mineshaft))?,
            );
        }
        let stronghold = text(|s| &s.stronghold);
        if is_parsable(&stronghold) {
            seeds.stronghold = Some(
                stronghold
                    .parse()
                    .map_err(|_| invalid(STRONGHOLD_SEED, &stronghold))?,
            );
        }
        Ok(seeds)
    }
}

/// The `purpur.yml` settings read so far.
#[derive(Clone, Debug, PartialEq)]
pub struct PurpurConfig {
    pub verbose: bool,
    pub lagging_threshold: f64,
    /// `settings.generate-end-void-rings`: Purpur's rings of void around the End's main island.
    pub generate_end_void_rings: bool,
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
    /// `paper-world-defaults.yml`'s `chunks`.
    pub paper_world_chunks: ChunkSettings,
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
    let tab_complete = file.int("commands.tab-complete", 0);
    let send_namespaced = file.bool("commands.send-namespaced", false);
    // Each level's `SpigotWorldConfig` adds these defaults (written with the file); read once here.
    for (key, _, salt) in STRUCTURE_SALTS {
        file.get(
            &format!("world-settings.default.{key}"),
            Value::Int(salt.into()),
        );
    }
    let (key, salt) = BURIED_TREASURE_SEED;
    file.get(
        &format!("world-settings.default.{key}"),
        Value::Int(salt.into()),
    );
    for key in [MINESHAFT_SEED, STRONGHOLD_SEED] {
        file.get(
            &format!("world-settings.default.{key}"),
            Value::Str("default".into()),
        );
    }
    let seed_sections = SeedSections {
        doc: file.doc.clone(),
    };
    let config = SpigotConfig {
        tab_complete,
        send_namespaced,
        seed_sections,
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
        generate_end_void_rings: file.bool("settings.generate-end-void-rings", false),
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

/// `Configurations.initializeWorldDefaultsConfiguration` for `paper-world-defaults.yml` in `dir`,
/// the `chunks` settings above only: read with Configurate's coercions, then written back (sorted,
/// with `_version`). A value that can't be read stops the start, as Paper's loader does.
pub fn load_paper_world_defaults(dir: &Path) -> Result<ChunkSettings, ConfigError> {
    let path = dir.join("paper-world-defaults.yml");
    let file = file_name(&path);
    let mut doc = match YamlDocument::read(&path) {
        Ok(None) => YamlDocument::default(),
        Ok(Some(text)) => YamlDocument::parse(&text).map_err(|source| ConfigError::Syntax {
            file: file.clone(),
            source,
        })?,
        Err(source) => return Err(ConfigError::Io { file, source }),
    };
    if doc.get("_version") == Value::Null {
        doc.set("_version", Value::Int(PAPER_WORLD_VERSION.into()));
    }
    let invalid = |path, message| ConfigError::Invalid {
        file: file.clone(),
        path,
        message,
    };
    let defaults = ChunkSettings::default();
    // `AutosavePeriod` (`FallbackValue.Int`): `default`, an int, or a parsable string; a negative
    // value is the default too (`negToDef`).
    let auto_save = match doc.get("chunks.auto-save-interval") {
        Value::Null => None,
        Value::Str(s) if s.eq_ignore_ascii_case("default") => None,
        Value::Str(s) if is_parsable(&s) => Some(s.parse::<i32>().map_err(|_| {
            invalid(
                "chunks, auto-save-interval",
                format!("For input string: \"{s}\""),
            )
        })?),
        Value::Int(i) if i32::try_from(i).is_ok() => Some(i as i32),
        other => {
            return Err(invalid(
                "chunks, auto-save-interval",
                format!(
                    "{} is not a integer or 'default'",
                    other.java_string().unwrap_or_default()
                ),
            ));
        }
    }
    .filter(|&v| v >= 0);
    let int = |key: &'static str, default: usize| -> Result<usize, ConfigError> {
        configurate_int(&doc, &format!("chunks.{key}"))
            .map(|v| v.map_or(default, |v| v.max(0) as usize))
            .map_err(|m| invalid(key, m))
    };
    let max_per_tick = int(
        "max-auto-save-chunks-per-tick",
        defaults.max_auto_save_chunks_per_tick,
    )?;
    let min_count = int("min-chunk-unload-count", defaults.min_chunk_unload_count)?;
    let fraction = match doc.get("chunks.min-chunk-unload-fraction") {
        Value::Null => defaults.min_chunk_unload_fraction,
        Value::Int(i) => i as f64,
        Value::Float(f) => f,
        Value::Str(s) => s.trim().parse().map_err(|_| {
            invalid(
                "chunks, min-chunk-unload-fraction",
                format!(
                    "Failed to coerce input value of type class java.lang.String to double: {s}"
                ),
            )
        })?,
        _ => {
            return Err(invalid(
                "chunks, min-chunk-unload-fraction",
                "Failed to coerce input value of a collection to double".into(),
            ));
        }
    };
    // Paper's `Duration`: any scalar's text.
    let delay = match doc.get("chunks.delay-chunk-unloads-by") {
        Value::Null => "10s".to_owned(),
        Value::Collection => {
            return Err(invalid(
                "chunks, delay-chunk-unloads-by",
                "Expected a scalar, got a collection".into(),
            ));
        }
        value => value.java_string().unwrap_or_default(),
    };
    let settings = ChunkSettings {
        auto_save_interval: i64::from(auto_save.unwrap_or(AUTOSAVE_PERIOD)),
        max_auto_save_chunks_per_tick: max_per_tick,
        min_chunk_unload_count: min_count,
        min_chunk_unload_fraction: fraction,
        delay_chunk_unloads_by: duration_seconds(&delay) * 20,
    };
    doc.set(
        "chunks.auto-save-interval",
        auto_save.map_or(Value::Str("default".into()), |v| Value::Int(v.into())),
    );
    doc.set(
        "chunks.max-auto-save-chunks-per-tick",
        Value::Int(max_per_tick as i128),
    );
    doc.set(
        "chunks.min-chunk-unload-count",
        Value::Int(min_count as i128),
    );
    doc.set("chunks.min-chunk-unload-fraction", Value::Float(fraction));
    doc.set(
        "chunks.delay-chunk-unloads-by",
        Value::Str(delay.as_str().into()),
    );
    if let Err(e) = write(
        &path,
        &doc.to_text(PAPER_WORLD_DEFAULTS_HEADER, "# ", KeyOrder::Sorted),
    ) {
        tracing::error!("Could not save {}: {e}", path.display());
    }
    Ok(settings)
}

/// Configurate's int scalar: an int (its low 32 bits), an integral float, or a number's text.
fn configurate_int(doc: &YamlDocument, path: &str) -> Result<Option<i32>, String> {
    match doc.get(path) {
        Value::Null => Ok(None),
        Value::Int(i) => Ok(Some(i as i32)),
        Value::Float(f) if f.fract() == 0.0 => Ok(Some(f as i32)),
        Value::Str(s) => s.trim().parse().map(Some).map_err(|_| {
            format!("Failed to coerce input value of type class java.lang.String to int: {s}")
        }),
        other => Err(format!(
            "Failed to coerce input value {} to int",
            other.java_string().unwrap_or_default()
        )),
    }
}

/// Paper's `Duration.getSeconds`: spaces dropped, the last character the unit (`d`, `h`, `m`,
/// else seconds), the other non-numeric characters dropped, the rest a double (0 if not one).
fn duration_seconds(text: &str) -> i64 {
    let text: String = text.chars().filter(|&c| c != ' ').collect();
    let Some(unit) = text.chars().last() else {
        return 0; // Java's charAt(-1) throws; an empty value is not a valid setting
    };
    let number: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
        .collect();
    let mut seconds: f64 = number.parse().unwrap_or(0.0);
    seconds *= match unit {
        'd' => 86_400.0,
        'h' => 3_600.0,
        'm' => 60.0,
        _ => 1.0,
    };
    i64::from(seconds as i32)
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
