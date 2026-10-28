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
//! is used. Every other argument is vanilla's (`net.minecraft.server.Main`).
#![forbid(unsafe_code)]

use horizon_client::{crash, logging};
use std::path::PathBuf;

/// Split `--resources DIR` from the server's own arguments.
fn split_resources(args: Vec<String>) -> (Option<String>, Vec<String>) {
    let mut resources = None;
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--resources" | "-resources" => resources = it.next(),
            _ => match arg.strip_prefix("--resources=") {
                Some(dir) => resources = Some(dir.to_owned()),
                None => rest.push(arg),
            },
        }
    }
    let resources = resources.or_else(|| std::env::var("HORIZON_RESOURCES").ok());
    (resources.filter(|r| !r.is_empty()), rest)
}

/// The arguments the game takes for a dedicated server.
fn game_args(resources: Option<String>, server: Vec<String>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(dir) = resources {
        args.push("--resources".to_owned());
        args.push(dir);
    }
    args.push("--dedicated".to_owned());
    args.extend(server);
    args
}

fn main() {
    let (resources, server) = split_resources(std::env::args().skip(1).collect());
    let help = server.iter().any(|a| a == "--help" || a == "-help");
    let home = PathBuf::from(".");
    if !help {
        logging::init(&home, "Server thread");
        log::info!("Horizon server {}", crash::version());
        crash::install(home.clone(), "server");
    }
    if let Err(error) = horizon_client::game::run_with(game_args(resources, server)) {
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
    fn the_resources_are_taken_out_and_the_rest_go_to_the_server() {
        let (resources, rest) =
            split_resources(strings(&["--nogui", "--resources", "/r", "--port", "25566"]));
        assert_eq!(resources.as_deref(), Some("/r"));
        assert_eq!(rest, strings(&["--nogui", "--port", "25566"]));
        assert_eq!(
            game_args(resources, rest),
            strings(&["--resources", "/r", "--dedicated", "--nogui", "--port", "25566"])
        );
        let (resources, rest) = split_resources(strings(&["--resources=/x", "--initSettings"]));
        assert_eq!(resources.as_deref(), Some("/x"));
        assert_eq!(rest, strings(&["--initSettings"]));
    }
}
