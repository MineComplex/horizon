//! The server over real TCP: a blocking client logs in through a forged Velocity answer while the
//! server runs on its own thread.
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::thread::JoinHandle;

use bytes::BytesMut;

use super::*;
use crate::azalea::chat::{FormattedText, read_component};
use crate::network::codec::{Inbound, Outbound, put_var_int};
use crate::network::login::tests::{forged_answer, hex};
use crate::network::login::{self, PROTOCOL_VERSION};

fn config(max_players: i32) -> RuntimeConfig {
    RuntimeConfig {
        server_ip: "127.0.0.1".into(),
        server_port: 0,
        login: LoginConfig {
            velocity_secret: Some("s3cret".into()),
            ..LoginConfig::default()
        },
        tick_loop: TickLoopConfig::default(),
        max_players,
        online_mode: false,
        proxy_online_mode: true,
        white_list: false,
        pause_when_empty_seconds: -1,
        shutdown_message: Reason::Literal("Сервер перезапускается".into()),
        boot: Instant::now(),
    }
}

struct Running {
    addr: SocketAddr,
    control: std_mpsc::Sender<Control>,
    thread: JoinHandle<u8>,
}

fn run(config: RuntimeConfig, place_on_join: bool) -> Running {
    let mut server = Server::start(config).unwrap();
    server.place_on_join = place_on_join;
    Running {
        addr: server.local_addr(),
        control: server.control(),
        thread: std::thread::spawn(move || server.run()),
    }
}

impl Running {
    fn stop(self, control: Control) -> u8 {
        self.control.send(control).unwrap();
        self.thread.join().unwrap()
    }
}

struct Client {
    stream: TcpStream,
    buf: BytesMut,
    inbound: Inbound,
    outbound: Outbound,
}

impl Client {
    fn connect(addr: SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        Client {
            stream,
            buf: BytesMut::new(),
            inbound: Inbound::new(),
            outbound: Outbound::new(),
        }
    }

    fn send(&mut self, body: &[u8]) {
        let mut out = BytesMut::new();
        self.outbound.encode(body, &mut out).unwrap();
        self.stream.write_all(&out).unwrap();
    }

    /// The next packet body, or `None` once the server closed the connection.
    fn recv(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(packet) = self.inbound.decode(&mut self.buf).unwrap() {
                return Some(packet.to_vec());
            }
            let mut chunk = [0; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) | Err(_) => return None,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
            }
        }
    }

    /// Handshake, hello and the Velocity answer; returns the server's next packet.
    fn log_in(addr: SocketAddr) -> (Self, Vec<u8>) {
        let mut client = Client::connect(addr);
        let mut intention = vec![0];
        put_var_int(&mut intention, PROTOCOL_VERSION);
        intention.push(9);
        intention.extend(b"localhost");
        intention.extend([0x63, 0xdd, 2]);
        client.send(&intention);
        client.send(&hex("00054e6f746368069a79f444e94726a5befca90e38aaf5"));
        let query = client.recv().unwrap();
        let mut answer = vec![2];
        answer.extend_from_slice(&query[1..query.len() - 22]);
        answer.push(1);
        answer.extend(forged_answer());
        client.send(&answer);
        let next = client.recv().unwrap();
        (client, next)
    }

    /// All the way to the configuration state.
    fn join(addr: SocketAddr) -> Self {
        let (mut client, compression) = Client::log_in(addr);
        assert_eq!(compression, hex("038002"));
        client.inbound.set_compression(256);
        client.outbound.set_compression(256);
        let finished = client.recv().unwrap();
        assert_eq!(finished[0], 2, "login_finished");
        client.send(&[3]);
        client
    }
}

fn component(packet: &[u8]) -> FormattedText {
    read_component(&mut std::io::Cursor::new(&packet[1..])).unwrap()
}

#[test]
fn join_then_stop_from_the_console() {
    let server = run(config(20), false);
    let mut client = Client::join(server.addr);
    let code = server.stop(Control::Console("/stop".into()));
    assert_eq!(code, 0);
    // Configuration connections are closed without a packet (`handleAllDisconnections`).
    assert_eq!(client.recv(), None);
}

#[test]
fn a_full_server_refuses() {
    let server = run(config(0), false);
    let (mut client, refusal) = Client::log_in(server.addr);
    assert_eq!(
        refusal,
        login::disconnect_packet(&Reason::translatable("multiplayer.disconnect.server_full"))
    );
    assert_eq!(client.recv(), None);
    assert_eq!(server.stop(Control::Signal(Signal::Interrupt)), 130);
}

#[test]
fn a_duplicate_login_kicks_the_old_player() {
    let server = run(config(20), true);
    let mut first = Client::join(server.addr);
    let mut second = Client::join(server.addr);
    let kick = first.recv().unwrap();
    assert_eq!(kick[0], 2, "configuration disconnect");
    assert_eq!(
        component(&kick),
        listener::reason_component(&Reason::translatable(
            "multiplayer.disconnect.duplicate_login"
        ))
    );
    assert_eq!(first.recv(), None);
    // On stop, players get the shutdown message.
    assert_eq!(server.stop(Control::Signal(Signal::Terminate)), 143);
    let message = second.recv().unwrap();
    assert_eq!(
        component(&message),
        FormattedText::from("Сервер перезапускается")
    );
    assert_eq!(second.recv(), None);
}

/// `handleKeepAlive`: an answer to no keep-alive is a timeout.
#[test]
fn a_wrong_keep_alive_times_out() {
    let server = run(config(20), false);
    let mut client = Client::join(server.addr);
    client.send(&[4, 0, 0, 0, 0, 0, 0, 0, 7]);
    let kick = client.recv().unwrap();
    assert_eq!(
        component(&kick),
        listener::reason_component(&Reason::translatable("disconnect.timeout"))
    );
    assert_eq!(client.recv(), None);
    assert_eq!(server.stop(Control::Console("stop".into())), 0);
}

#[test]
fn keep_alive_schedule() {
    let (outgoing, mut sent) = mpsc::unbounded_channel();
    let (_to_tick, incoming) = mpsc::unbounded_channel();
    let conn = Connection {
        profile: GameProfile::offline("Notch"),
        address: "127.0.0.1:1".parse().unwrap(),
        transferred: false,
        state: ConnectionState::Configuration,
        incoming,
        outgoing,
    };
    let mut peer = Peer::new(conn, 1000);
    peer.keep_alive(15_999);
    assert!(sent.try_recv().is_err());
    peer.keep_alive(16_000);
    let Ok(listener::Outgoing::Packet(ping)) = sent.try_recv() else {
        panic!("expected a keep-alive")
    };
    assert_eq!(&ping[..], &[4, 0, 0, 0, 0, 0, 0, 0x3e, 0x80]);
    // Answered 40 ms later: the latency moves a quarter of the way.
    let mut answer = vec![4];
    answer.extend(16_000i64.to_be_bytes());
    assert!(peer.handle_keep_alive(&answer, 16_040));
    assert_eq!((peer.latency, peer.keep_alive_pending), (10, false));
    // Unanswered: a new one 15 s later, the kick only once 30 s have passed (Paper).
    peer.keep_alive(31_000);
    assert!(sent.try_recv().is_ok());
    peer.keep_alive(60_999);
    assert!(sent.try_recv().is_err());
    peer.keep_alive(61_000);
    assert!(sent.try_recv().is_ok(), "the disconnect packet");
    assert_eq!(
        peer.closing,
        Some(Reason::translatable("disconnect.timeout"))
    );
}

#[test]
fn a_taken_port_fails_to_start() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = config(20);
    config.server_port = i32::from(taken.local_addr().unwrap().port());
    assert!(Server::start(config).is_err());
}
