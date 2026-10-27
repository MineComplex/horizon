//! Velocity modern forwarding (PlayerDataForwarding): the backend asks on the
//! `velocity:player_info` login channel, the proxy answers with the player's address and
//! profile, signed with HMAC-SHA256 over the shared secret.
use hmac::{Hmac, Mac};
use horizon_protocol::{buf::Writer, packets::login::GameProfile};
use sha2::Sha256;

pub const CHANNEL: &str = "velocity:player_info";
pub const MODERN_DEFAULT: i32 = 1;
pub const MODERN_LAZY_SESSION: i32 = 4;
const MODERN_MAX_VERSION: i32 = MODERN_LAZY_SESSION;

/// The version a backend asks for: one byte in the query, else the default.
pub fn requested_version(query: &[u8]) -> i32 {
    match query {
        [v] => i32::from(*v as i8),
        _ => MODERN_DEFAULT,
    }
}

/// findForwardingVersion: the key-carrying versions only applied to 1.19-1.19.2.
fn forwarding_version(requested: i32) -> i32 {
    if requested.min(MODERN_MAX_VERSION) >= MODERN_LAZY_SESSION {
        MODERN_LAZY_SESSION
    } else {
        MODERN_DEFAULT
    }
}

/// createForwardingData: the signature, then the data it signs.
pub fn forwarding_data(
    secret: &[u8],
    address: &str,
    profile: &GameProfile,
    requested: i32,
) -> Vec<u8> {
    let mut w = Writer::new();
    w.varint(forwarding_version(requested));
    w.string(address);
    w.uuid(profile.uuid);
    w.string(&profile.name);
    // ProtocolUtils.writeProperties.
    w.varint(profile.properties.len() as i32);
    for p in &profile.properties {
        w.string(&p.name);
        w.string(&p.value);
        w.bool(p.signature.is_some());
        if let Some(s) = &p.signature {
            w.string(s);
        }
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(&w.data);
    let mut out = mac.finalize().into_bytes().to_vec();
    out.extend_from_slice(&w.data);
    out
}

/// What a backend does with the answer (Paper's VelocityProxy.checkIntegrity): the data after a
/// valid signature, or `None`.
pub fn check_integrity<'a>(secret: &[u8], answer: &'a [u8]) -> Option<&'a [u8]> {
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
    use horizon_protocol::{buf::Reader, packets::login::Property};

    #[test]
    fn signed_data_reads_back() {
        let profile = GameProfile {
            uuid: 0x1234,
            name: "Steve".into(),
            properties: vec![Property {
                name: "textures".into(),
                value: "e30=".into(),
                signature: None,
            }],
        };
        let answer = forwarding_data(b"secret", "203.0.113.5", &profile, requested_version(&[4]));
        assert!(check_integrity(b"other", &answer).is_none());
        let data = check_integrity(b"secret", &answer).unwrap();
        let mut r = Reader::new(data);
        assert_eq!(r.varint().unwrap(), MODERN_LAZY_SESSION);
        assert_eq!(r.string(255).unwrap(), "203.0.113.5");
        assert_eq!(r.uuid().unwrap(), 0x1234);
        assert_eq!(r.string(16).unwrap(), "Steve");
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string(64).unwrap(), "textures");
        assert_eq!(r.string(32767).unwrap(), "e30=");
        assert!(!r.bool().unwrap());
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn versions() {
        assert_eq!(requested_version(&[]), MODERN_DEFAULT);
        assert_eq!(forwarding_version(requested_version(&[2])), MODERN_DEFAULT);
        assert_eq!(
            forwarding_version(requested_version(&[9])),
            MODERN_LAZY_SESSION
        );
    }
}
