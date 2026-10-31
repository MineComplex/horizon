//! Biome sources and the BIOMES step (26.3's `BiomeSource`s, `ChunkGenerator.createBiomes`, with
//! Paper's section fill), the `BiomeManager` zoom and the noise spawn finder.
//!
//! Biomes are the synced `worldgen/biome` registry's ids (`registry::SyncedRegistry` order, the
//! protocol ids), the values a chunk section's biome container holds.
//!
//! - [`BiomeSource`]: the four vanilla sources as one enum; a multi-noise source owns its
//!   parameter list's search tree.
//! - [`codec`]: a level stem's `biome_source` (JSON, or `level.dat` NBT converted to it) and the
//!   multi-noise presets from the data generator's `biome_parameters` reports.
//! - [`zoom`]: `BiomeManager.getBiome` as Leaf computes it.
//! - [`spawn`]: where a new level's spawn search starts (`NoiseSpawnFinder`).
//!
//! Copied from Horizon's `horizon-world::worldgen::{biome, biome_source}` and adapted: biomes are
//! registry ids instead of names, the climate comes from the level's density program, and the
//! search state is the caller's (vanilla keeps it in a thread-local per tree). Horizon's
//! `BiomeChunk` storage is not copied: the proto chunk's sections hold the biomes.

pub mod codec;
pub mod spawn;
pub mod zoom;

#[cfg(test)]
mod oracle_tests;

use super::climate::{ClimateIndex, ClimateSampler, ClimateSearch};
use super::data::LevelDensity;
use super::density::{DensityContext, NodeId};
use super::proto_chunk::ProtoChunk;
use super::settings::RouterField;

/// `TheEndBiomeSource`'s five biomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndBiomes {
    pub center: u32,
    pub highlands: u32,
    pub midlands: u32,
    pub islands: u32,
    pub barrens: u32,
}

/// A level's biome source.
#[derive(Clone, Debug)]
pub enum BiomeSource {
    /// `FixedBiomeSource`.
    Fixed(u32),
    /// `CheckerboardColumnBiomeSource`: `biomes[floorMod((x >> shift) + (z >> shift), n)]`,
    /// `shift = scale + 2`.
    Checkerboard { biomes: Vec<u32>, scale: i32 },
    /// `TheEndBiomeSource`.
    End(EndBiomes),
    /// `MultiNoiseBiomeSource`, inline or from a preset: the list's tree.
    MultiNoise(ClimateIndex<u32>),
}

impl BiomeSource {
    /// `possibleBiomes`: the distinct biomes in first-seen order.
    pub fn possible_biomes(&self) -> Vec<u32> {
        let all: Vec<u32> = match self {
            Self::Fixed(biome) => vec![*biome],
            Self::Checkerboard { biomes, .. } => biomes.clone(),
            Self::End(end) => vec![
                end.center,
                end.highlands,
                end.midlands,
                end.islands,
                end.barrens,
            ],
            Self::MultiNoise(index) => index.entries().iter().map(|(_, b)| *b).collect(),
        };
        let mut distinct = Vec::new();
        for biome in all {
            if !distinct.contains(&biome) {
                distinct.push(biome);
            }
        }
        distinct
    }

    /// `createResolver(sampler).getNoiseBiome`: the biome at a quart, sampling the level's
    /// climate (or the End's erosion) with `context`. `search` is the worker's state for this
    /// source's tree.
    pub fn noise_biome(
        &self,
        density: &LevelDensity,
        context: &mut DensityContext,
        search: &mut ClimateSearch,
        quart: [i32; 3],
    ) -> u32 {
        match self {
            Self::Fixed(biome) => *biome,
            Self::Checkerboard { biomes, scale } => checkerboard(biomes, *scale, quart),
            Self::End(end) => end_biome(
                end,
                density,
                density.router(RouterField::Erosion),
                context,
                quart,
            ),
            Self::MultiNoise(index) => {
                *index.find(search, ClimateSampler::new(density).sample(context, quart))
            }
        }
    }

    /// The BIOMES step (`ChunkGenerator.doCreateBiomes`): a fresh caching context, the chunk
    /// resolver (`createResolverForChunk`: a multi-noise source samples its six climate fields
    /// over the chunk's quart box first), then Paper's `fillBiomesFromNoise` section by
    /// section, bottom to top. Blending and below-zero retrogen (upgraded pre-1.18 chunks) are
    /// not ported: 26.3 worlds only.
    pub fn create_biomes(
        &self,
        chunk: &mut ProtoChunk,
        density: &LevelDensity,
        search: &mut ClimateSearch,
    ) {
        let mut context = DensityContext::cached();
        let pos = chunk.pos();
        let quart_min = [
            pos.x.wrapping_shl(2),
            chunk.min_y() >> 2,
            pos.z.wrapping_shl(2),
        ];
        match self {
            Self::MultiNoise(index) => {
                let volume = ClimateSampler::new(density)
                    .sample_volume(&mut context, quart_min, [4, chunk.height() >> 2, 4])
                    .expect("a chunk's quart box is a valid volume");
                chunk.fill_biomes_from_noise(|quart| {
                    let target = volume
                        .target(quart)
                        .expect("the chunk fills inside its own volume");
                    *index.find(search, target)
                });
            }
            _ => chunk.fill_biomes_from_noise(|quart| {
                self.noise_biome(density, &mut context, search, quart)
            }),
        }
    }
}

fn checkerboard(biomes: &[u32], scale: i32, quart: [i32; 3]) -> u32 {
    // Java masks shift distances to five bits.
    let shift = scale.wrapping_add(2) as u32 & 31;
    let cell = (quart[0] >> shift).wrapping_add(quart[2] >> shift);
    biomes[cell.rem_euclid(biomes.len() as i32) as usize]
}

/// `TheEndBiomeSource.getNoiseBiome`: the central island within 64 chunks, else by the erosion
/// field at the chunk's centre (with the quart's block y).
fn end_biome(
    end: &EndBiomes,
    density: &LevelDensity,
    erosion: NodeId,
    context: &mut DensityContext,
    quart: [i32; 3],
) -> u32 {
    let block = quart.map(|v| v.wrapping_shl(2));
    let (chunk_x, chunk_z) = (block[0] >> 4, block[2] >> 4);
    if i64::from(chunk_x) * i64::from(chunk_x) + i64::from(chunk_z) * i64::from(chunk_z) <= 4096 {
        return end.center;
    }
    let center = |c: i32| c.wrapping_mul(2).wrapping_add(1).wrapping_mul(8);
    let height = density.program.sample(
        context,
        erosion,
        [center(chunk_x), block[1], center(chunk_z)],
    );
    if height > 0.25 {
        end.highlands
    } else if height >= -0.0625 {
        end.midlands
    } else if height < -0.21875 {
        end.islands
    } else {
        end.barrens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkerboard_columns() {
        let biomes = [7, 8, 9];
        // scale 0: cells of 4 quarts.
        assert_eq!(checkerboard(&biomes, 0, [0, 0, 0]), 7);
        assert_eq!(checkerboard(&biomes, 0, [4, 99, 0]), 8);
        assert_eq!(checkerboard(&biomes, 0, [4, 0, 4]), 9);
        assert_eq!(checkerboard(&biomes, 0, [-1, 0, 0]), 9);
        // A scale of 30 shifts by 32 & 31 = 0 in Java.
        assert_eq!(checkerboard(&biomes, 30, [1, 0, 1]), 9);
    }

    #[test]
    fn possible_biomes_are_distinct_in_order() {
        let source = BiomeSource::Checkerboard {
            biomes: vec![3, 1, 3, 2],
            scale: 2,
        };
        assert_eq!(source.possible_biomes(), vec![3, 1, 2]);
    }
}
