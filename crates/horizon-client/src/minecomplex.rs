//! MineComplex network: login with the launcher's token and website skins
//! from the CDN.
//!
//! The launcher passes its access token (a JWT) in `MINECOMPLEX_GAME_TOKEN`
//! or `--accessToken`, never the password or the refresh token. There is no
//! session server: once the connection is encrypted, the proxy asks for the
//! token on the [`AUTH_CHANNEL`] login channel and checks it itself.
//!
//! Skins are uploaded on the website and stored in the CDN. The proxy puts
//! them in the unsigned `textures` property, while vanilla only trusts
//! signed textures from Mojang's domains. Textures that all lie in the CDN
//! are trusted like signed ones; anything else is handled as before.
use crate::skins::{Textures, base64_decode};

/// Environment variable with the launcher's access token.
pub const GAME_TOKEN_ENV: &str = "MINECOMPLEX_GAME_TOKEN";
/// Login channel of the proxy's token request (AuthHandler.TOKEN_CHANNEL).
pub const AUTH_CHANNEL: &str = "minecomplex:auth";
/// Host of the CDN, the storage mirror of Rest.
pub const CDN_HOST: &str = "cdn.minecomplex.net";
const MAX_TOKEN_LENGTH: usize = 4096;

/// A compact JWS: `header.payload.signature` in base64url.
pub fn is_token(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    let base64url = |p: &str| {
        p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    value.len() <= MAX_TOKEN_LENGTH
        && parts.len() == 3
        && parts[0].len() >= 8
        && parts[1].len() >= 8
        && parts[2].len() >= 16
        && parts.iter().all(|p| base64url(p))
}

/// The launcher's token: the environment first, then `--accessToken`.
pub fn game_token(argument: Option<&str>) -> Option<String> {
    let env = std::env::var(GAME_TOKEN_ENV).ok();
    [env.as_deref(), argument]
        .into_iter()
        .flatten()
        .find(|t| is_token(t))
        .map(str::to_owned)
}

/// A player name the proxy accepts.
pub fn is_player_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The answer to a login custom query: the token as UTF-8 bytes, only on the
/// proxy's channel and only once the connection is encrypted. `None` is an
/// answer without a body (the game was not started by the launcher).
pub fn token_answer(channel: &str, encrypted: bool, token: Option<&str>) -> Option<Vec<u8>> {
    (channel == AUTH_CHANNEL && encrypted)
        .then_some(token)
        .flatten()
        .filter(|t| is_token(t))
        .map(|t| t.as_bytes().to_vec())
}

/// `https://cdn.minecomplex.net/objects/<2 hex>/<sha256>`: content-addressed
/// objects (ObjectStore of Rest), no port, user, query or fragment.
pub fn is_trusted_texture(url: &str) -> bool {
    let Some(path) = url
        .strip_prefix("https://")
        .and_then(|rest| rest.strip_prefix(CDN_HOST))
        .and_then(|rest| rest.strip_prefix("/objects/"))
    else {
        return false;
    };
    let hex = |s: &str, n: usize| {
        s.len() == n
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    matches!(path.split_once('/'), Some((dir, hash)) if hex(dir, 2) && hex(hash, 64))
}

/// The textures of a `textures` property if every texture lies in the CDN,
/// trusted as signed; `None` leaves the property to the usual check.
pub fn unpack_cdn(value: &str) -> Option<Textures> {
    let json: serde_json::Value = base64_decode(value)
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| serde_json::from_str(&s).ok())?;
    let textures = json.get("textures")?.as_object()?;
    let mut out = Textures {
        signed: true,
        ..Textures::default()
    };
    for (kind, texture) in textures {
        let url = texture.get("url")?.as_str()?;
        if !is_trusted_texture(url) {
            return None;
        }
        let slot = match kind.as_str() {
            "SKIN" => &mut out.skin,
            "CAPE" => &mut out.cape,
            "ELYTRA" => &mut out.elytra,
            _ => return None,
        };
        *slot = Some(url.to_owned());
    }
    (!textures.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "eyJhbGciOiJFUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2lnbmF0dXJlLXNpZ25hdHVyZQ";
    const SKIN: &str = "https://cdn.minecomplex.net/objects/ab/abababababababababababababababababababababababababababababababab";

    fn encode(json: &str) -> String {
        const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in json.as_bytes().chunks(3) {
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

    #[test]
    fn tokens() {
        assert!(is_token(TOKEN));
        assert!(!is_token(""));
        assert!(!is_token("a.b.c"));
        assert!(!is_token(&format!("{TOKEN}.x")));
        assert!(!is_token(&TOKEN.replace('c', "+")));
        assert!(!is_token(&format!("{}{}", TOKEN, "a".repeat(4096))));
    }

    #[test]
    fn token_only_on_encrypted_auth_channel() {
        let bytes = Some(TOKEN.as_bytes().to_vec());
        assert_eq!(token_answer(AUTH_CHANNEL, true, Some(TOKEN)), bytes);
        assert_eq!(token_answer(AUTH_CHANNEL, false, Some(TOKEN)), None);
        assert_eq!(
            token_answer("velocity:player_info", true, Some(TOKEN)),
            None
        );
        assert_eq!(token_answer(AUTH_CHANNEL, true, None), None);
        assert_eq!(token_answer(AUTH_CHANNEL, true, Some("bad")), None);
    }

    #[test]
    fn names() {
        assert!(is_player_name("Steve_01"));
        assert!(!is_player_name(""));
        assert!(!is_player_name("x".repeat(17).as_str()));
        assert!(!is_player_name("Стив"));
    }

    #[test]
    fn trusted_textures() {
        assert!(is_trusted_texture(SKIN));
        for url in [
            &SKIN.replace("https", "http"),
            &SKIN.replace(".net/", ".net:443/"),
            &SKIN.replace("cdn.", "evil.cdn."),
            &format!("{SKIN}?x=1"),
            &format!("{SKIN}#x"),
            &SKIN.replace("/ab/", "/AB/"),
            &SKIN[..SKIN.len() - 1],
            "https://textures.minecraft.net/texture/abc",
        ] {
            assert!(!is_trusted_texture(url), "{url}");
        }
    }

    #[test]
    fn cdn_textures_are_trusted_as_signed() {
        let value = encode(&format!(
            r#"{{"textures":{{"SKIN":{{"url":"{SKIN}","metadata":{{"model":"slim"}}}}}}}}"#
        ));
        let t = unpack_cdn(&value).unwrap();
        assert_eq!(t.skin.as_deref(), Some(SKIN));
        assert!(t.signed);
        // One texture elsewhere leaves the whole property to the usual check.
        let mixed = encode(&format!(
            r#"{{"textures":{{"SKIN":{{"url":"{SKIN}"}},"CAPE":{{"url":"https://textures.minecraft.net/texture/c"}}}}}}"#
        ));
        assert_eq!(unpack_cdn(&mixed), None);
        assert_eq!(unpack_cdn(&encode(r#"{"textures":{}}"#)), None);
        assert_eq!(unpack_cdn("!!"), None);
    }
}
