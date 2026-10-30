//! Port of `com.mojang.authlib.services.response.discovery.DiscoveryResponse` (a record).
use crate::com::mojang::authlib::services::response::discovery::Discovery;

const OFFLINE: &str = "offline";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryResponse {
    pub environment: String,
    pub product: String,
    pub discovery: Discovery,
}
impl DiscoveryResponse {
    pub fn new(environment: &str, product: &str, discovery: Discovery) -> Self {
        DiscoveryResponse { environment: environment.to_string(), product: product.to_string(), discovery }
    }
    pub fn environment(&self) -> String {
        self.environment.clone()
    }
    pub fn product(&self) -> String {
        self.product.clone()
    }
    pub fn discovery(&self) -> Discovery {
        self.discovery.clone()
    }
    pub fn is_offline(&self) -> bool {
        OFFLINE == self.environment
    }
    pub fn offline() -> Self {
        DiscoveryResponse::new("offline", "minecraft", Discovery::offline())
    }
}
