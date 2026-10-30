//! `EndIslandFunction`: 26.3's outer End island density (the central island comes from the
//! terrain graph). Copied from Horizon's `horizon-core::density::end_islands`.
//!
//! Purpur's `settings.generate-end-void-rings` (off by default, Leaf keeps the default) is not
//! ported: with it off the function is vanilla's.

use super::ops::{java_max, java_min};
use crate::util::random::{LegacyRandom, RandomSource};
use crate::worldgen::noise::{DensityVolume, SimplexNoise};

#[derive(Clone, Debug)]
pub struct EndIslands {
    noise: SimplexNoise,
}

impl EndIslands {
    pub fn new(world_seed: i64) -> Self {
        let mut random = LegacyRandom::new(world_seed);
        random.consume_count(17_292);
        Self {
            noise: SimplexNoise::with_skipped_offsets(&mut random, true),
        }
    }

    fn height(&self, x: i32, z: i32) -> f32 {
        // These divisions and remainders truncate toward zero, including for
        // negative coordinates. They are not block/chunk floor conversions.
        let center_x = x / 2;
        let center_z = z / 2;
        let local_x = x % 2;
        let local_z = z % 2;
        let mut height = -100.0;
        for offset_x in -12_i32..=12 {
            for offset_z in -12_i32..=12 {
                let island_x = i64::from(center_x.wrapping_add(offset_x));
                let island_z = i64::from(center_z.wrapping_add(offset_z));
                if island_x * island_x + island_z * island_z <= 4096
                    || self.noise.get2(island_x as f64, island_z as f64) >= -0.9
                {
                    continue;
                }
                let scale = ((island_x as f32).abs() * 3439.0 + (island_z as f32).abs() * 147.0)
                    % 13.0
                    + 9.0;
                let dx = (local_x - offset_x * 2) as f32;
                let dz = (local_z - offset_z * 2) as f32;
                let candidate = 100.0 - ((dx * dx + dz * dz) as f64).sqrt() as f32 * scale;
                let candidate = java_min(java_max(candidate, -100.0), 80.0);
                height = java_max(height, candidate);
            }
        }
        height
    }

    // Nearby volume columns visit overlapping 25-by-25 island neighborhoods.
    // Collect the eligible islands once, keeping their X/Z order.
    // Larger or coordinate-wrapping volumes keep the scalar evaluation path.
    fn volume_islands(&self, volume: DensityVolume) -> Option<Vec<(i32, i32, f32)>> {
        let sizes = volume.sizes();
        if sizes[0] * sizes[2] < 4 {
            return None;
        }
        let minimum = volume.minimum();
        let steps = volume.steps();
        let end = |axis: usize| {
            i32::try_from(
                i64::from(minimum[axis]) + (sizes[axis] - 1) as i64 * i64::from(steps[axis]),
            )
            .ok()
        };
        let first_x = minimum[0] / 8 / 2 - 12;
        let first_z = minimum[2] / 8 / 2 - 12;
        let last_x = end(0)? / 8 / 2 + 12;
        let last_z = end(2)? / 8 / 2 + 12;
        if last_x - first_x >= 64 || last_z - first_z >= 64 {
            return None;
        }
        let mut islands = Vec::new();
        for x in first_x..=last_x {
            for z in first_z..=last_z {
                if i64::from(x) * i64::from(x) + i64::from(z) * i64::from(z) <= 4096
                    || self.noise.get2(f64::from(x), f64::from(z)) >= -0.9
                {
                    continue;
                }
                let scale = ((x as f32).abs() * 3439.0 + (z as f32).abs() * 147.0) % 13.0 + 9.0;
                islands.push((x, z, scale));
            }
        }
        Some(islands)
    }

    fn density_from_islands(x: i32, z: i32, islands: &[(i32, i32, f32)]) -> f32 {
        let x = x / 8;
        let z = z / 8;
        let center_x = x / 2;
        let center_z = z / 2;
        let mut height = -100.0;
        for &(island_x, island_z, scale) in islands {
            let offset_x = island_x - center_x;
            let offset_z = island_z - center_z;
            if !(-12..=12).contains(&offset_x) || !(-12..=12).contains(&offset_z) {
                continue;
            }
            let dx = (x % 2 - offset_x * 2) as f32;
            let dz = (z % 2 - offset_z * 2) as f32;
            let candidate = 100.0 - ((dx * dx + dz * dz) as f64).sqrt() as f32 * scale;
            let candidate = java_min(java_max(candidate, -100.0), 80.0);
            height = java_max(height, candidate);
        }
        (height - 8.0) / 128.0
    }

    pub fn density(&self, x: i32, z: i32) -> f32 {
        (self.height(x / 8, z / 8) - 8.0) / 128.0
    }

    pub fn fill(&self, buffer: &mut [f32], volume: DensityVolume) {
        let [sx, sy, sz] = volume.sizes();
        let [mx, _, mz] = volume.minimum();
        let [dx, _, dz] = volume.steps();
        let islands = self.volume_islands(volume);
        for z in 0..sz {
            let block_z = mz.wrapping_add((z as i32).wrapping_mul(dz));
            for x in 0..sx {
                let block_x = mx.wrapping_add((x as i32).wrapping_mul(dx));
                let value = match &islands {
                    Some(islands) => Self::density_from_islands(block_x, block_z, islands),
                    None => self.density(block_x, block_z),
                };
                let start = (x + z * sx) * sy;
                buffer[start..start + sy].fill(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_island_volumes_match_scalar_at_truncation_and_world_boundaries() {
        let cases = [
            ([5, 3, 5], [0, -64, 0], [4, 8, 4]),
            ([5, 3, 5], [1920, -64, 2240], [8, 8, 8]),
            ([5, 3, 5], [-2049, -64, 3071], [4, 8, 4]),
            ([5, 3, 5], [1023, -64, -1025], [1, 8, 1]),
            ([5, 3, 5], [30_000_000, 0, -30_000_000], [8, 1, 8]),
            ([5, 3, 5], [i32::MIN, 0, i32::MIN], [8, 1, 8]),
            ([5, 3, 5], [i32::MAX - 32, 0, i32::MAX - 32], [8, 1, 8]),
            ([5, 3, 5], [i32::MAX - 12, 0, i32::MAX - 3], [8, 1, 8]),
            ([5, 3, 5], [4096, 0, -8192], [1024, 1, 1024]),
            ([2, 3, 2], [-4096, 0, 8192], [599, 1, 599]),
            ([1, 4, 1], [-15, 0, -17], [1, 1, 1]),
        ];
        for seed in [0, 12345, -1, i64::MIN, i64::MAX] {
            let islands = EndIslands::new(seed);
            let compare = |volume: DensityVolume| {
                let mut output = vec![123.0; volume.size()];
                islands.fill(&mut output, volume);
                let [sx, sy, sz] = volume.sizes();
                let mut index = 0;
                for z in 0..sz {
                    for x in 0..sx {
                        for _ in 0..sy {
                            let bx = volume.minimum()[0]
                                .wrapping_add((x as i32).wrapping_mul(volume.steps()[0]));
                            let bz = volume.minimum()[2]
                                .wrapping_add((z as i32).wrapping_mul(volume.steps()[2]));
                            let expected = islands.density(bx, bz);
                            assert_eq!(
                                output[index].to_bits(),
                                expected.to_bits(),
                                "seed {seed}, volume {volume:?}"
                            );
                            index += 1;
                        }
                    }
                }
            };
            for (sizes, minimum, steps) in cases {
                compare(DensityVolume::new(sizes, minimum, steps).unwrap());
            }
            for offset in -17..=17 {
                compare(
                    DensityVolume::new([5, 2, 5], [1024 + offset, -32, -offset], [3, 16, 5])
                        .unwrap(),
                );
            }
        }
    }
}
