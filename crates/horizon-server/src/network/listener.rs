//! The tokio side: accepting connections (`ServerConnectionListener`), one task per connection for
//! handshake and login, then a reader and a writer task per player.
//!
//! Ownership: a connection task owns its socket and both codec halves; nothing is shared between
//! connections but the read-only [`LoginConfig`] (one `Arc`, cloned into each task) and the sender of
//! the tick thread's event queue. The tick thread hears of a connection twice:
//! [`NetEvent::Login`] when a profile needs admitting (`PlayerList.canPlayerLogin`, waiting for a
//! duplicate to leave: the tick thread answers on the request's oneshot when it has decided, with the
//! session id for `login_finished`), and [`NetEvent::Joined`] once the client acknowledged the login.
//! A joined [`Connection`] is two queues: packet bodies in (the reader task decodes frames and sends
//! them in order) and packet bodies out (the writer task compresses, frames, encrypts and writes
//! them). The tick thread never blocks on a socket: both channels are unbounded, as Netty's queues.
//!
//! **What a connection may queue.** Leaf has no packet-rate kick: MineComplex's cleanup patches
//! (`0318-Cleanup.patch` on the server, `0073-Cleanup-v2.patch` on Paper) delete Paper's packet
//! limiter (`packet-limiter` in `paper-global.yml`, `Connection.killForPacketSpam`) and vanilla's
//! `rate-limit` (`RateKickingConnection`), so nothing here kicks for sending fast. What is ours, and
//! invisible to clients, is a bound on the memory a connection's unread packets take: each queued
//! packet holds [`INBOUND_BUDGET`] permits for its size until the tick thread drops it, and when
//! the budget is spent the reader task stops reading the socket, so a client that floods faster
//! than the tick thread drains waits in TCP's flow control instead of in the server's memory.
//! A byte budget rather than a packet count because packets range from a few bytes to 8 MiB.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::time::{Instant, sleep_until, timeout};
use uuid::Uuid;

use super::codec::{Inbound, Outbound};
use super::login::{self, Forwarded, GameProfile, Handshake, Login, LoginConfig, Step};
use super::{ConnectionState, Fault, Reason};
use crate::azalea::chat::translatable_component::{PrimitiveOrComponent, TranslatableComponent};
use crate::azalea::chat::{FormattedText, write_component};

/// `ReadTimeoutHandler(30)` of every server connection.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// One server tick at 20 TPS: the login listener's timeout counts ticks.
const TICK: Duration = Duration::from_millis(50);
/// The bytes of unhandled packets one connection may have queued for the tick thread: the largest
/// frame a client can send (`Varint21`: 2^21 - 1 bytes), so a client that sends at full speed always
/// has a frame in flight, and a connection costs at most this plus one packet (a packet bigger than
/// the budget takes all of it; decompressed packets are at most 8 MiB).
pub const INBOUND_BUDGET: usize = (1 << 21) - 1;

/// What the network side tells the tick thread.
#[derive(Debug)]
pub enum NetEvent {
    Login(LoginRequest),
    Joined(Connection),
}

#[derive(Debug)]
pub struct LoginRequest {
    pub forwarded: Forwarded,
    pub transferred: bool,
    pub reply: oneshot::Sender<LoginVerdict>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginVerdict {
    /// `ServerConnectionListener.getSessionId()`, sent in `login_finished`.
    Allow {
        session_id: Uuid,
    },
    Deny(Reason),
}

/// A player's connection after login, in the configuration state.
#[derive(Debug)]
pub struct Connection {
    pub profile: GameProfile,
    pub address: SocketAddr,
    pub transferred: bool,
    pub state: ConnectionState,
    pub incoming: mpsc::UnboundedReceiver<Incoming>,
    pub outgoing: mpsc::UnboundedSender<Outgoing>,
}

#[derive(Debug)]
pub enum Incoming {
    /// A packet body (id + payload) of the connection's current state.
    Packet(Packet),
    /// The bytes could not be decoded: the tick thread sends `disconnect.genericReason` with
    /// [`Fault::reason`] in its state's disconnect packet, then closes.
    Fault(Fault),
    /// Nothing read for 30 seconds (`disconnect.timeout`).
    TimedOut,
    /// The client closed the connection.
    Closed,
}

impl Connection {
    /// Queue a packet body for the writer task (dropped silently once the connection is closed).
    pub fn send(&self, body: impl Into<Bytes>) {
        let _ = self.outgoing.send(Outgoing::Packet(body.into()));
    }

    /// The wire half of `ServerCommonPacketListenerImpl.disconnect`: the state's disconnect packet,
    /// then close once it is written.
    pub fn disconnect(&self, reason: &Reason) {
        self.send(disconnect_packet(self.state, reason));
        self.close();
    }

    /// `Connection.disconnect` without a packet: flush what is queued and close.
    pub fn close(&self) {
        let _ = self.outgoing.send(Outgoing::Close);
    }
}

/// The disconnect packet of a state: `ClientboundLoginDisconnectPacket` (JSON) in login, the common
/// `ClientboundDisconnectPacket` (the reason as a network NBT component) in configuration (id 2)
/// and play (id 32). The handshake state has none; its connections are only closed.
pub fn disconnect_packet(state: ConnectionState, reason: &Reason) -> Vec<u8> {
    let id = match state {
        ConnectionState::Handshake | ConnectionState::Login => {
            return login::disconnect_packet(reason);
        }
        ConnectionState::Configuration => 2,
        ConnectionState::Play => 32,
    };
    let mut out = vec![id];
    write_component(&reason_component(reason), &mut out).expect("a reason is a small component");
    out
}

/// A reason as a chat component (string arguments stay strings, as Java's `Object` arguments).
pub fn reason_component(reason: &Reason) -> FormattedText {
    match reason {
        Reason::Literal(text) => FormattedText::from(text.as_str()),
        Reason::Component(json) => {
            FormattedText::from_json(json).expect("a component built from a FormattedText decodes")
        }
        Reason::Translatable(key, args) => TranslatableComponent::new(
            (*key).to_owned(),
            args.iter()
                .cloned()
                .map(PrimitiveOrComponent::String)
                .collect(),
        )
        .into(),
    }
}

/// A packet body with its share of the connection's [`INBOUND_BUDGET`], given back when it is dropped.
#[derive(Debug)]
pub struct Packet {
    pub body: Bytes,
    _budget: OwnedSemaphorePermit,
}

impl std::ops::Deref for Packet {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.body
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outgoing {
    Packet(Bytes),
    /// Flush what is queued and close.
    Close,
}

/// Accept connections until the listener fails, each in its own task.
pub async fn listen(
    listener: TcpListener,
    config: Arc<LoginConfig>,
    events: mpsc::UnboundedSender<NetEvent>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                // Netty's server channel sets TCP_NODELAY on each child.
                let _ = stream.set_nodelay(true);
                tokio::spawn(serve(stream, peer, Arc::clone(&config), events.clone()));
            }
            Err(e) => tracing::warn!("Failed to accept a connection: {e}"),
        }
    }
}

struct Reader {
    socket: OwnedReadHalf,
    buf: BytesMut,
    inbound: Inbound,
}

enum Read {
    Packet(Bytes),
    Fault(Fault),
    TimedOut,
    Closed,
}

impl Reader {
    /// The next packet body. Cancel-safe: bytes read stay in `buf`.
    async fn next(&mut self) -> Read {
        loop {
            match self.inbound.decode(&mut self.buf) {
                Ok(Some(packet)) => return Read::Packet(packet),
                Ok(None) => {}
                Err(fault) => return Read::Fault(fault),
            }
            match timeout(READ_TIMEOUT, self.socket.read_buf(&mut self.buf)).await {
                Err(_) => return Read::TimedOut,
                Ok(Ok(0) | Err(_)) => return Read::Closed,
                Ok(Ok(_)) => {}
            }
        }
    }
}

struct Writer {
    socket: OwnedWriteHalf,
    outbound: Outbound,
    buf: BytesMut,
}

impl Writer {
    async fn send(&mut self, body: &[u8]) -> Result<(), Fault> {
        self.buf.clear();
        self.outbound.encode(body, &mut self.buf)?;
        self.socket
            .write_all(&self.buf)
            .await
            .map_err(|e| Fault(format!("java.io.IOException: {e}")))
    }

    /// `ServerLoginPacketListenerImpl.disconnect`: the reason, then close.
    async fn disconnect(mut self, name: &str, reason: &Reason) {
        tracing::info!("Disconnecting {name}: {}", reason.to_json());
        let _ = self.send(&login::disconnect_packet(reason)).await;
        let _ = self.socket.shutdown().await;
    }
}

/// One connection: handshake, login, then the hand-over to the tick thread.
async fn serve(
    stream: TcpStream,
    peer: SocketAddr,
    config: Arc<LoginConfig>,
    events: mpsc::UnboundedSender<NetEvent>,
) {
    let (read, write) = stream.into_split();
    let mut reader = Reader {
        socket: read,
        buf: BytesMut::with_capacity(512),
        inbound: Inbound::new(),
    };
    let mut writer = Writer {
        socket: write,
        outbound: Outbound::new(),
        buf: BytesMut::new(),
    };

    // Handshake: any failure here closes without a word (no clientbound handshake packets).
    let Read::Packet(body) = reader.next().await else {
        return;
    };
    let transferred =
        match login::decode_intention(&body).map(|i| login::handle_intention(&i, &config)) {
            Err(_) | Ok(Handshake::Close) => return,
            Ok(Handshake::Disconnect(reason)) => {
                return writer.disconnect(&peer.to_string(), &reason).await;
            }
            Ok(Handshake::Login { transferred }) => transferred,
        };

    let mut login = Login::new(transferred);
    let deadline = Instant::now() + TICK * config.slow_login_ticks();
    let mut verdict: Option<oneshot::Receiver<LoginVerdict>> = None;
    let mut address = peer;
    let name = |login: &Login| match &login.requested_username {
        Some(n) => format!("{n} ({peer})"),
        None => peer.to_string(),
    };
    let profile = loop {
        let step = tokio::select! {
            read = reader.next() => match read {
                Read::Packet(body) => login::decode_login(&body)
                    .and_then(|packet| login.handle(packet, &config, peer.ip(), rand::random::<i32>)),
                Read::Fault(fault) => Err(fault),
                Read::TimedOut | Read::Closed => return,
            },
            answer = async { verdict.as_mut().expect("guarded").await }, if verdict.is_some() => {
                verdict = None;
                match answer {
                    Ok(LoginVerdict::Allow { session_id }) => {
                        let threshold = config.compression_threshold;
                        if threshold >= 0 {
                            // Paper: the packet and the switch in the same Netty task.
                            if writer.send(&login::compression_packet(threshold)).await.is_err() { return }
                            writer.outbound.set_compression(threshold);
                            reader.inbound.set_compression(threshold);
                        }
                        Ok(Step::Send(login.finish(session_id)))
                    }
                    Ok(LoginVerdict::Deny(reason)) => Ok(Step::Disconnect(reason)),
                    Err(_) => return, // the tick thread is gone: the server is stopping
                }
            },
            () = sleep_until(deadline) => Ok(Step::Disconnect(Reason::translatable("multiplayer.disconnect.slow_login"))),
        };
        match step {
            Ok(Step::Nothing) => {}
            Ok(Step::Send(packet)) => {
                if writer.send(&packet).await.is_err() {
                    return;
                }
            }
            Ok(Step::Verify(forwarded)) => {
                address = SocketAddr::new(forwarded.address, peer.port());
                let (reply, answer) = oneshot::channel();
                verdict = Some(answer);
                if events
                    .send(NetEvent::Login(LoginRequest {
                        forwarded,
                        transferred,
                        reply,
                    }))
                    .is_err()
                {
                    return;
                }
            }
            Ok(Step::Disconnect(reason)) => return writer.disconnect(&name(&login), &reason).await,
            Err(fault) => {
                tracing::debug!("Failed to sent packet: {fault}");
                return writer.disconnect(&name(&login), &fault.reason()).await;
            }
            Ok(Step::Configuration(profile)) => break profile,
        }
    };

    let (to_tick, incoming) = mpsc::unbounded_channel();
    let (outgoing, from_tick) = mpsc::unbounded_channel();
    let connection = Connection {
        profile,
        address,
        transferred,
        state: ConnectionState::Configuration,
        incoming,
        outgoing,
    };
    if events.send(NetEvent::Joined(connection)).is_err() {
        return;
    }
    // The writer closing (on `Outgoing::Close`, or when the tick thread drops the connection) closes
    // the whole socket, as Netty's `channel.close()`: the reader stops too.
    let (closed, on_close) = oneshot::channel();
    tokio::spawn(write_loop(writer, from_tick, closed));
    read_loop(reader, to_tick, on_close).await;
}

async fn read_loop(
    mut reader: Reader,
    to_tick: mpsc::UnboundedSender<Incoming>,
    mut on_close: oneshot::Receiver<()>,
) {
    // Shared by this task (which takes permits) and the tick thread (whose dropped packets give them
    // back): the one counter both sides of the queue must see.
    let budget = Arc::new(Semaphore::new(INBOUND_BUDGET));
    loop {
        let read = tokio::select! {
            read = reader.next() => read,
            _ = &mut on_close => Read::Closed,
        };
        let (message, last) = match read {
            Read::Packet(body) => {
                let cost = body.len().clamp(1, INBOUND_BUDGET) as u32;
                // Backpressure: nothing more is read from the socket until the tick thread has
                // dropped enough of what it was given.
                let permit = tokio::select! {
                    permit = Arc::clone(&budget).acquire_many_owned(cost) => permit.expect("never closed"),
                    _ = &mut on_close => return drop(to_tick.send(Incoming::Closed)),
                };
                (
                    Incoming::Packet(Packet {
                        body,
                        _budget: permit,
                    }),
                    false,
                )
            }
            Read::Fault(fault) => (Incoming::Fault(fault), true),
            Read::TimedOut => (Incoming::TimedOut, true),
            Read::Closed => (Incoming::Closed, true),
        };
        if to_tick.send(message).is_err() || last {
            return;
        }
    }
}

async fn write_loop(
    mut writer: Writer,
    mut from_tick: mpsc::UnboundedReceiver<Outgoing>,
    closed: oneshot::Sender<()>,
) {
    while let Some(Outgoing::Packet(body)) = from_tick.recv().await {
        if let Err(fault) = writer.send(&body).await {
            tracing::debug!("Failed to send packet: {fault}");
            break;
        }
    }
    let _ = writer.socket.shutdown().await;
    let _ = closed.send(());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::codec::put_var_int;
    use crate::network::login::tests::{NOTCH, forged_answer, hex};

    /// A client speaking through the same codec halves.
    struct Client {
        reader: Reader,
        writer: Writer,
    }

    impl Client {
        async fn connect(addr: SocketAddr) -> Self {
            let (read, write) = TcpStream::connect(addr).await.unwrap().into_split();
            Client {
                reader: Reader {
                    socket: read,
                    buf: BytesMut::new(),
                    inbound: Inbound::new(),
                },
                writer: Writer {
                    socket: write,
                    outbound: Outbound::new(),
                    buf: BytesMut::new(),
                },
            }
        }

        async fn recv(&mut self) -> Vec<u8> {
            match self.reader.next().await {
                Read::Packet(p) => p.to_vec(),
                _ => panic!("expected a packet"),
            }
        }

        async fn closed(&mut self) -> bool {
            matches!(self.reader.next().await, Read::Closed)
        }
    }

    async fn server(config: LoginConfig) -> (SocketAddr, mpsc::UnboundedReceiver<NetEvent>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (events, rx) = mpsc::unbounded_channel();
        tokio::spawn(listen(listener, Arc::new(config), events));
        (addr, rx)
    }

    fn intention(protocol: i32, intent: i32) -> Vec<u8> {
        let mut p = vec![0];
        put_var_int(&mut p, protocol);
        p.push(14);
        p.extend(b"localhost.test");
        p.extend([0x63, 0xdd]);
        p.push(intent as u8);
        p
    }

    #[tokio::test]
    async fn velocity_login_to_configuration() {
        let config = LoginConfig {
            velocity_secret: Some("s3cret".into()),
            ..LoginConfig::default()
        };
        let (addr, mut events) = server(config).await;
        let mut client = Client::connect(addr).await;
        client.writer.send(&intention(777, 2)).await.unwrap();
        client
            .writer
            .send(&hex("00054e6f746368069a79f444e94726a5befca90e38aaf5"))
            .await
            .unwrap();
        let query = client.recv().await;
        assert_eq!(query[0], 4);
        assert!(query.ends_with(&hex("1476656c6f636974793a706c617965725f696e666f04")));
        let mut answer = vec![2];
        answer.extend_from_slice(&query[1..query.len() - 22]); // the transaction id
        answer.push(1);
        answer.extend(forged_answer());
        client.writer.send(&answer).await.unwrap();

        let Some(NetEvent::Login(request)) = events.recv().await else {
            panic!("expected a login request")
        };
        assert_eq!(request.forwarded.profile.id, NOTCH);
        assert_eq!(request.forwarded.address.to_string(), "203.0.113.5");
        let session_id = uuid::uuid!("00112233-4455-6677-8899-aabbccddeeff");
        request
            .reply
            .send(LoginVerdict::Allow { session_id })
            .unwrap();

        // The compression packet goes out uncompressed, then everything is compressed both ways.
        assert_eq!(client.recv().await, hex("038002"));
        client.reader.inbound.set_compression(256);
        client.writer.outbound.set_compression(256);
        assert_eq!(
            client.recv().await,
            login::login_finished_packet(&request.forwarded.profile, session_id)
        );
        // The acknowledgement and a first configuration packet in one write.
        let mut both = BytesMut::new();
        client.writer.outbound.encode(&[3], &mut both).unwrap();
        client
            .writer
            .outbound
            .encode(&[0x07, 1, 2, 3], &mut both)
            .unwrap();
        client.writer.socket.write_all(&both).await.unwrap();

        let Some(NetEvent::Joined(mut connection)) = events.recv().await else {
            panic!("expected the hand-over")
        };
        assert_eq!(connection.state, ConnectionState::Configuration);
        assert_eq!(connection.profile.name, "Notch");
        assert_eq!(connection.address.ip().to_string(), "203.0.113.5");
        let Some(Incoming::Packet(packet)) = connection.incoming.recv().await else {
            panic!("expected a packet")
        };
        assert_eq!(&*packet, &[0x07, 1, 2, 3]);
        connection
            .outgoing
            .send(Outgoing::Packet(Bytes::from(vec![0x0e; 600])))
            .unwrap();
        assert_eq!(client.recv().await, vec![0x0e; 600]);

        // The inbound budget: 30 packets of 100 kB, but only what fits in the budget is queued
        // while the tick thread holds them; the rest arrives once they are dropped.
        let mut flood = BytesMut::new();
        for _ in 0..30 {
            client
                .writer
                .outbound
                .encode(&[0x14; 100_000], &mut flood)
                .unwrap();
        }
        client.writer.socket.write_all(&flood).await.unwrap();
        // Wait for what fits in the budget (however slow the machine), then check that nothing
        // more is queued while those are held: only a broken budget could fail this.
        let mut held = Vec::new();
        while held.len() < INBOUND_BUDGET / 100_000 {
            let Some(Incoming::Packet(packet)) = connection.incoming.recv().await else {
                panic!("expected a packet")
            };
            held.push(packet);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            connection.incoming.try_recv().is_err(),
            "a packet was queued past the budget"
        );
        drop(held);
        for _ in 0..30 - INBOUND_BUDGET / 100_000 {
            let Some(Incoming::Packet(packet)) = connection.incoming.recv().await else {
                panic!("expected a packet")
            };
            assert_eq!(packet.len(), 100_000);
        }
        connection.outgoing.send(Outgoing::Close).unwrap();
        assert!(client.closed().await);
        assert!(matches!(
            connection.incoming.recv().await,
            Some(Incoming::Closed)
        ));
    }

    #[tokio::test]
    async fn refusals() {
        let config = LoginConfig {
            velocity_secret: Some("s3cret".into()),
            ..LoginConfig::default()
        };
        let (addr, mut events) = server(config).await;
        // Status: closed without an answer.
        let mut client = Client::connect(addr).await;
        client.writer.send(&intention(777, 1)).await.unwrap();
        assert!(client.closed().await);
        // An old client.
        let mut client = Client::connect(addr).await;
        client.writer.send(&intention(776, 2)).await.unwrap();
        assert_eq!(
            client.recv().await,
            login::disconnect_packet(&Reason::Literal("Outdated client! Please use 26.3".into()))
        );
        assert!(client.closed().await);
        // A wrong secret.
        let mut client = Client::connect(addr).await;
        client.writer.send(&intention(777, 2)).await.unwrap();
        client
            .writer
            .send(&hex("00054e6f746368069a79f444e94726a5befca90e38aaf5"))
            .await
            .unwrap();
        let query = client.recv().await;
        let mut answer = vec![2];
        answer.extend_from_slice(&query[1..query.len() - 22]);
        answer.push(1);
        let mut forged = forged_answer();
        forged[0] ^= 0xff;
        answer.extend(forged);
        client.writer.send(&answer).await.unwrap();
        assert_eq!(
            client.recv().await,
            login::disconnect_packet(&Reason::Literal("Unable to verify player details".into()))
        );
        // The tick thread refuses the profile.
        let mut client = Client::connect(addr).await;
        client.writer.send(&intention(777, 2)).await.unwrap();
        client
            .writer
            .send(&hex("00054e6f746368069a79f444e94726a5befca90e38aaf5"))
            .await
            .unwrap();
        let query = client.recv().await;
        let mut answer = vec![2];
        answer.extend_from_slice(&query[1..query.len() - 22]);
        answer.push(1);
        answer.extend(forged_answer());
        client.writer.send(&answer).await.unwrap();
        let Some(NetEvent::Login(request)) = events.recv().await else {
            panic!()
        };
        request
            .reply
            .send(LoginVerdict::Deny(Reason::translatable(
                "multiplayer.disconnect.not_whitelisted",
            )))
            .unwrap();
        assert_eq!(
            client.recv().await,
            login::disconnect_packet(&Reason::translatable(
                "multiplayer.disconnect.not_whitelisted"
            ))
        );
        assert!(client.closed().await);
    }

    /// Gale's timeout set to 2 ticks: a client that stops after the hello gets `slow_login`.
    #[tokio::test]
    async fn slow_login() {
        let (addr, _events) = server(LoginConfig {
            slow_login_timeout: 2,
            ..LoginConfig::default()
        })
        .await;
        let mut client = Client::connect(addr).await;
        client.writer.send(&intention(777, 2)).await.unwrap();
        assert_eq!(
            client.recv().await,
            login::disconnect_packet(&Reason::translatable("multiplayer.disconnect.slow_login"))
        );
        assert!(client.closed().await);
    }
}
