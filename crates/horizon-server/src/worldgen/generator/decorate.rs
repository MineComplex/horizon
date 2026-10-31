//! The FEATURES step's decoration (`ChunkGenerator.applyBiomeDecoration`): the entry point the
//! pipeline calls, over [`crate::worldgen::feature::decorate::apply_biome_decoration`].
//!
//! Structure starts the chunk references are placed before each step's features
//! (`worldgen::structure::piece_place`: jigsaw pieces; other piece types are not ported yet).
//! Stand-ins until they are ported: feature types not ported are skipped, and a feature that fails
//! (mossy carpet placement) ends the chunk's decoration with a warning instead of failing the chunk.

use super::LevelGenerator;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::feature::decorate::apply_biome_decoration;
use crate::worldgen::placement::RegionWorld;
use crate::worldgen::placement::survival::BlockSurvivalRules;
use crate::worldgen::proto_chunk::GenerationRegion;
use crate::worldgen::structure::piece_place::{PieceTables, StartPlacer, writable_area};

/// Why a chunk's decoration failed.
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
    let tables = &generator.tables;
    let settings = generator.settings();
    let (min_y, height) = (
        generator.min_section_y * 16,
        generator.section_count as i32 * 16,
    );
    let level = WorldGenerationContext::new(min_y, height, settings.sea_level);
    // `new WorldGenerationContext(generator, level)`: the level's height clamped to the noise's.
    let generator_context = WorldGenerationContext::new(
        min_y.max(settings.noise.min_y),
        height.min(settings.noise.height),
        settings.sea_level,
    );
    let center = region.center();
    let data = &tables.data;
    let pieces = PieceTables {
        structures: data.structures(),
        processors: data.processor_lists(),
        features: data.features(),
        seed: generator.seed,
        known_shape: false,
    };
    let center_chunk = region.chunk(center).ok_or(DecorateError(format!(
        "the centre chunk {center:?} is outside its region"
    )))?;
    let mut structures = StartPlacer::new(
        pieces,
        generator.generate_structures,
        center_chunk.structures.all_references(),
        |pos, structure| {
            region
                .chunk(pos)
                .and_then(|c| c.structures.start(structure))
        },
        writable_area(center, min_y, height),
    );
    let mut world = RegionWorld {
        region,
        heightmaps: &tables.heightmaps,
        blocks: &tables.blocks,
        data: &tables.flags,
        level,
        generator: generator_context,
        zoom: generator.zoom,
        survival: BlockSurvivalRules {
            rules: &tables.survival,
            blocks: &tables.blocks,
            data: &tables.flags,
        },
        seed: generator.seed,
    };
    let result = apply_biome_decoration(
        &mut world,
        tables.data.features(),
        &generator.feature_steps,
        Some(&generator.possible_biomes),
        generator.seed,
        &mut structures,
        |p| generator.unported_features.contains(&p),
    );
    if let Err(e) = result {
        tracing::warn!(
            "Chunk [{}, {}]: {e}; its decoration stops there",
            center.x,
            center.z
        );
    }
    Ok(())
}
