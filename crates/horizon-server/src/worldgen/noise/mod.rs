//! The gradient noises of 26.3's world generation (`ImprovedNoise`, the smeared variant of
//! `BlendedNoise`'s octaves, `SimplexNoise`, `NormalNoise`'s octave stacks), copied from Horizon's
//! `horizon-core::noise` (checked there against the 26.3 jar) and reshaped for the server: a stack
//! owns its octaves as a closed enum instead of `Arc<dyn Noise>` layers.
//!
//! Coordinates are doubles; Perlin interpolation and its derivatives are floats, as in 26.3. Keep
//! the operation order: algebraic rearrangement changes world generation.

mod normal;
mod perlin;
mod simplex;
mod stack;
mod volume;

pub use normal::{
    NoiseBuildError, NormalNoise, NormalNoiseBuilder, Normalization, OctaveInfo, create_legacy_fbm,
};
pub use perlin::{PerlinNoise, SmearedPerlinNoise};
pub use simplex::SimplexNoise;
pub use stack::{NoiseLayer, NoiseStack, NoiseStackBuilder, Octave};
pub use volume::{DensityError, DensityVolume};

use crate::util::random::RandomSource;

/// `Interval.of` refused a NaN bound while a noise range was computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("noise interval bounds cannot include NaN")]
pub struct NanRange;

/// The immutable offsets and permutation table shared by gradient samplers.
#[derive(Clone, Debug)]
pub struct GradientTable {
    offsets: [f64; 3],
    permutations: [u8; 256],
}

impl GradientTable {
    pub fn new(random: &mut impl RandomSource, offset_scale: f64) -> Self {
        // Zero scale still consumes all three draws.
        let offsets = std::array::from_fn(|_| random.next_double() * offset_scale);
        let mut permutations = std::array::from_fn(|i| i as u8);
        for i in 0..256 {
            let other = i + random
                .next_int_bound((256 - i) as i32)
                .expect("a positive bound") as usize;
            permutations.swap(i, other);
        }
        Self {
            offsets,
            permutations,
        }
    }

    pub fn offsets(&self) -> &[f64; 3] {
        &self.offsets
    }
    pub fn permutations(&self) -> &[u8; 256] {
        &self.permutations
    }

    fn permute(&self, index: i32) -> i32 {
        i32::from(self.permutations[(index & 255) as usize])
    }

    fn gradient(&self, index: i32) -> [f32; 3] {
        PERLIN_GRADIENTS[(self.permute(index) & 15) as usize]
    }
}

// Perlin's 16-direction table includes four repeated directions. Simplex uses
// the first twelve directions, selecting them with remainder instead of masking.
const GRADIENTS: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

/// [`GRADIENTS`] as floats, so a dot product needs no conversions: every
/// component is -1, 0 or 1, which a float holds exactly.
const PERLIN_GRADIENTS: [[f32; 3]; 16] = {
    let mut out = [[0.0; 3]; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = [
            GRADIENTS[i][0] as f32,
            GRADIENTS[i][1] as f32,
            GRADIENTS[i][2] as f32,
        ];
        i += 1;
    }
    out
};

fn dot(gradient: [f32; 3], point: [f32; 3]) -> f32 {
    gradient[0] * point[0] + gradient[1] * point[1] + gradient[2] * point[2]
}

/// Vanilla's periodic coordinate reduction with the normal Far Lands flag off.
/// The fast-path boundary is the double immediately below 2^24.
pub fn wrap(value: f64) -> f64 {
    let half = f64::from_bits(16_777_216.0_f64.to_bits() - 1);
    if value >= -half && value < half {
        value
    } else {
        value - (value / 33_554_432.0 + 0.5).floor() * 33_554_432.0
    }
}

fn smoothstep(value: f32) -> f32 {
    value * value * value * (value * (value * 6.0 - 15.0) + 10.0)
}
fn smoothstep_derivative(value: f32) -> f32 {
    30.0 * value * value * (value - 1.0) * (value - 1.0)
}
fn lerp(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * (end - start)
}
fn lerp2(a: f32, b: f32, values: [f32; 4]) -> f32 {
    lerp(
        b,
        lerp(a, values[0], values[1]),
        lerp(a, values[2], values[3]),
    )
}
fn lerp3(fade: [f32; 3], values: [f32; 8]) -> f32 {
    let [a, b, c, d, e, f, g, h] = values;
    lerp(
        fade[2],
        lerp2(fade[0], fade[1], [a, b, c, d]),
        lerp2(fade[0], fade[1], [e, f, g, h]),
    )
}

#[cfg(test)]
mod tests;
