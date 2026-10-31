//! A level's chunk generator of any kind: the noise generator ([`LevelGenerator`]) or one of the
//! fixed-biome ones ([`FixedGenerator`]: flat, debug). What the level and its generation workers
//! ask of a generator, whichever it is.

use std::sync::Arc;

use super::convert::ChunkShape;
use super::fixed::{FixedGenerator, FixedWorker};
use super::{GenerationWorker, GeneratorError, LevelGenerator, StepError};
use crate::core::position::ChunkPos;
use crate::world::chunk::ChunkStatus;
use crate::worldgen::proto_chunk::{GenerationRegion, ProtoChunk};

/// A level's generator (immutable, shared with the generation workers).
#[derive(Debug)]
pub enum ChunkGenerator {
    Noise(Box<LevelGenerator>),
    Fixed(Box<FixedGenerator>),
}

impl From<LevelGenerator> for ChunkGenerator {
    fn from(generator: LevelGenerator) -> Self {
        Self::Noise(Box::new(generator))
    }
}

impl From<FixedGenerator> for ChunkGenerator {
    fn from(generator: FixedGenerator) -> Self {
        Self::Fixed(Box::new(generator))
    }
}

impl ChunkGenerator {
    /// `getSeaLevel`.
    pub fn sea_level(&self) -> i32 {
        match self {
            Self::Noise(g) => g.sea_level(),
            Self::Fixed(g) => g.sea_level(),
        }
    }
    /// `getOrigin`.
    pub fn origin(&self) -> ChunkPos {
        match self {
            Self::Noise(g) => g.origin(),
            Self::Fixed(g) => g.origin(),
        }
    }
    /// `getSpawnHeight`.
    pub fn spawn_height(&self) -> i32 {
        match self {
            Self::Noise(g) => g.spawn_height(),
            Self::Fixed(g) => g.spawn_height(),
        }
    }
    pub fn shape(&self) -> ChunkShape<'_> {
        match self {
            Self::Noise(g) => g.shape(),
            Self::Fixed(g) => g.shape(),
        }
    }
    pub fn empty_chunk(&self, pos: ChunkPos) -> ProtoChunk {
        match self {
            Self::Noise(g) => g.empty_chunk(pos),
            Self::Fixed(g) => g.empty_chunk(pos),
        }
    }
    /// A worker's state for this level.
    pub fn worker(&self) -> Result<ChunkWorker<'_>, GeneratorError> {
        Ok(match self {
            Self::Noise(g) => ChunkWorker::Noise(Box::new(g.worker()?)),
            Self::Fixed(g) => ChunkWorker::Fixed(g.worker()),
        })
    }
}

/// A generation worker of either kind.
pub enum ChunkWorker<'g> {
    Noise(Box<GenerationWorker<'g>>),
    Fixed(FixedWorker<'g>),
}

impl ChunkWorker<'_> {
    /// The generation step to `status` for the region's centre.
    pub fn run(
        &mut self,
        status: ChunkStatus,
        region: &mut GenerationRegion<'_>,
    ) -> Result<(), StepError> {
        match self {
            Self::Noise(w) => w.run(status, region),
            Self::Fixed(w) => w.run(status, region),
        }
    }
}

/// A generator as the level and its pool hold it.
pub type SharedGenerator = Arc<ChunkGenerator>;
