//! A proto chunk to and from its saved form (`SerializableChunkData.copyOf` / `read` for
//! `PROTOCHUNK` statuses), and the FULL step's `new LevelChunk(level, protoChunk)`.
//!
//! What the proto chunk does not keep (light of a chunk saved by Starlight, upgrade data,
//! CraftBukkit's values...) rides along in [`ProtoExtras`] and is written back as read.

use crate::block::BlockState;
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, Tag};
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::serial::RawChunkParts;
use crate::world::chunk::{ChunkData, ChunkStatus, ContainerFactory, SectionData};
use crate::world::level_chunk::{HeightmapFlags, LevelChunk, heightmaps_after};
use crate::world::ticks::ProtoChunkTicks;
use crate::worldgen::proto_chunk::ProtoChunk;
use crate::worldgen::structure::StructureRegistry;
use crate::worldgen::structure::start::{ChunkStructures, PieceChange};

/// What a level's chunks are made with.
#[derive(Clone, Copy, Debug)]
pub struct ChunkShape<'a> {
    pub min_section_y: i32,
    pub section_count: usize,
    pub factory: &'a ContainerFactory,
    /// `minecraft:void_air`.
    pub void_air: BlockState,
}

impl ChunkShape<'_> {
    /// `new ProtoChunk(pos, UpgradeData.EMPTY, level, ...)`: an empty chunk.
    pub fn empty(&self, pos: ChunkPos) -> ProtoChunk {
        ProtoChunk::new(
            pos,
            self.min_section_y,
            self.section_count,
            self.factory,
            self.void_air,
        )
    }
}

/// A proto chunk's saved parts that generation does not touch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProtoExtras {
    /// Starlight's `isLightCorrect` (always false for chunks generated here: light is not
    /// ported).
    pub light_correct: bool,
    pub last_update: i64,
    pub inhabited_time: i64,
    /// The saved light layers by section (`section` is `None` in them).
    pub light: Vec<SectionData>,
    /// Without `post_processing`, `structures` and `entities`, which the proto chunk holds. The
    /// saved block entities stay here (written before those generation adds).
    pub raw: RawChunkParts,
    /// `ChunkAccess.unsaved`: a generation step changed it since it was read or saved.
    pub unsaved: bool,
}

/// `SerializableChunkData.read` for a proto chunk of status `data.status` (not `FULL`). A start
/// whose inner id is malformed fails the read, as it fails the chunk's load in Leaf. `seed` is the
/// level's (`loadStaticStart`'s).
pub fn proto_from_data(
    data: ChunkData,
    shape: ChunkShape<'_>,
    structures: &StructureRegistry,
    seed: i64,
) -> Result<(ProtoChunk, ProtoExtras), String> {
    let pos = data.pos;
    let mut chunk = shape.empty(pos);
    chunk.set_status(data.status);
    let mut light = Vec::new();
    for mut section in data.sections {
        if let Some(s) = section.section.take() {
            let index = section.y - shape.min_section_y;
            if let Some(slot) = usize::try_from(index)
                .ok()
                .and_then(|i| chunk.sections_mut().get_mut(i))
            {
                *slot = s;
            }
        }
        if section.block_light.is_some()
            || section.sky_light.is_some()
            || section.block_light_state.is_some()
            || section.sky_light_state.is_some()
        {
            light.push(section);
        }
    }
    for &kind in heightmaps_after(data.status) {
        if let Some(words) = &data.heightmaps[kind as usize] {
            let words = words.iter().map(|&w| w as u64).collect();
            if !chunk.set_heightmap(kind, words) {
                tracing::warn!(
                    "Ignoring heightmap data for chunk {pos:?}, size does not match; expected: {}, got: {}",
                    (256 * usize::from(heightmap_bits(chunk.height()))).div_ceil(64),
                    words_len(&data.heightmaps[kind as usize])
                );
            }
        }
    }
    let mut raw = data.raw;
    if let Some(Tag::List(sections)) = raw.post_processing.take() {
        for (index, section) in sections.iter().enumerate() {
            let Tag::List(offsets) = section else {
                continue;
            };
            let base = chunk.min_y() + index as i32 * 16;
            for offset in offsets {
                if let Tag::Short(packed) = offset {
                    let packed = *packed as i32;
                    chunk.mark_postprocessing(
                        packed & 15,
                        base + ((packed >> 4) & 15),
                        (packed >> 8) & 15,
                    );
                }
            }
        }
    }
    if let Some(tag) = raw.structures.take() {
        chunk.structures = ChunkStructures::load(structures, &tag, pos, seed)?;
    }
    chunk.entities = std::mem::take(&mut raw.entities);
    chunk.block_ticks = ProtoChunkTicks::load(std::mem::take(&mut raw.block_ticks));
    chunk.fluid_ticks = ProtoChunkTicks::load(std::mem::take(&mut raw.fluid_ticks));
    Ok((
        chunk,
        ProtoExtras {
            light_correct: data.light_correct,
            last_update: data.last_update,
            inhabited_time: data.inhabited_time,
            light,
            raw,
            unsaved: false,
        },
    ))
}

fn heightmap_bits(height: i32) -> u8 {
    (height as u32 + 1).next_power_of_two().trailing_zeros() as u8
}

fn words_len(words: &Option<Vec<i64>>) -> usize {
    words.as_ref().map_or(0, Vec::len)
}

/// `SerializableChunkData.copyOf` for a proto chunk: its sections from one below the level to one
/// above (with the light it was read with), its status' heightmaps, post-processing marks (`packOffsets`:
/// a list per section), structures, entities and block entities.
pub fn proto_to_data(
    chunk: &ProtoChunk,
    extras: &ProtoExtras,
    structures: &StructureRegistry,
) -> ChunkData {
    let min_section_y = chunk.min_y() >> 4;
    let count = chunk.sections().len() as i32;
    let sections = (min_section_y - 1..=min_section_y + count)
        .map(|y| {
            let light = extras.light.iter().find(|s| s.y == y);
            let index = y - min_section_y;
            SectionData {
                y,
                section: (0..count)
                    .contains(&index)
                    .then(|| chunk.sections()[index as usize].clone()),
                block_light: light.and_then(|l| l.block_light.clone()),
                sky_light: light.and_then(|l| l.sky_light.clone()),
                block_light_state: light.and_then(|l| l.block_light_state),
                sky_light_state: light.and_then(|l| l.sky_light_state),
            }
        })
        .collect();
    let mut raw = extras.raw.clone();
    raw.post_processing = Some(Tag::List(
        chunk
            .postprocessing()
            .iter()
            .map(|offsets| Tag::List(offsets.iter().map(|&o| Tag::Short(o as i16)).collect()))
            .collect(),
    ));
    raw.structures = Some(chunk.structures.save(structures, chunk.pos()));
    raw.entities = chunk.entities.clone();
    raw.block_ticks = chunk.block_ticks.ticks().to_vec();
    raw.fluid_ticks = chunk.fluid_ticks.ticks().to_vec();
    raw.block_entities.extend(
        chunk
            .block_entities
            .values()
            .cloned()
            .collect::<Vec<Compound>>(),
    );
    ChunkData {
        pos: chunk.pos(),
        min_section_y,
        last_update: extras.last_update,
        inhabited_time: extras.inhabited_time,
        status: chunk.status(),
        light_correct: extras.light_correct,
        // Only the heightmaps of the status (`heightmapsAfter`) are saved.
        heightmaps: std::array::from_fn(|i| {
            let kind = Heightmap::ALL[i];
            if !heightmaps_after(chunk.status()).contains(&kind) {
                return None;
            }
            let words = chunk.heightmap(kind)?.words();
            Some(words.iter().map(|&w| w as i64).collect())
        }),
        sections,
        raw,
    }
}

/// The FULL step's `new LevelChunk(level, protoChunk)` (with `postLoadProtoChunk`'s entities kept
/// as NBT until entities are ported): unsaved when the proto chunk was.
pub fn proto_into_level_chunk(
    chunk: &ProtoChunk,
    extras: &ProtoExtras,
    shape: ChunkShape<'_>,
    structures: &StructureRegistry,
    heightmaps: &impl HeightmapFlags,
) -> LevelChunk {
    let mut data = proto_to_data(chunk, extras, structures);
    data.status = ChunkStatus::Full;
    let mut level = LevelChunk::from_data(data, shape.section_count, shape.factory, heightmaps);
    if extras.unsaved {
        level.mark_unsaved();
    }
    level
}

/// A piece change for a start a full chunk holds: the chunk keeps its starts as their saved
/// `structures` tag, which is read, changed and written again. True when it applied.
pub fn apply_piece_change_to_level(
    chunk: &mut LevelChunk,
    registry: &StructureRegistry,
    seed: i64,
    change: &PieceChange,
) -> bool {
    let pos = chunk.pos();
    let Some(tag) = chunk.raw.structures.as_ref() else {
        return false;
    };
    let Ok(mut structures) = ChunkStructures::load(registry, tag, pos, seed) else {
        return false;
    };
    if !structures.apply_change(change) {
        return false;
    }
    chunk.raw.structures = Some(structures.save(registry, pos));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::position::BlockPos;
    use crate::world::chunk::{SectionFlags, StateFlags};

    struct Flags;
    impl SectionFlags for Flags {
        fn section_flags(&self, state: BlockState) -> StateFlags {
            StateFlags(if state.0 == 0 { StateFlags::AIR } else { 0 })
        }
    }
    impl HeightmapFlags for Flags {
        fn heightmap_mask(&self, state: BlockState) -> u8 {
            if state.0 == 0 { 0 } else { 0b11_1111 }
        }
    }

    #[test]
    fn a_proto_chunk_round_trips_through_its_saved_form() {
        let factory = ContainerFactory::new(4, 2, 0);
        let shape = ChunkShape {
            min_section_y: -1,
            section_count: 3,
            factory: &factory,
            void_air: BlockState(3),
        };
        let registry = StructureRegistry::default();
        let mut chunk = shape.empty(ChunkPos::new(3, -2));
        chunk.set_block_state(1, 5, 2, BlockState(1), &Flags, &Flags);
        chunk.set_status(ChunkStatus::Terrain);
        chunk.prime_heightmaps(heightmaps_after(ChunkStatus::Terrain), &Flags);
        chunk.mark_postprocessing(1, 6, 2);
        chunk.mark_postprocessing(4, -3, 9);
        let extras = ProtoExtras {
            inhabited_time: 7,
            ..ProtoExtras::default()
        };
        let data = proto_to_data(&chunk, &extras, &registry);
        assert_eq!(data.sections.len(), 5);
        assert_eq!(data.status, ChunkStatus::Terrain);
        let (back, extras_back) = proto_from_data(data.clone(), shape, &registry, 0).unwrap();
        assert_eq!(back.block_state(1, 5, 2), BlockState(1));
        assert_eq!(back.postprocessing(), chunk.postprocessing());
        assert_eq!(
            back.height_at(Heightmap::MotionBlocking, 1, 2),
            Some(5),
            "the saved heightmaps of the status"
        );
        assert_eq!(extras_back.inhabited_time, 7);
        assert_eq!(proto_to_data(&back, &extras_back, &registry), data);
        let full = proto_into_level_chunk(
            &back,
            &ProtoExtras {
                unsaved: true,
                ..extras_back
            },
            shape,
            &registry,
            &Flags,
        );
        assert_eq!(full.status, ChunkStatus::Full);
        assert!(full.is_unsaved());
        assert_eq!(full.block_state(BlockPos::new(1, 5, 2)), BlockState(1));
    }
}
