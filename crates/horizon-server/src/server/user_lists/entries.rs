//! The four kinds of entry: `UserWhiteListEntry`, `ServerOpListEntry`, `UserBanListEntry` and
//! `IpBanListEntry` (the ban lists as `BanListEntry` and Spigot's `parseNameAndId` have them).

use std::net::IpAddr;

use jiff::tz::TimeZone;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::date::{format_date, parse_date};
use super::list::{Entry, Field, LoadFailure, as_bool, as_int, as_string};
use crate::network::login::GameProfile;
use crate::server::server_properties::PermissionLevel;

/// `NameAndId`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameAndId {
    pub id: Uuid,
    pub name: String,
}

impl NameAndId {
    /// `NameAndId.createOffline`.
    pub fn offline(name: &str) -> Self {
        Self {
            id: crate::azalea::crypto::offline::generate_uuid(name),
            name: name.to_owned(),
        }
    }

    /// `NameAndId.fromJson`: `None` without a `uuid` and a `name`, or with a `uuid` that
    /// `UUID.fromString` rejects.
    fn from_json(object: &Map<String, Value>) -> Result<Option<Self>, LoadFailure> {
        let (Some(uuid), Some(name)) = (as_string(object, "uuid")?, as_string(object, "name")?)
        else {
            return Ok(None);
        };
        Ok(java_uuid(&uuid).map(|id| Self { id, name }))
    }

    fn fields(&self) -> Vec<(&'static str, Field)> {
        vec![
            ("uuid", Field::Str(self.id.to_string())),
            ("name", Field::Str(self.name.clone())),
        ]
    }
}

impl From<&GameProfile> for NameAndId {
    fn from(profile: &GameProfile) -> Self {
        Self {
            id: profile.id,
            name: profile.name.clone(),
        }
    }
}

/// `UUID.fromString`: up to 36 characters in five dash separated hex numbers (`Long.parseLong`
/// each, so `1-2-3-4-5` and `+69a79f4-...` are UUIDs).
pub fn java_uuid(text: &str) -> Option<Uuid> {
    if text.len() > 36 {
        return None;
    }
    let parts: Vec<&str> = text.split('-').collect();
    let [a, b, c, d, e] = parts.as_slice() else {
        return None;
    };
    let hex = |s: &str| -> Option<u64> {
        let digits = s.strip_prefix('+').unwrap_or(s);
        u64::from_str_radix(digits, 16).ok()
    };
    let most = ((hex(a)? & 0xffff_ffff) << 32) | ((hex(b)? & 0xffff) << 16) | (hex(c)? & 0xffff);
    let least = ((hex(d)? & 0xffff) << 48) | (hex(e)? & 0xffff_ffff_ffff);
    Some(Uuid::from_u64_pair(most, least))
}

/// `UserWhiteListEntry`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhitelistEntry {
    pub user: NameAndId,
}

impl Entry for WhitelistEntry {
    type Lookup = NameAndId;

    fn lookup_key(user: &NameAndId) -> String {
        user.id.to_string()
    }

    fn key(&self) -> String {
        self.user.id.to_string()
    }

    fn read(object: &Map<String, Value>, _now: i64) -> Result<Option<Self>, LoadFailure> {
        Ok(NameAndId::from_json(object)?.map(|user| Self { user }))
    }

    fn fields(&self, _zone: &TimeZone) -> Vec<(&'static str, Field)> {
        self.user.fields()
    }
}

/// `ServerOpListEntry`: `permissions` is a `LevelBasedPermissionSet`, which is its level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpEntry {
    pub user: NameAndId,
    pub level: PermissionLevel,
    pub bypasses_player_limit: bool,
}

impl Entry for OpEntry {
    type Lookup = NameAndId;

    fn lookup_key(user: &NameAndId) -> String {
        user.id.to_string()
    }

    fn key(&self) -> String {
        self.user.id.to_string()
    }

    fn read(object: &Map<String, Value>, _now: i64) -> Result<Option<Self>, LoadFailure> {
        let Some(user) = NameAndId::from_json(object)? else {
            return Ok(None);
        };
        // `PermissionLevel.byId` clamps; a missing `level` is ALL.
        let level = as_int(object, "level")?.map_or(PermissionLevel::All, PermissionLevel::by_id);
        let bypasses_player_limit = as_bool(object, "bypassesPlayerLimit")?.unwrap_or(false);
        Ok(Some(Self {
            user,
            level,
            bypasses_player_limit,
        }))
    }

    fn fields(&self, _zone: &TimeZone) -> Vec<(&'static str, Field)> {
        let mut fields = self.user.fields();
        fields.push(("level", Field::Int(self.level.id())));
        fields.push((
            "bypassesPlayerLimit",
            Field::Bool(self.bypasses_player_limit),
        ));
        fields
    }
}

/// What `BanListEntry` adds to a user. Times are milliseconds since the epoch.
#[derive(Clone, Debug)]
pub struct Ban {
    pub created: i64,
    pub source: String,
    pub expires: Option<i64>,
    pub reason: Option<String>,
}

impl Ban {
    /// The constructor: no `source` is `(Unknown)`.
    pub fn new(
        created: i64,
        source: Option<String>,
        expires: Option<i64>,
        reason: Option<String>,
    ) -> Self {
        Self {
            created,
            source: source.unwrap_or_else(|| "(Unknown)".into()),
            expires,
            reason,
        }
    }

    /// `BanListEntry(user, object)` after `checkExpiry`: `Ok(None)` when the ban ran out already
    /// (the entry then has no user and is dropped).
    fn read(object: &Map<String, Value>, now: i64) -> Result<Option<Self>, LoadFailure> {
        let date = |key: &str| -> Result<Option<i64>, LoadFailure> {
            Ok(as_string(object, key)?.and_then(|text| parse_date(&text)))
        };
        let expires = date("expires")?;
        if expires.is_some_and(|e| e <= now) {
            return Ok(None);
        }
        Ok(Some(Self {
            created: date("created")?.unwrap_or(now),
            source: as_string(object, "source")?.unwrap_or_else(|| "(Unknown)".into()),
            expires,
            reason: as_string(object, "reason")?,
        }))
    }

    /// `BanListEntry.serialize`.
    fn fields(&self, zone: &TimeZone) -> Vec<(&'static str, Field)> {
        vec![
            ("created", Field::Str(format_date(self.created, zone))),
            ("source", Field::Str(self.source.clone())),
            (
                "expires",
                Field::Str(
                    self.expires
                        .map_or_else(|| "forever".into(), |e| format_date(e, zone)),
                ),
            ),
            (
                "reason",
                self.reason.clone().map_or(Field::Null, Field::Str),
            ),
        ]
    }

    /// `hasExpired`.
    pub fn has_expired(&self, now: i64) -> bool {
        self.expires.is_some_and(|e| e < now)
    }

    /// `BanListEntry.equals` (without the user).
    fn same_as(&self, other: &Self) -> bool {
        self.source == other.source && self.expires == other.expires && self.reason == other.reason
    }
}

/// `UserBanListEntry`. Spigot's `parseNameAndId` keeps an entry that has only one of `uuid` and
/// `name`; a missing name is not written back.
#[derive(Clone, Debug)]
pub struct UserBan {
    pub id: Uuid,
    pub name: Option<String>,
    pub ban: Ban,
}

impl UserBan {
    pub fn new(user: &NameAndId, ban: Ban) -> Self {
        Self {
            id: user.id,
            name: Some(user.name.clone()),
            ban,
        }
    }
}

impl Entry for UserBan {
    type Lookup = NameAndId;

    fn lookup_key(user: &NameAndId) -> String {
        user.id.to_string()
    }

    fn key(&self) -> String {
        self.id.to_string()
    }

    fn read(object: &Map<String, Value>, now: i64) -> Result<Option<Self>, LoadFailure> {
        let uuid = as_string(object, "uuid")?.and_then(|text| java_uuid(&text));
        let name = as_string(object, "name")?;
        let ban = Ban::read(object, now)?;
        match (uuid, name, ban) {
            // No user at all, or `checkExpiry` took it: `getUser()` is `null`.
            (None, None, _) | (_, _, None) => Ok(None),
            // `new NameAndId(null, name)`: `getKeyForUser` throws a `NullPointerException`.
            (None, Some(_), Some(_)) => Err(LoadFailure::Unreadable(
                "java.lang.NullPointerException: Cannot invoke \"java.util.UUID.toString()\" because the return value of \"net.minecraft.server.players.NameAndId.id()\" is null"
                    .into(),
            )),
            (Some(id), name, Some(ban)) => Ok(Some(Self { id, name, ban })),
        }
    }

    fn fields(&self, zone: &TimeZone) -> Vec<(&'static str, Field)> {
        let mut fields = vec![
            ("uuid", Field::Str(self.id.to_string())),
            ("name", self.name.clone().map_or(Field::Null, Field::Str)),
        ];
        fields.extend(self.ban.fields(zone));
        fields
    }

    fn has_expired(&self, now: i64) -> bool {
        self.ban.has_expired(now)
    }

    fn same_as(&self, other: &Self) -> bool {
        self.id == other.id && self.name == other.name && self.ban.same_as(&other.ban)
    }
}

/// `IpBanListEntry`: the user is the address text.
#[derive(Clone, Debug)]
pub struct IpBan {
    pub ip: String,
    pub ban: Ban,
}

impl Entry for IpBan {
    type Lookup = str;

    fn lookup_key(ip: &str) -> String {
        ip.to_owned()
    }

    fn key(&self) -> String {
        self.ip.clone()
    }

    fn read(object: &Map<String, Value>, now: i64) -> Result<Option<Self>, LoadFailure> {
        let ip = as_string(object, "ip")?;
        let ban = Ban::read(object, now)?;
        Ok(ip.zip(ban).map(|(ip, ban)| Self { ip, ban }))
    }

    fn fields(&self, zone: &TimeZone) -> Vec<(&'static str, Field)> {
        let mut fields = vec![("ip", Field::Str(self.ip.clone()))];
        fields.extend(self.ban.fields(zone));
        fields
    }

    fn has_expired(&self, now: i64) -> bool {
        self.ban.has_expired(now)
    }

    fn same_as(&self, other: &Self) -> bool {
        self.ip == other.ip && self.ban.same_as(&other.ban)
    }
}

/// `IpBanList.getIpFromAddress` for the address a connection has (`InetSocketAddress.toString()` is
/// `/host-literal:port`; the text after the `/` up to the first `:`). IPv6 literals are bracketed
/// there, so the first `:` comes after the first group: `[2001`. Kept as Java has it.
pub fn ip_key(address: IpAddr) -> String {
    match address {
        IpAddr::V4(v4) => v4.to_string(),
        // `InetAddress` makes an `Inet4Address` of an IPv4-mapped one.
        IpAddr::V6(v6) if v6.to_ipv4_mapped().is_some() => v6.to_ipv4_mapped().unwrap().to_string(),
        IpAddr::V6(v6) => {
            let text = format!("[{}]", java_ipv6(&v6));
            text.split(':').next().unwrap_or_default().to_owned()
        }
    }
}

/// `Inet6Address.getHostAddress`: eight groups of up to four lowercase hex digits, no `::`.
fn java_ipv6(address: &std::net::Ipv6Addr) -> String {
    address
        .segments()
        .iter()
        .map(|s| format!("{s:x}"))
        .collect::<Vec<_>>()
        .join(":")
}
