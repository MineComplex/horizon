//! The FEATURES step's decoration (`ChunkGenerator.applyBiomeDecoration`): the entry point the
//! pipeline calls. The decoration driver (structures' pieces placed into the region, then each
//! biome's placed features per step, with Paper's feature seeds) is being written separately; this
//! is a no-op until it is wired here.

use super::LevelGenerator;
use crate::worldgen::proto_chunk::GenerationRegion;

/// Why a chunk's decoration failed (a datapack's feature that cannot be placed).
#[derive(Debug, thiserror::Error)]
#[error("decoration failed: {0}")]
pub struct DecorateError(pub String);

/// `applyBiomeDecoration(region, chunk, structureManager)` for the region's centre chunk.
///
/// - `region` is the FEATURES step's: the centre and its 8 neighbours writable (write radius 1,
///   all at `terrain` or later), the chunks out to radius 8 readable (at `structure_starts` or
///   later: their structure starts are what `StructureManager.startsForStructure` reads).
/// - `generator` gives the level's seed, worldgen registries, block tables, biome zoom and
///   whether structures generate.
pub fn decorate(
    generator: &LevelGenerator,
    region: &mut GenerationRegion<'_>,
) -> Result<(), DecorateError> {
    let _ = (generator, region);
    Ok(())
}
