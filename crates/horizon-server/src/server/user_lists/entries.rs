//! The two kinds of entry: `UserWhiteListEntry` and `ServerOpListEntry`.

use serde_json::{Map, Value};
use uuid::Uuid;

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
    fn user(&self) -> &NameAndId {
        &self.user
    }

    fn read(object: &Map<String, Value>) -> Result<Option<Self>, LoadFailure> {
        Ok(NameAndId::from_json(object)?.map(|user| Self { user }))
    }

    fn fields(&self) -> Vec<(&'static str, Field)> {
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
    fn user(&self) -> &NameAndId {
        &self.user
    }

    fn read(object: &Map<String, Value>) -> Result<Option<Self>, LoadFailure> {
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

    fn fields(&self) -> Vec<(&'static str, Field)> {
        let mut fields = self.user.fields();
        fields.push(("level", Field::Int(self.level.id())));
        fields.push((
            "bypassesPlayerLimit",
            Field::Bool(self.bypasses_player_limit),
        ));
        fields
    }
}
