//! Port of `com.mojang.authlib.services.response.discovery.Endpoints` (a record).
use crate::prelude::*;
use crate::com::mojang::authlib::services::response::discovery::Endpoint;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub endpoints: HashMap<String, Endpoint>,
}
impl Endpoints {
    pub fn new(endpoints: HashMap<String, Endpoint>) -> Self {
        Endpoints { endpoints }
    }
    pub fn empty() -> Self {
        Endpoints::new(HashMap::new())
    }
    pub fn endpoints(&self) -> HashMap<String, Endpoint> {
        self.endpoints.clone()
    }
}
