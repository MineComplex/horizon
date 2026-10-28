//! Player names. This server never looks accounts up at Mojang (the proxy
//! authenticates players): a name nobody joined with resolves offline.
use crate::user_lists::NameAndId;

/// StringUtil.isValidPlayerName: at most 16 UTF-16 units, none a control
/// character, a space or above ASCII.
pub fn valid_player_name(name: &str) -> bool {
    name.encode_utf16().count() <= 16 && name.encode_utf16().all(|c| c > 32 && c < 127)
}

/// How the server resolves a name it was given (CachedUserNameToIdResolver.get
/// without usercache.json and without the profile service): the offline profile.
pub fn resolve(name: &str) -> NameAndId {
    NameAndId::offline(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_player_name("Alex_123"));
        assert!(!valid_player_name("two words"));
        assert!(!valid_player_name("ThisNameIsTooLong"));
        assert!(!valid_player_name("caf\u{e9}"));
        assert_eq!(resolve("Alex"), NameAndId::offline("Alex"));
    }
}
