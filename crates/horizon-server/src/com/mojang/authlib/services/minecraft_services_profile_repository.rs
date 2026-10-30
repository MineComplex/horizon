//! Port of `com.mojang.authlib.services.MinecraftServicesProfileRepository` (leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java).
use crate::prelude::*;
use crate::com::mojang::authlib::GameProfileRepository;
use crate::com::mojang::authlib::HttpDiscoveryService;
use crate::com::mojang::authlib::ProfileLookupCallback;
use crate::com::mojang::authlib::exceptions::{MinecraftClientException, PROFILE_NOT_FOUND_EXCEPTION, exception};
use crate::com::mojang::authlib::minecraft::client::MinecraftClient;
use crate::com::mojang::authlib::services::minecraft_services_discovery_service::MinecraftServicesDiscoveryServiceTrait;
use crate::com::mojang::authlib::services::response::NameAndId;
use crate::com::mojang::authlib::services::response::ProfileSearchResultsResponse;
use crate::com::mojang::authlib::services::response::discovery::Service;
use crate::java::net::Proxy;

// ---- class com.mojang.authlib.services.MinecraftServicesProfileRepository (leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:24)
pub struct MinecraftServicesProfileRepository {
    pub this: Weak<dyn MinecraftServicesProfileRepositoryTrait>,
    pub client: MinecraftClient, // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:31
    pub discovery_service: JRef<dyn MinecraftServicesDiscoveryServiceTrait>, // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:32
}
pub trait MinecraftServicesProfileRepositoryTrait: GameProfileRepository {
    fn as_minecraft_services_profile_repository(&self) -> &MinecraftServicesProfileRepository;
    fn find_profiles_by_names(&self, names: &Vec<String>, callback: JRef<dyn ProfileLookupCallback>); // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:39
    fn find_profile_by_name(&self, name: &str) -> Option<NameAndId>; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:109
}
impl std::ops::Deref for dyn MinecraftServicesProfileRepositoryTrait {
    type Target = MinecraftServicesProfileRepository;
    fn deref(&self) -> &MinecraftServicesProfileRepository {
        self.as_minecraft_services_profile_repository()
    }
}
pub const ENTRIES_PER_PAGE: i32 = 10; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:26
pub const MAX_FAIL_COUNT: i32 = 3; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:27
pub const DELAY_BETWEEN_PAGES: i32 = 100; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:28
pub const DELAY_BETWEEN_FAILURES: i32 = 750; // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:29
impl MinecraftServicesProfileRepository {
    pub fn this(&self) -> JRef<dyn MinecraftServicesProfileRepositoryTrait> {
        self.this.upgrade().expect("this")
    }
    pub fn new_base<S: MinecraftServicesProfileRepositoryTrait + 'static>(
        this: &Weak<S>,
        proxy: Proxy,
        discovery_service: JRef<dyn MinecraftServicesDiscoveryServiceTrait>,
    ) -> Self {
        let this: Weak<dyn MinecraftServicesProfileRepositoryTrait> = this.clone();
        MinecraftServicesProfileRepository { this, client: MinecraftClient::unauthenticated(proxy), discovery_service }
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:34
    pub fn new(proxy: Proxy, discovery_service: JRef<dyn MinecraftServicesDiscoveryServiceTrait>) -> JRef<Self> {
        Arc::new_cyclic(|this| Self::new_base(this, proxy, discovery_service))
    }
    pub fn find_profiles_by_names(&self, names: &Vec<String>, callback: JRef<dyn ProfileLookupCallback>) {
        // Collectors.toSet(): a HashSet (order unspecified); kept in first-seen order here.
        let criteria: IndexSet<String> = names.iter().filter(|name| !name.is_empty()).cloned().collect();
        let criteria: Vec<String> = criteria.into_iter().collect();

        let mut has_requested = false; // Paper - Don't sleep after profile lookups if not needed

        for request in criteria.chunks(ENTRIES_PER_PAGE as usize) {
            let normalized_request: Vec<String> = request.iter().map(|n| Self::normalize_name(n)).collect();

            let mut fail_count = 0;
            loop {
                let mut failed = false;

                let attempt = (|| -> Result<Vec<NameAndId>, MinecraftClientException> {
                    let url = HttpDiscoveryService::constant_url(
                        &self.discovery_service.get_url(Service::PROFILES, "getManyByName")?,
                    );
                    let response: Option<ProfileSearchResultsResponse> =
                        self.client.post(&url, normalized_request.clone())?;
                    Ok(response.map(|r| r.profiles()).unwrap_or_default())
                })();
                match attempt {
                    Ok(results) => {
                        fail_count = 0;

                        log::debug!("{} results returned, parsing", results.len());

                        let mut received: HashSet<String> = HashSet::with_capacity(results.len());
                        for profile in &results {
                            log::debug!("Successfully looked up profile {}", profile);
                            received.insert(Self::normalize_name(&profile.name));
                            callback.on_profile_lookup_succeeded(&profile.name, profile.id);
                        }

                        for name in request {
                            if received.contains(&Self::normalize_name(name)) {
                                continue;
                            }
                            log::debug!("Couldn't find profile {}", name);
                            callback.on_profile_lookup_failed(
                                name,
                                exception(
                                    PROFILE_NOT_FOUND_EXCEPTION,
                                    Some("Server did not find the requested profile".to_string()),
                                    None,
                                ),
                            );
                        }

                        // Paper start - Don't sleep after profile lookups if not needed
                        if !has_requested {
                            has_requested = true;
                            // `continue` in Java's do-while jumps to the condition (failed == false)
                            break;
                        }
                        // Paper end - Don't sleep after profile lookups if not needed

                        std::thread::sleep(std::time::Duration::from_millis(DELAY_BETWEEN_PAGES as u64));
                    }
                    Err(e) => {
                        fail_count += 1;

                        if fail_count == MAX_FAIL_COUNT {
                            for name in request {
                                log::debug!("Couldn't find profile {} because of a server error", name);
                                callback.on_profile_lookup_failed(name, e.to_authentication_exception());
                            }
                        } else {
                            std::thread::sleep(std::time::Duration::from_millis(DELAY_BETWEEN_FAILURES as u64));
                            failed = true;
                        }
                    }
                }
                if !failed {
                    break;
                }
            }
        }
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:39
    pub fn find_profile_by_name(&self, name: &str) -> Option<NameAndId> {
        let attempt = (|| -> Result<Option<NameAndId>, MinecraftClientException> {
            let url = self.discovery_service.get_url(Service::PROFILES, "getByName")?.replace("{name}", &Self::normalize_name(name));
            self.client.get(&HttpDiscoveryService::constant_url(&url))
        })();
        match attempt {
            Ok(result) => result,
            Err(e) => {
                // Paper start - only log details when debug is enabled
                // (MinecraftClientHttpException never occurs: no HTTP request is made, see the package doc)
                if crate::net::minecraft::server::minecraft_server::MinecraftServer::get_server().is_debugging() {
                    log::warn!("Couldn't find profile with name: {} {}", name, e);
                } else {
                    log::warn!("Couldn't find profile with name: {}", name);
                }
                // Paper end - only log details when debug is enabled
                None
            }
        }
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:109
    pub fn normalize_name(name: &str) -> String {
        name.to_lowercase()
    } // leaf-server/src/minecraft/java/com/mojang/authlib/services/MinecraftServicesProfileRepository.java:127
}
// ---- generated hierarchy glue (tools/leaf_port/gen.py): forwards each virtual method to the class that
// ---- declares it nearest to this one; keep the signatures in step with the trait.
impl MinecraftServicesProfileRepositoryTrait for MinecraftServicesProfileRepository {
    fn as_minecraft_services_profile_repository(&self) -> &MinecraftServicesProfileRepository {
        self
    }
    fn find_profiles_by_names(&self, names: &Vec<String>, callback: JRef<dyn ProfileLookupCallback>) {
        MinecraftServicesProfileRepository::find_profiles_by_names(self, names, callback)
    }
    fn find_profile_by_name(&self, name: &str) -> Option<NameAndId> {
        MinecraftServicesProfileRepository::find_profile_by_name(self, name)
    }
}
impl GameProfileRepository for MinecraftServicesProfileRepository {
    fn find_profiles_by_names(&self, names: &Vec<String>, callback: JRef<dyn ProfileLookupCallback>) {
        MinecraftServicesProfileRepositoryTrait::find_profiles_by_names(self, names, callback)
    }
    fn find_profile_by_name(&self, name: &str) -> Option<NameAndId> {
        MinecraftServicesProfileRepositoryTrait::find_profile_by_name(self, name)
    }
}
crate::jobject!(MinecraftServicesProfileRepository: dyn MinecraftServicesProfileRepositoryTrait, dyn GameProfileRepository);
