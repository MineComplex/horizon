//! `Aquifer.NoiseBasedAquifer`: the underground fluid pockets, their levels and the barriers
//! between them. Copied from Horizon's port (same arithmetic, Java's wrapping kept); the grid
//! cell neighbourhood of the last query is remembered, as that port does, since consecutive
//! queries of a column share it.

use std::collections::HashMap;

use super::{AquiferError, AquiferRandom, FluidBlocks, FluidStatus, GlobalFluidPicker, Sampler};
use crate::block::BlockState;
use crate::core::position::{BlockPos, ChunkPos};
use crate::worldgen::density::NodeId;
use crate::worldgen::noise::DensityVolume;

/// A grid cell index and its point, for each of the twelve cells around a position.
type Neighborhood = [(usize, BlockPos); 12];

const WAY_BELOW_MIN_Y: i32 = -32512;
const FLOWING_UPDATE_SIMILARITY: f64 = 1.0 - 44.0 / 25.0;
const MAX_GRID_CELLS: usize = 16_777_216;
const SURFACE_OFFSETS: [[i32; 2]; 13] = [
    [0, 0],
    [-2, -1],
    [-1, -1],
    [0, -1],
    [1, -1],
    [-3, 0],
    [-2, 0],
    [-1, 0],
    [1, 0],
    [-2, 1],
    [-1, 1],
    [0, 1],
    [1, 1],
];

/// The density roots, in `AquiferField` order.
const BARRIER: usize = 0;
const FLOODEDNESS: usize = 1;
const SPREAD: usize = 2;
const LAVA: usize = 3;
const EXCLUSION: usize = 4;
const SURFACE_LEVEL: usize = 5;

#[derive(Debug)]
pub struct NoiseBasedAquifer<'a> {
    roots: [NodeId; 6],
    picker: GlobalFluidPicker,
    random: AquiferRandom,
    blocks: FluidBlocks<'a>,
    minimum: [i32; 3],
    size_x: i32,
    size_z: i32,
    statuses: Vec<Option<FluidStatus>>,
    locations: Vec<i64>,
    surfaces: HashMap<i64, i32>,
    /// The twelve cells (index, point) around the last query's grid cell.
    neighborhood: Option<([i32; 3], Neighborhood)>,
    skip_above_y: i32,
    schedule: bool,
}

impl<'a> NoiseBasedAquifer<'a> {
    pub fn new(
        roots: [NodeId; 6],
        random: AquiferRandom,
        picker: GlobalFluidPicker,
        blocks: FluidBlocks<'a>,
        volume: DensityVolume,
        sampler: &mut Sampler<'_>,
    ) -> Result<Self, AquiferError> {
        let low = volume.minimum();
        let high = volume.maximum();
        let minimum = [
            low[0].wrapping_sub(5) >> 4,
            low[1].wrapping_add(1).div_euclid(12).wrapping_sub(1),
            low[2].wrapping_sub(5) >> 4,
        ];
        let maximum = [
            (high[0].wrapping_sub(5) >> 4).wrapping_add(1),
            high[1].wrapping_add(1).div_euclid(12).wrapping_add(1),
            (high[2].wrapping_sub(5) >> 4).wrapping_add(1),
        ];
        let sizes: [i32; 3] =
            std::array::from_fn(|i| maximum[i].wrapping_sub(minimum[i]).wrapping_add(1));
        let count = sizes
            .into_iter()
            .try_fold(1usize, |n, s| {
                if s > 0 {
                    n.checked_mul(s as usize)
                } else {
                    None
                }
            })
            .filter(|n| *n <= MAX_GRID_CELLS)
            .ok_or(AquiferError::InvalidGrid)?;
        let mut aquifer = Self {
            roots,
            picker,
            random,
            blocks,
            minimum,
            size_x: sizes[0],
            size_z: sizes[2],
            statuses: vec![None; count],
            locations: vec![i64::MAX; count],
            surfaces: HashMap::new(),
            neighborhood: None,
            skip_above_y: 0,
            schedule: false,
        };
        let max_surface = aquifer
            .max_surface_level(
                sampler,
                [minimum[0].wrapping_shl(4), minimum[2].wrapping_shl(4)],
                [
                    maximum[0].wrapping_shl(4).wrapping_add(9),
                    maximum[2].wrapping_shl(4).wrapping_add(9),
                ],
            )?
            .wrapping_add(8);
        let skip_grid = max_surface.wrapping_add(12).div_euclid(12).wrapping_add(1);
        aquifer.skip_above_y = skip_grid.wrapping_mul(12).wrapping_add(11).wrapping_sub(1);
        Ok(aquifer)
    }

    pub fn should_schedule_fluid_update(&self) -> bool {
        self.schedule
    }

    fn surface_level(&mut self, sampler: &mut Sampler<'_>, x: i32, z: i32) -> i32 {
        let x = (x >> 2).wrapping_shl(2);
        let z = (z >> 2).wrapping_shl(2);
        let key = ChunkPos::new(x, z).pack();
        if let Some(level) = self.surfaces.get(&key) {
            return *level;
        }
        let level = sampler.value(self.roots[SURFACE_LEVEL], [x, 0, z]).floor() as i32;
        self.surfaces.insert(key, level);
        level
    }

    /// The surface levels of the quart columns around the chunk, which it caches; their maximum.
    fn max_surface_level(
        &mut self,
        sampler: &mut Sampler<'_>,
        minimum: [i32; 2],
        maximum: [i32; 2],
    ) -> Result<i32, AquiferError> {
        let low = minimum.map(|v| v >> 2);
        let high = maximum.map(|v| v >> 2);
        let sizes: [i32; 2] = std::array::from_fn(|i| high[i].wrapping_sub(low[i]).wrapping_add(1));
        let volume = DensityVolume::new(
            [sizes[0], 1, sizes[1]],
            [low[0].wrapping_shl(2), 0, low[1].wrapping_shl(2)],
            [4, 1, 4],
        )
        .map_err(|_| AquiferError::InvalidGrid)?;
        let mut buffer = vec![0.0f32; volume.size()];
        sampler.program.fill(
            sampler.context,
            self.roots[SURFACE_LEVEL],
            &mut buffer,
            volume,
        );
        let mut maximum = i32::MIN;
        let mut index = 0;
        for z in 0..sizes[1] {
            for x in 0..sizes[0] {
                let value = f64::from(buffer[index]).floor() as i32;
                index += 1;
                let block = |cell: i32, offset: i32| cell.wrapping_add(offset).wrapping_shl(2);
                self.surfaces.insert(
                    ChunkPos::new(block(low[0], x), block(low[1], z)).pack(),
                    value,
                );
                maximum = maximum.max(value);
            }
        }
        Ok(maximum)
    }

    /// `getIndex`; like Java it checks only the final index.
    fn grid_index(&self, cell: [i32; 3]) -> Result<usize, AquiferError> {
        let relative: [i32; 3] = std::array::from_fn(|i| cell[i].wrapping_sub(self.minimum[i]));
        let index = relative[1]
            .wrapping_mul(self.size_z)
            .wrapping_add(relative[2])
            .wrapping_mul(self.size_x)
            .wrapping_add(relative[0]);
        if index < 0 || index as usize >= self.locations.len() {
            return Err(AquiferError::OutsideGrid);
        }
        Ok(index as usize)
    }

    /// `getAquiferStatus`.
    fn status(&mut self, sampler: &mut Sampler<'_>, index: usize) -> FluidStatus {
        if let Some(status) = self.statuses[index] {
            return status;
        }
        let pos = BlockPos::unpack(self.locations[index]);
        let status = self.compute_fluid(sampler, [pos.x, pos.y, pos.z]);
        self.statuses[index] = Some(status);
        status
    }

    /// `computeFluid`.
    fn compute_fluid(&mut self, sampler: &mut Sampler<'_>, [x, y, z]: [i32; 3]) -> FluidStatus {
        let global = self.picker.compute(y);
        let upper = y.wrapping_add(12);
        let lower = y.wrapping_sub(12);
        let mut minimum = i32::MAX;
        let mut center_fluid = false;
        for [dx, dz] in SURFACE_OFFSETS {
            let sx = x.wrapping_add(dx.wrapping_mul(16));
            let sz = z.wrapping_add(dz.wrapping_mul(16));
            let surface = self.surface_level(sampler, sx, sz);
            let adjusted = surface.wrapping_add(8);
            let center = dx == 0 && dz == 0;
            if center && lower > adjusted {
                return global;
            }
            let above = upper > adjusted;
            if above || center {
                let status = self.picker.compute(adjusted);
                if !self.blocks.is_air(status.at(adjusted)) {
                    if center {
                        center_fluid = true;
                    }
                    if above {
                        return status;
                    }
                }
            }
            minimum = minimum.min(surface);
        }
        let level = self.compute_surface_level(sampler, [x, y, z], global, minimum, center_fluid);
        let mut state = global.state;
        if level <= -10
            && level != WAY_BELOW_MIN_Y
            && state != self.blocks.lava_state
            && sampler
                .value(
                    self.roots[LAVA],
                    [x.div_euclid(64), y.div_euclid(40), z.div_euclid(64)],
                )
                .abs()
                > 0.3
        {
            state = self.blocks.lava_state;
        }
        FluidStatus { level, state }
    }

    fn compute_surface_level(
        &self,
        sampler: &mut Sampler<'_>,
        position: [i32; 3],
        global: FluidStatus,
        minimum: i32,
        center_fluid: bool,
    ) -> i32 {
        let [x, y, z] = position;
        let (dry, flooded) = if sampler.value(self.roots[EXCLUSION], position) > 0.0 {
            (-1.0, -1.0)
        } else {
            let distance = minimum.wrapping_add(8).wrapping_sub(y);
            let factor = if center_fluid {
                // clampedMap(distance, 0, 64, 1, 0)
                let alpha = distance as f64 / 64.0;
                if alpha < 0.0 {
                    1.0
                } else if alpha > 1.0 {
                    0.0
                } else {
                    1.0 + alpha * (0.0 - 1.0)
                }
            } else {
                0.0
            };
            let value = sampler
                .value(self.roots[FLOODEDNESS], position)
                .clamp(-1.0, 1.0);
            let alpha = (factor - 1.0) / (0.0 - 1.0);
            let threshold_flooded = -0.3 + alpha * (0.8 - -0.3);
            let threshold_dry = -0.8 + alpha * (0.4 - -0.8);
            (value - threshold_dry, value - threshold_flooded)
        };
        if flooded > 0.0 {
            return global.level;
        }
        if dry > 0.0 {
            let spread = sampler.value(
                self.roots[SPREAD],
                [x.div_euclid(16), y.div_euclid(40), z.div_euclid(16)],
            ) * 10.0;
            let offset = ((spread / 3.0).floor() as i32).wrapping_mul(3);
            return minimum.min(
                y.div_euclid(40)
                    .wrapping_mul(40)
                    .wrapping_add(20)
                    .wrapping_add(offset),
            );
        }
        WAY_BELOW_MIN_Y
    }

    /// `calculatePressure`; `memo` is the barrier noise at the position, NaN until sampled.
    fn pressure(
        &self,
        sampler: &mut Sampler<'_>,
        position: [i32; 3],
        memo: &mut f64,
        a: FluidStatus,
        b: FluidStatus,
    ) -> f64 {
        let y = position[1];
        let (sa, sb) = (a.at(y), b.at(y));
        if (self.blocks.is_lava(sa) && self.blocks.is_water(sb))
            || (self.blocks.is_water(sa) && self.blocks.is_lava(sb))
        {
            return 2.0;
        }
        let gap = a.level.wrapping_sub(b.level).wrapping_abs();
        if gap == 0 {
            return 0.0;
        }
        let midpoint = 0.5 * a.level.wrapping_add(b.level) as f64;
        let dy = y as f64 + 0.5 - midpoint;
        let edge = gap as f64 / 2.0 - dy.abs();
        let gradient = if dy > 0.0 {
            let n = 0.0 + edge;
            if n > 0.0 { n / 1.5 } else { n / 2.5 }
        } else {
            let n = 3.0 + edge;
            if n > 0.0 { n / 3.0 } else { n / 10.0 }
        };
        let barrier = if gradient.is_nan() || (-2.0..=2.0).contains(&gradient) {
            if memo.is_nan() {
                *memo = sampler.value(self.roots[BARRIER], position);
            }
            *memo
        } else {
            0.0
        };
        2.0 * (barrier + gradient)
    }

    /// The twelve cells whose points can be nearest to a position in grid cell `grid`, their
    /// points drawn on first use.
    fn neighbors(&mut self, grid: [i32; 3]) -> Result<Neighborhood, AquiferError> {
        if let Some((key, entries)) = &self.neighborhood
            && *key == grid
        {
            return Ok(*entries);
        }
        let mut entries = [(0, BlockPos::default()); 12];
        let mut next = 0;
        for dx in 0..=1i32 {
            for dy in -1..=1i32 {
                for dz in 0..=1i32 {
                    let cell = [
                        grid[0].wrapping_add(dx),
                        grid[1].wrapping_add(dy),
                        grid[2].wrapping_add(dz),
                    ];
                    let index = self.grid_index(cell)?;
                    if self.locations[index] == i64::MAX {
                        let [ox, oy, oz] = self.random.offsets(cell);
                        self.locations[index] = BlockPos::new(
                            cell[0].wrapping_shl(4).wrapping_add(ox),
                            cell[1].wrapping_mul(12).wrapping_add(oy),
                            cell[2].wrapping_shl(4).wrapping_add(oz),
                        )
                        .pack();
                    }
                    entries[next] = (index, BlockPos::unpack(self.locations[index]));
                    next += 1;
                }
            }
        }
        self.neighborhood = Some((grid, entries));
        Ok(entries)
    }

    /// `computeSubstance`: `None` is the generator's default block.
    pub fn compute_substance(
        &mut self,
        sampler: &mut Sampler<'_>,
        [x, y, z]: [i32; 3],
        density: f64,
    ) -> Result<Option<BlockState>, AquiferError> {
        if density > 0.0 {
            self.schedule = false;
            return Ok(None);
        }
        let global = self.picker.compute(y);
        if y > self.skip_above_y {
            self.schedule = false;
            return Ok(Some(global.at(y)));
        }
        if self.blocks.is_lava(global.at(y)) {
            self.schedule = false;
            return Ok(Some(self.blocks.lava_state));
        }
        let grid = [
            x.wrapping_sub(5) >> 4,
            y.wrapping_add(1).div_euclid(12),
            z.wrapping_sub(5) >> 4,
        ];
        // The four nearest points; equal distances go before the earlier ones, as in vanilla.
        let mut distances = [i32::MAX; 4];
        let mut indices = [0usize; 4];
        for (index, point) in self.neighbors(grid)? {
            let ax = point.x.wrapping_sub(x);
            let ay = point.y.wrapping_sub(y);
            let az = point.z.wrapping_sub(z);
            let distance = ax
                .wrapping_mul(ax)
                .wrapping_add(ay.wrapping_mul(ay))
                .wrapping_add(az.wrapping_mul(az));
            if let Some(slot) = distances.iter().position(|&d| d >= distance) {
                distances.copy_within(slot..3, slot + 1);
                indices.copy_within(slot..3, slot + 1);
                distances[slot] = distance;
                indices[slot] = index;
            }
        }
        let a = self.status(sampler, indices[0]);
        let s01 = similarity(distances[0], distances[1]);
        let candidate = a.at(y);
        if s01 <= 0.0 {
            self.schedule = if s01 >= FLOWING_UPDATE_SIMILARITY {
                a != self.status(sampler, indices[1])
            } else {
                false
            };
            return Ok(Some(candidate));
        }
        if self.blocks.is_water(candidate)
            && self
                .blocks
                .is_lava(self.picker.compute(y.wrapping_sub(1)).at(y.wrapping_sub(1)))
        {
            self.schedule = true;
            return Ok(Some(candidate));
        }
        let position = [x, y, z];
        let mut memo = f64::NAN;
        let b = self.status(sampler, indices[1]);
        if density + s01 * self.pressure(sampler, position, &mut memo, a, b) > 0.0 {
            self.schedule = false;
            return Ok(None);
        }
        let c = self.status(sampler, indices[2]);
        let s02 = similarity(distances[0], distances[2]);
        if s02 > 0.0
            && density + s01 * s02 * self.pressure(sampler, position, &mut memo, a, c) > 0.0
        {
            self.schedule = false;
            return Ok(None);
        }
        let s12 = similarity(distances[1], distances[2]);
        if s12 > 0.0
            && density + s01 * s12 * self.pressure(sampler, position, &mut memo, b, c) > 0.0
        {
            self.schedule = false;
            return Ok(None);
        }
        let may_flow = a != b
            || (s12 >= FLOWING_UPDATE_SIMILARITY && b != c)
            || (s02 >= FLOWING_UPDATE_SIMILARITY && a != c);
        self.schedule = may_flow
            || (s02 >= FLOWING_UPDATE_SIMILARITY
                && similarity(distances[0], distances[3]) >= FLOWING_UPDATE_SIMILARITY
                && a != self.status(sampler, indices[3]));
        Ok(Some(candidate))
    }
}

fn similarity(a: i32, b: i32) -> f64 {
    1.0 - b.wrapping_sub(a) as f64 / 25.0
}
