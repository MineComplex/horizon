//! The account the MineComplex launcher started the game with: its name, UUID
//! and access token, which the proxy asks for at login and checks itself
//! (crate::minecomplex). The game never talks to Mojang's or Microsoft's
//! services. The access token stays in this process's memory: it is never
//! logged, printed or written anywhere.
use std::sync::{Arc, OnceLock};

/// A signed-in account.
pub struct AuthSession {
    pub name: String,
    pub uuid: u128,
    access_token: String,
}
impl std::fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthSession")
            .field("name", &self.name)
            .field("uuid", &horizon_protocol::crypto::uuid_string(self.uuid))
            .field("access_token", &"<hidden>")
            .finish()
    }
}

/// A UUID with or without dashes.
pub fn parse_uuid(text: &str) -> Option<u128> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    (hex.len() == 32)
        .then(|| u128::from_str_radix(&hex, 16).ok())
        .flatten()
}

impl AuthSession {
    /// The MineComplex launcher's identity and access token.
    pub fn minecomplex(name: &str, uuid: u128, access_token: &str) -> Self {
        Self {
            name: name.to_owned(),
            uuid,
            access_token: access_token.to_owned(),
        }
    }
    /// The answer to the proxy's token request (crate::minecomplex::token_answer).
    pub fn minecomplex_answer(&self, channel: &str, encrypted: bool) -> Option<Vec<u8>> {
        crate::minecomplex::token_answer(channel, encrypted, Some(self.access_token.as_str()))
    }
}

static CURRENT: OnceLock<Arc<AuthSession>> = OnceLock::new();

/// The account this game was started with (set once at start).
pub fn set_current(session: AuthSession) {
    let _ = CURRENT.set(Arc::new(session));
}
pub fn current() -> Option<Arc<AuthSession>> {
    CURRENT.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_stays_hidden() {
        let s = AuthSession::minecomplex("Notch", 0x069a79f444e94726a5befca90e38aaf5, "mock-token");
        assert!(!format!("{s:?}").contains("mock-token"));
        assert_eq!(
            parse_uuid("069a79f4-44e9-4726-a5be-fca90e38aaf5"),
            Some(0x069a79f444e94726a5befca90e38aaf5)
        );
        assert_eq!(parse_uuid("nope"), None);
    }
}
