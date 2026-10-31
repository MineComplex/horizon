//! Placing structure starts in the FEATURES step: `StructureStart.placeInChunk` and, for jigsaw
//! pieces, `PoolElementStructurePiece.place` → `StructurePoolElement.place` (templates through
//! [`place_in_world`], features through the feature registry, lists in order).
//!
//! Paper wraps the level in a `TransformerLevelAccessor` for plugins' structure transformers; with
//! no plugins it passes every call through, so nothing of it is ported. Pieces of types not ported
//! yet are skipped. What a template places besides blocks (block entity NBT, entities) has nowhere
//! to go until block entities and entities are ported; it is dropped (a stand-in).

use std::borrow::Cow;

use super::jigsaw::JigsawPiece;
use super::place::{PlaceContext, PlaceError, PlaceSettings, PlacedTemplate, place_in_world};
use super::pool::{ElementKind, PoolElementId, Projection, template_of};
use super::processor::{ProcessError, Processor, ProcessorLists};
use super::start::{StructurePiece, StructureStart};
use super::{Decoration, StructureId, StructureRegistry};
use crate::core::position::{BlockPos, ChunkPos};
use crate::util::random::{RandomSource, WorldgenRandom, XoroshiroRandom};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::density::StructureBounds;
use crate::worldgen::feature::FeatureRegistry;
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::feature::decorate::StructureDecorator;
use crate::worldgen::state_provider::ProviderError;

/// `Block.UPDATE_CLIENTS | Block.UPDATE_KNOWN_SHAPE`.
const PIECE_FLAGS: i32 = 2 | 16;

/// The tables piece placement reads.
#[derive(Clone, Copy)]
pub struct PieceTables<'a> {
    pub structures: &'a StructureRegistry,
    pub processors: &'a ProcessorLists,
    pub features: &'a FeatureRegistry,
    /// The level seed (processors' `getLevel().getSeed()`).
    pub seed: i64,
}

fn provider_error(e: PlaceError) -> ProviderError {
    match e {
        PlaceError::Process(ProcessError::World(e)) => ProviderError::World(e),
        PlaceError::Process(ProcessError::Bound(e)) => ProviderError::Random(e),
        PlaceError::Palette { .. } => ProviderError::Invalid("palette index out of bounds"),
    }
}

/// `BoundingBox.intersects(other)`.
pub fn intersects(a: StructureBounds, b: StructureBounds) -> bool {
    let (amin, amax, bmin, bmax) = (a.minimum(), a.maximum(), b.minimum(), b.maximum());
    (0..3).all(|i| amax[i] >= bmin[i] && amin[i] <= bmax[i])
}

/// `getWritableArea(chunk)`: the chunk's columns from one above the bottom to the top.
pub fn writable_area(chunk: ChunkPos, min_y: i32, height: i32) -> StructureBounds {
    let (x, z) = (chunk.x << 4, chunk.z << 4);
    StructureBounds::from_corners([x, min_y + 1, z], [x + 15, min_y + height - 1, z + 15])
}

impl PieceTables<'_> {
    /// `StructureStart.placeInChunk(level, structureManager, generator, random, chunkBB, chunkPos)`.
    pub fn place_start<W: FeatureWorld, R: RandomSource>(
        &self,
        start: &StructureStart,
        world: &mut W,
        random: &mut R,
        chunk_box: StructureBounds,
    ) -> Result<(), ProviderError> {
        let Some(reference) = reference_of(start) else {
            return Ok(());
        };
        let target = PieceTarget {
            chunk_box,
            reference,
            keep_jigsaws: false,
        };
        let mut placed = Vec::new();
        for piece in &start.children {
            if !piece
                .bounding_box()
                .is_some_and(|b| intersects(b, chunk_box))
            {
                continue;
            }
            if let StructurePiece::Jigsaw(piece) = piece {
                self.place_piece(piece, world, random, target, &mut placed)?;
            }
        }
        // What the templates placed besides blocks has nowhere to go yet (module docs).
        drop(placed);
        Ok(())
    }

    /// `PoolElementStructurePiece.place(level, ..., chunkBB, referencePos, keepJigsaws)`; each
    /// template placement's block entities and entities are pushed to `placed`.
    pub fn place_piece<W: FeatureWorld, R: RandomSource>(
        &self,
        piece: &JigsawPiece,
        world: &mut W,
        random: &mut R,
        target: PieceTarget,
        placed: &mut Vec<PlacedTemplate>,
    ) -> Result<bool, ProviderError> {
        self.place_element(piece.element, piece, world, random, target, placed)
    }

    fn place_element<W: FeatureWorld, R: RandomSource>(
        &self,
        element: PoolElementId,
        piece: &JigsawPiece,
        world: &mut W,
        random: &mut R,
        target: PieceTarget,
        placed: &mut Vec<PlacedTemplate>,
    ) -> Result<bool, ProviderError> {
        let (chunk_box, reference, keep_jigsaws) =
            (target.chunk_box, target.reference, target.keep_jigsaws);
        let element_data = self.structures.pools.element(element);
        match &element_data.kind {
            ElementKind::Empty => Ok(true),
            ElementKind::Feature { feature, .. } => {
                self.features
                    .place_placed(*feature, world, random, piece.position)
            }
            ElementKind::List(children) => {
                for &child in children {
                    if !self.place_element(child, piece, world, random, target, placed)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ElementKind::Single {
                legacy,
                location,
                processors,
                liquid,
                ..
            } => {
                let known = self.processors.known();
                // `SinglePoolElement.getSettings`, with `LegacySinglePoolElement`'s swap: the
                // structure-block ignore popped, structure-and-air ignored last.
                let mut list: Vec<Cow<'_, Processor>> = Vec::new();
                if !legacy {
                    list.push(Cow::Owned(known.ignore_structure_block()));
                }
                if !keep_jigsaws {
                    list.push(Cow::Owned(Processor::JigsawReplacement));
                }
                list.extend(
                    self.processors
                        .resolve(processors)
                        .0
                        .iter()
                        .map(Cow::Borrowed),
                );
                if element_data.projection == Projection::TerrainMatching {
                    list.push(Cow::Owned(Processor::Gravity {
                        heightmap: Heightmap::WorldSurfaceWg,
                        offset: -1,
                    }));
                }
                if *legacy {
                    list.push(Cow::Owned(known.ignore_structure_and_air()));
                }
                let settings = PlaceSettings {
                    rotation: piece.rotation,
                    bounding_box: Some(chunk_box),
                    known_shape: true,
                    ignore_entities: false,
                    finalize_entities: true,
                    liquid: liquid.unwrap_or(piece.liquid),
                    processors: list,
                    ..PlaceSettings::default()
                };
                let template = template_of(&self.structures.templates, location);
                let cx = PlaceContext {
                    known,
                    seed: self.seed,
                };
                let result = place_in_world(
                    world,
                    cx,
                    template,
                    piece.position,
                    reference,
                    &settings,
                    random,
                    PIECE_FLAGS,
                )
                .map_err(provider_error)?;
                // Data markers: `handleDataMarker` does nothing for pool elements.
                Ok(result.map(|result| placed.push(result)).is_some())
            }
        }
    }
}

/// Where a piece is placed: the chunk's writable box, the start's reference position and whether
/// jigsaw blocks stay (`/place jigsaw` keeps them; generation replaces them).
#[derive(Clone, Copy, Debug)]
pub struct PieceTarget {
    pub chunk_box: StructureBounds,
    pub reference: BlockPos,
    pub keep_jigsaws: bool,
}

/// `placeInChunk`'s reference: the centre piece's box centre (`getCenter`) at its bottom.
pub fn reference_of(start: &StructureStart) -> Option<BlockPos> {
    let center = start
        .children
        .first()
        .and_then(StructurePiece::bounding_box)?;
    let (min, max) = (center.minimum(), center.maximum());
    Some(BlockPos::new(
        min[0] + (max[0] - min[0] + 1) / 2,
        min[1],
        min[2] + (max[2] - min[2] + 1) / 2,
    ))
}

/// The structures of the FEATURES step for one chunk: the registered structures by decoration
/// step (`structuresByStep`, registration order), each with the valid starts the chunk references
/// (`startsForStructure`), cloned out of the region before its decoration starts.
pub struct StartPlacer<'a> {
    pub tables: PieceTables<'a>,
    by_step: Vec<Vec<StructureId>>,
    starts: Vec<(StructureId, Vec<StructureStart>)>,
    chunk_box: StructureBounds,
}

impl<'a> StartPlacer<'a> {
    /// `generate` is `structureManager.shouldGenerateStructures()`; `references` the centre
    /// chunk's references and `start_at` a chunk's start of a structure.
    pub fn new<'s>(
        tables: PieceTables<'a>,
        generate: bool,
        references: impl Iterator<Item = (StructureId, &'s [i64])>,
        start_at: impl Fn(ChunkPos, StructureId) -> Option<&'s StructureStart>,
        chunk_box: StructureBounds,
    ) -> Self {
        let mut by_step = vec![Vec::new(); Decoration::ALL.len()];
        if generate {
            for (i, structure) in tables.structures.structures.iter().enumerate() {
                // Structures written inline in sets are not in the registry.
                if !structure.id.contains("#inline") {
                    by_step[structure.step as usize].push(StructureId(i as u32));
                }
            }
        }
        let starts = references
            .map(|(structure, keys)| {
                let starts = (keys.iter())
                    .filter_map(|&key| start_at(ChunkPos::unpack(key), structure))
                    .filter(|start| start.is_valid())
                    .cloned()
                    .collect();
                (structure, starts)
            })
            .collect();
        Self {
            tables,
            by_step,
            starts,
            chunk_box,
        }
    }
}

impl<W: FeatureWorld> StructureDecorator<W> for StartPlacer<'_> {
    fn structures_in_step(&self, step: usize) -> usize {
        self.by_step.get(step).map_or(0, Vec::len)
    }

    fn place(
        &mut self,
        step: usize,
        index: usize,
        world: &mut W,
        random: &mut WorldgenRandom<XoroshiroRandom>,
    ) -> Result<(), ProviderError> {
        let structure = self.by_step[step][index];
        let Some((_, starts)) = self.starts.iter().find(|(s, _)| *s == structure) else {
            return Ok(());
        };
        for start in starts {
            self.tables
                .place_start(start, world, random, self.chunk_box)?;
        }
        Ok(())
    }
}
