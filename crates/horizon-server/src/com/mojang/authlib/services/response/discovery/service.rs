//! Port of `com.mojang.authlib.services.response.discovery.Service`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    AUTHENTICATION,
    SESSION,
    PLAYER,
    PROFILES,
    TELEMETRY,
}
impl std::fmt::Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
