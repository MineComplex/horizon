//! `horizon-proxy [velocity.toml] [--network]`: runs the proxy from the directory of its config
//! (the forwarding secret and `plugins/` live beside it). The Network plugin is enabled when
//! `plugins/network/config.toml` exists; `--network` writes the default one first.
//!
//! The console takes `end`/`shutdown`, `glist`, `send <player|all> <server>` and the plugins'
//! commands (`maintenance`, `proxy`).
use horizon_proxy::{
    Proxy, ProxyHandle, config, launcher_token, log, network, players::Order, plugin::Plugin, text,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::io::AsyncBufReadExt;

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("Velocity worker")
        .build()
        .expect("the event loop starts");
    if let Err(e) = runtime.block_on(run()) {
        eprintln!("horizon-proxy: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let write_network = args.iter().any(|a| a == "--network");
    args.retain(|a| a != "--network");
    let path = PathBuf::from(
        args.first()
            .cloned()
            .unwrap_or_else(|| "velocity.toml".into()),
    );
    let config = config::Config::load(&path)?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(".".as_ref())
        .to_path_buf();
    let secret = config::forwarding_secret(&config, &dir)?;

    let network_path = dir.join("plugins/network/config.toml");
    if write_network && !network_path.exists() {
        let folder = network_path.parent().expect("a folder");
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        std::fs::write(&network_path, network::config::DEFAULT)
            .map_err(|e| format!("{}: {e}", network_path.display()))?;
    }
    let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
    if network_path.exists() {
        let token = launcher_token(&config)?.map(Arc::new);
        plugins.push(network::Network::new(network_path, token)?);
    } else if config.auth.mode == config::AuthMode::Offline {
        log("auth.mode is \"offline\": anyone can join with any name");
    } else {
        log(
            "The Network plugin is off (no plugins/network/config.toml): the account comes from the launcher token and the name from the client",
        );
    }

    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .map_err(|e| format!("bind {}: {e}", config.bind))?;
    let proxy = Proxy::new(config, secret, None, plugins)?;
    proxy.enable().await?;
    log(&format!(
        "Listening on {}",
        listener.local_addr().map_err(|e| e.to_string())?
    ));
    let handle = proxy.handle().clone();
    tokio::select! {
        () = proxy.serve(listener) => {}
        () = console(handle) => {
            log("Shutting down the proxy...");
            let reason = || Order::Kick(text::colored("Прокси выключается.", "red"));
            proxy.handle().players.order_all(reason);
            tokio::time::sleep(Duration::from_millis(500)).await;
            proxy.disable().await;
        }
    }
    Ok(())
}

/// The console until `end`.
async fn console(proxy: ProxyHandle) {
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        match line {
            "" => {}
            "end" | "shutdown" | "stop" => return,
            "help" => log(
                "Commands: end, glist [all], send <player|all> <server>, and the plugins' commands",
            ),
            _ => {
                if !proxy.console(line).await {
                    log("Unknown command. Type \"help\" for help.");
                }
            }
        }
    }
    // stdin closed (a service): keep running.
    std::future::pending::<()>().await;
}
