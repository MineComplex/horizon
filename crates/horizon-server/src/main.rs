//! `horizon-server`: Horizon's Minecraft 26.3 server, run the way Paper and
//! Leaf are. Started in the server's folder, it reads `server.properties`,
//! `eula.txt` and `config/paper-global.yml` (Velocity forwarding) there,
//! keeps the worlds, `logs/` and `crash-reports/` beside them, and takes
//! commands on its console:
//!
//! ```sh
//! horizon-server [--resources DIR] [--nogui] [--port N] [--world NAME] [--universe DIR] [--initSettings]
//! ```
//!
//! `--resources` (or `HORIZON_RESOURCES`) names the prepared 26.3 data
//! (`horizon-prepare`); without it the `resources/` folder beside the program
//! is used. For tests: `--control-socket PATH` (what `horizon-ctl` and a
//! client's `--server-control` ask, game/server_control.rs), `--time`,
//! `--weather`, `--dimension`, `--test-terrain`, and the component fixtures
//! (`HORIZON_DISPENSE_FIXTURE`, `HORIZON_SCULK_FIXTURE`). Every other argument
//! is vanilla's (`net.minecraft.server.Main`).
#![forbid(unsafe_code)]

mod plugins;

use horizon_client::{crash, logging};
use std::path::PathBuf;

/// Horizon's own options, which go before vanilla's: they take a value,
/// except `--test-terrain`.
const HORIZON_OPTIONS: &[&str] = &[
    "--resources",
    "--control-socket",
    "--time",
    "--weather",
    "--dimension",
    "--render-distance",
];

/// Split Horizon's options (`--resources DIR`, `--control-socket PATH`...)
/// from vanilla's server arguments. A `--dedicated` is dropped: the server
/// always is.
fn split_options(args: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut horizon = Vec::new();
    let mut rest = Vec::new();
    let mut resources = false;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let name = if name == "-resources" {
            "--resources".to_owned()
        } else {
            name
        };
        if HORIZON_OPTIONS.contains(&name.as_str()) {
            resources |= name == "--resources";
            horizon.push(name);
            if let Some(value) = inline.or_else(|| it.next()) {
                horizon.push(value);
            }
        } else if name == "--test-terrain" {
            horizon.push(name);
        } else if name != "--dedicated" {
            rest.push(arg);
        }
    }
    if !resources
        && let Ok(dir) = std::env::var("HORIZON_RESOURCES")
        && !dir.is_empty()
    {
        horizon.push("--resources".to_owned());
        horizon.push(dir);
    }
    (horizon, rest)
}

/// The arguments the game takes: Horizon's, then the dedicated server's.
/// A component fixture (game/fixtures.rs) runs without `--dedicated`.
fn game_args(horizon: Vec<String>, server: Vec<String>, fixture: bool) -> Vec<String> {
    let mut args = horizon;
    if !fixture {
        args.push("--dedicated".to_owned());
    }
    args.extend(server);
    args
}

fn main() {
    let (horizon, server) = split_options(std::env::args().skip(1).collect());
    let help = server.iter().any(|a| a == "--help" || a == "-help");
    let fixture = ["HORIZON_DISPENSE_FIXTURE", "HORIZON_SCULK_FIXTURE"]
        .iter()
        .any(|v| std::env::var_os(v).is_some());
    let home = PathBuf::from(".");
    if !help {
        logging::init(&home, "Server thread");
        log::info!("Horizon server {}", crash::version());
        crash::install(home.clone(), "server");
    }
    horizon_client::game::set_plugins(plugins::installed());
    if let Err(error) = horizon_client::game::run_with(
        horizon_client::game::Program::Server,
        game_args(horizon, server, fixture),
    ) {
        log::error!("{error}");
        if !help {
            crash::report_error(&home, "server", &*error);
        }
        logging::flush();
        std::process::exit(1);
    }
    logging::flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn horizon_options_go_first_and_the_rest_to_the_server() {
        let (horizon, rest) = split_options(strings(&[
            "--nogui",
            "--resources",
            "/r",
            "--control-socket=/c.sock",
            "--test-terrain",
            "--dedicated",
            "--port",
            "25566",
        ]));
        assert_eq!(
            horizon,
            strings(&[
                "--resources",
                "/r",
                "--control-socket",
                "/c.sock",
                "--test-terrain"
            ])
        );
        assert_eq!(rest, strings(&["--nogui", "--port", "25566"]));
        assert_eq!(
            game_args(horizon.clone(), rest.clone(), false)[5..],
            strings(&["--dedicated", "--nogui", "--port", "25566"])
        );
        assert!(!game_args(horizon, rest, true).contains(&"--dedicated".to_owned()));
    }
}
