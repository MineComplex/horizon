//! `BiomeManager.getBiome`'s fuzzy zoom: which quart a block reads its biome from, as Leaf runs
//! it.
//!
//! Leaf replaces vanilla's loop with Carpet-Fixes' (`0087-Carpet-Fixes-Optimized-getBiome-method`):
//! the same eight jittered corners in the same order, with a strict `>` to take a nearer one, but
//! the squared distance is summed `x² + y² + z²` where vanilla sums `z² + y² + x²`, so the two can
//! round differently on near ties. Its early exits skip only corners whose lower bound already
//! exceeds the best distance (rounding is monotonic, so such a corner could not have won) and do
//! not change the result. Paper's and Leaf's fiddle table, `(i - 512) * (0.9 / 1024)`, is
//! bit-identical to vanilla's `(i / 1024 - 0.5) * 0.9`. Leaf's per-level result cache (`0218`,
//! `OptimizeBiome`) stores results only.
//!
//! Copied from Horizon's `horizon-core::biome` (whose corner prefixes are mixed once and shared)
//! with Leaf's summation order.

use crate::core::position::BlockPos;

const A: i64 = 6_364_136_223_846_793_005;
const B: i64 = 1_442_695_040_888_963_407;

/// `LinearCongruentialGenerator.next` without the addend.
fn mix(m: i64) -> i64 {
    m.wrapping_mul(m.wrapping_mul(A).wrapping_add(B))
}

/// `getFiddle`: a jitter in [-0.45, 0.45) from bits 24..34 of the mixed seed.
fn fiddle(seed: i64) -> f64 {
    ((((seed as u64) >> 24) & 1023) as f64 - 512.0) * (0.9 / 1024.0)
}

/// A level's `BiomeManager` zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BiomeZoom {
    seed: i64,
}

impl BiomeZoom {
    /// From the obfuscated seed (`BiomeManager.obfuscateSeed`, what the login packet carries),
    /// not the world seed.
    pub const fn new(obfuscated_seed: i64) -> Self {
        Self {
            seed: obfuscated_seed,
        }
    }

    /// The quart `getBiome(x, y, z)` reads: the nearest of the eight jittered corners around
    /// the block shifted by -2, the first in corner order on a tie.
    pub fn quart_at_block(self, block: BlockPos) -> [i32; 3] {
        let shifted = [block.x, block.y, block.z].map(|v| v.wrapping_sub(2));
        let base = shifted.map(|v| v >> 2);
        let fractions = shifted.map(|v| f64::from(v & 3) / 4.0);
        let c = base.map(i64::from);
        let seed = self.seed;
        // The six coordinate steps of each corner; the first ones are shared between corners
        // (`next(m, salt + offset) == next(m, salt) + offset`).
        let root = mix(seed);
        let m0: [i64; 2] = std::array::from_fn(|x| root.wrapping_add(c[0] + x as i64));
        let m1: [[i64; 2]; 2] = std::array::from_fn(|x| {
            std::array::from_fn(|y| mix(m0[x]).wrapping_add(c[1] + y as i64))
        });
        let step = |prev: &[[[i64; 2]; 2]; 2], axis: usize| -> [[[i64; 2]; 2]; 2] {
            std::array::from_fn(|x| {
                std::array::from_fn(|y| {
                    std::array::from_fn(|z| {
                        let offset = [x, y, z][axis] as i64;
                        mix(prev[x][y][z]).wrapping_add(c[axis] + offset)
                    })
                })
            })
        };
        let m2: [[[i64; 2]; 2]; 2] = std::array::from_fn(|x| {
            std::array::from_fn(|y| {
                std::array::from_fn(|z| mix(m1[x][y]).wrapping_add(c[2] + z as i64))
            })
        });
        let m3 = step(&m2, 0);
        let m4 = step(&m3, 1);
        let m5 = step(&m4, 2);
        let mut selected = 0;
        let mut best = f64::INFINITY;
        for corner in 0..8 {
            let (x, y, z) = (corner >> 2 & 1, corner >> 1 & 1, corner & 1);
            let fx = m5[x][y][z];
            let fy = mix(fx).wrapping_add(seed);
            let fz = mix(fy).wrapping_add(seed);
            let dx = fractions[0] - x as f64 + fiddle(fx);
            let dy = fractions[1] - y as f64 + fiddle(fy);
            let dz = fractions[2] - z as f64 + fiddle(fz);
            let distance = dx * dx + dy * dy + dz * dz;
            if best > distance {
                best = distance;
                selected = corner;
            }
        }
        [
            base[0].wrapping_add((selected >> 2 & 1) as i32),
            base[1].wrapping_add((selected >> 1 & 1) as i32),
            base[2].wrapping_add((selected & 1) as i32),
        ]
    }
}

/// `getNoiseBiomeAtPosition(BlockPos)`: the quart containing the block, no zoom.
pub fn noise_quart_at_block(block: BlockPos) -> [i32; 3] {
    [block.x, block.y, block.z].map(|v| v >> 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Leaf's loop written out per corner, without shared prefixes or early exits.
    fn reference(seed: i64, block: BlockPos) -> [i32; 3] {
        let shifted = [block.x, block.y, block.z].map(|v| v.wrapping_sub(2));
        let base = shifted.map(|v| v >> 2);
        let fractions = shifted.map(|v| f64::from(v & 3) / 4.0);
        let next = |m: i64, salt: i64| mix(m).wrapping_add(salt);
        let mut selected = base;
        let mut best = f64::INFINITY;
        for corner in 0..8 {
            let offsets = [(corner >> 2) & 1, (corner >> 1) & 1, corner & 1];
            let quart: [i32; 3] = std::array::from_fn(|i| base[i].wrapping_add(offsets[i]));
            let mut m = seed;
            for coordinate in quart.into_iter().cycle().take(6) {
                m = next(m, i64::from(coordinate));
            }
            let square = |v: f64| v * v;
            let sx = square(fractions[0] - f64::from(offsets[0]) + fiddle(m));
            m = next(m, seed);
            let sy = square(fractions[1] - f64::from(offsets[1]) + fiddle(m));
            m = next(m, seed);
            let distance = sx + sy + square(fractions[2] - f64::from(offsets[2]) + fiddle(m));
            if best > distance {
                best = distance;
                selected = quart;
            }
        }
        selected
    }

    #[test]
    fn quart_at_block_matches_the_reference() {
        let extremes = [
            i32::MIN,
            i32::MIN + 1,
            -2,
            -1,
            0,
            1,
            2,
            3,
            i32::MAX - 1,
            i32::MAX,
        ];
        for seed in [0i64, 1, -1, i64::MIN, i64::MAX, 0x0123_4567_89ab_cdef] {
            let zoom = BiomeZoom::new(seed);
            for x in -20..20 {
                for y in -20..20 {
                    for z in -20..20 {
                        let block = BlockPos::new(x * 3, y, z * 5);
                        assert_eq!(zoom.quart_at_block(block), reference(seed, block));
                    }
                }
            }
            for x in extremes {
                for y in extremes {
                    for z in extremes {
                        let block = BlockPos::new(x, y, z);
                        assert_eq!(zoom.quart_at_block(block), reference(seed, block));
                    }
                }
            }
        }
    }

    #[test]
    fn fiddle_matches_vanilla_formula() {
        for i in 0..1024i64 {
            let vanilla = ((i as f64) / 1024.0 - 0.5) * 0.9;
            assert_eq!(fiddle(i << 24).to_bits(), vanilla.to_bits());
        }
    }
}
