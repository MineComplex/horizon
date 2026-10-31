//! The user lists of a dedicated server: `whitelist.json`, `ops.json`, `banned-players.json` and
//! `banned-ips.json` (`StoredUserList` and its entries), and the login checks that read them
//! (`PlayerList.canPlayerLogin`, Paper's `isWhiteListedLogin` and `canBypassFullServerLogin`).
//!
//! The ban lists are `PlayerList` before MineComplex's cleanup (the sources under
//! `.gradle/caches/paperweight/taskCache/runLeafSetup`); the final tree no longer has them. Paper
//! and Spigot only, derived from sources (the 26.3 jar is vanilla): the lists' `ConcurrentHashMap`
//! (the save order, [`map`]), `removeExpired` before a save, the `.backup` of an unreadable file,
//! `messages.whitelist`, and `isWhiteListedLogin` letting operators past the whitelist. The
//! Bukkit events (`ProfileWhitelistVerifyEvent`, `PlayerServerFullCheckEvent`) have no listeners.
//!
//! Not ported: `OldUsersConverter` (the pre-1.7.6 `.txt` lists); the commands that edit the lists
//! (`/whitelist`, `/op`, `/ban`... use [`UserList::add`] and [`UserList::remove`], which save);
//! `enforce-whitelist` (it kicks players when the whitelist is reloaded); a file Gson would read
//! leniently (comments, single quotes) is unreadable here.

mod date;
mod entries;
mod list;
mod map;

#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod tests;

use std::net::IpAddr;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use jiff::tz::TimeZone;

pub use date::{format_date, format_login_date, parse_date};
pub use entries::{Ban, IpBan, NameAndId, OpEntry, UserBan, WhitelistEntry, ip_key, java_uuid};
pub use list::{Entry, Field, LoadFailure, UserList};

use crate::azalea::chat::FormattedText;
use crate::azalea::chat::translatable_component::{PrimitiveOrComponent, TranslatableComponent};
use crate::network::Reason;
use crate::server::server_properties::PermissionLevel;

pub const USER_BAN_LIST_FILE: &str = "banned-players.json";
pub const IP_BAN_LIST_FILE: &str = "banned-ips.json";
pub const OP_LIST_FILE: &str = "ops.json";
pub const WHITELIST_FILE: &str = "whitelist.json";

/// The wall clock as `new Date()` reads it: milliseconds since the epoch.
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// What the login checks read besides the lists.
#[derive(Clone, Copy, Debug)]
pub struct LoginRules<'a> {
    /// `white-list` (`isUsingWhitelist`).
    pub using_whitelist: bool,
    /// Spigot's `messages.whitelist`, as `SpigotConfig` transformed it.
    pub whitelist_message: &'a str,
    pub max_players: i32,
    /// `players.size()`.
    pub online_players: usize,
}

/// The four lists of `PlayerList`.
pub struct UserLists {
    pub whitelist: UserList<WhitelistEntry>,
    pub ops: UserList<OpEntry>,
    pub bans: UserList<UserBan>,
    pub ip_bans: UserList<IpBan>,
    /// The JVM's default time zone, for the kick message's expiry date.
    zone: TimeZone,
}

impl UserLists {
    /// The lists of the server folder `dir`, empty, with the files not read yet.
    pub fn new(dir: &Path, zone: TimeZone) -> Self {
        Self {
            whitelist: UserList::new(dir.join(WHITELIST_FILE), zone.clone()),
            ops: UserList::new(dir.join(OP_LIST_FILE), zone.clone()),
            bans: UserList::new(dir.join(USER_BAN_LIST_FILE), zone.clone()),
            ip_bans: UserList::new(dir.join(IP_BAN_LIST_FILE), zone.clone()),
            zone,
        }
    }

    /// `DedicatedPlayerList.loadAndSaveFiles`: read the bans (and write them back, which makes the
    /// files), the operators and the whitelist; write the operators back and the whitelist when
    /// it had no file. A list that cannot be read is logged and left as far as it got.
    pub fn load_and_save(dir: &Path, zone: TimeZone, now: i64) -> Self {
        let mut lists = Self::new(dir, zone);
        fn done<T, E: std::fmt::Display>(what: &str, result: Result<T, E>) {
            if let Err(e) = result {
                tracing::warn!("{what}: {e}");
            }
        }
        done("Failed to load user banlist: ", lists.bans.load(now));
        done("Failed to save user banlist: ", lists.bans.save(now));
        done("Failed to load ip banlist: ", lists.ip_bans.load(now));
        done("Failed to save ip banlist: ", lists.ip_bans.save(now));
        done("Failed to load operators list: ", lists.ops.load(now));
        done("Failed to load white-list: ", lists.whitelist.load(now));
        done("Failed to save operators list: ", lists.ops.save(now));
        if !lists.whitelist.file().exists() {
            done("Failed to save white-list: ", lists.whitelist.save(now));
        }
        lists
    }

    /// `PlayerList.isOp` (there is no single-player owner on a dedicated server).
    pub fn is_op(&mut self, user: &NameAndId, now: i64) -> bool {
        self.ops.contains(user, now)
    }

    /// `ServerOpList.canBypassPlayerLimit`.
    pub fn can_bypass_player_limit(&mut self, user: &NameAndId, now: i64) -> bool {
        self.ops
            .get(user, now)
            .is_some_and(|entry| entry.bypasses_player_limit)
    }

    /// `MinecraftServer.getProfilePermissions`: an operator's own level (`op-permission-level` when
    /// the entry vanished in between), else ALL.
    pub fn profile_permissions(
        &mut self,
        user: &NameAndId,
        operator_level: PermissionLevel,
        now: i64,
    ) -> PermissionLevel {
        if !self.is_op(user, now) {
            return PermissionLevel::All;
        }
        self.ops.get(user, now).map_or(operator_level, |e| e.level)
    }

    /// `PlayerList.canPlayerLogin(address, user)`: why this login is refused, if it is. In order: a
    /// player ban, the whitelist (operators pass), an IP ban, a full server (operators with
    /// `bypassesPlayerLimit` pass). `address` is the connection's, or what Velocity forwarded.
    pub fn can_player_login(
        &mut self,
        user: &NameAndId,
        address: IpAddr,
        rules: &LoginRules<'_>,
        now: i64,
    ) -> Option<Reason> {
        // `isBanned(user) && (ban = get(user)) != null`.
        let banned = self.bans.contains(user, now);
        if let Some(ban) = self.bans.get(user, now).filter(|_| banned) {
            return Some(ban_message(
                &self.zone,
                "multiplayer.disconnect.banned.reason",
                "multiplayer.disconnect.banned.expiration",
                &ban.ban,
            ));
        }
        // Paper's `isWhiteListedLogin`.
        let is_op = self.is_op(user, now);
        if rules.using_whitelist && !is_op && !self.whitelist.contains(user, now) {
            return Some(Reason::Literal(rules.whitelist_message.to_owned()));
        }
        let ip = ip_key(address);
        let banned = self.ip_bans.contains(&ip, now);
        if let Some(ban) = self.ip_bans.get(&ip, now).filter(|_| banned) {
            return Some(ban_message(
                &self.zone,
                "multiplayer.disconnect.banned_ip.reason",
                "multiplayer.disconnect.banned_ip.expiration",
                &ban.ban,
            ));
        }
        // `canBypassFullServerLogin`.
        let full =
            i64::try_from(rules.online_players).unwrap_or(i64::MAX) >= rules.max_players.into();
        (full && !self.can_bypass_player_limit(user, now))
            .then(|| Reason::translatable("multiplayer.disconnect.server_full"))
    }
}

/// The kick of a ban: `translatable(reason_key, reason message)`, with the expiry appended as a
/// sibling when the ban runs out (`BAN_DATE_FORMAT`, in the JVM's time zone).
fn ban_message(zone: &TimeZone, reason_key: &str, expiration_key: &str, ban: &Ban) -> Reason {
    {
        // `getReasonMessage`: the reason as a literal, or the default's translation.
        let reason = match &ban.reason {
            Some(text) => FormattedText::from(text.as_str()),
            None => TranslatableComponent::new(
                "multiplayer.disconnect.banned.reason.default".into(),
                Vec::new(),
            )
            .into(),
        };
        let mut message = TranslatableComponent::new(
            reason_key.to_owned(),
            vec![PrimitiveOrComponent::FormattedText(reason)],
        );
        if let Some(expires) = ban.expires {
            message.base.siblings.push(
                TranslatableComponent::new(
                    expiration_key.to_owned(),
                    vec![PrimitiveOrComponent::String(format_login_date(
                        expires, zone,
                    ))],
                )
                .into(),
            );
        }
        Reason::component(&message.into())
    }
}
