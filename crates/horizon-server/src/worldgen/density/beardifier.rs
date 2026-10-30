//! `Beardifier`: structure terrain adaptation from rigid pieces and jigsaw junctions, the value of
//! the `minecraft:beardifier` field while a chunk's terrain is filled. Copied from Horizon's
//! `horizon-core::density::beardifier` without its modpack terrain kernels. Which pieces a chunk
//! sees comes with the structure port (WP5); this is the density they produce.

use std::sync::LazyLock;

use crate::worldgen::noise::DensityVolume;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructureBounds {
    minimum: [i32; 3],
    maximum: [i32; 3],
}

impl StructureBounds {
    /// Normalized inclusive bounds; `None` when a minimum exceeds its maximum.
    pub fn new(minimum: [i32; 3], maximum: [i32; 3]) -> Option<Self> {
        (0..3)
            .all(|axis| minimum[axis] <= maximum[axis])
            .then_some(Self { minimum, maximum })
    }
    pub fn minimum(self) -> [i32; 3] {
        self.minimum
    }
    pub fn maximum(self) -> [i32; 3] {
        self.maximum
    }
    pub fn contains(self, position: [i32; 3]) -> bool {
        (0..3).all(|i| position[i] >= self.minimum[i] && position[i] <= self.maximum[i])
    }
    fn intersects(self, minimum: [i32; 3], maximum: [i32; 3]) -> bool {
        (0..3).all(|i| self.maximum[i] >= minimum[i] && self.minimum[i] <= maximum[i])
    }
}

/// `TerrainAdjustment`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainAdjustment {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

#[derive(Clone, Copy, Debug)]
pub struct RigidPiece {
    pub bounds: StructureBounds,
    pub adjustment: TerrainAdjustment,
    pub ground_level_delta: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct JigsawJunction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
}

/// The pieces and junctions near one chunk.
#[derive(Clone, Debug, Default)]
pub struct Beardifier {
    pieces: Vec<RigidPiece>,
    junctions: Vec<JigsawJunction>,
    affected: Option<StructureBounds>,
}

impl Beardifier {
    /// A missing affected box makes the whole field zero, even with pieces.
    pub fn new(
        pieces: Vec<RigidPiece>,
        junctions: Vec<JigsawJunction>,
        affected: Option<StructureBounds>,
    ) -> Self {
        Self {
            pieces,
            junctions,
            affected,
        }
    }

    fn sample_unchecked(&self, [x, y, z]: [i32; 3]) -> f32 {
        let mut value = 0.;
        let kernel = &*KERNEL;
        for piece in &self.pieces {
            let low = piece.bounds.minimum;
            let high = piece.bounds.maximum;
            let dx = 0.max(low[0].wrapping_sub(x).max(x.wrapping_sub(high[0])));
            let dz = 0.max(low[2].wrapping_sub(z).max(z.wrapping_sub(high[2])));
            let ground = low[1].wrapping_add(piece.ground_level_delta);
            let dy = y.wrapping_sub(ground);
            let distance_y = match piece.adjustment {
                TerrainAdjustment::None => 0,
                TerrainAdjustment::Bury | TerrainAdjustment::BeardThin => dy,
                TerrainAdjustment::BeardBox => {
                    0.max(ground.wrapping_sub(y).max(y.wrapping_sub(high[1])))
                }
                TerrainAdjustment::Encapsulate => {
                    0.max(low[1].wrapping_sub(y).max(y.wrapping_sub(high[1])))
                }
            };
            value += match piece.adjustment {
                TerrainAdjustment::None => 0.,
                TerrainAdjustment::Bury => bury(dx as f32, distance_y as f32 / 2., dz as f32),
                TerrainAdjustment::BeardThin | TerrainAdjustment::BeardBox => {
                    beard(kernel, dx, distance_y, dz, dy) * 0.8
                }
                TerrainAdjustment::Encapsulate => {
                    bury(dx as f32 / 2., distance_y as f32 / 2., dz as f32 / 2.) * 0.8
                }
            };
        }
        for junction in &self.junctions {
            let dx = x.wrapping_sub(junction.source_x);
            let dy = y.wrapping_sub(junction.source_ground_y);
            let dz = z.wrapping_sub(junction.source_z);
            value += beard(kernel, dx, dy, dz, dy) * 0.4;
        }
        value
    }

    pub fn sample(&self, position: [i32; 3]) -> f32 {
        if self
            .affected
            .is_some_and(|bounds| bounds.contains(position))
        {
            self.sample_unchecked(position)
        } else {
            0.
        }
    }

    pub fn fill(&self, buffer: &mut [f32], volume: DensityVolume) {
        buffer.fill(0.);
        let Some(bounds) = self.affected else {
            return;
        };
        let minimum = volume.minimum();
        if !bounds.intersects(minimum, volume.maximum()) {
            return;
        }
        let steps = volume.steps();
        let sizes = volume.sizes();
        // Deliberately floor, not ceil: the first bulk lattice sample can lie
        // below the affected bounds and is evaluated without the scalar guard.
        let low: [i32; 3] = std::array::from_fn(|i| {
            bounds.minimum[i]
                .wrapping_sub(minimum[i])
                .max(0)
                .div_euclid(steps[i])
        });
        let high: [i32; 3] = std::array::from_fn(|i| {
            ((sizes[i] - 1) as i32).min(
                bounds.maximum[i]
                    .wrapping_sub(minimum[i])
                    .div_euclid(steps[i]),
            )
        });
        for z in low[2]..=high[2] {
            for x in low[0]..=high[0] {
                for y in low[1]..=high[1] {
                    let local = [x, y, z];
                    let index = volume
                        .index(local.map(|v| v as usize))
                        .expect("bounded structure density indices");
                    let point = std::array::from_fn(|i| {
                        minimum[i].wrapping_add(local[i].wrapping_mul(steps[i]))
                    });
                    buffer[index] = self.sample_unchecked(point);
                }
            }
        }
    }
}

fn bury(x: f32, y: f32, z: f32) -> f32 {
    let squared = (x * x + y * y) + z * z;
    if squared >= 36. {
        0.
    } else {
        1. - squared.sqrt() / 6.
    }
}

fn beard(kernel: &[f32; 13824], x: i32, y: i32, z: i32, ground_delta: i32) -> f32 {
    let indices = [x, y, z].map(|v| v.wrapping_add(12));
    if indices.iter().any(|v| !(0..24).contains(v)) {
        return 0.;
    }
    let vertical = ground_delta as f32 + 0.5;
    let squared = (x as f32 * x as f32 + vertical * vertical) + z as f32 * z as f32;
    let factor = (-vertical * fast_inverse_sqrt(f64::from(squared / 2.)) as f32) / 2.;
    factor * kernel[((indices[2] * 24 + indices[0]) * 24 + indices[1]) as usize]
}

/// `Mth.fastInvSqrt(double)`.
fn fast_inverse_sqrt(value: f64) -> f64 {
    let half = 0.5 * value;
    let bits = 6_910_469_410_427_058_090i64.wrapping_sub((value.to_bits() as i64) >> 1);
    let estimate = f64::from_bits(bits as u64);
    estimate * (1.5 - half * estimate * estimate)
}

/// `Beardifier.BEARD_KERNEL`: a seed-independent constant, computed once.
static KERNEL: LazyLock<Box<[f32; 13824]>> = LazyLock::new(|| {
    Box::new(std::array::from_fn(|i| {
        let z = (i / 576) as f64 - 12.;
        let x = ((i / 24) % 24) as f64 - 12.;
        let y = (i % 24) as f64 - 12. + 0.5;
        let squared = (x * x + y * y) + z * z;
        crate::util::jmath::pow(std::f64::consts::E, -squared / 16.) as f32
    }))
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_match_points_inside_the_affected_box() {
        let bounds = StructureBounds::new([0, 60, 0], [8, 70, 8]).unwrap();
        let beardifier = Beardifier::new(
            vec![RigidPiece {
                bounds,
                adjustment: TerrainAdjustment::BeardThin,
                ground_level_delta: 1,
            }],
            vec![JigsawJunction {
                source_x: 4,
                source_ground_y: 64,
                source_z: 12,
            }],
            StructureBounds::new([-12, 40, -12], [20, 90, 24]),
        );
        let volume = DensityVolume::new([16, 48, 16], [-4, 48, -4], [1; 3]).unwrap();
        let mut buffer = vec![9.0; volume.size()];
        beardifier.fill(&mut buffer, volume);
        let mut nonzero = 0;
        for z in 0..16 {
            for x in 0..16 {
                for y in 0..48 {
                    let position = [x as i32 - 4, y as i32 + 48, z as i32 - 4];
                    let value = buffer[volume.index([x, y, z]).unwrap()];
                    assert_eq!(value.to_bits(), beardifier.sample(position).to_bits());
                    nonzero += usize::from(value != 0.0);
                }
            }
        }
        assert!(nonzero > 1000);
        assert_eq!(Beardifier::default().sample([0, 64, 0]), 0.0);
    }
}
