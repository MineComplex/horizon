//! Horizon's server: Leaf 26.3 (Paper, Purpur, Gale, Pufferfish, Lithium, Moonrise) ported from Java.
//!
//! Each Java file is one module at its package path (`net/minecraft/server/MinecraftServer.java` ->
//! `crate::net::minecraft::server::minecraft_server`); the Bukkit/Paper API keeps its packages
//! (`crate::org::bukkit`...) and is implemented on the NMS types themselves (`api_impl`).
//! How Java maps to Rust here: PORTING.md.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused,
    dead_code,
    unreachable_code,
    clippy::all
)]
#![recursion_limit = "256"]

pub mod api_prelude;
pub mod prelude;
pub mod prelude_traits;
pub mod rt;
pub mod text;
pub mod lang;
include!("generated_mods.rs");
