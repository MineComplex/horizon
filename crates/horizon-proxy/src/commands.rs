//! Velocity's built-in commands: `/server`, `/glist`, `/send` (ServerCommand, GlistCommand,
//! SendCommand), with their permissions.
use crate::{
    ProxyHandle,
    players::Order,
    plugin::{Command, Source},
    text,
};
use horizon_world::nbt::Tag;
use std::sync::Arc;

/// ServerCommand.MAX_SERVERS_TO_LIST.
const MAX_SERVERS_TO_LIST: usize = 50;

pub fn builtin() -> Vec<Command> {
    vec![
        Command {
            name: "server".into(),
            aliases: Vec::new(),
            // `velocity.command.server` unless set to false.
            permission: Some("!velocity.command.server".into()),
            run: Arc::new(|proxy, source, args| Box::pin(server(proxy, source, args))),
            suggest: Some(Arc::new(|proxy, _, args| match args.len() {
                1 => proxy.config.servers.addresses.keys().cloned().collect(),
                _ => Vec::new(),
            })),
        },
        Command {
            name: "glist".into(),
            aliases: Vec::new(),
            permission: Some("velocity.command.glist".into()),
            run: Arc::new(|proxy, source, args| Box::pin(glist(proxy, source, args))),
            suggest: Some(Arc::new(|proxy, _, args| match args.len() {
                1 => std::iter::once("all".to_owned())
                    .chain(proxy.config.servers.addresses.keys().cloned())
                    .collect(),
                _ => Vec::new(),
            })),
        },
        Command {
            name: "send".into(),
            aliases: Vec::new(),
            permission: Some("velocity.command.send".into()),
            run: Arc::new(|proxy, source, args| Box::pin(send(proxy, source, args))),
            suggest: Some(Arc::new(|proxy, _, args| match args.len() {
                1 => ["all", "current"]
                    .into_iter()
                    .map(str::to_owned)
                    .chain(proxy.players.all().into_iter().map(|p| p.info.name))
                    .collect(),
                2 => proxy.config.servers.addresses.keys().cloned().collect(),
                _ => Vec::new(),
            })),
        },
    ]
}

fn yellow(text: &str) -> Tag {
    text::colored(text, "yellow")
}

async fn server(proxy: ProxyHandle, source: Source, args: Vec<String>) {
    let Source::Player(player) = &source else {
        return source.message(text::colored("Only players may run this command.", "red"));
    };
    if let Some(name) = args.first() {
        return player.connect(name);
    }
    // outputServerInformation.
    let current = player.server.clone().unwrap_or_else(|| "<unknown>".into());
    source.message(yellow(&format!(
        "На данный момент вы подключены к серверу {current}."
    )));
    let servers: Vec<&String> = proxy.config.servers.addresses.keys().collect();
    if servers.len() > MAX_SERVERS_TO_LIST {
        return source.message(text::colored(
            "Настроено слишком много серверов. Для просмотра всех доступных серверов, используйте автозаполнение клавишей Tab.",
            "red",
        ));
    }
    let mut parts = vec![yellow("Доступные серверы: ")];
    for (i, server) in servers.iter().enumerate() {
        if i > 0 {
            parts.push(text::colored(", ", "gray"));
        }
        let color = if **server == current { "green" } else { "gray" };
        parts.push(text::colored(server, color));
    }
    source.message(text::join(parts));
}

async fn glist(proxy: ProxyHandle, source: Source, args: Vec<String>) {
    let by_server = proxy.players.by_server();
    let line = |server: &str, names: &[String]| {
        text::join(vec![
            text::colored(&format!("[{server}] "), "dark_aqua"),
            text::colored(&format!("({})", names.len()), "gray"),
            text::colored(&format!(": {}", names.join(", ")), "white"),
        ])
    };
    match args.first().map(String::as_str) {
        None => {}
        Some(all) if all.eq_ignore_ascii_case("all") => {
            for (server, names) in &by_server {
                source.message(line(server, names));
            }
        }
        Some(server) => {
            let Some(server) = proxy
                .config
                .servers
                .addresses
                .keys()
                .find(|s| s.eq_ignore_ascii_case(server))
            else {
                let text = format!("Указанный сервер {server} не существует.");
                return source.message(text::colored(&text, "red"));
            };
            let names = by_server.get(server).cloned().unwrap_or_default();
            return source.message(line(server, &names));
        }
    }
    let count = proxy.players.count();
    source.message(yellow(&match count {
        1 => "1 игрок подключен к прокси на данный момент.".to_owned(),
        n => format!("{n} игрок(а, ов) подключены к прокси на данный момент."),
    }));
    if args.is_empty() {
        source.message(text::colored(
            "Чтобы просмотреть всех игроков на серверах, используйте /glist all.",
            "gray",
        ));
    }
}

/// SendCommand: `/send <player|all|current> <server>`.
async fn send(proxy: ProxyHandle, source: Source, args: Vec<String>) {
    let [who, server] = args.as_slice() else {
        return source.message(text::colored("/send <никнейм> <сервер>", "red"));
    };
    let Some(server) = proxy
        .config
        .servers
        .addresses
        .keys()
        .find(|s| s.eq_ignore_ascii_case(server))
        .cloned()
    else {
        let text = format!("Указанный сервер {server} не существует.");
        return source.message(text::colored(&text, "red"));
    };
    let targets = match who.to_lowercase().as_str() {
        "all" => proxy.players.all(),
        "current" => {
            let Source::Player(player) = &source else {
                return source.message(text::colored("Only players may use current.", "red"));
            };
            let here = player.server.clone();
            proxy
                .players
                .all()
                .into_iter()
                .filter(|p| p.server.is_some() && p.server == here)
                .collect()
        }
        _ => match proxy.players.find(who) {
            Some(p) => vec![p],
            None => {
                let text = format!("Игрок {who} не найден.");
                return source.message(text::colored(&text, "red"));
            }
        },
    };
    for p in &targets {
        p.order(Order::Send(server.clone()));
    }
    source.message(text::colored(
        &format!("Отправлено игроков на {server}: {}", targets.len()),
        "green",
    ));
}
