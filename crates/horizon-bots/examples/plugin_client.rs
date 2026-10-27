//! A scripted offline client for testing a server's plugins (docs/plugins.md):
//! it joins, runs its steps and prints what the server sends it, one line
//! each (`RECV <kind>: <text>`), so a test can read the outcome.
//!
//!   plugin_client --server 127.0.0.1:25565 --name Steve \
//!       --step "wait 500" --step "cmd spawn" --step "chat hello" --step "wait 1000"
//!
//! Steps: `wait MS`, `chat TEXT`, `cmd LINE` (no slash), `complete TEXT`
//! (a command suggestion request), `click SLOT [right]` (in the last opened
//! window), `close`, `dig X Y Z` (a creative break), `mine X Y Z` (start and
//! finish breaking), `use X Y Z FACE`
//! (right click a block face), `move X Y Z`, `slot N` (hotbar), `sneak on|off`,
//! `sprint on|off`, `drop`, `swap`, `respawn`.
use horizon_protocol::{
    PROTOCOL_VERSION,
    buf::{Reader, Writer},
    connection::Connection,
    crypto::offline_uuid,
    ids,
    packets::{
        Empty, configuration,
        handshake::{self, Intention},
        login::{self, ServerHello},
        play,
    },
};
use horizon_world::nbt::Tag;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A text component's plain text: text, translations as `[key args]`, extras.
fn plain(tag: &Tag) -> String {
    match tag {
        Tag::String(s) => s.to_string_lossy(),
        Tag::List(items) => items.iter().map(plain).collect(),
        Tag::Compound(_) => {
            let mut out = String::new();
            if let Some(t) = tag.get("text") {
                out.push_str(&plain(t));
            }
            if let Some(Tag::String(key)) = tag.get("translate") {
                let args: Vec<String> = match tag.get("with") {
                    Some(Tag::List(w)) => w.iter().map(plain).collect(),
                    _ => Vec::new(),
                };
                out.push_str(&format!("[{} {}]", key.to_string_lossy(), args.join(" ")));
            }
            if let Some(e) = tag.get("extra") {
                out.push_str(&plain(e));
            }
            out
        }
        Tag::Int(v) => v.to_string(),
        _ => String::new(),
    }
}

fn join(host: &str, port: u16, name: &str) -> Result<Connection, String> {
    let mut c = Connection::connect(host, port, Duration::from_secs(10)).map_err(err)?;
    c.send(
        &handshake::Serverbound::Intention(Intention {
            protocol: PROTOCOL_VERSION,
            host: host.into(),
            port,
            intent: handshake::LOGIN,
        })
        .encode(),
    )
    .map_err(err)?;
    c.send(
        &login::Serverbound::Hello(ServerHello {
            name: name.into(),
            uuid: offline_uuid(name),
        })
        .encode(),
    )
    .map_err(err)?;
    loop {
        let (id, body) = c.recv().map_err(err)?;
        match login::Clientbound::decode(id, &body).map_err(err)? {
            login::Clientbound::Compression(p) => c.set_compression(p.threshold),
            login::Clientbound::LoginFinished(_) => {
                c.send(&login::Serverbound::LoginAcknowledged(Empty).encode())
                    .map_err(err)?;
                break;
            }
            login::Clientbound::Disconnect(p) => return Err(format!("refused: {}", p.reason_json)),
            login::Clientbound::Hello(_) => return Err("online mode".into()),
            login::Clientbound::CustomQuery(p) => c
                .send(
                    &login::Serverbound::CustomQueryAnswer(login::CustomQueryAnswer {
                        transaction: p.transaction,
                        payload: None,
                    })
                    .encode(),
                )
                .map_err(err)?,
            _ => {}
        }
    }
    use ids::configuration::clientbound as cb;
    c.send(
        &configuration::Serverbound::ClientInformation(configuration::ClientInformation::new(4))
            .encode(),
    )
    .map_err(err)?;
    loop {
        let (id, body) = c.recv().map_err(err)?;
        if !matches!(
            id,
            cb::SELECT_KNOWN_PACKS
                | cb::KEEP_ALIVE
                | cb::PING
                | cb::CODE_OF_CONDUCT
                | cb::DISCONNECT
                | cb::FINISH_CONFIGURATION
        ) {
            continue;
        }
        let reply = match configuration::Clientbound::decode(id, &body).map_err(err)? {
            configuration::Clientbound::SelectKnownPacks(p) => {
                configuration::Serverbound::SelectKnownPacks(p)
            }
            configuration::Clientbound::KeepAlive(p) => configuration::Serverbound::KeepAlive(p),
            configuration::Clientbound::Ping(p) => configuration::Serverbound::Pong(p),
            configuration::Clientbound::CodeOfConduct(_) => {
                configuration::Serverbound::AcceptCodeOfConduct(Empty)
            }
            configuration::Clientbound::Disconnect(_) => {
                return Err("disconnected during configuration".into());
            }
            configuration::Clientbound::FinishConfiguration(_) => {
                c.send(&configuration::Serverbound::FinishConfiguration(Empty).encode())
                    .map_err(err)?;
                return Ok(c);
            }
            _ => continue,
        };
        c.send(&reply.encode()).map_err(err)?;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let server = flag("--server").unwrap_or_else(|| "127.0.0.1:25565".into());
    let name = flag("--name").unwrap_or_else(|| "Steve".into());
    let steps: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| *i > 0 && args[i - 1] == "--step" && !a.is_empty())
        .map(|(_, a)| a.clone())
        .collect();
    let (host, port) = server.rsplit_once(':').expect("HOST:PORT");
    let port: u16 = port.parse().expect("a port");
    let conn = match join(host, port, &name) {
        Ok(c) => c,
        Err(e) => {
            println!("ERROR join: {e}");
            std::process::exit(1);
        }
    };
    println!("JOINED {name}");
    let (mut reader, writer) = conn.split();
    let writer = Arc::new(Mutex::new(writer));
    let window = Arc::new(AtomicI32::new(0));
    let state = Arc::new(AtomicI32::new(0));
    let sequence = Arc::new(AtomicI32::new(1));
    let position = Arc::new(Mutex::new([0.0f64; 3]));
    let last = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    {
        let writer = writer.clone();
        let window = window.clone();
        let state = state.clone();
        let position = position.clone();
        let last = last.clone();
        std::thread::spawn(move || {
            use ids::play::clientbound as cb;
            let send = |body: Vec<u8>| {
                let _ = writer.lock().unwrap().send(&body);
            };
            let mut loaded = false;
            loop {
                let Ok((id, body)) = reader.recv() else {
                    println!("CLOSED");
                    std::process::exit(0);
                };
                last.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                let mut r = Reader::new(&body);
                match id {
                    cb::KEEP_ALIVE => {
                        if let Ok(v) = r.i64() {
                            let mut w = Writer::new();
                            w.varint(ids::play::serverbound::KEEP_ALIVE);
                            w.i64(v);
                            send(w.data);
                        }
                        continue;
                    }
                    cb::PING => {
                        if let Ok(v) = r.i32() {
                            let mut w = Writer::new();
                            w.varint(ids::play::serverbound::PONG);
                            w.i32(v);
                            send(w.data);
                        }
                        continue;
                    }
                    cb::COMMAND_SUGGESTIONS => {
                        let (_, _start, _len) = (r.varint(), r.varint(), r.varint());
                        let n = r.varint().unwrap_or(0);
                        let mut words = Vec::new();
                        for _ in 0..n {
                            words.push(r.string(32767).unwrap_or_default());
                            if r.bool().unwrap_or(false) {
                                let _ = r.nbt();
                            }
                        }
                        println!("RECV suggestions: {}", words.join(","));
                        continue;
                    }
                    _ => {}
                }
                let Ok(packet) = play::Clientbound::decode(id, &body) else {
                    continue;
                };
                match packet {
                    play::Clientbound::SystemChat(p) if p.overlay => {
                        println!("RECV actionbar: {}", plain(&p.content));
                    }
                    play::Clientbound::SystemChat(p) => {
                        println!("RECV system: {}", plain(&p.content))
                    }
                    play::Clientbound::PlayerChat(p) => println!("RECV chat: {}", p.content),
                    play::Clientbound::SetTitleText(p) => {
                        println!("RECV title: {}", plain(&p.text))
                    }
                    play::Clientbound::SetSubtitleText(p) => {
                        println!("RECV subtitle: {}", plain(&p.text));
                    }
                    play::Clientbound::OpenScreen(p) => {
                        window.store(p.container, Ordering::Relaxed);
                        println!("RECV open: {} {}", p.menu, plain(&p.title));
                    }
                    play::Clientbound::ContainerSetContent(p) if p.container != 0 => {
                        state.store(p.state, Ordering::Relaxed);
                        let filled = p.items.iter().filter(|s| s.is_some()).count();
                        let with = |id: i32| {
                            p.items
                                .iter()
                                .flatten()
                                .filter(|s| s.added.iter().any(|(k, _)| *k == id))
                                .count()
                        };
                        use horizon_protocol::item::component as c;
                        println!(
                            "RECV content: window {} slots {} filled {filled} named {} lore {} glint {}",
                            p.container,
                            p.items.len(),
                            with(c::ITEM_NAME),
                            with(c::LORE),
                            with(c::ENCHANTMENT_GLINT_OVERRIDE)
                        );
                    }
                    play::Clientbound::ContainerSetContent(p) => {
                        state.store(p.state, Ordering::Relaxed);
                    }
                    play::Clientbound::ContainerClose(p) => {
                        println!("RECV closed: {}", p.container)
                    }
                    play::Clientbound::BossEvent(p) => match p.action {
                        play::BossAction::Add { name, .. } => {
                            println!("RECV bossbar add: {}", plain(&name))
                        }
                        play::BossAction::Remove => println!("RECV bossbar remove"),
                        _ => {}
                    },
                    play::Clientbound::SetObjective(p) => {
                        let title = p.display.as_ref().map(|d| plain(&d.0)).unwrap_or_default();
                        println!("RECV objective: {} {} {title}", p.name, p.method);
                    }
                    play::Clientbound::SetScore(p) => {
                        let line = p.display.as_ref().map(plain).unwrap_or_default();
                        println!("RECV score: {} {line}", p.score);
                    }
                    play::Clientbound::BlockUpdate(p) => {
                        println!(
                            "RECV block: {} {} {} {}",
                            p.pos[0], p.pos[1], p.pos[2], p.state
                        );
                    }
                    play::Clientbound::PlayerPosition(p) => {
                        let mut at = position.lock().unwrap();
                        let pos: [f64; 3] = std::array::from_fn(|i| {
                            if p.relatives & (1 << i) != 0 {
                                at[i] + p.position[i]
                            } else {
                                p.position[i]
                            }
                        });
                        *at = pos;
                        println!("RECV teleport: {:.2} {:.2} {:.2}", pos[0], pos[1], pos[2]);
                        send(
                            play::Serverbound::AcceptTeleportation(play::AcceptTeleportation {
                                id: p.teleport,
                                x: pos[0],
                                y: pos[1],
                                z: pos[2],
                                yaw: p.yaw,
                                pitch: p.pitch,
                            })
                            .encode(),
                        );
                        // The level has loaded around the first position, as
                        // vanilla's client says once its loading screen goes.
                        if !loaded {
                            loaded = true;
                            send(play::Serverbound::PlayerLoaded(Empty).encode());
                        }
                    }
                    play::Clientbound::Commands(p) => {
                        let root = &p.nodes[p.root as usize];
                        let mut names: Vec<&str> = root
                            .children
                            .iter()
                            .map(|&c| p.nodes[c as usize].name.as_str())
                            .collect();
                        names.sort();
                        println!("RECV commands: {}", names.join(","));
                    }
                    play::Clientbound::SetHealth(p) => {
                        println!("RECV health: {:.1} {}", p.health, p.food)
                    }
                    play::Clientbound::GameEvent(p) if p.event == 3 => {
                        println!("RECV gamemode: {}", p.param);
                    }
                    play::Clientbound::PlayerAbilities(p) => {
                        println!("RECV abilities: {}", p.flags)
                    }
                    play::Clientbound::TabList(p) => {
                        println!("RECV tablist: {} | {}", plain(&p.header), plain(&p.footer));
                    }
                    play::Clientbound::CustomPayload(p) => {
                        println!(
                            "RECV payload: {} {}",
                            p.channel,
                            String::from_utf8_lossy(&p.data)
                        );
                    }
                    play::Clientbound::Disconnect(p) => {
                        println!("RECV disconnect: {}", plain(&p.reason));
                    }
                    play::Clientbound::ChunkBatchFinished(_) => send(
                        play::Serverbound::ChunkBatchReceived(play::ChunkBatchReceived {
                            chunks_per_tick: 9.0,
                        })
                        .encode(),
                    ),
                    _ => {}
                }
            }
        });
    }
    let send = |body: Vec<u8>| {
        let _ = writer.lock().unwrap().send(&body);
    };
    for step in steps {
        let (verb, rest) = step.split_once(' ').unwrap_or((step.as_str(), ""));
        let words: Vec<&str> = rest.split_whitespace().collect();
        let at = *position.lock().unwrap();
        // `~` is relative to where the server last put the client.
        let num = |i: usize| {
            let w = words.get(i).copied().unwrap_or("0");
            match w.strip_prefix('~') {
                Some(rest) if i < 3 => at[i].floor() + rest.parse::<f64>().unwrap_or(0.0),
                _ => w.parse::<f64>().unwrap_or(0.0),
            }
        };
        println!("STEP {step}");
        match verb {
            "wait" => std::thread::sleep(Duration::from_millis(num(0) as u64)),
            "chat" => {
                let millis = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as i64);
                send(
                    play::Serverbound::Chat(play::Chat {
                        message: rest.into(),
                        timestamp: millis,
                        salt: 0,
                        signature: None,
                        last_seen: play::LastSeenUpdate {
                            offset: 0,
                            acknowledged: [0; 3],
                            checksum: 0,
                        },
                    })
                    .encode(),
                );
            }
            "cmd" => send(
                play::Serverbound::ChatCommand(play::ChatCommand {
                    command: rest.into(),
                })
                .encode(),
            ),
            "complete" => {
                let mut w = Writer::new();
                w.varint(ids::play::serverbound::COMMAND_SUGGESTION);
                w.varint(1);
                w.string(&format!("/{rest}"));
                send(w.data);
            }
            "click" => send(
                play::Serverbound::ContainerClick(play::ContainerClick {
                    container: window.load(Ordering::Relaxed),
                    state: state.load(Ordering::Relaxed),
                    slot: num(0) as i16,
                    button: i8::from(words.get(1) == Some(&"right")),
                    input: 0,
                    changed: Vec::new(),
                    carried: None,
                })
                .encode(),
            ),
            "close" => send(
                play::Serverbound::ContainerClose(play::ContainerClose {
                    container: window.load(Ordering::Relaxed),
                })
                .encode(),
            ),
            // `dig`: START_DESTROY_BLOCK alone, as a creative client breaks;
            // `mine`: then STOP_DESTROY_BLOCK (26.3's ids), as a survival one.
            "dig" | "mine" => {
                let pos = [num(0) as i32, num(1) as i32, num(2) as i32];
                let actions: &[i32] = if verb == "dig" { &[0] } else { &[0, 3] };
                for &action in actions {
                    let seq = sequence.fetch_add(1, Ordering::Relaxed);
                    send(
                        play::Serverbound::PlayerAction(play::PlayerAction {
                            action,
                            pos,
                            direction: 1,
                            sequence: seq,
                        })
                        .encode(),
                    );
                }
            }
            "use" => {
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                send(
                    play::Serverbound::UseItemOn(play::UseItemOn {
                        hand: 0,
                        pos: [num(0) as i32, num(1) as i32, num(2) as i32],
                        face: num(3) as i32,
                        cursor: [0.5, 1.0, 0.5],
                        inside: false,
                        world_border: false,
                        sequence: seq,
                    })
                    .encode(),
                );
            }
            // USE_ITEM with the main hand, looking at `yaw pitch`: eat, drink,
            // throw, a bucket.
            "use-item" => {
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                send(
                    play::Serverbound::UseItem(play::UseItem {
                        hand: 0,
                        sequence: seq,
                        yaw: num(0) as f32,
                        pitch: num(1) as f32,
                    })
                    .encode(),
                );
            }
            "move" => {
                *position.lock().unwrap() = [num(0), num(1), num(2)];
                send(
                    play::Serverbound::MovePlayerPos(play::MovePlayerPos {
                        x: num(0),
                        y: num(1),
                        z: num(2),
                        flags: 1,
                    })
                    .encode(),
                );
                send(play::Serverbound::ClientTickEnd(Empty).encode());
            }
            "slot" => send(
                play::Serverbound::SetCarriedItem(play::SetCarriedItem {
                    slot: num(0) as i16,
                })
                .encode(),
            ),
            // PlayerInput's shift bit (32).
            "sneak" => send(
                play::Serverbound::PlayerInput(play::PlayerInput {
                    flags: if rest == "on" { 32 } else { 0 },
                })
                .encode(),
            ),
            // START_SPRINTING (1) and STOP_SPRINTING (2).
            "sprint" => send(
                play::Serverbound::PlayerCommand(play::PlayerCommand {
                    id: 0,
                    action: if rest == "on" { 1 } else { 2 },
                    data: 0,
                })
                .encode(),
            ),
            // DROP_ITEM (5) and SWAP_ITEM_WITH_OFFHAND (7).
            "drop" | "swap" => {
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                send(
                    play::Serverbound::PlayerAction(play::PlayerAction {
                        action: if verb == "drop" { 5 } else { 7 },
                        pos: [0, 0, 0],
                        direction: 0,
                        sequence: seq,
                    })
                    .encode(),
                );
            }
            // PERFORM_RESPAWN.
            "respawn" => {
                send(play::Serverbound::ClientCommand(play::ClientCommand { action: 0 }).encode())
            }
            other => println!("ERROR unknown step {other}"),
        }
    }
    // Let the last answers arrive.
    std::thread::sleep(Duration::from_millis(500));
    println!("DONE");
}
