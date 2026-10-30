//! The network side: TCP connections from the proxy, their byte pipeline, and the handshake and login
//! states as Leaf runs them behind Velocity (`online-mode=false`, Paper's modern forwarding).
//!
//! What Netty's pipeline did is split in three:
//! - [`codec`]: the byte pipeline of one connection (AES/CFB8, `Varint21` frames, zlib above the
//!   threshold), as two plain halves over `bytes` buffers: [`codec::Inbound`] and [`codec::Outbound`].
//! - [`login`]: the handshake and login packets and the login state machine
//!   (`ServerHandshakePacketListenerImpl`, `ServerLoginPacketListenerImpl` with Paper's, Leaf's and
//!   MineComplex's patches), synchronous and free of I/O.
//! - [`listener`]: the tokio tasks. One task per connection runs handshake and login, asks the tick
//!   thread to admit the profile ([`listener::NetEvent::Login`]), and once the client acknowledges the
//!   login it hands the connection over ([`listener::NetEvent::Joined`]): from then on a reader task
//!   sends packet bodies to the tick thread and a writer task sends what the tick thread queues.
//!
//! Java exceptions that reach `Connection.exceptionCaught` become a [`Fault`] carrying the text Java
//! would put in `disconnect.genericReason` (`"Internal Exception: " + cause`), since that text is what
//! the client shows.

pub mod codec;
pub mod listener;
pub mod login;

use std::fmt::Write as _;

/// Protocol states of a connection (`ConnectionProtocol`), in the order a player goes through them.
/// The status state is not served (MineComplex's Cleanup: Velocity answers pings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Handshake,
    Login,
    Configuration,
    Play,
}

/// An error Java would raise into `Connection.exceptionCaught`: `self.0` is the exception's
/// `toString()` (class name, colon, message), e.g.
/// `io.netty.handler.codec.DecoderException: Badly compressed packet - ...`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Fault(pub String);

impl Fault {
    pub fn decoder(message: impl std::fmt::Display) -> Self {
        Fault(format!(
            "io.netty.handler.codec.DecoderException: {message}"
        ))
    }

    pub fn illegal_state(message: impl std::fmt::Display) -> Self {
        Fault(format!("java.lang.IllegalStateException: {message}"))
    }

    /// The reason `exceptionCaught` sends: `disconnect.genericReason` with `"Internal Exception: " + cause`.
    pub fn reason(&self) -> Reason {
        Reason::Translatable(
            "disconnect.genericReason",
            vec![format!("Internal Exception: {}", self.0)],
        )
    }
}

/// The text components the network side sends (disconnect reasons), in the JSON Minecraft writes for
/// them: `ComponentSerialization.CODEC` collapses a plain literal to a JSON string, and Gson runs with
/// `disableHtmlEscaping` (`ByteBufCodecs.lenientJson`). Paper's legacy (`§`) messages without codes
/// are plain literals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    Literal(String),
    /// A translation key and its string arguments.
    Translatable(&'static str, Vec<String>),
}

impl Reason {
    pub fn translatable(key: &'static str) -> Self {
        Reason::Translatable(key, Vec::new())
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        match self {
            Reason::Literal(text) => gson_string(&mut out, text),
            Reason::Translatable(key, args) => {
                out.push_str("{\"translate\":");
                gson_string(&mut out, key);
                if !args.is_empty() {
                    out.push_str(",\"with\":[");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        gson_string(&mut out, arg);
                    }
                    out.push(']');
                }
                out.push('}');
            }
        }
        out
    }
}

/// Gson's `JsonWriter.string` without HTML escaping.
fn gson_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '\u{2028}' | '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
