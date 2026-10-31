//! Where structures may start: `StructurePlacement` and its types, and a level's
//! `ChunkGeneratorStructureState` with Paper's structure seeds from `spigot.yml`.
//!
//! A level builds one [`LevelPlacements`] when its generator is created, with every concentric
//! ring position already found ([`ring_searches`], then [`LevelPlacements::new`]): vanilla starts
//! those biome searches as background tasks on first use and joins them in order (Leaf `0097`
//! keeps the order); here they are one per-level computation whose searches the caller may run
//! in parallel. Afterwards it is immutable and can be shared read-only with generation workers.
//!
//! Paper (`ChunkGeneratorStructureState.injectSpigot`, `KeyedRandomSpreadStructurePlacement`,
//! `AbstractSpreadingStructurePlacement.isStructureChunk`, `generateRingPositions`):
//! - a `minecraft:` random-spread set takes its salt from `world-settings.*.seed-*`
//!   ([`SpigotWorldSeeds::salt`]) and keeps its key, which gives `minecraft:mineshafts` the
//!   `seed-mineshaft` override and `minecraft:buried_treasures` the `seed-buriedtreasure` one in
//!   their frequency reducers (`legacy_type_3`/`legacy_type_2`; the other reducers ignore it);
//! - an exclusion zone checks the other set's own placement from the registry: the jar's salt
//!   and no key, so no Spigot seed applies there;
//! - `minecraft:strongholds`' rings start from `seed-stronghold` when it is set.
//!
//! Leaf's FastRNG (`performance.faster-random-generator`, off by default and for worldgen) and
//! its Secure Seed (left out by the user's decision) would replace the random sources here.

use super::{
    BiomeSet, Placement, RandomSpread, SpreadType, Spreading, StructureId, StructureRegistry,
    StructureSetId,
};
use crate::core::position::{BlockPos, ChunkPos};
use crate::server::config::SpigotWorldSeeds;
use crate::util::jmath;
use crate::util::random::{LegacyRandom, RandomSource, WorldgenRandom};

/// `AbstractSpreadingStructurePlacement.HIGHLY_ARBITRARY_RANDOM_SALT`.
const HIGHLY_ARBITRARY_RANDOM_SALT: i32 = 10387320;

fn worldgen_random() -> WorldgenRandom<LegacyRandom> {
    WorldgenRandom::new(LegacyRandom::new(0))
}

impl SpreadType {
    /// `RandomSpreadType.evaluate`; `limit` is at least 1 (spacing > separation).
    fn evaluate(self, random: &mut WorldgenRandom<LegacyRandom>, limit: i32) -> i32 {
        let mut next = || random.next_int_bound(limit).unwrap_or(0);
        match self {
            Self::Linear => next(),
            Self::Triangular => (next() + next()) / 2,
        }
    }
}

impl RandomSpread {
    /// `getPotentialStructureChunk`: the one chunk of `(x, z)`'s spacing cell that may hold a
    /// start, with the set's salt or Paper's.
    pub fn potential_chunk(&self, seed: i64, salt: i32, x: i32, z: i32) -> ChunkPos {
        let cell_x = x.div_euclid(self.spacing);
        let cell_z = z.div_euclid(self.spacing);
        let mut random = worldgen_random();
        random.set_large_feature_with_salt(seed, cell_x, cell_z, salt);
        let limit = self.spacing - self.separation;
        let spread_x = self.spread_type.evaluate(&mut random, limit);
        let spread_z = self.spread_type.evaluate(&mut random, limit);
        ChunkPos::new(
            cell_x.wrapping_mul(self.spacing).wrapping_add(spread_x),
            cell_z.wrapping_mul(self.spacing).wrapping_add(spread_z),
        )
    }

    /// `isPlacementChunk`.
    pub fn is_placement_chunk(&self, seed: i64, salt: i32, x: i32, z: i32) -> bool {
        self.potential_chunk(seed, salt, x, z) == ChunkPos::new(x, z)
    }
}

impl super::FrequencyReduction {
    /// `FrequencyReductionMethod.shouldGenerate` with Paper's `saltOverride`.
    pub fn should_generate(
        self,
        seed: i64,
        salt: i32,
        x: i32,
        z: i32,
        probability: f32,
        salt_override: Option<i32>,
    ) -> bool {
        let mut random = worldgen_random();
        match self {
            // The salt goes where x is expected, as in vanilla.
            Self::Default => {
                random.set_large_feature_with_salt(seed, salt, x, z);
                random.next_float() < probability
            }
            Self::LegacyType1 => {
                let (cx, cz) = (x >> 4, z >> 4);
                random.set_seed(i64::from(cx ^ (cz << 4)) ^ seed);
                random.next_int();
                // `(int) (1.0F / probability)`: Java's saturating cast.
                let bound = (1.0 / probability) as i32;
                random.next_int_bound(bound).is_ok_and(|v| v == 0)
            }
            Self::LegacyType2 => {
                let salt = salt_override.unwrap_or(HIGHLY_ARBITRARY_RANDOM_SALT);
                random.set_large_feature_with_salt(seed, x, z, salt);
                random.next_float() < probability
            }
            Self::LegacyType3 => {
                match salt_override {
                    None => random.set_large_feature_seed(seed, x, z),
                    Some(salt) => random.set_large_feature_with_salt(seed, x, z, salt),
                }
                random.next_double() < f64::from(probability)
            }
        }
    }
}

impl Spreading {
    /// `applyAdditionalChunkRestrictions`: the frequency, when below 1.
    pub fn additional_restrictions(
        &self,
        seed: i64,
        salt: i32,
        x: i32,
        z: i32,
        salt_override: Option<i32>,
    ) -> bool {
        self.frequency >= 1.0
            || self.frequency_reduction.should_generate(
                seed,
                salt,
                x,
                z,
                self.frequency,
                salt_override,
            )
    }
}

/// `StructurePlacement.getLocatePos`: the chunk's corner at y 0, plus the locate offset.
pub fn locate_pos(placement: &Placement, chunk: ChunkPos) -> BlockPos {
    let [dx, dy, dz] = placement.locate_offset();
    BlockPos::new(
        (chunk.x << 4).wrapping_add(dx),
        dy,
        (chunk.z << 4).wrapping_add(dz),
    )
}

/// What a level's placements are built from.
#[derive(Clone, Copy, Debug)]
pub struct LevelParams<'a> {
    /// The level's seed (`getLevelSeed`).
    pub seed: i64,
    /// `concentricRingsSeed`: the level seed, or 0 for a flat level.
    pub ring_seed: i64,
    /// `getDimensionOrigin`.
    pub origin: ChunkPos,
    /// The candidate sets in registry order: every set, or a flat level's structure overrides.
    pub sets: &'a [StructureSetId],
    /// `biomeSource.possibleBiomes()`.
    pub possible_biomes: &'a BiomeSet,
    /// The level's `SpigotWorldConfig` seeds.
    pub seeds: &'a SpigotWorldSeeds,
}

impl LevelParams<'_> {
    /// `hasBiomesForStructureSet`, the filter of `possibleStructureSets`.
    fn possible_sets(&self, registry: &StructureRegistry) -> Vec<StructureSetId> {
        (self.sets.iter().copied())
            .filter(|&set| {
                registry.set(set).structures.iter().any(|(structure, _)| {
                    (registry.structure(*structure).biomes.iter())
                        .any(|biome| self.possible_biomes.contains(biome))
                })
            })
            .collect()
    }
}

/// One stronghold-style ring position before its biome search (`generateRingPositions`).
#[derive(Clone, Debug)]
pub struct RingSearch {
    pub set: StructureSetId,
    /// The ring position before the search.
    pub chunk: ChunkPos,
    /// The search's own random source (`random.fork()`).
    pub random: LegacyRandom,
}

impl RingSearch {
    /// The search's centre block (x, z): the chunk's (8, 8).
    pub fn center(&self) -> (i32, i32) {
        (
            (self.chunk.x << 4).wrapping_add(8),
            (self.chunk.z << 4).wrapping_add(8),
        )
    }

    /// The ring chunk: the found block's chunk, or the initial one when nothing was found.
    pub fn result(&self, found: Option<(i32, i32)>) -> ChunkPos {
        found.map_or(self.chunk, |(x, z)| ChunkPos::new(x >> 4, z >> 4))
    }

    /// The vanilla search (`BiomeSource.findBiomeHorizontal` over 112 blocks at y 0, a random
    /// preferred biome of the square's samples) for a biome source answering `biome_at` (a
    /// caching resolver's noise biome at quart x, y, z). A source that overrides the search
    /// (`FixedBiomeSource`) gives its own answer to [`Self::result`] instead.
    pub fn find(
        &self,
        preferred: &BiomeSet,
        biome_at: impl FnMut(i32, i32, i32) -> u32,
    ) -> ChunkPos {
        let (x, z) = self.center();
        let mut random = self.random.clone();
        let found = find_biome_horizontal(
            [x, 0, z],
            112,
            1,
            |biome| preferred.contains(biome),
            &mut random,
            false,
            biome_at,
        );
        self.result(found.map(|([x, _, z], _)| (x, z)))
    }
}

/// `BiomeSource.findBiomeHorizontal`: the square of quarts `radius` blocks around `origin` (rings
/// growing from the centre when `find_closest`, else only the full square), every `skip` quarts;
/// among the allowed biomes found, the first when `find_closest`, else a random one (each new
/// match replaces the result with chance `1 / (matches + 1)`). Returns the block (x, origin y, z)
/// and the biome. This is the biome source's default search, kept here for the ring positions
/// until the biome sources (WP2) take it.
pub fn find_biome_horizontal(
    origin: [i32; 3],
    radius: i32,
    skip: i32,
    allowed: impl Fn(u32) -> bool,
    random: &mut impl RandomSource,
    find_closest: bool,
    mut biome_at: impl FnMut(i32, i32, i32) -> u32,
) -> Option<([i32; 3], u32)> {
    let (center_x, center_z) = (origin[0] >> 2, origin[2] >> 2);
    let (noise_radius, noise_y) = (radius >> 2, origin[1] >> 2);
    let mut result = None;
    let mut found = 0;
    let mut current = if find_closest { 0 } else { noise_radius };
    while current <= noise_radius {
        let mut z = -current;
        while z <= current {
            let z_edge = z.abs() == current;
            let mut x = -current;
            while x <= current {
                if !find_closest || z_edge || x.abs() == current {
                    let (qx, qz) = (center_x + x, center_z + z);
                    let biome = biome_at(qx, noise_y, qz);
                    if allowed(biome) {
                        if result.is_none() || random.next_int_bound(found + 1) == Ok(0) {
                            let position = [qx << 2, origin[1], qz << 2];
                            if find_closest {
                                return Some((position, biome));
                            }
                            result = Some((position, biome));
                        }
                        found += 1;
                    }
                }
                x += skip;
            }
            z += skip;
        }
        current += skip;
    }
    result
}

/// `generateRingPositions`' ring positions before their searches, for every concentric-rings set
/// among the level's possible sets, sets in order. Run each search (in parallel if wanted) and
/// give the results, in this order, to [`LevelPlacements::new`].
pub fn ring_searches(registry: &StructureRegistry, params: &LevelParams<'_>) -> Vec<RingSearch> {
    let mut searches = Vec::new();
    for set in params.possible_sets(registry) {
        let Placement::ConcentricRings(rings) = &registry.set(set).placement else {
            continue;
        };
        let seed = match params.seeds.stronghold {
            Some(seed) if registry.set(set).id == "minecraft:strongholds" => seed,
            _ => params.ring_seed,
        };
        let mut random = LegacyRandom::new(seed);
        let (distance, count) = (rings.distance, rings.count);
        let mut spread = rings.spread;
        let mut angle = random.next_double() * std::f64::consts::PI * 2.0;
        let (mut in_circle, mut circle) = (0, 0);
        for i in 0..count {
            let dist = f64::from(4 * distance + distance * circle * 6)
                + (random.next_double() - 0.5) * (f64::from(distance) * 2.5);
            let x = jmath::round(jmath::cos(angle) * dist) as i32;
            let z = jmath::round(jmath::sin(angle) * dist) as i32;
            searches.push(RingSearch {
                set,
                chunk: ChunkPos::new(x, z),
                random: random.fork(),
            });
            angle += std::f64::consts::PI * 2.0 / f64::from(spread);
            in_circle += 1;
            if in_circle == spread {
                circle += 1;
                in_circle = 0;
                spread += 2 * spread / (circle + 1);
                spread = spread.min(count - i);
                angle += random.next_double() * std::f64::consts::PI * 2.0;
            }
        }
    }
    searches
}

/// A possible set as the level places it (Paper's view of its placement).
#[derive(Clone, Debug)]
struct LevelSet {
    id: StructureSetId,
    /// The set's salt, or Paper's for a `minecraft:` random spread.
    salt: i32,
    /// Paper's `saltOverride` for the keyed mineshaft and buried treasure sets.
    salt_override: Option<i32>,
    /// Concentric ring positions.
    rings: Option<Vec<ChunkPos>>,
}

/// A level's `ChunkGeneratorStructureState`: its possible structure sets, as Paper places them,
/// and their ring positions. Immutable once built.
#[derive(Clone, Debug)]
pub struct LevelPlacements {
    seed: i64,
    origin: ChunkPos,
    sets: Vec<LevelSet>,
}

impl LevelPlacements {
    /// `ring_positions` are the results of [`ring_searches`] for the same `params`, in order.
    pub fn new(
        registry: &StructureRegistry,
        params: &LevelParams<'_>,
        ring_positions: &[ChunkPos],
    ) -> Self {
        let mut ring_positions = ring_positions.iter().copied();
        let sets = params
            .possible_sets(registry)
            .into_iter()
            .map(|id| {
                let set = registry.set(id);
                let mut level_set = LevelSet {
                    id,
                    salt: set.placement.spreading().map_or(0, |s| s.salt),
                    salt_override: None,
                    rings: None,
                };
                match &set.placement {
                    Placement::RandomSpread(_) => {
                        if let Some(path) = set.id.strip_prefix("minecraft:") {
                            level_set.salt = params.seeds.salt(path).unwrap_or(level_set.salt);
                            level_set.salt_override = match path {
                                "mineshafts" => params.seeds.mineshaft,
                                "buried_treasures" => Some(params.seeds.buried_treasure),
                                _ => None,
                            };
                        }
                    }
                    Placement::ConcentricRings(rings) => {
                        let count = rings.count.max(0) as usize;
                        level_set.rings = Some(ring_positions.by_ref().take(count).collect());
                    }
                    Placement::DimensionOrigin => {}
                }
                level_set
            })
            .collect();
        Self {
            seed: params.seed,
            origin: params.origin,
            sets,
        }
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// `possibleStructureSets`, in order.
    pub fn possible_sets(&self) -> impl Iterator<Item = StructureSetId> + '_ {
        self.sets.iter().map(|set| set.id)
    }

    /// `getRingPositionsFor`: `None` for a set that is not a possible concentric-rings set.
    pub fn ring_positions(&self, set: StructureSetId) -> Option<&[ChunkPos]> {
        self.level_set(set)?.rings.as_deref()
    }

    /// `getPlacementsForStructure`: the possible sets placing `structure` where one of its
    /// biomes can exist, in set order.
    pub fn sets_for_structure<'a>(
        &'a self,
        registry: &'a StructureRegistry,
        structure: StructureId,
        possible_biomes: &'a BiomeSet,
    ) -> impl Iterator<Item = StructureSetId> + 'a {
        let placeable = (registry.structure(structure).biomes.iter())
            .any(|biome| possible_biomes.contains(biome));
        self.sets
            .iter()
            .filter(move |set| {
                placeable && (registry.set(set.id).structures.iter()).any(|(s, _)| *s == structure)
            })
            .map(|set| set.id)
    }

    fn level_set(&self, set: StructureSetId) -> Option<&LevelSet> {
        self.sets.iter().find(|level_set| level_set.id == set)
    }

    /// `isStructureChunk` of a possible set as `createStructures` asks it: Paper's salt and
    /// key. `false` for a set that is not possible here.
    pub fn is_structure_chunk(
        &self,
        registry: &StructureRegistry,
        set: StructureSetId,
        x: i32,
        z: i32,
    ) -> bool {
        self.level_set(set).is_some_and(|level_set| {
            self.placement_allows(registry, set, level_set.salt, level_set.salt_override, x, z)
        })
    }

    /// `isStructureChunk` of a set's registry placement (the jar's salt, no key), as an exclusion
    /// zone names it.
    fn registry_placement_allows(
        &self,
        registry: &StructureRegistry,
        set: StructureSetId,
        x: i32,
        z: i32,
    ) -> bool {
        let salt = (registry.set(set).placement.spreading()).map_or(0, |s| s.salt);
        self.placement_allows(registry, set, salt, None, x, z)
    }

    fn placement_allows(
        &self,
        registry: &StructureRegistry,
        set: StructureSetId,
        salt: i32,
        salt_override: Option<i32>,
        x: i32,
        z: i32,
    ) -> bool {
        let (spreading, placed) = match &registry.set(set).placement {
            Placement::DimensionOrigin => return self.origin == ChunkPos::new(x, z),
            Placement::RandomSpread(spread) => (
                &spread.spreading,
                spread.is_placement_chunk(self.seed, salt, x, z),
            ),
            Placement::ConcentricRings(rings) => (
                &rings.spreading,
                self.ring_positions(set)
                    .is_some_and(|positions| positions.contains(&ChunkPos::new(x, z))),
            ),
        };
        placed
            && spreading.additional_restrictions(self.seed, salt, x, z, salt_override)
            && spreading.exclusion_zone.is_none_or(|zone| {
                !self.has_structure_chunk_in_range(registry, zone.other_set, x, z, zone.chunk_count)
            })
    }

    /// `hasStructureChunkInRange` for an exclusion zone's set.
    pub fn has_structure_chunk_in_range(
        &self,
        registry: &StructureRegistry,
        set: StructureSetId,
        x: i32,
        z: i32,
        range: i32,
    ) -> bool {
        (x.wrapping_sub(range)..=x.wrapping_add(range)).any(|test_x| {
            (z.wrapping_sub(range)..=z.wrapping_add(range))
                .any(|test_z| self.registry_placement_allows(registry, set, test_x, test_z))
        })
    }

    /// `createStructures`' choices for one chunk: for each possible set (in order) without a
    /// valid start already in the chunk (`has_start`) whose placement allows the chunk, the
    /// set's structures are tried with `try_generate` (which makes and keeps a start and says
    /// whether it did): a lone structure once, several in weighted random order (drawn from
    /// `setLargeFeatureSeed`) until one starts.
    pub fn create_starts(
        &self,
        registry: &StructureRegistry,
        chunk: ChunkPos,
        mut has_start: impl FnMut(StructureId) -> bool,
        mut try_generate: impl FnMut(StructureId) -> bool,
    ) {
        for level_set in &self.sets {
            let set = registry.set(level_set.id);
            if set.structures.iter().any(|(s, _)| has_start(*s)) {
                continue;
            }
            if !self.is_structure_chunk(registry, level_set.id, chunk.x, chunk.z) {
                continue;
            }
            if let [(only, _)] = set.structures[..] {
                try_generate(only);
                continue;
            }
            let mut options = set.structures.clone();
            let mut random = worldgen_random();
            random.set_large_feature_seed(self.seed, chunk.x, chunk.z);
            let mut total: i32 = options.iter().map(|(_, weight)| weight).sum();
            while !options.is_empty() {
                let mut choice = random.next_int_bound(total).unwrap_or(0);
                let mut index = 0;
                for (_, weight) in &options {
                    choice -= weight;
                    if choice < 0 {
                        break;
                    }
                    index += 1;
                }
                let (selected, weight) = options[index];
                if try_generate(selected) {
                    break;
                }
                options.remove(index);
                total -= weight;
            }
        }
    }
}
