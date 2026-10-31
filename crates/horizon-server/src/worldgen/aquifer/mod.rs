//! `Aquifer` (26.3's `levelgen.Aquifer`): which fluid, air or solid fills a position the density
//! leaves empty. [`Aquifer::Disabled`] is `createDisabled` (Nether, End), [`NoiseBasedAquifer`]
//! the underground fluid pockets with their barriers. Copied from Horizon's
//! `horizon-world::worldgen::aquifer`, whose density samplers became node ids of the level's
//! [`DensityProgram`] and whose registry traits became plain block ids.

mod noise_based;

pub use noise_based::NoiseBasedAquifer;

use super::density::{DensityContext, DensityProgram, NodeId};
use super::noise::DensityVolume;
use super::settings::NoiseGeneratorSettings;
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::position::BlockPos;
use crate::util::random::{
    LegacyPositionalRandom, LegacyRandom, PositionalRandom, RandomSource,
    XoroshiroPositionalRandom, XoroshiroRandom,
};

/// Why an aquifer could not answer. Vanilla throws in the same places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AquiferError {
    #[error("aquifer grid is invalid or exceeds 16777216 cells")]
    InvalidGrid,
    #[error("aquifer query lies outside its grid")]
    OutsideGrid,
    #[error("the block table lacks air, cave_air, void_air, water or lava")]
    MissingBlock,
}

/// A density program with the context a chunk evaluates it in (an aquifer's
/// `DensitySampler.Bound`s in Java).
pub struct Sampler<'a> {
    pub program: &'a DensityProgram,
    pub context: &'a mut DensityContext,
}

impl Sampler<'_> {
    fn value(&mut self, node: NodeId, position: [i32; 3]) -> f64 {
        f64::from(self.program.sample(self.context, node, position))
    }
}

/// The blocks the aquifer tells apart (`state.is(Blocks.X)` compares the block, whatever its
/// state; "default lava" is the exact state), with the table that maps states to blocks.
#[derive(Clone, Copy, Debug)]
pub struct FluidBlocks<'a> {
    states: &'a BlockStates,
    air: [BlockId; 3],
    water: BlockId,
    lava: BlockId,
    lava_state: BlockState,
}

impl<'a> FluidBlocks<'a> {
    pub fn new(states: &'a BlockStates) -> Result<Self, AquiferError> {
        let block = |name| states.block_by_name(name).ok_or(AquiferError::MissingBlock);
        let lava = block("minecraft:lava")?;
        Ok(Self {
            states,
            air: [
                block("minecraft:air")?,
                block("minecraft:cave_air")?,
                block("minecraft:void_air")?,
            ],
            water: block("minecraft:water")?,
            lava,
            lava_state: states.default_state(lava),
        })
    }
    fn is_air(&self, state: BlockState) -> bool {
        self.air.contains(&self.states.block(state))
    }
    fn is_lava(&self, state: BlockState) -> bool {
        self.states.block(state) == self.lava
    }
    fn is_water(&self, state: BlockState) -> bool {
        self.states.block(state) == self.water
    }
}

/// `FluidStatus`: fluid fills below `level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidStatus {
    pub level: i32,
    pub state: BlockState,
}

impl FluidStatus {
    /// `at(blockY)`.
    pub const fn at(self, y: i32) -> BlockState {
        if y < self.level {
            self.state
        } else {
            BlockState::AIR
        }
    }
}

/// `NoiseBasedChunkGenerator.createFluidPicker`: lava below `min(-54, sea level)`, else the
/// default fluid up to sea level. It looks at the height only.
#[derive(Clone, Copy, Debug)]
pub struct GlobalFluidPicker {
    lava: FluidStatus,
    sea: FluidStatus,
}

impl GlobalFluidPicker {
    pub fn new(settings: &NoiseGeneratorSettings, blocks: &FluidBlocks<'_>) -> Self {
        Self {
            lava: FluidStatus {
                level: -54,
                state: blocks.lava_state,
            },
            sea: FluidStatus {
                level: settings.sea_level,
                state: settings.default_fluid,
            },
        }
    }
    pub fn compute(self, y: i32) -> FluidStatus {
        if y < (-54).min(self.sea.level) {
            self.lava
        } else {
            self.sea
        }
    }
}

/// `RandomState.getOrCreateRandomFactory("minecraft:aquifer")` of a level, kept as what the
/// aquifer asks of it: the three offsets of a grid cell's point.
#[derive(Clone, Copy, Debug)]
pub enum AquiferRandom {
    Legacy(LegacyPositionalRandom),
    Xoroshiro(XoroshiroPositionalRandom),
}

impl AquiferRandom {
    /// For a level's seed and the noise settings' `legacy_random_source`.
    pub fn new(seed: i64, legacy_random_source: bool) -> Self {
        if legacy_random_source {
            let root = LegacyRandom::new(seed).fork_positional();
            Self::Legacy(root.from_hash_of("minecraft:aquifer").fork_positional())
        } else {
            let root = XoroshiroRandom::new(seed).fork_positional();
            Self::Xoroshiro(root.from_hash_of("minecraft:aquifer").fork_positional())
        }
    }
    /// `at(cell)`, then `nextInt(10)`, `nextInt(9)`, `nextInt(10)`.
    fn offsets(&self, cell: [i32; 3]) -> [i32; 3] {
        fn draw(mut random: impl RandomSource) -> [i32; 3] {
            [10, 9, 10].map(|bound| random.next_int_bound(bound).expect("a positive bound"))
        }
        let at = BlockPos::new(cell[0], cell[1], cell[2]);
        match self {
            Self::Legacy(factory) => draw(factory.at(at)),
            Self::Xoroshiro(factory) => draw(factory.at(at)),
        }
    }
}

/// The aquifer of one noise chunk.
pub enum Aquifer<'a> {
    /// `Aquifer.createDisabled`.
    Disabled {
        picker: GlobalFluidPicker,
    },
    NoiseBased(Box<NoiseBasedAquifer<'a>>),
}

impl<'a> Aquifer<'a> {
    /// `NoiseChunk`'s choice: the settings' aquifer roots when there are some. `volume` is the
    /// chunk's; the roots are sampled in `sampler`'s context.
    pub fn new(
        roots: Option<[NodeId; 6]>,
        random: AquiferRandom,
        picker: GlobalFluidPicker,
        blocks: FluidBlocks<'a>,
        volume: DensityVolume,
        sampler: &mut Sampler<'_>,
    ) -> Result<Self, AquiferError> {
        Ok(match roots {
            Some(roots) => Self::NoiseBased(Box::new(NoiseBasedAquifer::new(
                roots, random, picker, blocks, volume, sampler,
            )?)),
            None => Self::Disabled { picker },
        })
    }

    /// `computeSubstance`: `None` is the generator's default block.
    pub fn compute_substance(
        &mut self,
        sampler: &mut Sampler<'_>,
        position: [i32; 3],
        density: f64,
    ) -> Result<Option<BlockState>, AquiferError> {
        match self {
            // A NaN density counts as not positive, as in Java.
            Self::Disabled { picker } => Ok((density.is_nan() || density <= 0.0)
                .then(|| picker.compute(position[1]).at(position[1]))),
            Self::NoiseBased(aquifer) => aquifer.compute_substance(sampler, position, density),
        }
    }

    pub fn should_schedule_fluid_update(&self) -> bool {
        match self {
            Self::Disabled { .. } => false,
            Self::NoiseBased(aquifer) => aquifer.should_schedule_fluid_update(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(sea_level: i32) -> GlobalFluidPicker {
        GlobalFluidPicker {
            lava: FluidStatus {
                level: -54,
                state: BlockState(7),
            },
            sea: FluidStatus {
                level: sea_level,
                state: BlockState(5),
            },
        }
    }

    #[test]
    fn fluid_fills_below_its_level_only() {
        let status = FluidStatus {
            level: 63,
            state: BlockState(5),
        };
        assert_eq!(status.at(62), BlockState(5));
        assert_eq!(status.at(63), BlockState::AIR);
    }

    #[test]
    fn global_picker_has_lava_below_the_lower_of_minus_54_and_sea_level() {
        assert_eq!(picker(63).compute(-55).state, BlockState(7));
        assert_eq!(picker(63).compute(-54).state, BlockState(5));
        // A sea level under -54 moves the lava floor down with it.
        assert_eq!(picker(-60).compute(-55).state, BlockState(5));
        assert_eq!(picker(-60).compute(-61).state, BlockState(7));
    }

    #[test]
    fn cell_points_stay_in_their_ranges_and_are_per_cell() {
        for random in [
            AquiferRandom::new(12345, false),
            AquiferRandom::new(12345, true),
        ] {
            for cell in [[0, 0, 0], [-3, -6, 9], [1000, 4, -1000]] {
                let [x, y, z] = random.offsets(cell);
                assert!((0..10).contains(&x) && (0..9).contains(&y) && (0..10).contains(&z));
                assert_eq!(random.offsets(cell), [x, y, z]);
            }
            assert_ne!(random.offsets([0, 0, 0]), random.offsets([0, 0, 1]));
        }
    }
}
