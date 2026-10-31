//! The noise part of the TERRAIN step: `NoiseChunk` and `NoiseBasedChunkGenerator.doFill` of
//! 26.3 (with Moonrise's inline fill, which changes no result), and `iterateNoiseColumn` behind
//! `getBaseHeight`/`getBaseColumn`.
//!
//! Final density comes from the level's [`DensityProgram`] over the whole chunk volume at once
//! (`sampleVolume`, which interpolates cells exactly as 26.3 does; the program is checked against
//! the jar), then each block asks the [`Aquifer`] what it is. Surface rules and carvers come with
//! their own steps. Copied from Horizon's `horizon-world::worldgen::terrain`, which kept a tree
//! of samplers behind traits; here the chunk is a [`ProtoChunk`] and the density a node of the
//! program. Not carried over: `SharedConstants.DEBUG_AQUIFERS` and
//! `DEBUG_DISABLE_FLUID_GENERATION` (off in every release), and old-world blending (26.3 worlds
//! have no chunks to blend).

use super::aquifer::{Aquifer, AquiferError, FluidBlocks, GlobalFluidPicker, Sampler};
use super::data::LevelDensity;
use super::density::DensityContext;
use super::noise::{DensityError, DensityVolume};
use super::proto_chunk::ProtoChunk;
use super::settings::{NoiseGeneratorSettings, RouterField};
use crate::block::{BlockState, BlockStates};
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{SectionFlags, StateFlags};
use crate::world::level_chunk::HeightmapFlags;

#[cfg(test)]
mod oracle_tests;

/// Why a chunk's terrain could not be filled. Vanilla's data never fails; a datapack can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TerrainError {
    #[error(transparent)]
    Aquifer(#[from] AquiferError),
    #[error("the noise volume is invalid: {0}")]
    Volume(#[from] DensityError),
}

/// A level's noise terrain: its compiled density, noise settings and fluids. Immutable; every
/// worker uses it with its own [`DensityContext`].
#[derive(Clone, Copy, Debug)]
pub struct NoiseTerrain<'a> {
    level: &'a LevelDensity,
    settings: &'a NoiseGeneratorSettings,
    blocks: FluidBlocks<'a>,
    picker: GlobalFluidPicker,
}

impl<'a> NoiseTerrain<'a> {
    pub fn new(
        level: &'a LevelDensity,
        settings: &'a NoiseGeneratorSettings,
        states: &'a BlockStates,
    ) -> Result<Self, AquiferError> {
        let blocks = FluidBlocks::new(states)?;
        Ok(Self {
            level,
            settings,
            blocks,
            picker: GlobalFluidPicker::new(settings, &blocks),
        })
    }

    /// `NoiseChunk`'s aquifer for `volume`.
    fn aquifer(
        &self,
        volume: DensityVolume,
        sampler: &mut Sampler<'_>,
    ) -> Result<Aquifer<'a>, AquiferError> {
        Aquifer::new(
            self.level.aquifers,
            self.level.aquifer_random,
            self.picker,
            self.blocks,
            volume,
            sampler,
        )
    }

    /// `doFill`: the chunk's blocks from the noise, with the `OCEAN_FLOOR_WG` and
    /// `WORLD_SURFACE_WG` heightmaps and the marks for fluids that may flow. `context` must be a
    /// caching one ([`DensityContext::cached`]) with the chunk's fields bound (its structures'
    /// beardifier) and no values of another chunk.
    pub fn fill(
        &self,
        chunk: &mut ProtoChunk,
        context: &mut DensityContext,
        flags: &impl SectionFlags,
        heightmap_flags: &impl HeightmapFlags,
    ) -> Result<(), TerrainError> {
        let noise = self
            .settings
            .noise
            .clamp_to_height(chunk.min_y(), chunk.height());
        if noise.height <= 0 {
            return Ok(());
        }
        let pos = chunk.pos();
        let volume = DensityVolume::new(
            [16, noise.height, 16],
            [pos.x.wrapping_shl(4), noise.min_y, pos.z.wrapping_shl(4)],
            [1; 3],
        )?;
        let program = &self.level.program;
        let mut aquifer = self.aquifer(volume, &mut Sampler { program, context })?;
        chunk.ensure_heightmap(Heightmap::OceanFloorWg);
        chunk.ensure_heightmap(Heightmap::WorldSurfaceWg);
        let [size_x, size_y, size_z] = volume.sizes();
        let [min_x, min_y, min_z] = volume.minimum();
        let mut density = vec![0.0f32; volume.size()];
        program.fill(
            context,
            self.level.router(RouterField::FinalDensity),
            &mut density,
            volume,
        );
        let default_block = self.settings.default_block;
        for z in 0..size_z {
            let block_z = min_z.wrapping_add(z as i32);
            for x in 0..size_x {
                let block_x = min_x.wrapping_add(x as i32);
                // The buffer is laid out Z, X, Y with Y innermost.
                let column = &density[(x + z * size_x) * size_y..][..size_y];
                for y in (0..size_y).rev() {
                    let block_y = min_y.wrapping_add(y as i32);
                    let state = aquifer
                        .compute_substance(
                            &mut Sampler { program, context },
                            [block_x, block_y, block_z],
                            f64::from(column[y]),
                        )?
                        .unwrap_or(default_block);
                    if state == BlockState::AIR {
                        continue;
                    }
                    chunk.set_block_raw(x as i32, block_y, z as i32, state, flags);
                    for kind in [Heightmap::OceanFloorWg, Heightmap::WorldSurfaceWg] {
                        chunk.update_heightmap(
                            kind,
                            x as i32,
                            block_y,
                            z as i32,
                            state,
                            heightmap_flags,
                        );
                    }
                    if aquifer.should_schedule_fluid_update()
                        && flags.section_flags(state).has(StateFlags::HAS_FLUID)
                    {
                        chunk.mark_postprocessing(block_x, block_y, block_z);
                    }
                }
            }
        }
        Ok(())
    }

    /// `iterateNoiseColumn`: the blocks of the column at `x`, `z`, top down, without structures
    /// or neighbours, for a world of `min_y` and `height`. `visit` gets each block's Y and state
    /// and returns whether to stop; the result is that Y plus one when it stopped.
    fn iterate_column(
        &self,
        x: i32,
        z: i32,
        (min_y, height): (i32, i32),
        context: &mut DensityContext,
        mut visit: impl FnMut(i32, BlockState) -> bool,
    ) -> Result<Option<i32>, TerrainError> {
        let noise = self.settings.noise.clamp_to_height(min_y, height);
        if noise.height <= 0 {
            return Ok(None);
        }
        context.reset();
        let volume = DensityVolume::new([1, noise.height, 1], [x, noise.min_y, z], [1; 3])?;
        let program = &self.level.program;
        let mut aquifer = self.aquifer(volume, &mut Sampler { program, context })?;
        let mut density = vec![0.0f32; volume.size()];
        program.fill(
            context,
            self.level.router(RouterField::FinalDensity),
            &mut density,
            volume,
        );
        for y in (0..density.len()).rev() {
            let block_y = noise.min_y.wrapping_add(y as i32);
            let state = aquifer
                .compute_substance(
                    &mut Sampler { program, context },
                    [x, block_y, z],
                    f64::from(density[y]),
                )?
                .unwrap_or(self.settings.default_block);
            if visit(block_y, state) {
                return Ok(Some(block_y.wrapping_add(1)));
            }
        }
        Ok(None)
    }

    /// `getBaseHeight`: one above the highest block `opaque` accepts, else the world's minimum.
    pub fn base_height(
        &self,
        x: i32,
        z: i32,
        world: (i32, i32),
        context: &mut DensityContext,
        opaque: impl Fn(BlockState) -> bool,
    ) -> Result<i32, TerrainError> {
        Ok(self
            .iterate_column(x, z, world, context, |_, state| opaque(state))?
            .unwrap_or(world.0))
    }

    /// `getBaseColumn`: the column's blocks from `NoiseSettings.minY` (clamped to the world) up,
    /// `None` when the noise has no height there.
    pub fn base_column(
        &self,
        x: i32,
        z: i32,
        world: (i32, i32),
        context: &mut DensityContext,
    ) -> Result<Option<(i32, Vec<BlockState>)>, TerrainError> {
        let mut states = Vec::new();
        let mut lowest = None;
        self.iterate_column(x, z, world, context, |y, state| {
            lowest = Some(y);
            states.push(state);
            false
        })?;
        states.reverse();
        Ok(lowest.map(|y| (y, states)))
    }
}
