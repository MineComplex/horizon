//! `ChunkGenerator.applyBiomeDecoration` (`addVanillaDecorations`): the features of the FEATURES
//! step.
//!
//! The chunk's decoration seed seeds a random once; then, step by step, the step's structures
//! (placed by a callback: their pieces are the structure port's) and the step's features are
//! placed, each with a random reseeded by `setFeatureSeed(decorationSeed, index, step)`. The
//! features of a step are those of the biomes found in the 3 x 3 chunks around the centre, in the
//! generator's feature order (`FeatureSorter`), tried at the chunk's origin through the placed
//! feature's modifiers with the biome check.
//!
//! The chunks of the region must have the status vanilla's have while features are placed
//! (`terrain` in 26.3: a write updates the four final heightmaps, priming the ones a chunk lacks).
//! `ChunkAccess.getHeight` primes a missing heightmap when it is first asked for; priming them all
//! before the first feature gives the same values, because updating a primed heightmap with each
//! write equals priming it afresh.
//! Paper's configurable feature seeds (`world-settings.feature-seeds`) are not read: every
//! feature uses the decoration seed, as with its default of -1. Leaf's secure seed is not ported.

use crate::core::position::{BlockPos, ChunkPos};
use crate::util::random::{WorldgenRandom, XoroshiroRandom};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::feature::sort::StepFeatureData;
use crate::worldgen::feature::{
    ConfiguredFeatureId, Feature, FeatureRegistry, FeatureWorld, PlacedFeatureId,
};
use crate::worldgen::placement::world::SurvivalRules;
use crate::worldgen::placement::{PlacementWorld, RegionWorld};
use crate::worldgen::proto_chunk::OutsideRegion;
use crate::worldgen::state_provider::ProviderError;
use crate::worldgen::structure::BiomeSet;

/// `ChunkStatus.FINAL_HEIGHTMAPS`.
const FINAL_HEIGHTMAPS: [Heightmap; 4] = [
    Heightmap::OceanFloor,
    Heightmap::WorldSurface,
    Heightmap::MotionBlocking,
    Heightmap::MotionBlockingNoLeaves,
];

/// The number of decoration steps (`GenerationStep.Decoration.values().length`).
pub const DECORATION_STEPS: usize = 11;

/// A feature or structure failing: vanilla reports it as a crash of the chunk ("Feature
/// placement", "Biome decoration").
#[derive(Debug, thiserror::Error)]
#[error("Feature placement of {feature} (decoration of chunk {center:?}): {source}")]
pub struct DecorationError {
    pub feature: String,
    pub center: ChunkPos,
    #[source]
    pub source: ProviderError,
}

/// The structures placed in the decoration steps (`StructureStart.placeInChunk` for the starts of
/// the chunk's structures), which come before the features of each step.
pub trait StructureDecorator<W> {
    /// How many structures of the registry have this decoration step; each takes the next feature
    /// seed index of the step, whether or not it has a start here.
    fn structures_in_step(&self, step: usize) -> usize;
    /// `structureManager.startsForStructure(...).forEach(start -> start.placeInChunk(...))` for
    /// the step's `index`-th structure, after `setFeatureSeed(decorationSeed, index, step)`.
    fn place(
        &mut self,
        step: usize,
        index: usize,
        world: &mut W,
        random: &mut WorldgenRandom<XoroshiroRandom>,
    ) -> Result<(), ProviderError>;
}

/// No structures (`StructureManager.shouldGenerateStructures()` false, or none ported).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoStructures;

impl<W> StructureDecorator<W> for NoStructures {
    fn structures_in_step(&self, _: usize) -> usize {
        0
    }
    fn place(
        &mut self,
        _: usize,
        _: usize,
        _: &mut W,
        _: &mut WorldgenRandom<XoroshiroRandom>,
    ) -> Result<(), ProviderError> {
        Ok(())
    }
}

/// The biomes stored in the 3 x 3 chunks around the centre (`possibleBiomes`), restricted to
/// the biome source's (`retainAll`) when given, as registry ids in ascending order.
pub fn possible_biomes(
    region: &crate::worldgen::proto_chunk::GenerationRegion<'_>,
    biome_count: usize,
    source: Option<&BiomeSet>,
) -> Result<Vec<u32>, OutsideRegion> {
    let center = region.center();
    let mut seen = vec![false; biome_count];
    for dz in -1..=1 {
        for dx in -1..=1 {
            let pos = ChunkPos::new(center.x + dx, center.z + dz);
            let chunk = region.chunk(pos).ok_or(OutsideRegion(pos))?;
            for section in chunk.sections() {
                for y in 0..4 {
                    for z in 0..4 {
                        for x in 0..4 {
                            seen[section.noise_biome(x, y, z) as usize] = true;
                        }
                    }
                }
            }
        }
    }
    Ok((0..biome_count as u32)
        .filter(|&b| seen[b as usize] && source.is_none_or(|s| s.contains(b)))
        .collect())
}

/// `ChunkGenerator.applyBiomeDecoration` over a generation region: the centre chunk's
/// decoration. `steps` is the generator's `FeatureSorter` result, `source_biomes` the biome
/// source's `possibleBiomes()` (if it is to be applied), `seed` the level seed.
#[allow(clippy::too_many_arguments)]
pub fn apply_biome_decoration<S: SurvivalRules>(
    world: &mut RegionWorld<'_, '_, S>,
    registry: &FeatureRegistry,
    steps: &[StepFeatureData],
    source_biomes: Option<&BiomeSet>,
    seed: i64,
    structures: &mut impl for<'a, 'b> StructureDecorator<RegionWorld<'a, 'b, S>>,
    skip: impl Fn(PlacedFeatureId) -> bool,
) -> Result<(), DecorationError> {
    let center = world.region.center();
    let failed = |feature: &str, source| DecorationError {
        feature: feature.to_owned(),
        center,
        source,
    };
    let biomes = possible_biomes(&*world.region, registry.biome_len(), source_biomes)
        .map_err(|e| failed("biome set", ProviderError::World(e.into())))?;
    let table = world.heightmaps;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let pos = ChunkPos::new(center.x + dx, center.z + dz);
            let chunk = world
                .region
                .chunk_mut(pos)
                .map_err(|e| failed("heightmaps", ProviderError::World(e.into())))?;
            let missing: Vec<Heightmap> = FINAL_HEIGHTMAPS
                .into_iter()
                .filter(|&kind| chunk.heightmap(kind).is_none())
                .collect();
            if !missing.is_empty() {
                chunk.prime_heightmaps(&missing, table);
            }
        }
    }
    let min_y = world.level_generation().min_y;
    let origin = BlockPos::new(center.x << 4, min_y, center.z << 4);
    decorate(
        world, registry, steps, &biomes, origin, seed, structures, skip,
    )
}

/// The loop of `addVanillaDecorations` over any world, given the chunk's possible biomes and
/// its origin (the corner of the lowest section). `skip` names placed features not to place at
/// all (those whose feature types are not ported yet); each feature has its own seeded random,
/// so skipping one changes nothing else but the blocks it would have written.
#[allow(clippy::too_many_arguments)]
pub fn decorate<W: FeatureWorld>(
    world: &mut W,
    registry: &FeatureRegistry,
    steps: &[StepFeatureData],
    biomes: &[u32],
    origin: BlockPos,
    seed: i64,
    structures: &mut impl StructureDecorator<W>,
    skip: impl Fn(PlacedFeatureId) -> bool,
) -> Result<(), DecorationError> {
    let center = origin.chunk();
    let failed = |feature: &str, source| DecorationError {
        feature: feature.to_owned(),
        center,
        source,
    };
    // The source of the random is irrelevant: `setDecorationSeed` reseeds it.
    let mut random = WorldgenRandom::new(XoroshiroRandom::new(0));
    let decoration_seed = random.set_decoration_seed(seed, origin.x, origin.z);
    for step in 0..DECORATION_STEPS.max(steps.len()) {
        for structure in 0..structures.structures_in_step(step) {
            random.set_feature_seed(decoration_seed, structure as i32, step as i32);
            structures
                .place(step, structure, world, &mut random)
                .map_err(|e| failed(&format!("structure {structure} of step {step}"), e))?;
        }
        let Some(data) = steps.get(step) else {
            continue;
        };
        // `IntArraySet` sorted: the indices of the step's features that some biome has.
        let mut wanted: Vec<usize> = Vec::new();
        for &biome in biomes {
            let features = &registry.biome(biome).steps;
            for &feature in features.get(step).map(Vec::as_slice).unwrap_or_default() {
                let position = data.index_of(feature).ok_or_else(|| {
                    failed(
                        &registry.placed(feature).id,
                        ProviderError::Invalid("a feature outside the generator's feature order"),
                    )
                })?;
                wanted.push(position);
            }
        }
        wanted.sort_unstable();
        wanted.dedup();
        for global in wanted {
            let feature = data.features()[global];
            random.set_feature_seed(decoration_seed, global as i32, step as i32);
            if skip(feature) {
                continue;
            }
            registry
                .place_placed_checked(feature, true, world, &mut random, origin)
                .map_err(|e| failed(&registry.placed(feature).id, e))?;
        }
    }
    Ok(())
}

/// The placed features of `steps` that place a feature type not ported yet (directly or through a
/// selector or patch): the decoration skips them until they are (a stand-in; see PORTING_STATE).
pub fn unported_features(
    registry: &FeatureRegistry,
    steps: &[StepFeatureData],
) -> std::collections::HashSet<PlacedFeatureId> {
    steps
        .iter()
        .flat_map(|step| step.features().iter().copied())
        .filter(|&p| !fully_ported(registry, registry.placed(p).feature, 0))
        .collect()
}

/// Whether the feature and everything it places is implemented, and does not place the pale moss
/// patch (not ported): the decoration oracle's `PORTED`.
fn fully_ported(registry: &FeatureRegistry, id: ConfiguredFeatureId, depth: u32) -> bool {
    if depth > 8 {
        return false;
    }
    match &registry.configured(id).feature {
        None => false,
        Some(Feature::Select(selector)) => selector
            .placed()
            .into_iter()
            .all(|p| fully_ported(registry, registry.placed(p).feature, depth + 1)),
        Some(Feature::Patch(patch)) => patch
            .placed()
            .iter()
            .all(|&p| fully_ported(registry, registry.placed(p).feature, depth + 1)),
        Some(Feature::Cave(cave)) => {
            cave.ready()
                && cave
                    .placed()
                    .iter()
                    .all(|&p| fully_ported(registry, registry.placed(p).feature, depth + 1))
        }
        Some(Feature::Tree(tree)) => !tree.places_pale_moss(),
        Some(Feature::FallenTree(tree)) => !tree.places_pale_moss(),
        Some(_) => true,
    }
}
