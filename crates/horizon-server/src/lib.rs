//! Horizon's server: Leaf 26.3's behaviour as a native Rust Minecraft server (ARCHITECTURE.md).
// azalea's optional features (simdnbt, serde_json, azalea-buf...) are off here: their cfg blocks stay compiled out.
#[allow(unexpected_cfgs)]
pub mod azalea;
pub mod util;
