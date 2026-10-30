//! `horizon-server`: Leaf 26.3 as far as it is ported (ARCHITECTURE.md). It loads the settings and the
//! language from the working directory, then runs the tick loop and the network
//! (`server::runtime`) with the overworld of `level-name` until `stop`, Ctrl-C or SIGTERM.
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use horizon_server::{
    azalea::language,
    registry::{self, Registries},
    server::{
        bootstrap::{self, BootstrapOptions, ServerPaths},
        level::{World, WorldSettings, WorldTables},
        runtime::{self, RuntimeConfig, Server, StartError},
        server_properties::{CommandLineOverrides, StartupOptions},
    },
    world::{
        chunk_io::compression_from_property,
        level_storage::{Difficulty, GameType},
    },
};

/// The options of Leaf's `Main` that the ported startup reads (same names and aliases; `-h` is the
/// host, as in Leaf, and help is `-?`/`--help`), plus `--vanilla-jar`, which Paperclip handles in Leaf.
#[derive(Parser, Debug)]
#[command(name = "horizon-server", version, disable_help_flag = true)]
struct Options {
    #[arg(short = '?', long = "help", action = clap::ArgAction::Help, help = "Show the help")]
    help: Option<bool>,
    #[arg(
        short = 'c',
        long = "config",
        default_value = "server.properties",
        help = "Properties file to use"
    )]
    config: PathBuf,
    #[arg(
        short = 'h',
        long = "host",
        visible_alias = "server-ip",
        help = "Host to listen on"
    )]
    host: Option<String>,
    #[arg(
        short = 'w',
        long = "world",
        visible_alias = "level-name",
        help = "World name"
    )]
    world: Option<String>,
    #[arg(
        short = 'p',
        long = "port",
        visible_alias = "server-port",
        help = "Port to listen on"
    )]
    port: Option<i32>,
    /// `Boolean.valueOf`: anything but `true` (any case) is false.
    #[arg(
        short = 'o',
        long = "online-mode",
        help = "Whether to use online authentication"
    )]
    online_mode: Option<String>,
    #[arg(
        short = 's',
        long = "size",
        visible_alias = "max-players",
        help = "Maximum amount of players"
    )]
    size: Option<i32>,
    #[arg(long = "server-name", help = "Name of the server")]
    server_name: Option<String>,
    #[arg(
        short = 'S',
        long = "spigot-settings",
        default_value = "spigot.yml",
        help = "File for spigot settings"
    )]
    spigot_settings: PathBuf,
    #[arg(
        long = "paper-settings-directory",
        visible_alias = "paper-dir",
        default_value = "config",
        help = "Directory for Paper settings"
    )]
    paper_settings_directory: PathBuf,
    #[arg(
        long = "purpur-settings",
        visible_alias = "purpur",
        default_value = "purpur.yml",
        help = "File for purpur settings"
    )]
    purpur_settings: PathBuf,
    #[arg(
        long = "vanilla-jar",
        default_value = "cache/mojang_26.3.jar",
        help = "The vanilla 26.3 server jar (Paperclip's download, or the game jar)"
    )]
    vanilla_jar: PathBuf,
    #[arg(
        long = "nogui",
        help = "Disables the graphical console (there is none)"
    )]
    nogui: bool,
}

fn main() -> std::process::ExitCode {
    let boot = std::time::Instant::now();
    tracing_subscriber::fmt().with_target(false).init();
    let options = Options::parse();
    let bootstrap_options = BootstrapOptions {
        paths: ServerPaths {
            properties: options.config,
            spigot_settings: options.spigot_settings,
            paper_settings_directory: options.paper_settings_directory,
            purpur_settings: options.purpur_settings,
            vanilla_jar: options.vanilla_jar,
            ..ServerPaths::default()
        },
        startup: StartupOptions {
            overrides: CommandLineOverrides {
                server_ip: options.host,
                level_name: options.world,
                server_port: options.port,
                online_mode: options.online_mode.map(|v| v.eq_ignore_ascii_case("true")),
                max_players: options.size,
                server_name: options.server_name,
            },
            ..StartupOptions::default()
        },
        velocity_secret_env: std::env::var("PAPER_VELOCITY_SECRET").ok(),
    };
    let mut config = match bootstrap::bootstrap(std::path::Path::new("."), bootstrap_options) {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("Failed to start the minecraft server: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let runtime_config = RuntimeConfig::from_bootstrap(&config, boot);
    // `WorldLoader.load` in Leaf's `Main`: the registries from the vanilla and Paper packs.
    let registries = match Registries::load(
        &mut config.vanilla_jar,
        &registry::default_report_path(std::path::Path::new(".")),
    ) {
        Ok(registries) => Arc::new(registries),
        Err(e) => {
            tracing::error!("Failed to load datapacks, can't proceed with server load: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let properties = config.properties.properties();
    let tables = WorldTables {
        blocks: Arc::new(config.blocks),
        block_data: Arc::new(config.block_data),
        registries: registries.clone(),
    };
    let settings = WorldSettings {
        level_name: &properties.level_name,
        compression: compression_from_property(&properties.region_file_compression),
        sync_chunk_writes: properties.sync_chunk_writes,
        new_world: (
            GameType::by_id(properties.game_mode as i32),
            Difficulty::by_name(properties.difficulty.name()).unwrap_or(Difficulty::Normal),
            properties.hardcore,
        ),
    };
    let _ = language::install(config.language);
    let open_world = || World::open(std::path::Path::new("."), &settings, &tables);
    let server = match Server::start(runtime_config, registries, open_world) {
        Ok(server) => server,
        // Paper: "Failed to bind to port" propagates out of `initServer`.
        Err(StartError::Bind(e)) => {
            tracing::error!(
                "Encountered an unexpected exception: java.lang.IllegalStateException: Failed to bind to port: {e}"
            );
            return std::process::ExitCode::FAILURE;
        }
        Err(StartError::World(e)) => {
            tracing::error!("Failed to load the world: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    server.listen_for_signals();
    if let Err(e) = runtime::spawn_console(server.control()) {
        tracing::error!("Could not start the console: {e}");
    }
    std::process::ExitCode::from(server.run())
}
