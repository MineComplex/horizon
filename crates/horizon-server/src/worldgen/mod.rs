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
//! - [`aquifer`]: the underground fluids and barriers (`Aquifer`).
//! - [`terrain`]: the noise fill (`NoiseChunk`, `doFill`) of a proto chunk.
//! - [`surface`]: the surface rules (`buildSurface`) with Paper's optionally flat bedrock, a biome's
//!   temperature.
//! - [`proto_chunk`]: the chunk the generation steps write into before it becomes a
//!   `LevelChunk`.
//! - [`structure`]: the structure registries, where structures start (with Paper's
//!   `spigot.yml` salts), structure starts and references in chunk NBT.
//! - [`feature`]: the configured and placed feature registries, each biome's features per
//!   step, and the feature sorter.
//! - [`placement`]: placement modifiers, block predicates, int and height providers, and the
//!   `FeaturePlacer` that runs a placed feature's chain over a world view.
//!
//! Ported from Horizon's generator (`horizon-world::worldgen`, `horizon-core::{noise,density}`),
//! which is checked against the 26.3 jar; this crate copies it, as it depends on no other Horizon
//! crate.

pub mod anchor;
pub mod aquifer;
pub mod biome;
pub mod climate;
pub mod codec;
pub mod data;
pub mod density;
pub mod feature;
pub mod noise;
pub mod placement;
pub mod proto_chunk;
pub mod settings;
pub mod state_provider;
pub mod structure;
pub mod surface;
pub mod terrain;

#[cfg(test)]
mod oracle_tests;
