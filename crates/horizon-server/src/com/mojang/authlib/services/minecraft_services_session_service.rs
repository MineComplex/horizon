//! Port of `com.mojang.authlib.services.MinecraftServicesSessionService`. Every request it would make to
//! Mojang fails as when the services are unavailable (see the package doc). Paper's
//! `PaperMinecraftSessionService` extends it: it holds one (`MinecraftServicesSessionService::new`) and
//! forwards to it as `super`.
use crate::prelude::*;
use crate::com::mojang::authlib::GameProfile;
use crate::com::mojang::authlib::HttpDiscoveryService;
use crate::com::mojang::authlib::SignatureState;
use crate::com::mojang::authlib::exceptions::{
    AUTHENTICATION_UNAVAILABLE_EXCEPTION, INSECURE_PUBLIC_KEY_INVALID_EXCEPTION, INSECURE_PUBLIC_KEY_MISSING_EXCEPTION,
    MinecraftClientException, exception, is_exception,
};
use crate::com::mojang::authlib::minecraft::client::MinecraftClient;
use crate::com::mojang::authlib::minecraft::minecraft_profile_textures;
use crate::com::mojang::authlib::minecraft::session_service::SessionServiceTrait;
use crate::com::mojang::authlib::minecraft::{MinecraftProfileTexture, MinecraftProfileTextureType, MinecraftProfileTextures};
use crate::com::mojang::authlib::properties::Property;
use crate::com::mojang::authlib::services::minecraft_services_discovery_service::MinecraftServicesDiscoveryServiceTrait;
use crate::com::mojang::authlib::services::response::discovery::Service;
use crate::com::mojang::authlib::services::{ProfileResult, ServicesKeySet, ServicesKeyType};
use crate::java::lang::Throwable;
use crate::java::net::Proxy;

/// Guava's `expireAfterWrite(6, TimeUnit.HOURS)`.
const INSECURE_PROFILES_EXPIRY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

pub struct MinecraftServicesSessionService {
    pub this: Weak<Self>,
    pub client: MinecraftClient,
    pub services_key_set: ServicesKeySet,
    pub discovery_service: JRef<dyn MinecraftServicesDiscoveryServiceTrait>,
    /// `LoadingCache<UUID, Optional<ProfileResult>> insecureProfiles`.
    pub insecure_profiles: dashmap::DashMap<Uuid, (std::time::Instant, Option<ProfileResult>)>,
}
impl MinecraftServicesSessionService {
    pub fn this(&self) -> JRef<Self> {
        self.this.upgrade().expect("this")
    }

    pub fn new(
        services_key_set: ServicesKeySet,
        proxy: Proxy,
        discovery_service: JRef<dyn MinecraftServicesDiscoveryServiceTrait>,
    ) -> JRef<Self> {
        Arc::new_cyclic(|this| MinecraftServicesSessionService {
            this: this.clone(),
            client: MinecraftClient::unauthenticated(proxy),
            services_key_set,
            discovery_service,
            insecure_profiles: dashmap::DashMap::new(),
        })
    }

    pub fn join_server(&self, profile_id: Uuid, authentication_token: &str, server_id: &str) -> Result<(), Throwable> {
        let attempt = (|| -> Result<Option<()>, MinecraftClientException> {
            let url = HttpDiscoveryService::constant_url(&self.discovery_service.get_url(Service::SESSION, "join")?);
            // JoinMinecraftServerRequest(accessToken, selectedProfile, serverId)
            self.client.post(&url, (authentication_token.to_string(), profile_id, server_id.to_string()))
        })();
        match attempt {
            Ok(_) => Ok(()),
            Err(e) => Err(e.to_authentication_exception()),
        }
    }

    pub fn has_joined_server(
        &self,
        profile_name: &str,
        server_id: &str,
        address: Option<std::net::IpAddr>,
    ) -> Result<Option<ProfileResult>, Throwable> {
        let mut arguments: IndexMap<String, Option<String>> = IndexMap::new();
        arguments.insert("username".to_string(), Some(profile_name.to_string()));
        arguments.insert("serverId".to_string(), Some(server_id.to_string()));
        if let Some(address) = address {
            arguments.insert("ip".to_string(), Some(address.to_string()));
        }
        let attempt = (|| -> Result<Option<(Option<Uuid>, Option<crate::com::mojang::authlib::properties::PropertyMap>)>, MinecraftClientException> {
            let url = HttpDiscoveryService::concatenate_url(
                &HttpDiscoveryService::constant_url(&self.discovery_service.get_url(Service::SESSION, "verify")?),
                &HttpDiscoveryService::build_query(Some(&arguments)),
            );
            // HasJoinedMinecraftServerResponse(id, properties, profileActions)
            self.client.get(&url)
        })();
        match attempt {
            Ok(Some((Some(id), properties))) => {
                let result = GameProfile::new_3(
                    id,
                    profile_name,
                    properties.unwrap_or_else(|| crate::com::mojang::authlib::properties::property_map::EMPTY.clone()),
                );
                Ok(Some(ProfileResult::new_2(result, HashSet::new())))
            }
            Ok(_) => Ok(None),
            Err(e) => {
                let unavailable = e.to_authentication_exception();
                if is_exception(&unavailable, AUTHENTICATION_UNAVAILABLE_EXCEPTION) {
                    return Err(unavailable);
                }
                Ok(None)
            }
        }
    }

    pub fn get_packed_textures(&self, profile: GameProfile) -> Option<Property> {
        profile.properties.get("textures").into_iter().next()
    }

    pub fn unpack_textures(&self, packed_textures: Property) -> MinecraftProfileTextures {
        let value = packed_textures.value();
        let signature_state = self.get_property_signature_state(&packed_textures);

        let result: serde_json::Value = match base64_decode(&value)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
        {
            Some(result) => result,
            None => {
                log::error!("Could not decode textures payload");
                return minecraft_profile_textures::EMPTY.clone();
            }
        };

        // MinecraftTexturesPayload(timestamp, profileId, profileName, isPublic, textures)
        let Some(textures_json) = result.get("textures").and_then(|t| t.as_object()) else {
            return minecraft_profile_textures::EMPTY.clone();
        };
        if textures_json.is_empty() {
            return minecraft_profile_textures::EMPTY.clone();
        }
        let mut textures: HashMap<MinecraftProfileTextureType, MinecraftProfileTexture> = HashMap::new();
        for (key, texture) in textures_json {
            let Some(type_) = MinecraftProfileTextureType::value_of(key) else { continue };
            let url = texture.get("url").and_then(|u| u.as_str());
            let metadata = texture.get("metadata").and_then(|m| m.as_object()).map(|m| {
                m.iter().map(|(k, v)| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect()
            });
            let allowed = match url {
                // Java: an unavailable discovery throws out of unpackTextures; here it rejects the payload.
                Some(url) => self.discovery_service.is_allowed_texture_domain(url).unwrap_or(false),
                None => false,
            };
            if !allowed {
                log::error!("Textures payload url is invalid: {}", url.unwrap_or("null"));
                return minecraft_profile_textures::EMPTY.clone();
            }
            textures.insert(type_, MinecraftProfileTexture::new(url.unwrap_or_default(), metadata));
        }

        MinecraftProfileTextures::new(
            textures.get(&MinecraftProfileTextureType::SKIN).cloned(),
            textures.get(&MinecraftProfileTextureType::CAPE).cloned(),
            textures.get(&MinecraftProfileTextureType::ELYTRA).cloned(),
            signature_state,
        )
    }

    pub fn fetch_profile(&self, profile_id: Uuid, require_secure: bool) -> Option<ProfileResult> {
        if !require_secure {
            if let Some(entry) = self.insecure_profiles.get(&profile_id) {
                if entry.0.elapsed() < INSECURE_PROFILES_EXPIRY {
                    return entry.1.clone();
                }
            }
            let loaded = self.fetch_profile_uncached(profile_id, false);
            self.insecure_profiles.insert(profile_id, (std::time::Instant::now(), loaded.clone()));
            return loaded;
        }
        self.fetch_profile_uncached(profile_id, true)
    }

    pub fn get_secure_property_value(&self, property: Property) -> Result<String, Throwable> {
        match self.get_property_signature_state(&property) {
            SignatureState::UNSIGNED => Err(exception(
                INSECURE_PUBLIC_KEY_MISSING_EXCEPTION,
                Some(format!("Missing signature from \"{}\"", property.name)),
                None,
            )),
            SignatureState::INVALID => Err(exception(
                INSECURE_PUBLIC_KEY_INVALID_EXCEPTION,
                Some(format!("Property \"{}\" has been tampered with (signature invalid)", property.name)),
                None,
            )),
            SignatureState::SIGNED => Ok(property.value()),
        }
    }

    fn get_property_signature_state(&self, property: &Property) -> SignatureState {
        if !property.has_signature() {
            return SignatureState::UNSIGNED;
        }
        if !self.services_key_set.keys(ServicesKeyType::PROFILE_PROPERTY).iter().any(|key| key.validate_property(property)) {
            return SignatureState::INVALID;
        }
        SignatureState::SIGNED
    }

    fn fetch_profile_uncached(&self, profile_id: Uuid, require_secure: bool) -> Option<ProfileResult> {
        let attempt = (|| -> Result<Option<GameProfile>, MinecraftClientException> {
            let url = HttpDiscoveryService::concatenate_url(
                &HttpDiscoveryService::constant_url(
                    &self
                        .discovery_service
                        .get_url(Service::SESSION, "getProfileById")?
                        .replace("{profileId}", &profile_id.simple().to_string()),
                ),
                &format!("unsigned={}", !require_secure),
            );
            // MinecraftProfilePropertiesResponse(id, name, properties, profileActions).profile()
            self.client.get(&url)
        })();
        match attempt {
            Ok(None) => {
                log::debug!("Couldn't fetch profile properties for {} as the profile does not exist", profile_id);
                None
            }
            Ok(Some(profile)) => {
                log::debug!("Successfully fetched profile properties for {}", profile);
                Some(ProfileResult::new_2(profile, HashSet::new()))
            }
            Err(e) => {
                log::warn!("Couldn't look up profile properties for {} {}", profile_id, e);
                None
            }
        }
    }
}
impl SessionServiceTrait for MinecraftServicesSessionService {
    fn join_server(&self, profile_id: Uuid, authentication_token: &str, server_id: &str) -> Result<(), Throwable> {
        MinecraftServicesSessionService::join_server(self, profile_id, authentication_token, server_id)
    }
    fn has_joined_server(
        &self,
        profile_name: &str,
        server_id: &str,
        address: Option<std::net::IpAddr>,
    ) -> Result<Option<ProfileResult>, Throwable> {
        MinecraftServicesSessionService::has_joined_server(self, profile_name, server_id, address)
    }
    fn get_packed_textures(&self, profile: GameProfile) -> Option<Property> {
        MinecraftServicesSessionService::get_packed_textures(self, profile)
    }
    fn unpack_textures(&self, packed_textures: Property) -> MinecraftProfileTextures {
        MinecraftServicesSessionService::unpack_textures(self, packed_textures)
    }
    fn fetch_profile(&self, profile_id: Uuid, require_secure: bool) -> Option<ProfileResult> {
        MinecraftServicesSessionService::fetch_profile(self, profile_id, require_secure)
    }
    fn get_secure_property_value(&self, property: Property) -> Result<String, Throwable> {
        MinecraftServicesSessionService::get_secure_property_value(self, property)
    }
}
crate::jobject!(MinecraftServicesSessionService: dyn SessionServiceTrait);

/// `Base64.getDecoder().decode(s)` (RFC 4648, padding optional); `None` where Java throws
/// IllegalArgumentException.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }
    let bytes = s.trim_end_matches('=').as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= value(c)? << (18 - 6 * i);
        }
        out.push((acc >> 16) as u8);
        if chunk.len() > 2 {
            out.push((acc >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(acc as u8);
        }
    }
    Some(out)
}
