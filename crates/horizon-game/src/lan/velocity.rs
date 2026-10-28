//! Velocity modern forwarding on the server's side (Paper's `VelocityProxy` and its
//! `ServerLoginPacketListenerImpl` patch): the server sits behind the proxy, so it asks the
//! proxy on the `velocity:player_info` login channel for the player's address and profile,
//! signed with HMAC-SHA256 over the secret both share. This server never talks to Mojang:
//! the proxy authenticates players.
use hmac::{Hmac, Mac};
use horizon_protocol::{buf::Reader, packets::login};
use sha2::Sha256;

pub const CHANNEL: &str = "velocity:player_info";
/// `MAX_SUPPORTED_FORWARDING_VERSION` (`MODERN_LAZY_SESSION`).
pub const MAX_VERSION: u8 = 4;

/// `proxies.velocity` of `config/paper-global.yml` while `enabled`.
pub struct Velocity {
    pub secret: Vec<u8>,
}

/// What the proxy forwarded: the player's real address and profile.
#[derive(Debug, PartialEq)]
pub struct Forwarded {
    pub address: std::net::IpAddr,
    pub uuid: u128,
    pub name: String,
    pub properties: Vec<login::Property>,
}

impl Velocity {
    /// The login query: one byte, the highest forwarding version understood.
    pub fn query(transaction: i32) -> login::Clientbound {
        login::Clientbound::CustomQuery(login::CustomQuery {
            transaction,
            channel: CHANNEL.to_owned(),
            data: vec![MAX_VERSION],
        })
    }
    /// `handleCustomQueryPacket`: the proxy's answer, checked and read. The error is the
    /// disconnect text (Paper's).
    pub fn read(&self, payload: Option<&[u8]>) -> Result<Forwarded, &'static str> {
        let answer = payload.ok_or("This server requires you to connect with Velocity.")?;
        let data =
            check_integrity(&self.secret, answer).ok_or("Unable to verify player details")?;
        let bad = |_| "Unable to verify player details";
        let mut r = Reader::new(data);
        let version = r.varint().map_err(bad)?;
        if !(1..=i32::from(MAX_VERSION)).contains(&version) {
            return Err("Unsupported forwarding version");
        }
        let address = r
            .string(i16::MAX as usize)
            .map_err(bad)?
            .parse()
            .map_err(|_| "Unable to verify player details")?;
        let uuid = r.uuid().map_err(bad)?;
        let name = r.string(16).map_err(bad)?;
        let count = r.varint().map_err(bad)?;
        let mut properties = Vec::new();
        for _ in 0..count.max(0) {
            let name = r.string(i16::MAX as usize).map_err(bad)?;
            let value = r.string(i16::MAX as usize).map_err(bad)?;
            let signature = if r.bool().map_err(bad)? {
                Some(r.string(i16::MAX as usize).map_err(bad)?)
            } else {
                None
            };
            properties.push(login::Property {
                name,
                value,
                signature,
            });
        }
        Ok(Forwarded {
            address,
            uuid,
            name,
            properties,
        })
    }
}

/// `VelocityProxy.checkIntegrity`: the data after a valid signature.
fn check_integrity<'a>(secret: &[u8], answer: &'a [u8]) -> Option<&'a [u8]> {
    if answer.len() < 32 {
        return None;
    }
    let (signature, data) = answer.split_at(32);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).ok()?;
    mac.update(data);
    mac.verify_slice(signature).ok().map(|()| data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol::buf::Writer;

    fn answer(secret: &[u8], version: i32) -> Vec<u8> {
        let mut w = Writer::new();
        w.varint(version);
        w.string("203.0.113.5");
        w.uuid(0x1234);
        w.string("Steve");
        w.varint(1);
        w.string("textures");
        w.string("e30=");
        w.bool(true);
        w.string("sig");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(&w.data);
        let mut out = mac.finalize().into_bytes().to_vec();
        out.extend_from_slice(&w.data);
        out
    }

    #[test]
    fn a_signed_answer_gives_the_player() {
        let v = Velocity {
            secret: b"secret".to_vec(),
        };
        let got = v.read(Some(&answer(b"secret", 4))).unwrap();
        assert_eq!(
            got.address,
            "203.0.113.5".parse::<std::net::IpAddr>().unwrap()
        );
        assert_eq!((got.uuid, got.name.as_str()), (0x1234, "Steve"));
        assert_eq!(got.properties[0].signature.as_deref(), Some("sig"));
    }

    #[test]
    fn unsigned_or_missing_answers_are_refused() {
        let v = Velocity {
            secret: b"secret".to_vec(),
        };
        assert_eq!(
            v.read(None),
            Err("This server requires you to connect with Velocity.")
        );
        assert_eq!(
            v.read(Some(&answer(b"other", 4))),
            Err("Unable to verify player details")
        );
        assert_eq!(
            v.read(Some(&answer(b"secret", 9))),
            Err("Unsupported forwarding version")
        );
    }

    #[test]
    fn the_query_asks_for_the_highest_version() {
        let login::Clientbound::CustomQuery(q) = Velocity::query(7) else {
            panic!()
        };
        assert_eq!(
            (q.transaction, q.channel.as_str(), q.data),
            (7, CHANNEL, vec![4])
        );
    }
}
