//! Density functions (`net.minecraft.world.level.levelgen.densityfunction` in 26.3).
//!
//! Three layers, each owned by one place:
//!
//! - [`DensityGraph`]: the seed-independent definitions of the registries (`worldgen/noise`,
//!   `worldgen/density_function` and the roots of the noise settings), an append-only arena of
//!   [`GraphNode`]s with vanilla's equality and range analysis. Built once by the data layer.
//! - [`DensityCompiler`] → [`DensityProgram`]: per level (seed + noise settings), the graph's
//!   roots compiled with vanilla's passes into one program: a `Vec` of closed-enum nodes with
//!   children by index, owning its seeded noises. Immutable; one per level, read by every worker.
//! - [`DensityContext`]: a worker's scratch buffers, `cache` cells and bound world fields.
//!
//! Horizon's version of this (`horizon-core::density`) compiled to a tree of
//! `Arc<dyn DensitySampler>`; the program replaces that object graph, with the same arithmetic.

mod beardifier;
mod compile;
mod end_islands;
mod graph;
mod interval;
mod ops;
mod program;
mod spline;
#[cfg(test)]
mod tests;

pub use beardifier::{Beardifier, JigsawJunction, RigidPiece, StructureBounds, TerrainAdjustment};
pub use compile::DensityCompiler;
pub use end_islands::EndIslands;
pub use graph::{
    DensityGraph, FunctionId, GraphBinary, GraphError, GraphNode, GraphUnary, NoiseId,
};
pub use interval::{Interval, IntervalError};
pub use ops::{DistanceMetric, RoundMode, TilingMode};
pub use program::{DensityContext, DensityProgram, WorldFields};
pub use spline::{CubicSpline, SplineError, SplinePoint};

use crate::util::random::RandomSource;
use crate::worldgen::noise::{NoiseBuildError, NoiseStack, Octave, SmearedPerlinNoise};

/// A node of a [`DensityProgram`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Why a level's density functions could not be compiled. Vanilla's data never fails; a datapack
/// can, and then the level cannot be created.
#[derive(Debug, thiserror::Error)]
pub enum ProgramError {
    #[error(transparent)]
    Graph(GraphError),
    #[error(transparent)]
    Noise(#[from] NoiseBuildError),
    #[error(transparent)]
    Spline(#[from] SplineError),
    #[error("range: {0}")]
    Range(#[from] IntervalError),
    #[error("more than 65536 cached density functions")]
    TooManyCaches,
    #[error("a cache marker reached compilation unprepared")]
    UnpreparedCache,
    #[error("an interpolation or surface cell size is not positive")]
    NonpositiveCell,
}

/// The fields a chunk binds at evaluation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorldField {
    BlendAlpha,
    BlendOffset,
    Beardifier,
}

impl WorldField {
    pub fn domain_axes(self) -> u8 {
        match self {
            Self::BlendAlpha | Self::BlendOffset => 5,
            Self::Beardifier => 7,
        }
    }
    pub fn range(self) -> Interval {
        match self {
            Self::BlendAlpha => Interval::new(0.0, 1.0).expect("an ordered interval"),
            Self::BlendOffset | Self::Beardifier => Interval::INFINITE,
        }
    }
    /// The value without a binding.
    fn fallback(self) -> f32 {
        match self {
            Self::BlendAlpha => 1.0,
            Self::BlendOffset | Self::Beardifier => 0.0,
        }
    }
}

/// `shift`, `shift_a` and `shift_b`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShiftNoiseKind {
    Shift,
    ShiftA,
    ShiftB,
}

/// `old_blended_noise`'s parameters.
#[derive(Clone, Copy, Debug)]
pub struct BlendedNoise {
    pub xz_scale: f64,
    pub y_scale: f64,
    pub xz_factor: f64,
    pub y_factor: f64,
    pub smear_scale_multiplier: f64,
}

/// The three octave stacks of a blended noise.
#[derive(Clone, Debug)]
pub struct BlendedFbmSet {
    pub min_limit: NoiseStack,
    pub max_limit: NoiseStack,
    pub main: NoiseStack,
}

impl BlendedNoise {
    pub fn create_fbm_set(
        &self,
        random: &mut impl RandomSource,
    ) -> Result<BlendedFbmSet, NoiseBuildError> {
        let smear = (684.412 * self.y_scale) * self.smear_scale_multiplier;
        let main_smear = smear / self.y_factor;
        Ok(BlendedFbmSet {
            min_limit: create_blended_fbm(random, -15, smear, 0.9999847412109375)?,
            max_limit: create_blended_fbm(random, -15, smear, 0.9999847412109375)?,
            main: create_blended_fbm(random, -7, main_smear, 12.75)?,
        })
    }

    pub fn range(&self) -> Result<Interval, IntervalError> {
        let smear = (684.412 * self.y_scale) * self.smear_scale_multiplier;
        let mut range = Interval::exact(0.0)?;
        let mut frequency = 1.0;
        let mut amplitude = 0.9999847412109375 / 65535.0;
        for _ in 0..16 {
            let base = Interval::symmetric(((smear * frequency).abs() + 2.0) as f32)?;
            range = range.add_range(base.multiply_range(Interval::exact(amplitude as f32)?));
            frequency /= 2.0;
            amplitude *= 2.0;
        }
        Ok(range)
    }
}

/// High-to-low frequency smeared octaves, drawn straight from the caller's stream. At most 4096
/// layers, refused before any draw.
pub fn create_blended_fbm(
    random: &mut impl RandomSource,
    first_octave: i32,
    smear: f64,
    amplitude: f64,
) -> Result<NoiseStack, NoiseBuildError> {
    if first_octave > 0 {
        return Err(NoiseBuildError::PositiveLegacyOctaves);
    }
    if first_octave < -4095 {
        return Err(NoiseBuildError::TooManyOctaves);
    }
    let count = 1 - first_octave;
    let power = if count > 1023 {
        f64::INFINITY
    } else {
        f64::from_bits(((count + 1023) as u64) << 52)
    };
    let mut amplitude = amplitude / (power - 1.0);
    let mut frequency = 1.0;
    let mut builder = NoiseStack::builder();
    for _ in 0..count {
        builder.add(
            Octave::Smeared(SmearedPerlinNoise::new(random, smear * frequency)),
            frequency,
            amplitude as f32,
        );
        frequency /= 2.0;
        amplitude *= 2.0;
    }
    Ok(builder.build()?)
}
