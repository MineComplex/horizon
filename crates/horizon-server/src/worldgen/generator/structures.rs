//! The STRUCTURE_STARTS and STRUCTURE_REFERENCES steps (`ChunkGenerator.createStructures`,
//! `createReferences`) over `worldgen::structure`'s placements and start/reference storage.
//!
//! Making a start (`Structure.generate`: the generation point, the biome check at it, the pieces)
//! is [`generate_start`]: pieces (jigsaw, templates, legacy pieces) are being ported separately,
//! so it makes none yet and no structure starts — the placement choices around it are vanilla's.

use super::LevelGenerator;
use crate::core::position::ChunkPos;
use crate::worldgen::proto_chunk::{GenerationRegion, OutsideRegion, ProtoChunk};
use crate::worldgen::structure::StructureId;
use crate::worldgen::structure::start::{StructureStart, create_references};

/// `tryGenerateStructure`'s `structure.generate(...)` for `structure` starting in `chunk`, with
/// the references a previous start of it in the chunk had (`fetchReferences`). `None` is an
/// invalid start (no generation point, a biome the structure does not allow, or no pieces).
///
/// The pieces are not ported yet: no start is made.
pub fn generate_start(
    generator: &LevelGenerator,
    structure: StructureId,
    chunk: ChunkPos,
    references: i32,
) -> Option<StructureStart> {
    let _ = (generator, structure, chunk, references);
    None
}

/// `createStructures` for the chunk (when the level generates structures): for each possible
/// set whose placement allows the chunk and that has no valid start here yet, its structures are
/// tried (`create_starts`' order) until one makes a valid start, which the chunk keeps.
pub fn create_structures(generator: &LevelGenerator, chunk: &mut ProtoChunk) {
    let pos = chunk.pos();
    let registry = generator.data().structures();
    let structures = &mut chunk.structures;
    // Both callbacks see the starts made so far (vanilla stores each in the chunk at once).
    let chosen: std::cell::RefCell<Vec<(StructureId, StructureStart)>> = Default::default();
    generator.placements().create_starts(
        registry,
        pos,
        |structure| {
            chosen
                .borrow()
                .iter()
                .any(|(s, start)| *s == structure && start.is_valid())
                || structures
                    .start(structure)
                    .is_some_and(StructureStart::is_valid)
        },
        |structure| {
            let references = structures.start(structure).map_or(0, |s| s.references);
            match generate_start(generator, structure, pos, references) {
                Some(start) if start.is_valid() => {
                    chosen.borrow_mut().push((structure, start));
                    true
                }
                _ => false,
            }
        },
    );
    for (structure, start) in chosen.into_inner() {
        structures.set_start(structure, start);
    }
}

/// `createReferences`: every valid start within 8 chunks whose box meets the centre's columns
/// is referenced by the centre chunk. The region reaches 8 chunks (STRUCTURE_STARTS done there).
pub fn create_references_in(region: &mut GenerationRegion<'_>) -> Result<(), OutsideRegion> {
    let center = region.center();
    let references = create_references(center, |pos| region.chunk(pos).map(|c| &c.structures));
    let chunk = region.chunk_mut(center)?;
    for (structure, key) in references {
        chunk.structures.add_reference(structure, key);
    }
    Ok(())
}
