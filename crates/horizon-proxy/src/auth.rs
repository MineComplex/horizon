//! Who may join (the fork's AuthHandler). The client sends its launcher token on the
//! [`TOKEN_CHANNEL`] login channel once the connection is encrypted; a handler turns the name and
//! token into a profile or a refusal.
use crate::{plugin::Fut, text};
use horizon_protocol::{
    packets::login::{GameProfile, Property},
    rsa::{RsaPublicKey, SignatureHash},
};
use horizon_world::nbt::Tag;
use std::{net::IpAddr, sync::Arc};

/// AuthHandler.TOKEN_CHANNEL.
pub const TOKEN_CHANNEL: &str = "minecomplex:auth";
/// The longest token the proxy reads.
pub const MAX_TOKEN_BYTES: usize = 4096;

pub enum AuthResult {
    Allowed(GameProfile),
    Denied(Tag),
}

/// AuthHandler: may wait on the network.
pub trait AuthHandler: Send + Sync {
    fn authenticate<'a>(
        &'a self,
        address: IpAddr,
        username: &'a str,
        token: Option<&'a str>,
    ) -> Fut<'a, AuthResult>;
}

/// "Сервер авторизации временно недоступен" (InitialLoginSessionHandler.AUTH_UNAVAILABLE).
pub fn unavailable() -> Tag {
    text::legacy(&format!(
        "{}Сервер авторизации временно недоступен, попробуйте позже.",
        text::ERROR
    ))
}

/// LauncherAuthHandler.deny: the network's error prefix and the reason.
fn deny(reason: &str) -> AuthResult {
    AuthResult::Denied(text::legacy(&format!("{}{reason}", text::ERROR)))
}

const RELAUNCH: &str = "Перезапустите игру из лаунчера.";

/// An account of the network: its id, name and profile properties (the skin).
pub type Account = (u128, String, Vec<Property>);

/// The account behind a launcher session (AccountManager.checkLogin of Core); `None` when the
/// session ended.
pub trait Accounts: Send + Sync {
    fn check_login(&self, account: u128, session: u128)
    -> Fut<'_, Result<Option<Account>, String>>;
}

/// LauncherAuthHandler of the Network plugin.
pub struct LauncherAuth {
    pub token: Option<Arc<LauncherToken>>,
    /// Core's account check; without it the account id comes from the token and the name from
    /// the client.
    pub accounts: Option<Arc<dyn Accounts>>,
}
impl LauncherAuth {
    async fn check(&self, username: &str, token: Option<&str>) -> AuthResult {
        let Some(token) = token else {
            return deny(
                "Вход на сервер возможен только через лаунчер MineComplex. Запустите игру из лаунчера.",
            );
        };
        let Some(verifier) = &self.token else {
            return deny("Сервер авторизации временно недоступен, попробуйте позже.");
        };
        let Some(claims) = verifier.verify(token, unix_now()) else {
            return deny(&format!(
                "Сессия лаунчера недействительна или истекла. {RELAUNCH}"
            ));
        };
        let (uuid, name, properties) = match &self.accounts {
            None => (claims.account, username.to_owned(), Vec::new()),
            Some(accounts) => match accounts.check_login(claims.account, claims.session).await {
                Ok(Some(account)) => account,
                Ok(None) => return deny("Сессия лаунчера завершена. Войдите в лаунчер заново."),
                Err(_) => return AuthResult::Denied(unavailable()),
            },
        };
        if !name.eq_ignore_ascii_case(username) {
            return deny(&format!(
                "Ник не совпадает с аккаунтом лаунчера. {RELAUNCH}"
            ));
        }
        AuthResult::Allowed(GameProfile {
            uuid,
            name,
            properties,
        })
    }
}
impl AuthHandler for LauncherAuth {
    fn authenticate<'a>(
        &'a self,
        _: IpAddr,
        username: &'a str,
        token: Option<&'a str>,
    ) -> Fut<'a, AuthResult> {
        Box::pin(self.check(username, token))
    }
}

/// `auth.mode = "offline"`: offline UUIDs for any name.
pub struct OfflineAuth;
impl AuthHandler for OfflineAuth {
    fn authenticate<'a>(
        &'a self,
        _: IpAddr,
        username: &'a str,
        _: Option<&'a str>,
    ) -> Fut<'a, AuthResult> {
        crate::plugin::done(AuthResult::Allowed(GameProfile {
            uuid: horizon_protocol::crypto::offline_uuid(username),
            name: username.to_owned(),
            properties: Vec::new(),
        }))
    }
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// LauncherToken: an RS256 JWT from the launcher's backend.
pub struct LauncherToken {
    key: RsaPublicKey,
    issuer: String,
    ttl_seconds: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claims {
    pub account: u128,
    pub session: u128,
}
/// Clock skew allowed on `iat`.
const SKEW: i64 = 60;

impl LauncherToken {
    /// parseKey: base64 X.509, with or without PEM armour.
    pub fn new(encoded: &str, issuer: &str, ttl_minutes: u64) -> Result<Self, String> {
        let base64: String = encoded
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .flat_map(|l| l.chars())
            .filter(|c| !c.is_whitespace())
            .collect();
        let der = base64_decode(&base64).ok_or("the key is not base64")?;
        let key = RsaPublicKey::from_der(&der).map_err(|e| e.to_string())?;
        Ok(Self {
            key,
            issuer: issuer.to_owned(),
            ttl_seconds: ttl_minutes as i64 * 60,
        })
    }

    /// The claims of a valid token at `now` (seconds); `None` for anything else.
    pub fn verify(&self, token: &str, now: i64) -> Option<Claims> {
        if token.len() > MAX_TOKEN_BYTES {
            return None;
        }
        let parts: Vec<&str> = token.split('.').collect();
        let [header, payload, signature] = parts[..] else {
            return None;
        };
        let json = |part: &str| -> Option<serde_json::Value> {
            serde_json::from_slice(&base64_decode(part)?).ok()
        };
        // Only RS256: a token signed otherwise (HS256, "none") is never trusted.
        if json(header)?.get("alg")?.as_str()? != "RS256" {
            return None;
        }
        let signed = format!("{header}.{payload}");
        if !self.key.verify(
            SignatureHash::Sha256,
            signed.as_bytes(),
            &base64_decode(signature)?,
        ) {
            return None;
        }
        let claims = json(payload)?;
        let string = |name: &str| claims.get(name).and_then(|v| v.as_str());
        if string("iss")? != self.issuer {
            return None;
        }
        let issued = claims.get("iat")?.as_i64()?;
        if issued > now + SKEW || issued + self.ttl_seconds <= now {
            return None;
        }
        Some(Claims {
            account: parse_uuid(string("sub")?)?,
            session: parse_uuid(string("sid")?)?,
        })
    }
}

/// UUID.fromString for the canonical dashed form.
pub fn parse_uuid(text: &str) -> Option<u128> {
    let groups: Vec<&str> = text.split('-').collect();
    if groups.iter().map(|g| g.len()).collect::<Vec<_>>() != [8, 4, 4, 4, 12] {
        return None;
    }
    u128::from_str_radix(&groups.concat(), 16).ok()
}

/// Base64 with the standard alphabet and padding (Base64.getEncoder).
pub fn base64_encode(data: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// Base64, standard or URL-safe alphabet, padding optional.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = acc << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    (bits < 6).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made with openssl: a 1024-bit key and RS256 tokens issued at 1_800_000_000.
    const KEY: &str = include_str!("../tests/data/launcher_key.pub");
    const TOKEN: &str = include_str!("../tests/data/token.jwt");
    const WRONG_ISSUER: &str = include_str!("../tests/data/token_wrong_issuer.jwt");
    const IAT: i64 = 1_800_000_000;

    fn verifier() -> LauncherToken {
        LauncherToken::new(KEY, "minecomplex", 720).unwrap()
    }

    #[test]
    fn valid_token() {
        let claims = verifier().verify(TOKEN.trim(), IAT + 10).unwrap();
        assert_eq!(claims.account, 0x11111111_2222_3333_4444_555555555555);
        assert_eq!(claims.session, 0xaaaaaaaa_bbbb_cccc_dddd_eeeeeeeeeeee);
    }

    #[test]
    fn expired_early_forged_or_foreign_tokens_fail() {
        let v = verifier();
        let token = TOKEN.trim();
        assert!(v.verify(token, IAT + 720 * 60).is_none());
        assert!(v.verify(token, IAT - SKEW - 1).is_none());
        assert!(v.verify(WRONG_ISSUER.trim(), IAT).is_none());
        // Another payload under the same signature.
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged = "eyJpc3MiOiJtaW5lY29tcGxleCJ9";
        parts[1] = forged;
        assert!(v.verify(&parts.join("."), IAT).is_none());
        // alg none.
        assert!(v.verify("eyJhbGciOiJub25lIn0.e30.", IAT).is_none());
    }

    #[test]
    fn launcher_handler_messages() {
        let auth = LauncherAuth {
            token: Some(Arc::new(verifier())),
            accounts: None,
        };
        let ip = IpAddr::from([127, 0, 0, 1]);
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let denied = |r: AuthResult| match r {
            AuthResult::Denied(reason) => text::plain(&reason),
            AuthResult::Allowed(_) => panic!("allowed"),
        };
        let none = denied(rt.block_on(auth.authenticate(ip, "Steve", None)));
        assert!(none.contains("только через лаунчер"), "{none}");
        let bad = denied(rt.block_on(auth.authenticate(ip, "Steve", Some("x.y.z"))));
        assert!(bad.contains("недействительна"), "{bad}");
    }

    #[test]
    fn uuids_and_base64() {
        assert_eq!(parse_uuid("00000000-0000-0000-0000-00000000000f"), Some(15));
        assert_eq!(parse_uuid("0000000000000000000000000000000f"), None);
        assert_eq!(base64_decode("aGk"), Some(b"hi".to_vec()));
        assert_eq!(base64_decode("aGk="), Some(b"hi".to_vec()));
        assert_eq!(base64_decode("-_8"), Some(vec![0xfb, 0xff]));
        assert_eq!(base64_decode("a"), None);
        assert_eq!(base64_decode("a$"), None);
        assert_eq!(base64_encode(b"hi"), "aGk=");
        assert_eq!(
            base64_decode(&base64_encode(b"horizon")).unwrap(),
            b"horizon"
        );
    }
}
