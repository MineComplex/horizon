//! Startup up to the point where Leaf creates the levels: the settings files and the language, in
//! Leaf's order and with its log lines.
//!
//! 1. `Main`: `LeafConfig.loadConfig` and `ConfigModule.loadAfterBootstrap` (`config/leaf-global.yml`);
//! 2. `Bootstrap.bootStrap`: vanilla's `Language` (`en_us.json`, here from the vanilla jar the admin
//!    provides, opened and checked first);
//! 3. `Main`: `DedicatedServerSettings` and `forceSave` (`server.properties`, with the command line's
//!    overrides);
//! 4. `DedicatedServer.initServer`: "Starting minecraft server version 26.3", "Loading properties",
//!    `SpigotConfig.init` (`spigot.yml`), Paper's global config (`config/paper-global.yml`), Gale's (not
//!    read, see `config`), `PurpurConfig.init` (`purpur.yml`), "Default game type: ...".
//!
//! Leaf's `purgeOutdated` (moving pre-2023 `leaf.yml`/`pufferfish.yml` to a backup folder) is not ported.
use std::path::{Path, PathBuf};

use super::{
    config::{self, ConfigError, YamlConfigs},
    server_properties::{DedicatedServerSettings, ServerPropertiesError, StartupOptions},
    vanilla_jar::{self, VanillaJar, VanillaJarError},
};
use crate::azalea::language::Language;

/// The files startup reads. Relative paths are taken from the server directory; the defaults are
/// Leaf's (its options' defaults, and the fixed `config/leaf-global.yml`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerPaths {
    /// `-c`/`--config`.
    pub properties: PathBuf,
    /// `-S`/`--spigot-settings`.
    pub spigot_settings: PathBuf,
    /// `--paper-dir`/`--paper-settings-directory`: where `paper-global.yml` is.
    pub paper_settings_directory: PathBuf,
    /// `--purpur`/`--purpur-settings`.
    pub purpur_settings: PathBuf,
    /// Not an option in Leaf: always `config/leaf-global.yml`.
    pub leaf_global: PathBuf,
    /// The vanilla 26.3 server jar (Paperclip's `cache/mojang_26.3.jar`).
    pub vanilla_jar: PathBuf,
}

impl Default for ServerPaths {
    fn default() -> Self {
        Self {
            properties: "server.properties".into(),
            spigot_settings: "spigot.yml".into(),
            paper_settings_directory: "config".into(),
            purpur_settings: "purpur.yml".into(),
            leaf_global: Path::new("config").join("leaf-global.yml"),
            vanilla_jar: Path::new("cache").join(format!("mojang_{}.jar", vanilla_jar::VERSION_ID)),
        }
    }
}

/// What startup is given: nothing is read from the environment or the working directory here.
#[derive(Clone, Debug, Default)]
pub struct BootstrapOptions {
    pub paths: ServerPaths,
    pub startup: StartupOptions,
    /// The `PAPER_VELOCITY_SECRET` environment variable, which replaces the configured secret.
    pub velocity_secret_env: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error(transparent)]
    VanillaJar(#[from] VanillaJarError),
    #[error(transparent)]
    Properties(#[from] ServerPropertiesError),
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// Everything startup loaded.
#[derive(Debug)]
pub struct ServerConfig {
    pub properties: DedicatedServerSettings,
    pub yaml: YamlConfigs,
    /// The `en_us` table, for `language::install` (left to the caller: it is process-wide).
    pub language: Language,
    /// Kept open for the data the server reads later (`data/minecraft/...`).
    pub vanilla_jar: VanillaJar,
}

/// Loads the settings and the language from `server_dir`, writing the files back as Leaf does.
pub fn bootstrap(
    server_dir: &Path,
    options: BootstrapOptions,
) -> Result<ServerConfig, BootstrapError> {
    let paths = &options.paths;
    let path = |p: &Path| server_dir.join(p);

    let leaf = config::load_leaf_global(&path(&paths.leaf_global));

    let mut vanilla_jar = VanillaJar::open(&path(&paths.vanilla_jar))?;
    let language = vanilla_jar.load_language();

    let properties =
        DedicatedServerSettings::load(&path(&paths.properties), options.startup.clone())?;
    properties.force_save();

    tracing::info!(
        "Starting minecraft server version {}",
        vanilla_jar::VERSION_ID
    );
    tracing::info!("Loading properties");
    let spigot = config::load_spigot(&path(&paths.spigot_settings))?;
    let paper = config::load_paper_global(
        &path(&paths.paper_settings_directory),
        options.velocity_secret_env.as_deref(),
    )?;
    let purpur = config::load_purpur(&path(&paths.purpur_settings))?;
    tracing::info!(
        "Default game type: {}",
        properties
            .properties()
            .game_mode
            .name()
            .to_ascii_uppercase()
    );

    Ok(ServerConfig {
        properties,
        yaml: YamlConfigs {
            leaf,
            spigot,
            paper,
            purpur,
        },
        language,
        vanilla_jar,
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    #[test]
    fn loads_and_writes_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in [
            ("version.json", r#"{"id": "26.3"}"#),
            (
                "assets/minecraft/lang/en_us.json",
                r#"{"menu.quit": "Quit Game"}"#,
            ),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        std::fs::create_dir(dir.path().join("cache")).unwrap();
        std::fs::write(
            dir.path().join("cache/mojang_26.3.jar"),
            zip.finish().unwrap().into_inner(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("purpur.yml"),
            "settings:\n  lagging-threshold: 18\n",
        )
        .unwrap();

        let config = bootstrap(dir.path(), BootstrapOptions::default()).unwrap();
        assert_eq!(config.language.get("menu.quit"), Some("Quit Game"));
        assert_eq!(config.yaml.tick_loop().catchup_ticks, 5);
        assert_eq!(config.yaml.tick_loop().lagging_threshold, 18.0);
        assert!(config.yaml.leaf.including_5s_in_get_tps);
        assert!(!config.yaml.spigot.send_namespaced);
        for file in [
            "server.properties",
            "spigot.yml",
            "purpur.yml",
            "config/paper-global.yml",
            "config/leaf-global.yml",
        ] {
            assert!(dir.path().join(file).is_file(), "{file}");
        }

        std::fs::remove_file(dir.path().join("cache/mojang_26.3.jar")).unwrap();
        let err = bootstrap(dir.path(), BootstrapOptions::default()).unwrap_err();
        assert!(
            matches!(
                err,
                BootstrapError::VanillaJar(VanillaJarError::Open { .. })
            ),
            "{err}"
        );
    }
}
