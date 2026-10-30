//! Port of `com.mojang.authlib.Environment` (a record).
use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Environment {
    pub discovery_url: String,
    pub name: String,
}
impl Environment {
    pub fn new(discovery_url: &str, name: &str) -> Self {
        Environment { discovery_url: discovery_url.to_string(), name: name.to_string() }
    }
    pub fn discovery_url(&self) -> String {
        self.discovery_url.clone()
    }
    pub fn name(&self) -> String {
        self.name.clone()
    }
}
impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Environment[discoveryUrl={}, name={}]", self.discovery_url, self.name)
    }
}
crate::jobject!(Environment);
