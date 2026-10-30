//! authlib 10.0.77 (`com.mojang.authlib`), ported for a server without Mojang's online mode: logins come
//! through the Velocity proxy's forwarding (MineComplex's own auth), so every call that would reach
//! Mojang's services answers as authlib does when they are unavailable (offline discovery, empty key
//! sets, no profile lookups).
pub mod environment;
pub mod environment_parser;
pub mod exceptions;
pub mod game_profile;
pub mod game_profile_repository;
pub mod http_discovery_service;
pub mod profile_lookup_callback;
pub mod services;
pub mod signature_state;
pub use self::environment::Environment;
pub use self::environment_parser::EnvironmentParser;
pub use self::game_profile::GameProfile;
pub use self::game_profile_repository::GameProfileRepository;
pub use self::http_discovery_service::HttpDiscoveryService;
pub use self::profile_lookup_callback::ProfileLookupCallback;
pub use self::signature_state::SignatureState;
