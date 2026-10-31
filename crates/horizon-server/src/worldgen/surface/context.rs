//! `MaterialRuleContext` and the rules' and conditions' evaluation: the position being decided,
//! the column's surface depth and gradient, the stone above and below, and the lazily sampled
//! inputs (biome, noises, secondary depth, preliminary surface, ore vein densities).

use super::rules::{Condition, ConditionId, Gradient, OreVein, Rule, RuleId};
use super::{SurfaceRandom, SurfaceSystem};
use crate::block::BlockState;
use crate::core::position::BlockPos;
use crate::util::random::{PositionalRandom, RandomSource};
use crate::worldgen::anchor::{OptionallyFlatBedrock, WorldGenerationContext};
use crate::worldgen::density::{DensityContext, FunctionId, NodeId};
use crate::worldgen::noise::DensityVolume;
use crate::worldgen::structure::BiomeSet;

/// A chunk's rule evaluation state, for one worker.
pub(super) struct RuleContext<'s, B> {
    system: &'s SurfaceSystem<'s>,
    density: &'s mut DensityContext,
    biome_getter: B,
    /// The noise volume up to the highest filled section (`expectedVolume`), which the ore veins'
    /// densities are prefilled over.
    expected: DensityVolume,
    /// The one-block-high volume of the preliminary surface level, at `y = 0`.
    preliminary_volume: DensityVolume,
    generation: WorldGenerationContext,
    /// The biome conditions `possibleBiomes` decide, by condition.
    decided: Vec<Option<bool>>,
    x: i32,
    y: i32,
    z: i32,
    gradient: [i32; 2],
    depth: i32,
    stone_above: i32,
    stone_below: i32,
    water_height: i32,
    biome: Option<u32>,
    secondary: Option<f64>,
    min_surface: Option<i32>,
    /// By the rules' noises: the column's value, or the block's for 3D conditions.
    noise_2d: Vec<Option<f64>>,
    noise_3d: Vec<Option<f64>>,
    preliminary: Option<Vec<f32>>,
    /// Prefilled buffers over `expected`, by function root.
    prefilled: Vec<(NodeId, Vec<f32>)>,
}

impl<'s, B: FnMut(BlockPos) -> u32> RuleContext<'s, B> {
    pub(super) fn new(
        system: &'s SurfaceSystem<'s>,
        density: &'s mut DensityContext,
        expected: DensityVolume,
        generation: WorldGenerationContext,
        possible_biomes: Option<&BiomeSet>,
        biome_getter: B,
    ) -> Self {
        let [size_x, _, size_z] = expected.sizes();
        let [min_x, _, min_z] = expected.minimum();
        let preliminary_volume =
            DensityVolume::new([size_x as i32, 1, size_z as i32], [min_x, 0, min_z], [1; 3])
                .expect("a chunk's column volume is valid");
        // `BiomeCondition.compile`: with the biomes the chunk may hold known, a condition that
        // can never match is false and one every possible biome satisfies is true.
        let decided = possible_biomes.map_or_else(Vec::new, |possible| {
            system
                .rules
                .conditions()
                .iter()
                .map(|condition| match condition {
                    Condition::Biome(set) if !set.iter().any(|b| possible.contains(b)) => {
                        Some(false)
                    }
                    Condition::Biome(set) if possible.iter().all(|b| set.contains(b)) => Some(true),
                    _ => None,
                })
                .collect()
        });
        let noises = system.rule_noises.len();
        Self {
            system,
            density,
            biome_getter,
            expected,
            preliminary_volume,
            generation,
            decided,
            x: 0,
            y: 0,
            z: 0,
            gradient: [0; 2],
            depth: 0,
            stone_above: 0,
            stone_below: 0,
            water_height: 0,
            biome: None,
            secondary: None,
            min_surface: None,
            noise_2d: vec![None; noises],
            noise_3d: vec![None; noises],
            preliminary: None,
            prefilled: Vec::new(),
        }
    }

    /// `updateXZ`.
    pub(super) fn update_xz(&mut self, x: i32, z: i32, gradient: [i32; 2]) {
        (self.x, self.z, self.gradient) = (x, z, gradient);
        self.depth = self.system.surface_depth(x, z);
        self.secondary = None;
        self.min_surface = None;
        self.noise_2d.fill(None);
        self.noise_3d.fill(None);
    }

    /// `updateY`.
    pub(super) fn update_y(&mut self, stone_above: i32, stone_below: i32, water: i32, y: i32) {
        self.biome = None;
        self.noise_3d.fill(None);
        (
            self.y,
            self.stone_above,
            self.stone_below,
            self.water_height,
        ) = (y, stone_above, stone_below, water);
    }

    /// The biome getter, for the column's surface biome.
    pub(super) fn biome_at(&mut self, pos: BlockPos) -> u32 {
        (self.biome_getter)(pos)
    }

    fn biome(&mut self) -> u32 {
        match self.biome {
            Some(biome) => biome,
            None => {
                let biome = (self.biome_getter)(BlockPos::new(self.x, self.y, self.z));
                *self.biome.insert(biome)
            }
        }
    }

    fn secondary(&mut self) -> f64 {
        *self.secondary.get_or_insert_with(|| {
            f64::from(
                self.system
                    .noises
                    .secondary
                    .get3(f64::from(self.x), 0.0, f64::from(self.z)),
            )
        })
    }

    /// `getMinSurfaceLevel`: the preliminary surface level of the column, from the chunk's
    /// prefilled column volume when the column is in it, else sampled.
    pub(super) fn min_surface_level(&mut self) -> i32 {
        if let Some(level) = self.min_surface {
            return level;
        }
        let program = &self.system.level.program;
        let node = self.system.preliminary_surface;
        let level = match self.preliminary_volume.index_of_block([self.x, 0, self.z]) {
            Some(index) => {
                let volume = self.preliminary_volume;
                let buffer = self.preliminary.get_or_insert_with(|| {
                    let mut buffer = vec![0.0; volume.size()];
                    program.fill(self.density, node, &mut buffer, volume);
                    buffer
                });
                buffer[index]
            }
            None => program.sample(self.density, node, [self.x, 0, self.z]),
        };
        let minimum = (level.floor() as i32)
            .wrapping_add(self.depth)
            .wrapping_sub(8);
        *self.min_surface.insert(minimum)
    }

    /// An ore vein function at the block: from the buffer prefilled over the expected volume when
    /// the block is in it (`getDensitiesInChunk(function, true)`), else sampled.
    fn prefilled(&mut self, function: FunctionId) -> f32 {
        let program = &self.system.level.program;
        let node = self.system.level.surface_node(function);
        let position = [self.x, self.y, self.z];
        let Some(index) = self.expected.index_of_block(position) else {
            return program.sample(self.density, node, position);
        };
        let slot = match self.prefilled.iter().position(|(n, _)| *n == node) {
            Some(slot) => slot,
            None => {
                let mut buffer = vec![0.0; self.expected.size()];
                program.fill(self.density, node, &mut buffer, self.expected);
                self.prefilled.push((node, buffer));
                self.prefilled.len() - 1
            }
        };
        self.prefilled[slot].1[index]
    }

    /// `rule.tryApply` at the current position.
    pub(super) fn apply(&mut self, rule: RuleId) -> Option<BlockState> {
        let rules = self.system.rules;
        match rules.rule(rule) {
            Rule::Block(state) => Some(*state),
            Rule::Bandlands => Some(self.system.band(self.x, self.z, self.y)),
            Rule::Sequence(items) => items.iter().find_map(|item| self.apply(*item)),
            Rule::If { condition, then } => {
                if self.test(*condition) {
                    self.apply(*then)
                } else {
                    None
                }
            }
            Rule::OreVein(vein) => self.ore_vein(vein),
        }
    }

    /// `OreVeinRule`'s evaluator.
    fn ore_vein(&mut self, vein: &OreVein) -> Option<BlockState> {
        let density = self.prefilled(vein.density);
        if density <= 0.0 {
            return None;
        }
        // The random is local to the block, so drawing all three floats now changes nothing.
        let at = BlockPos::new(self.x, self.y, self.z);
        let [first, second, third] = match self.system.ore_random {
            SurfaceRandom::Legacy(root) => floats(root.at(at)),
            SurfaceRandom::Xoroshiro(root) => floats(root.at(at)),
        };
        if first > density {
            return None;
        }
        let richness = self.prefilled(vein.richness);
        let program = &self.system.level.program;
        let gap = program.sample(
            self.density,
            self.system.level.surface_node(vein.filler_gap),
            [self.x, self.y, self.z],
        );
        Some(if second < richness && gap < 0.0 {
            if third < vein.raw_ore_chance {
                vein.raw_ore
            } else {
                vein.ore
            }
        } else {
            vein.filler
        })
    }

    fn noise(&mut self, noise: usize, is_3d: bool) -> f64 {
        let cache = if is_3d {
            &mut self.noise_3d
        } else {
            &mut self.noise_2d
        };
        if let Some(value) = cache[noise] {
            return value;
        }
        let y = if is_3d { f64::from(self.y) } else { 0.0 };
        let value =
            f64::from(self.system.rule_noises[noise].get3(f64::from(self.x), y, f64::from(self.z)));
        let cache = if is_3d {
            &mut self.noise_3d
        } else {
            &mut self.noise_2d
        };
        *cache[noise].insert(value)
    }

    /// A vertical gradient: vanilla's, or Paper's with its flat bedrock setting.
    fn gradient(&self, gradient: &Gradient) -> bool {
        let condition = OptionallyFlatBedrock {
            random_name: String::new(),
            true_at_and_below: gradient.true_at_and_below,
            false_at_and_above: gradient.false_at_and_above,
            is_roof: gradient.paper_roof == Some(true),
        };
        let flat = self.system.flat_bedrock && gradient.paper_roof.is_some();
        let bounds = condition.bounds(self.generation, flat);
        let (x, y, z) = (self.x, self.y, self.z);
        match &self.system.rule_randoms[gradient.random] {
            SurfaceRandom::Legacy(random) => bounds.test(random, x, y, z),
            SurfaceRandom::Xoroshiro(random) => bounds.test(random, x, y, z),
        }
    }

    /// `ConditionEvaluator.test`.
    fn test(&mut self, id: ConditionId) -> bool {
        let rules = self.system.rules;
        match rules.condition(id) {
            Condition::Biome(set) => match self.decided.get(id.index()).copied().flatten() {
                Some(decided) => decided,
                None => set.contains(self.biome()),
            },
            Condition::Noise {
                noise,
                min,
                max,
                is_3d,
            } => {
                let value = self.noise(*noise, *is_3d);
                value >= *min && value <= *max
            }
            Condition::Gradient(gradient) => self.gradient(gradient),
            Condition::YAbove {
                anchor,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                let above = if *add_stone_depth {
                    self.stone_above
                } else {
                    0
                };
                self.y.wrapping_add(above)
                    >= anchor
                        .resolve(self.generation)
                        .wrapping_add(self.depth.wrapping_mul(*surface_depth_multiplier))
            }
            Condition::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                let above = if *add_stone_depth {
                    self.stone_above
                } else {
                    0
                };
                self.water_height == i32::MIN
                    || self.y.wrapping_add(above)
                        >= self
                            .water_height
                            .wrapping_add(*offset)
                            .wrapping_add(self.depth.wrapping_mul(*surface_depth_multiplier))
            }
            Condition::Temperature => {
                let biome = self.biome();
                self.system.climates.cold_enough_to_snow(
                    biome,
                    BlockPos::new(self.x, self.y, self.z),
                    self.system.settings.sea_level,
                )
            }
            Condition::Steep => self.gradient[0] <= -4 || self.gradient[1] >= 4,
            Condition::Not(inner) => !self.test(*inner),
            Condition::Hole => self.depth <= 0,
            Condition::AbovePreliminarySurface => self.y >= self.min_surface_level(),
            Condition::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                ceiling,
            } => {
                let stone = if *ceiling {
                    self.stone_below
                } else {
                    self.stone_above
                };
                let surface = if *add_surface_depth { self.depth } else { 0 };
                let secondary = if *secondary_depth_range == 0 {
                    0
                } else {
                    // `Mth.map(secondary, -1, 1, 0, range)`, cast to int.
                    let t = (self.secondary() - -1.0) / (1.0 - -1.0);
                    (0.0 + t * (f64::from(*secondary_depth_range) - 0.0)) as i32
                };
                stone
                    <= 1i32
                        .wrapping_add(*offset)
                        .wrapping_add(surface)
                        .wrapping_add(secondary)
            }
        }
    }
}

fn floats(mut random: impl RandomSource) -> [f32; 3] {
    std::array::from_fn(|_| random.next_float())
}
