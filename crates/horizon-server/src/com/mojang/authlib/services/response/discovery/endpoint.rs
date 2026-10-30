//! Port of `com.mojang.authlib.services.response.discovery.Endpoint` (a record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub uri: String,
    pub valid_uris: Option<Vec<String>>,
}
impl Endpoint {
    pub fn new(uri: &str) -> Self {
        Self::new_2(uri, Some(Vec::new()))
    }
    pub fn new_2(uri: &str, valid_uris: Option<Vec<String>>) -> Self {
        Endpoint { uri: uri.to_string(), valid_uris }
    }
    pub fn uri(&self) -> String {
        self.uri.clone()
    }
    pub fn valid_uris(&self) -> Option<Vec<String>> {
        self.valid_uris.clone()
    }
}
