//! `horizon-server`: Leaf 26.3's startup, as far as it is ported (ARCHITECTURE.md). It loads the settings
//! and the language from the working directory, reports them and exits: the levels, the tick loop and
//! the network are not ported yet.
use std::path::PathBuf;

use clap::Parser;
use horizon_server::{
    azalea::language,
    server::{
        bootstrap::{self, BootstrapOptions, ServerPaths},
        server_properties::{CommandLineOverrides, StartupOptions},
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
    let config = match bootstrap::bootstrap(std::path::Path::new("."), bootstrap_options) {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("Failed to start the minecraft server: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let properties = config.properties.properties();
    let yaml = &config.yaml;
    let tick_loop = yaml.tick_loop();
    println!(
        "horizon-server: startup settings loaded from {}",
        config.vanilla_jar.path().display()
    );
    println!(
        "  server.properties: level-name={} server-port={} online-mode={} max-players={}",
        properties.level_name,
        properties.server_port,
        properties.online_mode,
        properties.max_players
    );
    println!(
        "  velocity: enabled={} online-mode={}; tab-complete={} send-namespaced={}",
        yaml.paper.velocity.enabled,
        yaml.paper.velocity.online_mode,
        yaml.spigot.tab_complete,
        yaml.spigot.send_namespaced
    );
    println!(
        "  tick loop: catch-up-ticks={} lagging-threshold={} including-5s-in-get-tps={}",
        tick_loop.catchup_ticks, tick_loop.lagging_threshold, yaml.leaf.including_5s_in_get_tps
    );
    println!("  language: {} translations", config.language.len());
    let _ = language::install(config.language);
    println!(
        "horizon-server: the levels, the tick loop and the network are not ported yet; exiting."
    );
    std::process::ExitCode::SUCCESS
}
