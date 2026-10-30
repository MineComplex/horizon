//! Handshake and login: the packets (protocol 777) and the login state machine as Leaf runs it.
//!
//! Leaf's behaviour, from vanilla 26.3 plus Paper's, Leaf's and MineComplex's patches:
//! - Handshake (`ServerHandshakePacketListenerImpl`): a status intention gets no answer, the connection
//!   is just closed (MineComplex's Cleanup removed the status listener); a transfer while
//!   `accepts-transfers=false` gets `multiplayer.disconnect.transfers_disabled`; a protocol other than
//!   777 gets Spigot's `messages.outdated-client` / `outdated-server` (older protocols: client, newer:
//!   server), formatted with the version name. The host name may be 32767 long (Spigot). No connection
//!   throttle and no BungeeCord forwarding (both removed by the Cleanup).
//! - Hello: no username validation (the Cleanup removed it). With `online-mode=false` Leaf's
//!   `AsyncPreAuthenticateEvent` starts cancelled, so the server never sends an encryption request;
//!   with Velocity forwarding on it sends a `velocity:player_info` query (random transaction id,
//!   payload: the highest forwarding version it reads, 4) and stays in `HELLO`. Without Velocity the
//!   profile is the offline one (`UUIDUtil.createOfflinePlayerUUID`, no properties).
//! - Query answer (`VelocityProxy`): the answer to our transaction id must carry a payload
//!   ("This server requires you to connect with Velocity."), signed with HMAC-SHA256 over the secret
//!   ("Unable to verify player details"), in a version up to 4 (else `IllegalStateException`); it
//!   gives the player's address and profile (UUID, name, properties). Any other answer, and any
//!   cookie response, gets `multiplayer.disconnect.unexpected_query_response`.
//! - Then the tick thread decides (`PlayerList.canPlayerLogin`, waiting for a duplicate to leave); if
//!   admitted, compression is sent and switched on when `network-compression-threshold >= 0`, then
//!   `login_finished`; the client's `login_acknowledged` moves the connection to configuration.
//!
//! Online mode (encryption request, session server) is not here: behind Velocity Leaf never uses it.

use std::net::IpAddr;

use bytes::BufMut;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;

use super::codec::{put_var_int, read_var_int};
use super::{Fault, Reason};

/// `SharedConstants.getCurrentVersion()` of the 26.3 server jar.
pub const PROTOCOL_VERSION: i32 = 777;
pub const VERSION_NAME: &str = "26.3";
/// `VelocityProxy.MAX_SUPPORTED_FORWARDING_VERSION` (`MODERN_LAZY_SESSION`).
pub const MAX_SUPPORTED_FORWARDING_VERSION: u8 = 4;
pub const PLAYER_INFO_CHANNEL: &str = "velocity:player_info";
/// `ServerLoginPacketListenerImpl.MAX_TICKS_BEFORE_LOGIN`.
pub const MAX_TICKS_BEFORE_LOGIN: u32 = 600;

/// What login reads from the server's configuration.
#[derive(Clone, Debug)]
pub struct LoginConfig {
    /// `accepts-transfers` (server.properties).
    pub accepts_transfers: bool,
    /// `network-compression-threshold` (server.properties); negative turns compression off.
    pub compression_threshold: i32,
    /// `proxies.velocity.secret` when `proxies.velocity.enabled` (paper-global.yml).
    pub velocity_secret: Option<String>,
    /// Spigot's `messages.outdated-client` / `outdated-server` (`{0}` is the version name).
    pub outdated_client_message: String,
    pub outdated_server_message: String,
    /// Gale's `misc.premium-account-slow-login-timeout` (below 1: 600 ticks).
    pub slow_login_timeout: i32,
}

impl Default for LoginConfig {
    fn default() -> Self {
        Self {
            accepts_transfers: false,
            compression_threshold: 256,
            velocity_secret: None,
            outdated_client_message: "Outdated client! Please use {0}".into(),
            outdated_server_message: "Outdated server! I'm still on {0}".into(),
            slow_login_timeout: -1,
        }
    }
}

impl LoginConfig {
    /// Ticks the client has to finish logging in before `multiplayer.disconnect.slow_login`.
    pub fn slow_login_ticks(&self) -> u32 {
        if self.slow_login_timeout < 1 {
            MAX_TICKS_BEFORE_LOGIN
        } else {
            self.slow_login_timeout as u32
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameProfile {
    pub id: Uuid,
    pub name: String,
    /// In `ImmutableMultimap` order: keys by first appearance, each key's values together.
    pub properties: Vec<Property>,
}

impl GameProfile {
    /// `UUIDUtil.createOfflineProfile`.
    pub fn offline(name: &str) -> Self {
        Self {
            id: crate::azalea::crypto::offline::generate_uuid(name),
            name: name.into(),
            properties: Vec::new(),
        }
    }
}

/// A packet payload reader with `FriendlyByteBuf`'s limits.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], Fault> {
        if n > self.0.len() {
            return Err(Fault(format!(
                "java.lang.IndexOutOfBoundsException: length({n}) exceeds readable bytes({})",
                self.0.len()
            )));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn var_int(&mut self) -> Result<i32, Fault> {
        match read_var_int(self.0)? {
            Some((v, n)) => {
                self.0 = &self.0[n..];
                Ok(v)
            }
            None => Err(Fault("java.lang.IndexOutOfBoundsException: VarInt".into())),
        }
    }

    fn bool(&mut self) -> Result<bool, Fault> {
        Ok(self.bytes(1)?[0] != 0)
    }

    fn uuid(&mut self) -> Result<Uuid, Fault> {
        Ok(Uuid::from_slice(self.bytes(16)?).expect("16 bytes"))
    }

    /// `Utf8String.read`: at most `max * 3` bytes, at most `max` UTF-16 units.
    fn utf(&mut self, max: usize) -> Result<String, Fault> {
        let len = self.var_int()?;
        if len > (max * 3) as i32 {
            return Err(Fault::decoder(format!(
                "The received encoded string buffer length is longer than maximum allowed ({len} > {})",
                max * 3
            )));
        }
        if len < 0 {
            return Err(Fault::decoder(
                "The received encoded string buffer length is less than zero! Weird string!",
            ));
        }
        if len as usize > self.0.len() {
            return Err(Fault::decoder(format!(
                "Not enough bytes in buffer, expected {len}, but got {}",
                self.0.len()
            )));
        }
        let s = String::from_utf8_lossy(self.bytes(len as usize)?).into_owned();
        let units = s.encode_utf16().count();
        if units > max {
            return Err(Fault::decoder(format!(
                "The received string length is longer than maximum allowed ({units} > {max})"
            )));
        }
        Ok(s)
    }

    fn byte_array(&mut self, max: usize) -> Result<&'a [u8], Fault> {
        let len = self.var_int()?;
        if len < 0 || len as usize > max {
            return Err(Fault::decoder(format!(
                "ByteArray with size {len} is bigger than allowed {max}"
            )));
        }
        self.bytes(len as usize)
    }
}

fn put_utf(out: &mut Vec<u8>, s: &str) {
    put_var_int(out, s.len() as i32);
    out.put_slice(s.as_bytes());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Status,
    Login,
    Transfer,
}

/// `ClientIntentionPacket` (handshake id 0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intention {
    pub protocol_version: i32,
    pub host_name: String,
    pub port: u16,
    pub intent: Intent,
}

/// Decode the handshake packet. Any failure closes the connection without an answer: the handshake
/// state has no clientbound packets to carry a reason.
pub fn decode_intention(body: &[u8]) -> Result<Intention, Fault> {
    let mut r = Reader(body);
    let id = r.var_int()?;
    if id != 0 {
        return Err(Fault::decoder(format!("Received unknown packet id {id}")));
    }
    let wrap = |_| Fault::decoder("Failed to decode packet 'serverbound/minecraft:intention'");
    let protocol_version = r.var_int().map_err(wrap)?;
    let host_name = r.utf(32767).map_err(wrap)?;
    let port = r.bytes(2).map_err(wrap)?;
    let intent = match r.var_int().map_err(wrap)? {
        1 => Intent::Status,
        2 => Intent::Login,
        3 => Intent::Transfer,
        _ => return Err(wrap(Fault(String::new()))),
    };
    if !r.0.is_empty() {
        return Err(Fault::decoder(
            "java.io.IOException: extra bytes after the intention packet",
        ));
    }
    Ok(Intention {
        protocol_version,
        host_name,
        port: u16::from_be_bytes([port[0], port[1]]),
        intent,
    })
}

/// What the handshake listener does with an intention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handshake {
    /// Close without a word (status requests).
    Close,
    /// Send `login_disconnect` with the reason, then close.
    Disconnect(Reason),
    /// Go on in the login state.
    Login { transferred: bool },
}

pub fn handle_intention(intention: &Intention, config: &LoginConfig) -> Handshake {
    let transferred = match intention.intent {
        Intent::Status => return Handshake::Close,
        Intent::Transfer if !config.accepts_transfers => {
            return Handshake::Disconnect(Reason::translatable(
                "multiplayer.disconnect.transfers_disabled",
            ));
        }
        Intent::Transfer => true,
        Intent::Login => false,
    };
    if intention.protocol_version != PROTOCOL_VERSION {
        let message = if intention.protocol_version < PROTOCOL_VERSION {
            &config.outdated_client_message
        } else {
            &config.outdated_server_message
        };
        return Handshake::Disconnect(Reason::Literal(message.replace("{0}", VERSION_NAME)));
    }
    Handshake::Login { transferred }
}

/// Serverbound login packets, by id (`LoginProtocols.SERVERBOUND`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginPacket {
    Hello {
        name: String,
        profile_id: Uuid,
    },
    Key,
    CustomQueryAnswer {
        transaction_id: i32,
        payload: Option<Vec<u8>>,
    },
    LoginAcknowledged,
    CookieResponse,
}

const LOGIN_PACKETS: [(&str, &str); 5] = [
    ("hello", "ServerboundHelloPacket"),
    ("key", "ServerboundKeyPacket"),
    ("custom_query_answer", "ServerboundCustomQueryAnswerPacket"),
    ("login_acknowledged", "ServerboundLoginAcknowledgedPacket"),
    ("cookie_response", "ServerboundCookieResponsePacket"),
];

pub fn decode_login(body: &[u8]) -> Result<LoginPacket, Fault> {
    let mut r = Reader(body);
    let id = r.var_int()?;
    let Some(&(name, class)) = usize::try_from(id).ok().and_then(|i| LOGIN_PACKETS.get(i)) else {
        return Err(Fault::decoder(format!("Received unknown packet id {id}")));
    };
    let packet = (|| {
        Ok(match id {
            0 => LoginPacket::Hello {
                name: r.utf(16)?,
                profile_id: r.uuid()?,
            },
            1 => {
                r.byte_array(r.0.len())?;
                r.byte_array(r.0.len())?;
                LoginPacket::Key
            }
            2 => {
                let transaction_id = r.var_int()?;
                // Paper's `QueryAnswerPayload`: present flag, then every remaining byte.
                let payload = if r.bool()? {
                    if r.0.len() > 1_048_576 {
                        return Err(Fault(String::new()));
                    }
                    Some(r.bytes(r.0.len())?.to_vec())
                } else {
                    None
                };
                LoginPacket::CustomQueryAnswer {
                    transaction_id,
                    payload,
                }
            }
            3 => LoginPacket::LoginAcknowledged,
            _ => {
                r.utf(32767)?;
                if r.bool()? {
                    r.byte_array(5120)?;
                }
                LoginPacket::CookieResponse
            }
        })
    })()
    .map_err(|_: Fault| {
        Fault::decoder(format!(
            "Failed to decode packet 'serverbound/minecraft:{name}'"
        ))
    })?;
    if !r.0.is_empty() {
        return Err(Fault::decoder(format!(
            "java.io.IOException: Packet login/serverbound/minecraft:{name} ({class}) was larger than I expected, found {} bytes extra whilst reading packet serverbound/minecraft:{name}",
            r.0.len()
        )));
    }
    Ok(packet)
}

/// `ClientboundLoginDisconnectPacket` (id 0): the reason as JSON, at most 262144 characters.
pub fn disconnect_packet(reason: &Reason) -> Vec<u8> {
    let mut out = vec![0];
    put_utf(&mut out, &reason.to_json());
    out
}

/// `ClientboundLoginFinishedPacket` (id 2).
pub fn login_finished_packet(profile: &GameProfile, session_id: Uuid) -> Vec<u8> {
    let mut out = vec![2];
    out.put_slice(profile.id.as_bytes());
    put_utf(&mut out, &profile.name);
    put_var_int(&mut out, profile.properties.len() as i32);
    for p in &profile.properties {
        put_utf(&mut out, &p.name);
        put_utf(&mut out, &p.value);
        out.put_u8(p.signature.is_some().into());
        if let Some(signature) = &p.signature {
            put_utf(&mut out, signature);
        }
    }
    out.put_slice(session_id.as_bytes());
    out
}

/// `ClientboundLoginCompressionPacket` (id 3).
pub fn compression_packet(threshold: i32) -> Vec<u8> {
    let mut out = vec![3];
    put_var_int(&mut out, threshold);
    out
}

/// `ClientboundCustomQueryPacket` (id 4) with Paper's `PlayerInfoChannelPayload`.
pub fn velocity_query_packet(transaction_id: i32) -> Vec<u8> {
    let mut out = vec![4];
    put_var_int(&mut out, transaction_id);
    put_utf(&mut out, PLAYER_INFO_CHANNEL);
    out.push(MAX_SUPPORTED_FORWARDING_VERSION);
    out
}

/// What Velocity forwarded (`VelocityProxy.readAddress` and `createProfile`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forwarded {
    pub address: IpAddr,
    pub profile: GameProfile,
}

/// `VelocityProxy.checkIntegrity` then the reads Paper does: `Ok(None)` when the signature is wrong.
pub fn read_forwarded(secret: &str, payload: &[u8]) -> Result<Option<Forwarded>, Fault> {
    let mut r = Reader(payload);
    let signature = r.bytes(32)?;
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes any key length");
    mac.update(r.0);
    if mac.verify_slice(signature).is_err() {
        return Ok(None);
    }
    let version = r.var_int()?;
    if version > i32::from(MAX_SUPPORTED_FORWARDING_VERSION) {
        return Err(Fault::illegal_state(format!(
            "Unsupported forwarding version {version}, wanted upto {MAX_SUPPORTED_FORWARDING_VERSION}"
        )));
    }
    let text = r.utf(32767)?;
    // Guava's `InetAddresses.forString`.
    let address = text.parse().map_err(|_| {
        Fault(format!(
            "java.lang.IllegalArgumentException: '{text}' is not an IP string literal."
        ))
    })?;
    let id = r.uuid()?;
    let name = r.utf(16)?;
    let count = r.var_int()?;
    let mut properties: Vec<Property> = Vec::new();
    for _ in 0..count.max(0) {
        let property = Property {
            name: r.utf(32767)?,
            value: r.utf(32767)?,
            signature: if r.bool()? { Some(r.utf(32767)?) } else { None },
        };
        // `ImmutableMultimap.Builder`: a key seen before keeps its place.
        match properties.iter().rposition(|p| p.name == property.name) {
            Some(last) => properties.insert(last + 1, property),
            None => properties.push(property),
        }
    }
    Ok(Some(Forwarded {
        address,
        profile: GameProfile {
            id,
            name,
            properties,
        },
    }))
}

/// `ServerLoginPacketListenerImpl.State`, the states this server reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginState {
    Hello,
    /// The profile is known; the tick thread is deciding.
    Verifying,
    /// `login_finished` sent; waiting for `login_acknowledged`.
    ProtocolSwitching,
    Accepted,
}

/// What the connection task does after a login packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    Send(Vec<u8>),
    /// Ask the tick thread to admit this profile.
    Verify(Forwarded),
    Disconnect(Reason),
    /// `login_acknowledged`: the connection is in the configuration state.
    Configuration(GameProfile),
}

/// One connection's login listener.
#[derive(Debug)]
pub struct Login {
    pub state: LoginState,
    pub transferred: bool,
    velocity_login_message_id: i32,
    pub requested_username: Option<String>,
    profile: Option<GameProfile>,
}

impl Login {
    pub fn new(transferred: bool) -> Self {
        Self {
            state: LoginState::Hello,
            transferred,
            velocity_login_message_id: -1,
            requested_username: None,
            profile: None,
        }
    }

    /// Handle one packet. `peer` is the socket's address (kept as the player's when there is no
    /// proxy); `transaction_id` draws the Velocity query's id (`ThreadLocalRandom.nextInt`).
    pub fn handle(
        &mut self,
        packet: LoginPacket,
        config: &LoginConfig,
        peer: IpAddr,
        transaction_id: impl FnOnce() -> i32,
    ) -> Result<Step, Fault> {
        let unexpected_query = Step::Disconnect(Reason::translatable(
            "multiplayer.disconnect.unexpected_query_response",
        ));
        let requires_velocity = || {
            Step::Disconnect(Reason::Literal(
                "This server requires you to connect with Velocity.".into(),
            ))
        };
        match packet {
            LoginPacket::Hello {
                name,
                profile_id: _,
            } => {
                if self.state != LoginState::Hello {
                    return Err(Fault::illegal_state("Unexpected hello packet"));
                }
                self.requested_username = Some(name.clone());
                if config.velocity_secret.is_some() {
                    self.velocity_login_message_id = transaction_id();
                    return Ok(Step::Send(velocity_query_packet(
                        self.velocity_login_message_id,
                    )));
                }
                Ok(self.verify(Forwarded {
                    address: peer,
                    profile: GameProfile::offline(&name),
                }))
            }
            LoginPacket::Key => Err(Fault::illegal_state("Unexpected key packet")),
            LoginPacket::CustomQueryAnswer {
                transaction_id,
                payload,
            } => {
                let Some(secret) = &config.velocity_secret else {
                    return Ok(unexpected_query);
                };
                if transaction_id != self.velocity_login_message_id {
                    return Ok(unexpected_query);
                }
                let Some(payload) = payload else {
                    return Ok(requires_velocity());
                };
                match read_forwarded(secret, &payload)? {
                    None => Ok(Step::Disconnect(Reason::Literal(
                        "Unable to verify player details".into(),
                    ))),
                    Some(forwarded) => Ok(self.verify(forwarded)),
                }
            }
            LoginPacket::LoginAcknowledged => {
                if self.state != LoginState::ProtocolSwitching {
                    return Err(Fault::illegal_state(
                        "Unexpected login acknowledgement packet",
                    ));
                }
                self.state = LoginState::Accepted;
                Ok(Step::Configuration(
                    self.profile
                        .clone()
                        .expect("profile set before login_finished"),
                ))
            }
            LoginPacket::CookieResponse => Ok(unexpected_query),
        }
    }

    /// `startClientVerification`, after Paper's pre-login events (no plugins here).
    fn verify(&mut self, forwarded: Forwarded) -> Step {
        tracing::info!(
            "UUID of player {} is {}",
            forwarded.profile.name,
            forwarded.profile.id
        );
        self.profile = Some(forwarded.profile.clone());
        self.state = LoginState::Verifying;
        Step::Verify(forwarded)
    }

    /// The tick thread admitted the player: `finishLoginAndWaitForClient`'s packet. The caller sends
    /// [`compression_packet`] and switches compression on first.
    pub fn finish(&mut self, session_id: Uuid) -> Vec<u8> {
        self.state = LoginState::ProtocolSwitching;
        login_finished_packet(self.profile.as_ref().expect("verified profile"), session_id)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    pub const NOTCH: Uuid = uuid::uuid!("069a79f4-44e9-4726-a5be-fca90e38aaf5");
    /// `PlayerDataForwarding.createForwardingData` v4 for Notch at 203.0.113.5 and its HMAC under
    /// `s3cret`, from Java.
    pub const FWD_DATA: &str = "040b3230332e302e3131332e35069a79f444e94726a5befca90e38aaf5054e6f74636801087465787475726573046533303d010463326c6e";
    pub const FWD_MAC: &str = "57f60c8f07ef68f64a4d0e8a4ffad14804b1e68493dfe755b030d602d0a4fd56";

    pub fn forged_answer() -> Vec<u8> {
        let mut payload = hex(FWD_MAC);
        payload.extend(hex(FWD_DATA));
        payload
    }

    #[test]
    fn clientbound_packets_like_java() {
        assert_eq!(compression_packet(256), hex("038002"));
        assert_eq!(
            disconnect_packet(&Reason::translatable("multiplayer.disconnect.slow_login")),
            hex(
                "00317b227472616e736c617465223a226d756c7469706c617965722e646973636f6e6e6563742e736c6f775f6c6f67696e227d"
            )
        );
        let fault = Fault::illegal_state("Unexpected hello packet");
        assert_eq!(
            disconnect_packet(&fault.reason()),
            hex(
                "0080017b227472616e736c617465223a22646973636f6e6e6563742e67656e65726963526561736f6e222c2277697468223a5b22496e7465726e616c20457863657074696f6e3a206a6176612e6c616e672e496c6c6567616c5374617465457863657074696f6e3a20556e65787065637465642068656c6c6f207061636b6574225d7d"
            )
        );
        let literal = Reason::Literal(
            "Outdated server! I'm still on 26.3 \"q\" \\ <&> \n\u{1}\u{2028}é".into(),
        );
        assert_eq!(
            disconnect_packet(&literal),
            hex(
                "0042224f7574646174656420736572766572212049276d207374696c6c206f6e2032362e33205c22715c22205c5c203c263e205c6e5c75303030315c7532303238c3a922"
            )
        );
        let profile = GameProfile {
            id: NOTCH,
            name: "Notch".into(),
            properties: vec![
                Property {
                    name: "textures".into(),
                    value: "e30=".into(),
                    signature: Some("c2ln".into()),
                },
                Property {
                    name: "x".into(),
                    value: "y".into(),
                    signature: None,
                },
            ],
        };
        assert_eq!(
            login_finished_packet(
                &profile,
                uuid::uuid!("00112233-4455-6677-8899-aabbccddeeff")
            ),
            hex(
                "02069a79f444e94726a5befca90e38aaf5054e6f74636802087465787475726573046533303d010463326c6e017801790000112233445566778899aabbccddeeff"
            )
        );
        assert_eq!(
            velocity_query_packet(-123456789),
            hex("04ebe590c50f1476656c6f636974793a706c617965725f696e666f04")
        );
    }

    #[test]
    fn serverbound_packets_from_java() {
        let intention =
            decode_intention(&hex("00890610706c61792e6578616d706c652e6e657463dd02")).unwrap();
        assert_eq!(
            intention,
            Intention {
                protocol_version: 777,
                host_name: "play.example.net".into(),
                port: 25565,
                intent: Intent::Login
            }
        );
        assert_eq!(
            decode_login(&hex("00054e6f746368069a79f444e94726a5befca90e38aaf5")).unwrap(),
            LoginPacket::Hello {
                name: "Notch".into(),
                profile_id: NOTCH
            }
        );
        assert_eq!(
            decode_login(&hex("03")).unwrap(),
            LoginPacket::LoginAcknowledged
        );
        assert_eq!(
            decode_login(&hex("0300")).unwrap_err().0,
            "io.netty.handler.codec.DecoderException: java.io.IOException: Packet login/serverbound/minecraft:login_acknowledged (ServerboundLoginAcknowledgedPacket) was larger than I expected, found 1 bytes extra whilst reading packet serverbound/minecraft:login_acknowledged"
        );
        assert_eq!(
            decode_login(&hex("05")).unwrap_err().0,
            "io.netty.handler.codec.DecoderException: Received unknown packet id 5"
        );
        // 17 characters is one too many for a name.
        let mut long = vec![0, 17];
        long.extend(b"ABCDEFGHIJKLMNOPQ");
        long.extend([0; 16]);
        assert_eq!(
            decode_login(&long).unwrap_err().0,
            "io.netty.handler.codec.DecoderException: Failed to decode packet 'serverbound/minecraft:hello'"
        );
    }

    #[test]
    fn handshake_answers() {
        let config = LoginConfig::default();
        let at = |protocol_version, intent| {
            handle_intention(
                &Intention {
                    protocol_version,
                    host_name: String::new(),
                    port: 0,
                    intent,
                },
                &config,
            )
        };
        assert_eq!(at(777, Intent::Status), Handshake::Close);
        assert_eq!(
            at(777, Intent::Login),
            Handshake::Login { transferred: false }
        );
        assert_eq!(
            at(777, Intent::Transfer),
            Handshake::Disconnect(Reason::translatable(
                "multiplayer.disconnect.transfers_disabled"
            ))
        );
        assert_eq!(
            at(776, Intent::Login),
            Handshake::Disconnect(Reason::Literal("Outdated client! Please use 26.3".into()))
        );
        assert_eq!(
            at(778, Intent::Login),
            Handshake::Disconnect(Reason::Literal("Outdated server! I'm still on 26.3".into()))
        );
    }

    #[test]
    fn offline_uuid_like_java() {
        assert_eq!(
            GameProfile::offline("Notch").id,
            uuid::uuid!("b50ad385-829d-3141-a216-7e7d7539ba7f")
        );
    }

    #[test]
    fn velocity_forwarding() {
        let forwarded = read_forwarded("s3cret", &forged_answer()).unwrap().unwrap();
        assert_eq!(forwarded.address, "203.0.113.5".parse::<IpAddr>().unwrap());
        assert_eq!(forwarded.profile.id, NOTCH);
        assert_eq!(forwarded.profile.name, "Notch");
        assert_eq!(
            forwarded.profile.properties,
            [Property {
                name: "textures".into(),
                value: "e30=".into(),
                signature: Some("c2ln".into())
            }]
        );
        assert_eq!(read_forwarded("other", &forged_answer()).unwrap(), None);
        // A version above 4, correctly signed.
        let mut data = hex(FWD_DATA);
        data[0] = 5;
        let mut mac = Hmac::<Sha256>::new_from_slice(b"s3cret").unwrap();
        mac.update(&data);
        let mut payload = mac.finalize().into_bytes().to_vec();
        payload.extend(data);
        assert_eq!(
            read_forwarded("s3cret", &payload).unwrap_err().0,
            "java.lang.IllegalStateException: Unsupported forwarding version 5, wanted upto 4"
        );
    }

    #[test]
    fn login_states() {
        let config = LoginConfig {
            velocity_secret: Some("s3cret".into()),
            ..LoginConfig::default()
        };
        let peer: IpAddr = "127.0.0.1".parse().unwrap();
        let hello = || LoginPacket::Hello {
            name: "Notch".into(),
            profile_id: NOTCH,
        };
        let mut login = Login::new(false);
        assert_eq!(
            login.handle(hello(), &config, peer, || 7).unwrap(),
            Step::Send(velocity_query_packet(7))
        );
        let answer = |id, payload| LoginPacket::CustomQueryAnswer {
            transaction_id: id,
            payload,
        };
        assert_eq!(
            login.handle(answer(8, None), &config, peer, || 0).unwrap(),
            Step::Disconnect(Reason::translatable(
                "multiplayer.disconnect.unexpected_query_response"
            ))
        );
        assert_eq!(
            login.handle(answer(7, None), &config, peer, || 0).unwrap(),
            Step::Disconnect(Reason::Literal(
                "This server requires you to connect with Velocity.".into()
            ))
        );
        let mut bad = forged_answer();
        bad[40] ^= 1;
        assert_eq!(
            login
                .handle(answer(7, Some(bad)), &config, peer, || 0)
                .unwrap(),
            Step::Disconnect(Reason::Literal("Unable to verify player details".into()))
        );
        let Step::Verify(forwarded) = login
            .handle(answer(7, Some(forged_answer())), &config, peer, || 0)
            .unwrap()
        else {
            panic!("expected verification");
        };
        assert_eq!(forwarded.profile.name, "Notch");
        assert_eq!(
            login.handle(hello(), &config, peer, || 0).unwrap_err(),
            Fault::illegal_state("Unexpected hello packet")
        );
        assert_eq!(
            login
                .handle(LoginPacket::LoginAcknowledged, &config, peer, || 0)
                .unwrap_err(),
            Fault::illegal_state("Unexpected login acknowledgement packet")
        );
        login.finish(Uuid::nil());
        assert!(matches!(
            login.handle(LoginPacket::LoginAcknowledged, &config, peer, || 0).unwrap(),
            Step::Configuration(p) if p.id == NOTCH
        ));
        // Without Velocity: the offline profile straight away.
        let mut offline = Login::new(false);
        let Step::Verify(f) = offline
            .handle(hello(), &LoginConfig::default(), peer, || 0)
            .unwrap()
        else {
            panic!("expected verification");
        };
        assert_eq!(
            (f.address, f.profile),
            (peer, GameProfile::offline("Notch"))
        );
    }
}
