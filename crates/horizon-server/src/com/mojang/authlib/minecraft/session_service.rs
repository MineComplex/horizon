//! Port of `com.mojang.authlib.minecraft.SessionService` (an interface). Its form stays the skeleton's
//! value handle: `SessionService(JRef<dyn SessionServiceTrait>)`, built with `SessionService::new(arc)` from
//! any implementation (`MinecraftServicesSessionService`, Paper's `PaperMinecraftSessionService`), and
//! derefs to the trait, so `service.fetch_profile(id, true)` reads as Java.
use crate::prelude::*;
use crate::com::mojang::authlib::GameProfile;
use crate::com::mojang::authlib::minecraft::MinecraftProfileTextures;
use crate::com::mojang::authlib::minecraft::minecraft_profile_textures;
use crate::com::mojang::authlib::properties::Property;
use crate::com::mojang::authlib::services::ProfileResult;
use crate::java::lang::Throwable;

pub trait SessionServiceTrait: JObject {
    /// throws AuthenticationException
    fn join_server(&self, profile_id: Uuid, authentication_token: &str, server_id: &str) -> Result<(), Throwable>;
    /// throws AuthenticationUnavailableException
    fn has_joined_server(
        &self,
        profile_name: &str,
        server_id: &str,
        address: Option<std::net::IpAddr>,
    ) -> Result<Option<ProfileResult>, Throwable>;
    fn get_packed_textures(&self, profile: GameProfile) -> Option<Property>;
    fn unpack_textures(&self, packed_textures: Property) -> MinecraftProfileTextures;
    fn get_textures(&self, profile: GameProfile) -> MinecraftProfileTextures {
        let packed = self.get_packed_textures(profile);
        match packed {
            Some(packed) => self.unpack_textures(packed),
            None => minecraft_profile_textures::EMPTY.clone(),
        }
    }
    fn fetch_profile(&self, profile_id: Uuid, require_secure: bool) -> Option<ProfileResult>;
    /// throws InsecurePublicKeyException
    fn get_secure_property_value(&self, property: Property) -> Result<String, Throwable>;
}

#[derive(Clone)]
pub struct SessionService(pub JRef<dyn SessionServiceTrait>);
impl SessionService {
    pub fn new(service: JRef<dyn SessionServiceTrait>) -> Self {
        SessionService(service)
    }
}
impl std::ops::Deref for SessionService {
    type Target = dyn SessionServiceTrait;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
