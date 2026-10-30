//! Port of `com.mojang.authlib.services.MinecraftServicesDiscoveryService` (leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java).
//! Discovery never reaches Mojang here (see the package doc): `create_discovery_supplier` answers
//! `DiscoveryResponse::offline()`, as authlib's `RetryableFetch` does before its first fetch, so every
//! endpoint lookup fails with `MinecraftClientException(SERVICE_UNAVAILABLE)`.
use crate::prelude::*;
use crate::com::mojang::authlib::Environment;
use crate::com::mojang::authlib::EnvironmentParser;
use crate::com::mojang::authlib::GameProfileRepository;
use crate::com::mojang::authlib::HttpDiscoveryService;
use crate::com::mojang::authlib::exceptions::{MinecraftClientException, MinecraftClientExceptionErrorType};
use crate::com::mojang::authlib::minecraft::SessionService;
use crate::com::mojang::authlib::minecraft::UserApiService;
use crate::com::mojang::authlib::minecraft::user_api_service;
use crate::com::mojang::authlib::services::MinecraftServicesEnvironment;
use crate::com::mojang::authlib::services::MinecraftServicesSessionService;
use crate::com::mojang::authlib::services::ServicesKeySet;
use crate::com::mojang::authlib::services::minecraft_services_profile_repository::MinecraftServicesProfileRepository;
use crate::com::mojang::authlib::services::services_key_set;
use crate::com::mojang::authlib::services::response::discovery::DiscoveryResponse;
use crate::com::mojang::authlib::services::response::discovery::Endpoint;
use crate::com::mojang::authlib::services::response::discovery::Service;
use crate::java::net::Proxy;
use crate::java::util::concurrent::ScheduledExecutorService;
use crate::java::util::function::Supplier;

// ---- class com.mojang.authlib.services.MinecraftServicesDiscoveryService (leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:26)
pub struct MinecraftServicesDiscoveryService {
    /// `extends HttpDiscoveryService` (its `proxy`, `getProxy()`).
    pub base: HttpDiscoveryService,
    pub this: Weak<dyn MinecraftServicesDiscoveryServiceTrait>,
    pub services_key_set_enabled: bool, // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:37
    pub discovery_supplier: JRef<dyn Supplier<DiscoveryResponse>>, // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:38
}
impl std::ops::Deref for MinecraftServicesDiscoveryService {
    type Target = HttpDiscoveryService;
    fn deref(&self) -> &HttpDiscoveryService {
        &self.base
    }
}
pub trait MinecraftServicesDiscoveryServiceTrait: JObject {
    fn as_minecraft_services_discovery_service(&self) -> &MinecraftServicesDiscoveryService;
    fn create_minecraft_session_service(&self) -> SessionService; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:93
    fn create_profile_repository(&self) -> JRef<dyn GameProfileRepository>; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:98
    fn create_user_api_service(&self, access_token: &str) -> UserApiService; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:103
    fn create_friends_service(&self, access_token: &str) -> JObjectRef /* FriendsService */; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:107
    fn get_services_key_set(&self) -> ServicesKeySet; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:111
    fn get_url(&self, service: Service, endpoint_key: &str) -> Result<String, MinecraftClientException>; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:124
    fn get_valid_uris(&self, service: Service, endpoint_key: &str) -> Result<Vec<String>, MinecraftClientException>; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:129
    fn is_allowed_texture_domain(&self, url: &str) -> Result<bool, MinecraftClientException>; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:150
}
impl std::ops::Deref for dyn MinecraftServicesDiscoveryServiceTrait {
    type Target = MinecraftServicesDiscoveryService;
    fn deref(&self) -> &MinecraftServicesDiscoveryService {
        self.as_minecraft_services_discovery_service()
    }
}
pub static DISCOVERY_EXECUTOR: LazyLock<ScheduledExecutorService> = LazyLock::new(|| {
    crate::java::util::concurrent::Executors::new_scheduled_thread_pool(
        1,
        crate::com::google::common::util::concurrent::ThreadFactoryBuilder::new()
            .set_name_format("Minecraft Services Discovery")
            .set_daemon(true)
            .build(),
    )
}); // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:30

/// What `createFriendsService` returns: friends need a Mojang account, which this server has none of.
pub struct OfflineFriendsService;
crate::jobject!(OfflineFriendsService);

impl MinecraftServicesDiscoveryService {
    pub fn this(&self) -> JRef<dyn MinecraftServicesDiscoveryServiceTrait> {
        self.this.upgrade().expect("this")
    }
    pub fn new_base<S: MinecraftServicesDiscoveryServiceTrait + 'static>(
        this: &Weak<S>,
        proxy: Proxy,
        services_key_set_enabled: bool,
        discovery_supplier: JRef<dyn Supplier<DiscoveryResponse>>,
    ) -> Self {
        let this: Weak<dyn MinecraftServicesDiscoveryServiceTrait> = this.clone();
        MinecraftServicesDiscoveryService {
            base: HttpDiscoveryService::new(proxy),
            this,
            services_key_set_enabled,
            discovery_supplier,
        }
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:40
    pub fn new(
        proxy: Proxy,
        services_key_set_enabled: bool,
        discovery_supplier: JRef<dyn Supplier<DiscoveryResponse>>,
    ) -> JRef<Self> {
        Arc::new_cyclic(|this| Self::new_base(this, proxy, services_key_set_enabled, discovery_supplier))
    }
    pub fn create(proxy: Proxy) -> JRef<dyn MinecraftServicesDiscoveryServiceTrait> {
        Self::create_3(proxy, true, Self::determine_environment())
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:50
    pub fn create_2(proxy: Proxy, services_key_set_enabled: bool) -> JRef<dyn MinecraftServicesDiscoveryServiceTrait> {
        Self::create_3(proxy, services_key_set_enabled, Self::determine_environment())
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:54
    pub fn create_3(
        proxy: Proxy,
        services_key_set_enabled: bool,
        environment: Environment,
    ) -> JRef<dyn MinecraftServicesDiscoveryServiceTrait> {
        log::info!("Environment: {}", environment);
        let supplier = Self::create_discovery_supplier(proxy.clone(), environment);
        MinecraftServicesDiscoveryService::new(proxy, services_key_set_enabled, supplier)
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:58
    pub fn create_discovery_supplier(proxy: Proxy, environment: Environment) -> JRef<dyn Supplier<DiscoveryResponse>> {
        // Java: RetryableFetch.fetch(discoveryUrl, DISCOVERY_EXECUTOR, client.get, DiscoveryResponse.offline(), ...)
        // starts with the offline response and refreshes it from Mojang; nothing is fetched here.
        let _ = (proxy, environment);
        Arc::new(|| DiscoveryResponse::offline())
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:71
    pub fn create_offline(proxy: Proxy) -> JRef<dyn MinecraftServicesDiscoveryServiceTrait> {
        MinecraftServicesDiscoveryService::new(proxy, false, Arc::new(|| DiscoveryResponse::offline()))
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:83
    pub fn determine_environment() -> Environment {
        EnvironmentParser::get_environment_from_properties()
            .unwrap_or_else(|| MinecraftServicesEnvironment::PROD.get_environment())
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:87
    pub fn create_minecraft_session_service(&self) -> SessionService {
        SessionService::new(MinecraftServicesSessionService::new(
            self.this().get_services_key_set(),
            self.get_proxy(),
            self.this(),
        ))
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:93
    pub fn create_profile_repository(&self) -> JRef<dyn GameProfileRepository> {
        MinecraftServicesProfileRepository::new(self.get_proxy(), self.this())
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:98
    pub fn create_user_api_service(&self, access_token: &str) -> UserApiService {
        // Java: new MinecraftServicesUserApiService(accessToken, this, proxy), a Mojang account's API.
        let _ = access_token;
        user_api_service::OFFLINE.clone()
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:103
    pub fn create_friends_service(&self, access_token: &str) -> JObjectRef /* FriendsService */ {
        let _ = access_token;
        Arc::new(OfflineFriendsService)
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:107
    pub fn get_services_key_set(&self) -> ServicesKeySet {
        if !self.services_key_set_enabled {
            return services_key_set::EMPTY.clone();
        }
        // Java: MinecraftServicesKeyInfo.get(getUrl(AUTHENTICATION, "getPublicKeys"), client) fetches
        // Mojang's public keys; with discovery offline the fetch fails and the set is empty.
        services_key_set::EMPTY.clone()
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:111
    pub fn get_url(&self, service: Service, endpoint_key: &str) -> Result<String, MinecraftClientException> {
        let endpoint = self.get_endpoint(service, endpoint_key)?;
        Ok(endpoint.uri())
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:124
    pub fn get_valid_uris(&self, service: Service, endpoint_key: &str) -> Result<Vec<String>, MinecraftClientException> {
        let endpoint = self.get_endpoint(service, endpoint_key)?;
        Ok(endpoint.valid_uris().unwrap_or_default())
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:129
    pub fn get_endpoint(&self, service: Service, endpoint_key: &str) -> Result<Endpoint, MinecraftClientException> {
        let discovery = self.discovery_supplier.get();
        if discovery.is_offline() {
            log::warn!("Services are unavailable, unable to fetch uri for {}/{}", service, endpoint_key);
            return Err(MinecraftClientException::new(
                MinecraftClientExceptionErrorType::SERVICE_UNAVAILABLE,
                "Services are unavailable",
            ));
        }
        let endpoints = discovery.discovery().map_endpoints(service);
        match endpoints.endpoints.get(endpoint_key) {
            Some(endpoint) => Ok(endpoint.clone()),
            None => {
                log::warn!("Services are available, but unable to find uri for {}/{}", service, endpoint_key);
                Err(MinecraftClientException::new(
                    MinecraftClientExceptionErrorType::SERVICE_UNAVAILABLE,
                    "Services are unavailable",
                ))
            }
        }
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:135
    pub fn is_allowed_texture_domain(&self, url: &str) -> Result<bool, MinecraftClientException> {
        let valid_uris = self.this().get_valid_uris(Service::PROFILES, "getTexture")?;
        for valid_uri in &valid_uris {
            if url.starts_with(&valid_uri.replace("{textureId}", "")) {
                return Ok(true);
            }
        }
        Ok(false)
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesDiscoveryService.java:150
}
// ---- generated hierarchy glue (tools/leaf_port/gen.py): forwards each virtual method to the class that
// ---- declares it nearest to this one; keep the signatures in step with the trait.
impl MinecraftServicesDiscoveryServiceTrait for MinecraftServicesDiscoveryService {
    fn as_minecraft_services_discovery_service(&self) -> &MinecraftServicesDiscoveryService {
        self
    }
    fn create_minecraft_session_service(&self) -> SessionService {
        MinecraftServicesDiscoveryService::create_minecraft_session_service(self)
    }
    fn create_profile_repository(&self) -> JRef<dyn GameProfileRepository> {
        MinecraftServicesDiscoveryService::create_profile_repository(self)
    }
    fn create_user_api_service(&self, access_token: &str) -> UserApiService {
        MinecraftServicesDiscoveryService::create_user_api_service(self, access_token)
    }
    fn create_friends_service(&self, access_token: &str) -> JObjectRef /* FriendsService */ {
        MinecraftServicesDiscoveryService::create_friends_service(self, access_token)
    }
    fn get_services_key_set(&self) -> ServicesKeySet {
        MinecraftServicesDiscoveryService::get_services_key_set(self)
    }
    fn get_url(&self, service: Service, endpoint_key: &str) -> Result<String, MinecraftClientException> {
        MinecraftServicesDiscoveryService::get_url(self, service, endpoint_key)
    }
    fn get_valid_uris(&self, service: Service, endpoint_key: &str) -> Result<Vec<String>, MinecraftClientException> {
        MinecraftServicesDiscoveryService::get_valid_uris(self, service, endpoint_key)
    }
    fn is_allowed_texture_domain(&self, url: &str) -> Result<bool, MinecraftClientException> {
        MinecraftServicesDiscoveryService::is_allowed_texture_domain(self, url)
    }
}
crate::jobject!(MinecraftServicesDiscoveryService: dyn MinecraftServicesDiscoveryServiceTrait);
