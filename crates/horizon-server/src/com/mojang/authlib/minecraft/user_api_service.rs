//! Port of `com.mojang.authlib.minecraft.UserApiService` as the server uses it: a handle over the
//! interface. The server only ever has `OFFLINE` (no Mojang accounts here, see the package doc).
use crate::prelude::*;

pub trait UserApiServiceTrait: JObject {
    fn properties(&self) -> UserApiServiceUserProperties;
    fn is_blocked_player(&self, player_id: Uuid) -> bool;
    fn refresh_block_list(&self);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserApiServiceUserProperties {
    pub flags: HashSet<UserApiServiceUserFlag>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UserApiServiceUserFlag {
    SERVERS_ALLOWED,
    REALMS_ALLOWED,
    CHAT_ALLOWED,
    TELEMETRY_ENABLED,
    PROFANITY_FILTER_ENABLED,
    OPTIONAL_TELEMETRY_AVAILABLE,
}

pub static OFFLINE_PROPERTIES: LazyLock<UserApiServiceUserProperties> = LazyLock::new(|| UserApiServiceUserProperties {
    flags: [
        UserApiServiceUserFlag::CHAT_ALLOWED,
        UserApiServiceUserFlag::REALMS_ALLOWED,
        UserApiServiceUserFlag::SERVERS_ALLOWED,
    ]
    .into_iter()
    .collect(),
});

struct OfflineUserApiService;
impl UserApiServiceTrait for OfflineUserApiService {
    fn properties(&self) -> UserApiServiceUserProperties {
        OFFLINE_PROPERTIES.clone()
    }
    fn is_blocked_player(&self, _player_id: Uuid) -> bool {
        false
    }
    fn refresh_block_list(&self) {}
}
crate::jobject!(OfflineUserApiService: dyn UserApiServiceTrait);

pub static OFFLINE: LazyLock<UserApiService> = LazyLock::new(|| UserApiService(Arc::new(OfflineUserApiService)));

#[derive(Clone)]
pub struct UserApiService(pub JRef<dyn UserApiServiceTrait>);
impl std::ops::Deref for UserApiService {
    type Target = dyn UserApiServiceTrait;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
