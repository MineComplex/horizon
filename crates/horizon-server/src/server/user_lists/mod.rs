//! The user lists of a dedicated server: `whitelist.json` and `ops.json` (`StoredUserList` and its
//! entries), and the login check that reads them (`PlayerList.canPlayerLogin` as MineComplex's Leaf
//! has it: Paper's `isWhiteListedLogin`, then `canBypassFullServerLogin`).
//!
//! MineComplex's cleanup (patch 0318) removed the ban lists (`banned-players.json`,
//! `banned-ips.json`) and the `.txt` list converter; nobody is refused for a ban. Paper and Spigot
//! only, derived from sources (the 26.3 jar is vanilla): the lists' `ConcurrentHashMap` (the save
//! order, [`map`]), the `.backup` of an unreadable file, `messages.whitelist` and the whitelist letting
//! operators past. The Bukkit events (`ProfileWhitelistVerifyEvent`, `PlayerServerFullCheckEvent`)
//! have no listeners.
//!
//! Not ported: the commands that edit the lists (`/whitelist`, `/op`... use [`UserList::add`] and
//! [`UserList::remove`], which save); `enforce-whitelist` (it kicks players when the whitelist is
//! reloaded); a file Gson would read leniently (comments, single quotes) is unreadable here.

mod entries;
mod list;
mod map;

#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod tests;

use std::path::Path;

pub use entries::{NameAndId, OpEntry, WhitelistEntry, java_uuid};
pub use list::{Entry, Field, LoadFailure, UserList};

use crate::network::Reason;
use crate::server::legacy_text;
use crate::server::server_properties::PermissionLevel;

pub const OP_LIST_FILE: &str = "ops.json";
pub const WHITELIST_FILE: &str = "whitelist.json";

/// What the login check reads besides the lists.
#[derive(Clone, Copy, Debug)]
pub struct LoginRules<'a> {
    /// `white-list` (`isUsingWhitelist`).
    pub using_whitelist: bool,
    /// Spigot's `messages.whitelist`, as `SpigotConfig` transformed it (`&` codes already `§`).
    pub whitelist_message: &'a str,
    pub max_players: i32,
    /// `players.size()`.
    pub online_players: usize,
}

/// The lists of `PlayerList`.
pub struct UserLists {
    pub whitelist: UserList<WhitelistEntry>,
    pub ops: UserList<OpEntry>,
}

impl UserLists {
    /// The lists of the server folder `dir`, empty, with the files not read yet.
    pub fn new(dir: &Path) -> Self {
        Self {
            whitelist: UserList::new(dir.join(WHITELIST_FILE)),
            ops: UserList::new(dir.join(OP_LIST_FILE)),
        }
    }

    /// `DedicatedPlayerList.loadAndSaveFiles`: read the operators and the whitelist, write the
    /// operators back, and the whitelist when it had no file. A list that cannot be read is logged
    /// and left as far as it got.
    pub fn load_and_save(dir: &Path) -> Self {
        let mut lists = Self::new(dir);
        fn done<E: std::fmt::Display>(what: &str, result: Result<(), E>) {
            if let Err(e) = result {
                tracing::warn!("{what}: {e}");
            }
        }
        done("Failed to load operators list: ", lists.ops.load());
        done("Failed to load white-list: ", lists.whitelist.load());
        done("Failed to save operators list: ", lists.ops.save());
        if !lists.whitelist.file().exists() {
            done("Failed to save white-list: ", lists.whitelist.save());
        }
        lists
    }

    /// `PlayerList.isOp` (there is no single-player owner on a dedicated server).
    pub fn is_op(&self, user: &NameAndId) -> bool {
        self.ops.contains(user)
    }

    /// `ServerOpList.canBypassPlayerLimit`.
    pub fn can_bypass_player_limit(&self, user: &NameAndId) -> bool {
        self.ops
            .get(user)
            .is_some_and(|entry| entry.bypasses_player_limit)
    }

    /// `MinecraftServer.getProfilePermissions`: an operator's own level, else ALL.
    pub fn profile_permissions(&self, user: &NameAndId) -> PermissionLevel {
        self.ops.get(user).map_or(PermissionLevel::All, |e| e.level)
    }

    /// `PlayerList.canPlayerLogin`: why this login is refused, if it is. First the whitelist
    /// (operators pass), then a full server (operators with `bypassesPlayerLimit` pass).
    ///
    /// `isWhiteListedLogin` deserializes `messages.whitelist` on every call, listed or not and
    /// whitelist on or off, so a message Adventure cannot read fails every login here (`Err`).
    pub fn can_player_login(
        &self,
        user: &NameAndId,
        rules: &LoginRules<'_>,
    ) -> Result<Option<Reason>, legacy_text::StringIndexOutOfBounds> {
        let refusal = whitelist_refusal(rules.whitelist_message)?;
        if rules.using_whitelist && !self.is_op(user) && !self.whitelist.contains(user) {
            return Ok(Some(refusal));
        }
        let full =
            i64::try_from(rules.online_players).unwrap_or(i64::MAX) >= rules.max_players.into();
        Ok((full && !self.can_bypass_player_limit(user))
            .then(|| Reason::translatable("multiplayer.disconnect.server_full")))
    }
}

/// The kick of `isWhiteListedLogin`: the message as `legacySection().deserialize` makes it. A text
/// without `§` is a plain literal, which vanilla writes as a bare string.
fn whitelist_refusal(message: &str) -> Result<Reason, legacy_text::StringIndexOutOfBounds> {
    let component = legacy_text::deserialize(message)?;
    Ok(if message.contains('§') {
        Reason::component(&component)
    } else {
        Reason::Literal(message.to_owned())
    })
}
