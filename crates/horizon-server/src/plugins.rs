//! The plugins this server is built with. A plugin is a crate implementing
//! `horizon_server_api::Plugin` (crates/horizon-plugin-example is one): add it
//! to Cargo.toml's dependencies and its type below, then build the server.
//! (Rust has no stable ABI to load plugins from `plugins/` at run time, so
//! they are compiled in, as horizon-proxy's are.)
use horizon_server_api::Plugin;

/// The plugins, in the order the server loads them (each after what it `depend`s on).
pub fn installed() -> Vec<Box<dyn Plugin>> {
    vec![
        #[cfg(feature = "example-plugin")]
        Box::new(horizon_plugin_example::Example::default()),
    ]
}
