//! The proxy between a scripted client and two scripted backends: the encrypted login,
//! modern forwarding, `/server`, a kick that falls back to the lobby, the status ping, a plugin's
//! permissions, command and ping, the command tree, Tab completion and the PROXY protocol.
use horizon_protocol::{
    PROTOCOL_VERSION,
    buf::{Codec, Reader, Writer},
    connection::Connection,
    ids::{configuration as cfg, login as lg, play},
    packets::{handshake, login, play::Commands, status},
    rsa::RsaPublicKey,
    text::literal,
};
use horizon_proxy::{
    Proxy, ProxyHandle,
    auth::OfflineAuth,
    config::Config,
    forwarding,
    players::PlayerInfo,
    plugin::{Command, Fut, Ping, Plugin, Tristate},
    text,
};
use std::{io::Write, net::TcpListener, sync::Arc, thread, time::Duration};

const SECRET: &[u8] = b"test-secret";
const BRAND: &[u8] = b"\x0fminecraft:brand\x07vanilla";
const INFORMATION: &[u8] = b"client-information";

fn body(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut w = Writer::new();
    w.varint(id);
    w.bytes(payload);
    w.data
}

fn expect(c: &mut Connection, id: i32) -> Vec<u8> {
    let (got, payload) = c.recv().unwrap();
    assert_eq!(got, id, "unexpected packet {got} (wanted {id})");
    payload
}

fn chat(text: &str) -> Vec<u8> {
    let mut w = Writer::new();
    w.nbt(&literal(text));
    w.bool(false);
    w.data
}

/// A backend with modern forwarding: logs the player in, configures them, greets them in chat
/// and kicks them on `/kickme`.
fn backend(name: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            thread::spawn(move || {
                let mut c = Connection::from_stream(stream).unwrap();
                let (id, payload) = c.recv().unwrap();
                let handshake::Serverbound::Intention(intent) =
                    handshake::Serverbound::decode(id, &payload).unwrap()
                else {
                    panic!("no handshake");
                };
                assert_eq!(intent.intent, handshake::LOGIN);
                expect(&mut c, lg::serverbound::HELLO);
                c.send(
                    &login::Clientbound::Compression(login::Compression { threshold: 64 }).encode(),
                )
                .unwrap();
                c.set_compression(64);
                c.send(
                    &login::Clientbound::CustomQuery(login::CustomQuery {
                        transaction: 7,
                        channel: forwarding::CHANNEL.into(),
                        data: vec![4],
                    })
                    .encode(),
                )
                .unwrap();
                let answer = expect(&mut c, lg::serverbound::CUSTOM_QUERY_ANSWER);
                let login::Serverbound::CustomQueryAnswer(answer) =
                    login::Serverbound::decode(lg::serverbound::CUSTOM_QUERY_ANSWER, &answer)
                        .unwrap()
                else {
                    unreachable!()
                };
                assert_eq!(answer.transaction, 7);
                let data = forwarding::check_integrity(SECRET, answer.payload.as_deref().unwrap())
                    .expect("signed with the shared secret")
                    .to_vec();
                let mut r = Reader::new(&data);
                assert_eq!(r.varint().unwrap(), forwarding::MODERN_LAZY_SESSION);
                assert_eq!(r.string(255).unwrap(), "127.0.0.1");
                let uuid = r.uuid().unwrap();
                let player = r.string(16).unwrap();
                assert_eq!(uuid, horizon_protocol::crypto::offline_uuid(&player));
                c.send(
                    &login::Clientbound::LoginFinished(login::LoginFinished {
                        profile: login::GameProfile {
                            uuid,
                            name: player.clone(),
                            properties: vec![],
                        },
                        session: uuid,
                    })
                    .encode(),
                )
                .unwrap();
                expect(&mut c, lg::serverbound::LOGIN_ACKNOWLEDGED);
                // The client's brand and information, replayed by the proxy on every server.
                assert_eq!(expect(&mut c, cfg::serverbound::CUSTOM_PAYLOAD), BRAND);
                assert_eq!(
                    expect(&mut c, cfg::serverbound::CLIENT_INFORMATION),
                    INFORMATION
                );
                c.send(&body(cfg::clientbound::FINISH_CONFIGURATION, &[]))
                    .unwrap();
                expect(&mut c, cfg::serverbound::FINISH_CONFIGURATION);
                c.send(&body(play::clientbound::LOGIN, name.as_bytes()))
                    .unwrap();
                // A tree with the server's own `server` command, which the proxy's replaces.
                let tree = Commands {
                    nodes: vec![
                        command_node(0, "", vec![1]),
                        command_node(1, "server", vec![]),
                    ],
                    root: 0,
                };
                let mut w = Writer::new();
                Commands::write(&tree, &mut w);
                c.send(&body(play::clientbound::COMMANDS, &w.data)).unwrap();
                c.send(&body(
                    play::clientbound::SYSTEM_CHAT,
                    &chat(&format!("welcome to {name}")),
                ))
                .unwrap();
                while let Ok((id, payload)) = c.recv() {
                    if id == play::serverbound::CHAT_COMMAND
                        && Reader::new(&payload).string(256).unwrap() == "kickme"
                    {
                        let mut w = Writer::new();
                        w.nbt(&literal("bye"));
                        c.send(&body(play::clientbound::DISCONNECT, &w.data))
                            .unwrap();
                    }
                }
            });
        }
    });
    port
}

fn command_node(
    kind: u8,
    name: &str,
    children: Vec<i32>,
) -> horizon_protocol::packets::play::CommandNode {
    horizon_protocol::packets::play::CommandNode {
        kind,
        executable: kind != 0,
        restricted: false,
        children,
        redirect: None,
        name: name.into(),
        parser: None,
        suggestions: None,
    }
}

/// A plugin: every permission but `/send`, a `hello` command and a ping of its own.
struct TestPlugin;
impl Plugin for TestPlugin {
    fn name(&self) -> &str {
        "test"
    }
    fn enable(self: Arc<Self>, proxy: ProxyHandle) -> Fut<'static, Result<(), String>> {
        proxy.register_command(Command {
            name: "hello".into(),
            aliases: vec!["hi".into()],
            permission: Some("test.hello".into()),
            run: Arc::new(|_, source, args| {
                Box::pin(async move {
                    let text = format!("hi {} {}", source.name(), args.join(" "));
                    source.message(text::colored(&text, "green"));
                })
            }),
            suggest: None,
        });
        Box::pin(async { Ok(()) })
    }
    fn permission(&self, _: &PlayerInfo, permission: &str) -> Tristate {
        if permission == "velocity.command.send" {
            Tristate::False
        } else {
            Tristate::True
        }
    }
    fn ping(&self, ping: &mut Ping) {
        ping.version_name = "Test".into();
    }
}

fn proxy(lobby: u16, pvp: u16) -> u16 {
    proxy_with(lobby, pvp, false)
}

fn proxy_with(lobby: u16, pvp: u16, haproxy: bool) -> u16 {
    let config = Config::parse(&format!(
        r#"
bind = "127.0.0.1:0"
motd = "<#09add3>Horizon"
show-max-players = 10
forwarding-secret-file = "unused"
[servers]
lobby = "127.0.0.1:{lobby}"
pvp = "127.0.0.1:{pvp}"
try = ["lobby", "pvp"]
[advanced]
login-ratelimit = 0
command-rate-limit = 0
tab-complete-rate-limit = 0
haproxy-protocol = {haproxy}
[auth]
mode = "offline"
"#
    ))
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let plugins: Vec<Arc<dyn Plugin>> = vec![Arc::new(TestPlugin)];
            let proxy = Proxy::new(
                config,
                SECRET.to_vec(),
                Some(Arc::new(OfflineAuth)),
                plugins,
            )
            .unwrap();
            proxy.enable().await.unwrap();
            proxy.serve(listener).await;
        });
    });
    port
}

fn client(port: u16, intent: i32) -> Connection {
    let mut c = Connection::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
    c.reader
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    c.send(
        &handshake::Serverbound::Intention(handshake::Intention {
            protocol: PROTOCOL_VERSION,
            host: "localhost".into(),
            port,
            intent,
        })
        .encode(),
    )
    .unwrap();
    c
}

/// The next chat message's text; command trees on the way are checked: the proxy's commands
/// the player may run are there, `/send` (denied) is not, and the server's `server` is replaced.
fn chat_text(c: &mut Connection) -> String {
    loop {
        let (id, payload) = c.recv().unwrap();
        if id == play::clientbound::COMMANDS {
            let tree = Commands::read(&mut Reader::new(&payload)).unwrap();
            let names: Vec<&str> = tree.nodes[tree.root as usize]
                .children
                .iter()
                .map(|i| tree.nodes[*i as usize].name.as_str())
                .collect();
            for name in ["server", "glist", "hello", "hi"] {
                assert_eq!(names.iter().filter(|n| **n == name).count(), 1, "{names:?}");
            }
            assert!(!names.contains(&"send"), "{names:?}");
            continue;
        }
        assert_eq!(id, play::clientbound::SYSTEM_CHAT, "unexpected packet {id}");
        return text::plain(&Reader::new(&payload).nbt().unwrap());
    }
}

/// The client's side of joining a server through configuration.
fn configure(c: &mut Connection, server: &str) {
    expect(c, cfg::clientbound::FINISH_CONFIGURATION);
    c.send(&body(cfg::serverbound::FINISH_CONFIGURATION, &[]))
        .unwrap();
    assert_eq!(expect(c, play::clientbound::LOGIN), server.as_bytes());
}

fn command(c: &mut Connection, line: &str) {
    let mut w = Writer::new();
    w.string(line);
    c.send(&body(play::serverbound::CHAT_COMMAND, &w.data))
        .unwrap();
}

#[test]
fn login_switch_fallback_and_ping() {
    let port = proxy(backend("lobby"), backend("pvp"));

    // Login: encryption, the token query, compression, then configuration.
    let mut c = client(port, handshake::LOGIN);
    c.send(
        &login::Serverbound::Hello(login::ServerHello {
            name: "Steve".into(),
            uuid: 0,
        })
        .encode(),
    )
    .unwrap();
    let hello = expect(&mut c, lg::clientbound::HELLO);
    let login::Clientbound::Hello(hello) =
        login::Clientbound::decode(lg::clientbound::HELLO, &hello).unwrap()
    else {
        unreachable!()
    };
    assert!(!hello.should_authenticate);
    let key = RsaPublicKey::from_der(&hello.public_key).unwrap();
    let secret = [5u8; 16];
    c.send(
        &login::Serverbound::Key(login::Key {
            key: key.encrypt(&secret).unwrap(),
            challenge: key.encrypt(&hello.challenge).unwrap(),
        })
        .encode(),
    )
    .unwrap();
    c.enable_encryption(&secret);
    let query = expect(&mut c, lg::clientbound::CUSTOM_QUERY);
    let login::Clientbound::CustomQuery(query) =
        login::Clientbound::decode(lg::clientbound::CUSTOM_QUERY, &query).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(query.channel, "minecomplex:auth");
    c.send(
        &login::Serverbound::CustomQueryAnswer(login::CustomQueryAnswer {
            transaction: query.transaction,
            payload: None,
        })
        .encode(),
    )
    .unwrap();
    let threshold = expect(&mut c, lg::clientbound::LOGIN_COMPRESSION);
    c.set_compression(Reader::new(&threshold).varint().unwrap());
    expect(&mut c, lg::clientbound::LOGIN_FINISHED);
    c.send(&body(lg::serverbound::LOGIN_ACKNOWLEDGED, &[]))
        .unwrap();
    c.send(&body(cfg::serverbound::CUSTOM_PAYLOAD, BRAND))
        .unwrap();
    c.send(&body(cfg::serverbound::CLIENT_INFORMATION, INFORMATION))
        .unwrap();
    configure(&mut c, "lobby");
    assert_eq!(chat_text(&mut c), "welcome to lobby");

    // The server list sees the player.
    let mut ping = client(port, handshake::STATUS);
    ping.send(&body(0, &[])).unwrap();
    let json = expect(&mut ping, 0);
    let status: serde_json::Value =
        serde_json::from_str(&Reader::new(&json).string(32767).unwrap()).unwrap();
    assert_eq!(status["players"]["online"], 1);
    assert_eq!(status["version"]["name"], "Test");
    assert_eq!(status["description"]["text"], "Horizon");
    ping.send(&status::Serverbound::PingRequest(status::Ping { time: 9 }).encode())
        .unwrap();
    assert_eq!(expect(&mut ping, 1), 9i64.to_be_bytes());

    // `/server pvp`: back to configuration, then the new server.
    command(&mut c, "server nope");
    assert!(chat_text(&mut c).contains("nope"));
    command(&mut c, "hi there");
    assert_eq!(chat_text(&mut c), "hi Steve there");
    // `/send` is denied to this player: the server gets it.
    let mut w = Writer::new();
    w.varint(3);
    w.string("/server p");
    c.send(&body(play::serverbound::COMMAND_SUGGESTION, &w.data))
        .unwrap();
    let answer = expect(&mut c, play::clientbound::COMMAND_SUGGESTIONS);
    let mut r = Reader::new(&answer);
    assert_eq!(
        (
            r.varint().unwrap(),
            r.varint().unwrap(),
            r.varint().unwrap()
        ),
        (3, 8, 1)
    );
    assert_eq!(r.varint().unwrap(), 1);
    assert_eq!(r.string(64).unwrap(), "pvp");
    command(&mut c, "server pvp");
    expect(&mut c, play::clientbound::START_CONFIGURATION);
    c.send(&body(play::serverbound::CONFIGURATION_ACKNOWLEDGED, &[]))
        .unwrap();
    configure(&mut c, "pvp");
    assert_eq!(chat_text(&mut c), "welcome to pvp");
    command(&mut c, "glist all");
    assert_eq!(chat_text(&mut c), "[pvp] (1): Steve");
    assert!(chat_text(&mut c).starts_with("1 игрок"));

    // Kicked from pvp: the proxy falls back to the lobby and says why.
    command(&mut c, "kickme");
    expect(&mut c, play::clientbound::START_CONFIGURATION);
    c.send(&body(play::serverbound::CONFIGURATION_ACKNOWLEDGED, &[]))
        .unwrap();
    configure(&mut c, "lobby");
    assert_eq!(chat_text(&mut c), "Вы были кикнуты с сервера pvp: bye");
    assert_eq!(chat_text(&mut c), "welcome to lobby");

    // A second login with the same name is refused.
    let mut twin = client(port, handshake::LOGIN);
    twin.send(
        &login::Serverbound::Hello(login::ServerHello {
            name: "steve".into(),
            uuid: 0,
        })
        .encode(),
    )
    .unwrap();
    let hello = expect(&mut twin, lg::clientbound::HELLO);
    let login::Clientbound::Hello(hello) =
        login::Clientbound::decode(lg::clientbound::HELLO, &hello).unwrap()
    else {
        unreachable!()
    };
    let key = RsaPublicKey::from_der(&hello.public_key).unwrap();
    twin.send(
        &login::Serverbound::Key(login::Key {
            key: key.encrypt(&secret).unwrap(),
            challenge: key.encrypt(&hello.challenge).unwrap(),
        })
        .encode(),
    )
    .unwrap();
    twin.enable_encryption(&secret);
    let query = expect(&mut twin, lg::clientbound::CUSTOM_QUERY);
    let transaction = Reader::new(&query).varint().unwrap();
    twin.send(
        &login::Serverbound::CustomQueryAnswer(login::CustomQueryAnswer {
            transaction,
            payload: None,
        })
        .encode(),
    )
    .unwrap();
    let reason = expect(&mut twin, lg::clientbound::LOGIN_DISCONNECT);
    assert!(
        Reader::new(&reason)
            .string(4096)
            .unwrap()
            .contains("уже играет")
    );
}

#[test]
fn proxy_protocol_header() {
    let port = proxy_with(backend("lobby"), backend("pvp"), true);
    // With the header: the ping is answered.
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"PROXY TCP4 203.0.113.9 127.0.0.1 51000 25565\r\n")
        .unwrap();
    let mut c = Connection::from_stream(stream).unwrap();
    c.reader
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    c.send(
        &handshake::Serverbound::Intention(handshake::Intention {
            protocol: PROTOCOL_VERSION,
            host: "localhost".into(),
            port,
            intent: handshake::STATUS,
        })
        .encode(),
    )
    .unwrap();
    c.send(&body(0, &[])).unwrap();
    expect(&mut c, 0);
    // Without it the connection is closed.
    let mut plain = client(port, handshake::STATUS);
    plain.send(&body(0, &[])).unwrap();
    assert!(plain.recv().is_err());
}
