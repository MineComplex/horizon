//! World generation (26.3's `levelgen`, with Paper's and Leaf's changes): the foundations the
//! generation steps build on.
//!
//! - [`noise`]: gradient noises and octave stacks.
//! - [`density`]: density functions, compiled per level into one [`density::DensityProgram`].
//! - [`codec`]: the worldgen registries' JSON as vanilla's codecs read it.
//! - [`data`]: the worldgen registries of the datapacks, decoded once at startup into immutable
//!   tables ([`data::WorldgenData`]), and the per-level compilation of their noise settings.
//! - [`settings`]: `noise_settings` (heights, router, aquifer roots, spawn targets...).
//! - [`anchor`]: vertical anchors and Paper's optionally flat bedrock condition.
//! - [`climate`]: climate parameters, the climate sampler and the biome search tree.
//! - [`biome`]: biome sources, the BIOMES step, the biome zoom and the spawn finder.
//! - [`proto_chunk`]: the chunk the generation steps write into before it becomes a
//!   `LevelChunk`.
//!
//! Ported from Horizon's generator (`horizon-world::worldgen`, `horizon-core::{noise,density}`),
//! which is checked against the 26.3 jar; this crate copies it, as it depends on no other Horizon
//! crate.

pub mod anchor;
pub mod biome;
pub mod climate;
pub mod codec;
pub mod data;
pub mod density;
pub mod noise;
pub mod proto_chunk;
pub mod settings;

#[cfg(test)]
mod oracle_tests;
