//! Port of `com.mojang.authlib.services.response.discovery.Discovery` (a record).
use crate::com::mojang::authlib::services::response::discovery::{Endpoints, Service};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    pub product: String,
    pub authentication: Endpoints,
    pub session: Endpoints,
    pub player: Endpoints,
    pub profiles: Endpoints,
    pub telemetry: Endpoints,
}
impl Discovery {
    pub fn offline() -> Self {
        Discovery {
            product: "offline".to_string(),
            authentication: Endpoints::empty(),
            session: Endpoints::empty(),
            player: Endpoints::empty(),
            profiles: Endpoints::empty(),
            telemetry: Endpoints::empty(),
        }
    }

    pub fn map_endpoints(&self, service: Service) -> Endpoints {
        match service {
            Service::AUTHENTICATION => self.authentication.clone(),
            Service::SESSION => self.session.clone(),
            Service::PLAYER => self.player.clone(),
            Service::PROFILES => self.profiles.clone(),
            Service::TELEMETRY => self.telemetry.clone(),
        }
    }
}
