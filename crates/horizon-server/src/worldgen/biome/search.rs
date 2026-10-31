//! `BiomeSource.findBiomeHorizontal`, the default search of a biome source.

use crate::util::random::RandomSource;

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
