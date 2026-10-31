//! `BiomeSource.findBiomeHorizontal` (the strongholds' ring search, `FixedBiomeSource`'s own
//! override of it) and `findClosestBiome3d` (`/locate biome`), over a level's real biome source.

use super::BiomeSource;
use crate::util::random::RandomSource;
use crate::worldgen::climate::ClimateSearch;
use crate::worldgen::data::LevelDensity;
use crate::worldgen::density::DensityContext;

/// `BiomeSource.findBiomeHorizontal`: the square of quarts `radius` blocks around `origin` (rings
/// growing from the centre when `find_closest`, else only the full square), every `skip` quarts;
/// among the allowed biomes found, the first when `find_closest`, else a random one (each new
/// match replaces the result with chance `1 / (matches + 1)`). Returns the block (x, origin y, z)
/// and the biome. This is the biome source's default search (the strongholds' ring search).
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

impl BiomeSource {
    /// `findBiomeHorizontal(x, y, z, radius, 1, allowed, random, false, randomState)` on this
    /// source with the level's climate: the square's quarts sampled with an uncached context
    /// (vanilla's caching resolver gives the same values). A fixed source answers with a random
    /// block of the square (`FixedBiomeSource.findBiomeHorizontal`).
    pub fn find_biome_horizontal(
        &self,
        density: &LevelDensity,
        origin: [i32; 3],
        radius: i32,
        allowed: impl Fn(u32) -> bool,
        random: &mut impl RandomSource,
    ) -> Option<([i32; 3], u32)> {
        if let Self::Fixed(biome) = self {
            if !allowed(*biome) {
                return None;
            }
            let dx = random.next_int_bound(radius * 2 + 1).unwrap_or(0);
            let dz = random.next_int_bound(radius * 2 + 1).unwrap_or(0);
            let position = [origin[0] - radius + dx, origin[1], origin[2] - radius + dz];
            return Some((position, *biome));
        }
        let mut context = DensityContext::uncached();
        let mut climate = ClimateSearch::default();
        find_biome_horizontal(origin, radius, 1, allowed, random, false, |x, y, z| {
            self.noise_biome(density, &mut context, &mut climate, [x, y, z])
        })
    }

    /// `findClosestBiome3d`: among the possible biomes `allowed` takes, the first one met on a
    /// spiral of columns `res_horizontal` blocks apart around the origin (out to `radius`), at
    /// heights `res_vertical` apart from the origin's, nearest first, within the level's
    /// `[min_y + 1, max_y + 1]` (`getMinY()` and `getMaxY()`). The result is the sampled block.
    pub fn find_closest_biome_3d(
        &self,
        density: &LevelDensity,
        origin: [i32; 3],
        radius: i32,
        (res_horizontal, res_vertical): (i32, i32),
        (min_y, max_y): (i32, i32),
        allowed: impl Fn(u32) -> bool,
    ) -> Option<([i32; 3], u32)> {
        let candidates: Vec<u32> = (self.possible_biomes().into_iter())
            .filter(|&b| allowed(b))
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let mut context = DensityContext::cached();
        let mut climate = ClimateSearch::default();
        let sample_radius = radius.div_euclid(res_horizontal);
        let ys = out_from_origin(origin[1], min_y + 1, max_y + 1, res_vertical);
        for (column_x, column_z) in spiral_around(sample_radius) {
            let block_x = origin[0].wrapping_add(column_x.wrapping_mul(res_horizontal));
            let block_z = origin[2].wrapping_add(column_z.wrapping_mul(res_horizontal));
            for &block_y in &ys {
                let quart = [block_x >> 2, block_y >> 2, block_z >> 2];
                let biome = self.noise_biome(density, &mut context, &mut climate, quart);
                if candidates.contains(&biome) {
                    return Some(([block_x, block_y, block_z], biome));
                }
            }
        }
        None
    }
}

/// `Mth.outFromOrigin(origin, lower, upper, step)`: the origin clamped into the bounds, then
/// alternately below and above it, a step further each time, while either side is in bounds.
pub fn out_from_origin(origin: i32, lower: i32, upper: i32, step: i32) -> Vec<i32> {
    assert!(lower <= upper && step >= 1, "outFromOrigin bounds");
    let origin = origin.clamp(lower, upper);
    let mut values = Vec::new();
    let mut cursor = origin;
    loop {
        let distance = (origin - cursor).abs();
        if origin - distance < lower && origin + distance > upper {
            return values;
        }
        values.push(cursor);
        let previous_negative = cursor <= origin;
        let can_move_positive = origin + distance + step <= upper;
        if !previous_negative || !can_move_positive {
            let attempted = origin - distance - if previous_negative { step } else { 0 };
            if attempted >= lower {
                cursor = attempted;
                continue;
            }
        }
        cursor = origin + distance + step;
    }
}

/// `BlockPos.spiralAround(ZERO, radius, EAST, SOUTH)`: the (x, z) offsets from the centre
/// outwards, in vanilla's order: the centre, then legs of growing length east, south, west and
/// north (`4 * radius + 1` legs, so the last one runs a block past the square, as vanilla's).
pub fn spiral_around(radius: i32) -> impl Iterator<Item = (i32, i32)> {
    const DIRECTIONS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let legs = 4 * radius;
    // The cursor starts one step south of the centre; the first step goes back north.
    let (mut x, mut z) = (0, 1);
    let (mut leg, mut leg_size, mut leg_index) = (-1i32, 0i32, 0i32);
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let (dx, dz) = DIRECTIONS[((leg + 4) % 4) as usize];
        x += dx;
        z += dz;
        if leg_index >= leg_size {
            if leg >= legs {
                done = true;
                return None;
            }
            leg += 1;
            leg_index = 0;
            leg_size = leg / 2 + 1;
        }
        leg_index += 1;
        Some((x, z))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spiral_walks_out_from_the_centre() {
        let walk: Vec<_> = spiral_around(1).collect();
        assert_eq!(
            walk[..9],
            [
                (0, 0),
                (1, 0),
                (1, 1),
                (0, 1),
                (-1, 1),
                (-1, 0),
                (-1, -1),
                (0, -1),
                (1, -1)
            ]
        );
        assert_eq!(walk.len(), 10);
        assert_eq!(spiral_around(0).collect::<Vec<_>>(), [(0, 0)]);
    }

    #[test]
    fn out_from_origin_alternates() {
        assert_eq!(out_from_origin(0, -3, 3, 2), [0, 2, -2]);
        assert_eq!(out_from_origin(5, 0, 4, 1), [4, 3, 2, 1, 0]);
    }
}
