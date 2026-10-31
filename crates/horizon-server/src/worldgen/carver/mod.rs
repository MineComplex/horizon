//! Carvers: the `worldgen/carver` registry, each biome's carver list, and the CARVERS part of the
//! TERRAIN step (`NoiseBasedChunkGenerator.generateCarvers` and `applyCarvingMask` of 26.3).
//!
//! - [`float_provider`] holds the float distributions the configurations use,
//! - [`mask`] `CarvingMask`,
//! - [`world`] `CaveWorldCarver` (also `nether_cave`) and `CanyonWorldCarver`.
//!
//! [`CarverStep::carve`] runs after the fill and the surface. Every chunk within 8 of the one being
//! built (17 x 17) picks its carver list from the biome at its minimum quart (`y = 0`, read
//! straight from the biome source: the "uncached resolver"), seeds a legacy random with
//! `setLargeFeatureSeed(seed + index, x, z)` per carver, and a carver whose start test passes digs
//! the part of its paths that reaches the chunk into the [`CarvingMask`]. The mask is then applied
//! column run by column run, top down: an uncarvable block stays, otherwise the chunk's aquifer
//! (the fill's own) says what replaces it, and dirt under a carved grass block or mycelium is
//! re-decided by the surface rule (`topMaterial`).
//!
//! Copied from Horizon's `horizon-world::worldgen::{carver, carving}` and `horizon-core::carving`
//! (a port against bounded, fallible generic outputs), simplified to what the server needs: the
//! mask is concrete, registries are decoded at startup, and the step works on a [`ProtoChunk`].
//! Not carried over: old-world blending's carving filter (26.3 worlds have no blended chunks)
//! and upgrading chunks (their mask reaches the top).

pub mod float_provider;
pub mod mask;
pub mod world;

#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod tests;

use serde_json::value::RawValue;

pub use mask::CarvingMask;
pub use world::WorldCarver;

use super::anchor::WorldGenerationContext;
use super::aquifer::{Aquifer, AquiferError, Sampler};
use super::biome::zoom::BiomeZoom;
use super::codec::{DecodeError, identifier, object, required, string};
use super::data::{LevelDensity, WorldgenError};
use super::density::DensityContext;
use super::proto_chunk::ProtoChunk;
use super::settings::NoiseGeneratorSettings;
use super::surface::SurfaceSystem;
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::position::{BlockPos, ChunkPos};
use crate::registry::{DataPack, ElementFile, for_each_element, load_tags, registry_dir};
use crate::util::random::{InvalidBound, LegacyRandom, WorldgenRandom};
use crate::world::chunk::{SectionFlags, StateFlags};
use crate::world::level_chunk::HeightmapFlags;

/// A `worldgen/carver` element's index (inline ones of biomes come after the registry's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CarverId(pub u32);

/// The carver registry of a set of datapacks and each biome's `BiomeGenerationSettings.carvers`.
/// Immutable once loaded.
#[derive(Debug)]
pub struct CarverRegistry {
    carvers: Vec<(String, WorldCarver)>,
    /// By `worldgen/biome` id (registration order): the biome's carvers in its list's order.
    biomes: Vec<Vec<CarverId>>,
    /// The members of `#minecraft:uncarvable`, sorted.
    uncarvable: Vec<BlockId>,
}

impl CarverRegistry {
    pub fn carver(&self, id: CarverId) -> &WorldCarver {
        &self.carvers[id.0 as usize].1
    }

    pub fn carver_id(&self, name: &str) -> Option<CarverId> {
        let index = self.carvers.iter().position(|(id, _)| id == name)?;
        Some(CarverId(index as u32))
    }

    pub fn len(&self) -> usize {
        self.carvers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.carvers.is_empty()
    }

    /// `BiomeGenerationSettings.getCarvers`; biomes are `worldgen/biome` ids.
    pub fn biome(&self, biome: u32) -> &[CarverId] {
        &self.biomes[biome as usize]
    }

    pub fn biome_len(&self) -> usize {
        self.biomes.len()
    }

    /// `state.is(BlockTags.UNCARVABLE)`.
    pub fn is_uncarvable(&self, block: BlockId) -> bool {
        self.uncarvable.binary_search(&block).is_ok()
    }

    /// Decodes `worldgen/carver`, then each biome's `carvers` (an id, `#tag`, an inline carver, or
    /// a list of those) and the `uncarvable` block tag, of `packs` (bottom to top).
    pub fn load(packs: &[DataPack], blocks: &BlockStates) -> Result<Self, WorldgenError> {
        let error = |file: &ElementFile<'_>, source: DecodeError| WorldgenError {
            file: file.file.to_owned(),
            pack: file.pack.id.clone(),
            source,
        };
        let parse = |file: &ElementFile<'_>| {
            serde_json::from_slice::<Box<RawValue>>(file.bytes).map_err(|e| error(file, e.into()))
        };
        let mut carvers = Vec::new();
        for_each_element(packs, "minecraft:worldgen/carver", |file| {
            let carver = WorldCarver::decode(&parse(&file)?).map_err(|e| error(&file, e))?;
            carvers.push((file.id.to_owned(), carver));
            Ok(())
        })?;
        let tags = load_tags(packs, &registry_dir("minecraft:worldgen/carver"), |id| {
            carvers
                .iter()
                .position(|(name, _)| name == id)
                .map(|i| i as i32)
        });
        let mut biomes = Vec::new();
        for_each_element(packs, "minecraft:worldgen/biome", |file| {
            let list = decode_biome_carvers(&parse(&file)?, file.id, &tags, &mut carvers)
                .map_err(|e| error(&file, e))?;
            biomes.push(list);
            Ok::<_, WorldgenError>(())
        })?;
        let block_tags = load_tags(packs, &registry_dir("minecraft:block"), |id| {
            blocks.block_by_name(id).map(|b| b.0 as i32)
        });
        let mut uncarvable: Vec<BlockId> = block_tags
            .iter()
            .find(|(tag, _)| tag == "minecraft:uncarvable")
            .map(|(_, members)| members.iter().map(|&b| BlockId(b as u32)).collect())
            .unwrap_or_default();
        uncarvable.sort_unstable();
        uncarvable.dedup();
        Ok(Self {
            carvers,
            biomes,
            uncarvable,
        })
    }
}

/// `BiomeGenerationSettings.CODEC`'s `carvers`: a `HolderSet<WorldCarver>`. Inline carvers are
/// added to the registry as `<biome>#carver<n>`.
fn decode_biome_carvers(
    raw: &RawValue,
    owner: &str,
    tags: &[(String, Vec<i32>)],
    carvers: &mut Vec<(String, WorldCarver)>,
) -> Result<Vec<CarverId>, DecodeError> {
    let fields = object(raw)?;
    let set = required(&fields, "carvers")?;
    let entries: Vec<Box<RawValue>> = if set.get().starts_with('[') {
        serde_json::from_str(set.get())?
    } else {
        vec![set.to_owned()]
    };
    let mut list = Vec::new();
    for entry in &entries {
        if entry.get().starts_with('{') {
            let id = format!("{owner}#carver{}", carvers.len());
            carvers.push((id, WorldCarver::decode(entry)?));
            list.push(CarverId(carvers.len() as u32 - 1));
            continue;
        }
        let name = string(entry)?;
        match name.strip_prefix('#') {
            Some(tag) => {
                let tag = identifier(tag)?;
                let (_, members) = tags.iter().find(|(t, _)| *t == tag).ok_or_else(|| {
                    DecodeError::Registry(format!("Missing tag: #{tag} in worldgen/carver"))
                })?;
                list.extend(members.iter().map(|&m| CarverId(m as u32)));
            }
            None => {
                let name = identifier(&name)?;
                let index = carvers
                    .iter()
                    .position(|(id, _)| *id == name)
                    .ok_or_else(|| DecodeError::Registry(format!("Missing carver: {name}")))?;
                list.push(CarverId(index as u32));
            }
        }
    }
    Ok(list)
}

/// Why a chunk's carving failed. Vanilla's data never does; a datapack can.
#[derive(Debug, thiserror::Error)]
pub enum CarveError {
    #[error("a carver drew from an empty range: {0}")]
    Bound(#[from] InvalidBound),
    #[error(transparent)]
    Aquifer(#[from] AquiferError),
}

/// A level's carving: the registry, the noise settings and the blocks the application tells apart.
/// Immutable; every worker carves with its own contexts.
#[derive(Clone, Copy, Debug)]
pub struct CarverStep<'a> {
    registry: &'a CarverRegistry,
    level: &'a LevelDensity,
    settings: &'a NoiseGeneratorSettings,
    states: &'a BlockStates,
    seed: i64,
    grass_block: BlockId,
    mycelium: BlockId,
    dirt: BlockId,
}

impl<'a> CarverStep<'a> {
    /// For a level of `seed`; `None` when the block table lacks grass, mycelium or dirt.
    pub fn new(
        registry: &'a CarverRegistry,
        level: &'a LevelDensity,
        settings: &'a NoiseGeneratorSettings,
        states: &'a BlockStates,
        seed: i64,
    ) -> Option<Self> {
        Some(Self {
            registry,
            level,
            settings,
            states,
            seed,
            grass_block: states.block_by_name("minecraft:grass_block")?,
            mycelium: states.block_by_name("minecraft:mycelium")?,
            dirt: states.block_by_name("minecraft:dirt")?,
        })
    }

    /// `generateCarvers`: digs the chunk's carving mask and applies it, returning the mask.
    ///
    /// - `aquifer` is the one the chunk's fill used ([`NoiseTerrain::fill_with_aquifer`]),
    ///   `density` the context of the fill, `surface` the level's surface for `topMaterial`.
    /// - `noise_biome` is the biome source read at a quart (`createUncachedResolver`), and `zoom`
    ///   the level's `BiomeManager` zoom over it, which the surface rule's biomes come from.
    ///
    /// [`NoiseTerrain::fill_with_aquifer`]: super::terrain::NoiseTerrain::fill_with_aquifer
    #[allow(clippy::too_many_arguments)]
    pub fn carve(
        &self,
        chunk: &mut ProtoChunk,
        aquifer: &mut Aquifer<'_>,
        surface: &SurfaceSystem<'_>,
        density: &mut DensityContext,
        flags: &impl SectionFlags,
        heightmap_flags: &impl HeightmapFlags,
        zoom: BiomeZoom,
        mut noise_biome: impl FnMut([i32; 3]) -> u32,
    ) -> Result<CarvingMask, CarveError> {
        let context = WorldGenerationContext::with_generator(
            chunk.min_y(),
            chunk.height(),
            self.settings.noise.min_y,
            self.settings.noise.height,
            self.settings.sea_level,
        );
        // Seven blocks under the top stay (an upgrading chunk would carve to the top).
        let max_y = context
            .min_y
            .wrapping_add(context.height)
            .wrapping_sub(1)
            .wrapping_sub(7);
        let mut mask = CarvingMask::new(context.min_y.wrapping_add(1), max_y);
        if self.settings.noise.height <= 0 {
            return Ok(mask);
        }
        let pos = chunk.pos();
        let mut random = WorldgenRandom::new(LegacyRandom::new(0));
        for dx in -8..=8 {
            for dz in -8..=8 {
                let source = ChunkPos::new(pos.x.wrapping_add(dx), pos.z.wrapping_add(dz));
                // `getBiomeGenerationSettingsForCarver`: the biome at the source's minimum quart.
                let quart = [
                    source.x.wrapping_shl(4) >> 2,
                    0,
                    source.z.wrapping_shl(4) >> 2,
                ];
                let biome = noise_biome(quart);
                for (index, &id) in self.registry.biome(biome).iter().enumerate() {
                    let carver = self.registry.carver(id);
                    random.set_large_feature_seed(
                        self.seed.wrapping_add(index as i64),
                        source.x,
                        source.z,
                    );
                    if carver.is_start_chunk(&mut random) {
                        carver.carve(context, &mut random, pos, source, &mut mask)?;
                    }
                }
            }
        }
        if !mask.is_empty() {
            self.apply(
                chunk,
                &mask,
                aquifer,
                surface,
                density,
                flags,
                heightmap_flags,
                context,
                zoom,
                &mut noise_biome,
            )?;
        }
        Ok(mask)
    }

    /// `applyCarvingMask` (without a blending filter).
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &self,
        chunk: &mut ProtoChunk,
        mask: &CarvingMask,
        aquifer: &mut Aquifer<'_>,
        surface: &SurfaceSystem<'_>,
        density: &mut DensityContext,
        flags: &impl SectionFlags,
        heightmap_flags: &impl HeightmapFlags,
        context: WorldGenerationContext,
        zoom: BiomeZoom,
        noise_biome: &mut impl FnMut([i32; 3]) -> u32,
    ) -> Result<(), CarveError> {
        let pos = chunk.pos();
        let has_fluid = |state: BlockState| flags.section_flags(state).has(StateFlags::HAS_FLUID);
        let program = &self.level.program;
        mask.visit(|x, z, bottom, top| {
            let mut has_grass = false;
            let world_x = pos.x.wrapping_shl(4).wrapping_add(x);
            let world_z = pos.z.wrapping_shl(4).wrapping_add(z);
            for world_y in (bottom..=top).rev() {
                let block = self.states.block(chunk.block_state(x, world_y, z));
                if self.registry.is_uncarvable(block) {
                    continue;
                }
                if block == self.grass_block || block == self.mycelium {
                    has_grass = true;
                }
                let Some(state) = aquifer.compute_substance(
                    &mut Sampler {
                        program,
                        context: density,
                    },
                    [world_x, world_y, world_z],
                    0.0,
                )?
                else {
                    continue;
                };
                chunk.set_block_state(x, world_y, z, state, flags, heightmap_flags);
                let fluid = has_fluid(state);
                if aquifer.should_schedule_fluid_update() && fluid {
                    chunk.mark_postprocessing(x, world_y, z);
                }
                if !has_grass {
                    continue;
                }
                let below = world_y.wrapping_sub(1);
                if self.states.block(chunk.block_state(x, below, z)) != self.dirt {
                    continue;
                }
                let top_material = surface.top_material(
                    chunk,
                    density,
                    heightmap_flags,
                    context,
                    |at| noise_biome(zoom.quart_at_block(at)),
                    BlockPos::new(world_x, below, world_z),
                    fluid,
                );
                if let Some(material) = top_material {
                    chunk.set_block_state(x, below, z, material, flags, heightmap_flags);
                    if has_fluid(material) {
                        chunk.mark_postprocessing(x, below, z);
                    }
                }
            }
            Ok::<_, CarveError>(())
        })
    }
}
