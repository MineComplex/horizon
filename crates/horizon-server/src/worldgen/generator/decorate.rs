//! The FEATURES step's decoration (`ChunkGenerator.applyBiomeDecoration`): the entry point the
//! pipeline calls, over [`crate::worldgen::feature::decorate::apply_biome_decoration`].
//!
//! Structure starts the chunk references are placed before each step's features
//! (`worldgen::structure::piece_place`), with the region's random (`WorldGenRegion.getRandom`);
//! what placing changes in their pieces goes back to the chunks holding the starts
//! (`GenerationRegion::defer_piece_changes`).
//! Stand-ins until they are ported: feature types not ported are skipped, and a feature that fails
//! ends the chunk's decoration with a warning instead of failing the chunk.

use super::LevelGenerator;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::feature::decorate::apply_biome_decoration;
use crate::worldgen::placement::RegionWorld;
use crate::worldgen::placement::survival::BlockSurvivalRules;
use crate::worldgen::placement::world::RegionRandom;
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
    if let Some(e) = decorate_with(generator, region, false)? {
        let center = region.center();
        tracing::warn!(
            "Chunk [{}, {}]: {e}; its decoration stops there",
            center.x,
            center.z
        );
    }
    Ok(())
}

/// [`decorate`], answering what stopped the decoration (a stand-in: feature errors end it) instead
/// of logging it. `known_shape` places the template pieces with `setKnownShape(true)`, which only
/// the oracles' worlds ask for ([`PieceTables::known_shape`]).
pub fn decorate_with(
    generator: &LevelGenerator,
    region: &mut GenerationRegion<'_>,
    known_shape: bool,
) -> Result<Option<String>, DecorateError> {
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
        known_shape,
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
        random: RegionRandom::new(generator.seed, settings.legacy_random_source, center),
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
    // The pieces the starts' placement changed go back to the chunks holding the starts.
    let changes = structures.finish();
    region.defer_piece_changes(changes);
    Ok(result.err().map(|e| e.to_string()))
}
