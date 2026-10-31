//! A whole loaded chunk (`LevelChunk` without its level): the sections of the dimension's height,
//! the heightmaps (`Heightmap`: `primeHeightmaps`, `update`, `setRawData`), and block get/set
//! without block updates, light, block entities or neighbour effects; converted from and to
//! [`ChunkData`] as `SerializableChunkData.read`/`copyOf` do. Light nibbles and the parts kept raw
//! ride along unchanged until their systems are ported.

use std::collections::{BTreeMap, HashSet};

use tracing::warn;

use super::chunk::serial::{Heightmap, LightData, RawChunkParts};
use super::chunk::{
    BitStorage, ChunkData, ChunkStatus, ContainerFactory, LevelChunkSection, SectionData,
    SectionFlags, StateFlags,
};
use crate::block::flags::BlockStateData;
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::position::{BlockPos, ChunkPos};

/// Which heightmaps count a state as opaque: bit `Heightmap as usize` (`Heightmap.Types.isOpaque`).
pub trait HeightmapFlags {
    fn heightmap_mask(&self, state: BlockState) -> u8;
}

/// The heightmap predicates of every state, as a byte per state.
pub struct HeightmapTable {
    masks: Vec<u8>,
}

impl HeightmapTable {
    pub fn new(state_count: u32, mut mask: impl FnMut(BlockState) -> u8) -> Self {
        Self {
            masks: (0..state_count).map(|s| mask(BlockState(s))).collect(),
        }
    }
    /// 26.3's predicates: `!isAir()` for the surfaces; the block tags `blocks_motion_in_heightmap`
    /// (ocean floors; and motion blocking, with any fluid) and
    /// `blocks_motion_in_heightmap_no_leaves` (with any fluid), resolved by the caller.
    pub fn vanilla(
        blocks: &BlockStates,
        data: &BlockStateData,
        blocks_motion: &HashSet<BlockId>,
        blocks_motion_no_leaves: &HashSet<BlockId>,
    ) -> Self {
        Self::new(blocks.state_count(), |state| {
            let block = blocks.block(state);
            let fluid = data.section_flags(state).has(StateFlags::HAS_FLUID);
            let motion = blocks_motion.contains(&block);
            let no_leaves = blocks_motion_no_leaves.contains(&block);
            let mut mask = 0;
            for (kind, opaque) in [
                (Heightmap::WorldSurfaceWg, !data.is_air(state)),
                (Heightmap::WorldSurface, !data.is_air(state)),
                (Heightmap::OceanFloorWg, motion),
                (Heightmap::OceanFloor, motion),
                (Heightmap::MotionBlocking, motion || fluid),
                (Heightmap::MotionBlockingNoLeaves, no_leaves || fluid),
            ] {
                mask |= u8::from(opaque) << kind as u8;
            }
            mask
        })
    }
}

impl HeightmapFlags for HeightmapTable {
    fn heightmap_mask(&self, state: BlockState) -> u8 {
        self.masks[state.0 as usize]
    }
}

/// `ChunkStatus.heightmapsAfter()`: the world generation pair before `terrain`, the final four after.
pub fn heightmaps_after(status: ChunkStatus) -> &'static [Heightmap] {
    if status < ChunkStatus::Terrain {
        &[Heightmap::OceanFloorWg, Heightmap::WorldSurfaceWg]
    } else {
        &[
            Heightmap::OceanFloor,
            Heightmap::WorldSurface,
            Heightmap::MotionBlocking,
            Heightmap::MotionBlockingNoLeaves,
        ]
    }
}

/// A light section's saved state (`SectionData` without its blocks), kept for writing back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SectionLight {
    block_light: Option<LightData>,
    sky_light: Option<LightData>,
    block_light_state: Option<i32>,
    sky_light_state: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelChunk {
    pos: ChunkPos,
    min_section_y: i32,
    sections: Vec<LevelChunkSection>,
    /// `SimpleBitStorage(ceillog2(height + 1), 256)` of first free heights above the minimum.
    heightmaps: [Option<BitStorage>; 6],
    light: BTreeMap<i32, SectionLight>,
    pub status: ChunkStatus,
    pub light_correct: bool,
    pub last_update: i64,
    pub inhabited_time: i64,
    pub raw: RawChunkParts,
    /// `ChunkAccess.unsaved`: a chunk read from disk starts saved; changes mark it.
    unsaved: bool,
}

fn index(x: i32, z: i32) -> usize {
    (x & 15) as usize + (z & 15) as usize * 16
}

impl LevelChunk {
    /// An all-air chunk of `section_count` sections, without heightmaps.
    pub fn empty(
        pos: ChunkPos,
        min_section_y: i32,
        section_count: usize,
        factory: &ContainerFactory,
    ) -> Self {
        Self {
            pos,
            min_section_y,
            sections: (0..section_count)
                .map(|_| LevelChunkSection::empty(factory))
                .collect(),
            heightmaps: Default::default(),
            light: BTreeMap::new(),
            status: ChunkStatus::Full,
            light_correct: false,
            last_update: 0,
            inhabited_time: 0,
            raw: RawChunkParts::default(),
            unsaved: false,
        }
    }

    /// `markUnsaved`.
    pub fn mark_unsaved(&mut self) {
        self.unsaved = true;
    }

    pub fn is_unsaved(&self) -> bool {
        self.unsaved
    }

    /// `tryMarkSaved`: true when it was unsaved.
    pub fn try_mark_saved(&mut self) -> bool {
        std::mem::replace(&mut self.unsaved, false)
    }

    /// `SerializableChunkData.read` then `new LevelChunk(level, protoChunk)`: sections by `Y`
    /// (missing ones empty), the stored heightmaps of the status (a full chunk keeps the final
    /// four), and those missing or of the wrong length primed from the blocks.
    pub fn from_data(
        data: ChunkData,
        section_count: usize,
        factory: &ContainerFactory,
        heightmap_flags: &impl HeightmapFlags,
    ) -> Self {
        let mut chunk = Self::empty(data.pos, data.min_section_y, section_count, factory);
        chunk.status = data.status;
        chunk.light_correct = data.light_correct;
        chunk.last_update = data.last_update;
        chunk.inhabited_time = data.inhabited_time;
        chunk.raw = data.raw;
        for s in data.sections {
            let light = SectionLight {
                block_light: s.block_light,
                sky_light: s.sky_light,
                block_light_state: s.block_light_state,
                sky_light_state: s.sky_light_state,
            };
            if light != SectionLight::default() {
                chunk.light.insert(s.y, light);
            }
            if let Some(section) = s.section
                && let Some(slot) = chunk.section_index(s.y << 4)
            {
                chunk.sections[slot] = section;
            }
        }
        let bits = chunk.heightmap_bits();
        let mut to_prime = Vec::new();
        for &kind in heightmaps_after(chunk.status) {
            let words = data.heightmaps[kind as usize].clone().map(|w| {
                let words = w.into_iter().map(|v| v as u64).collect();
                BitStorage::from_words(bits, 256, words)
            });
            match words {
                Some(Ok(storage)) => chunk.heightmaps[kind as usize] = Some(storage),
                Some(Err(e)) => {
                    warn!(
                        "Ignoring heightmap data for chunk {:?}, size does not match; expected: {}, got: {}",
                        chunk.pos, e.expected, e.got
                    );
                    to_prime.push(kind);
                }
                None => to_prime.push(kind),
            }
        }
        chunk.prime_heightmaps(&to_prime, heightmap_flags);
        chunk
    }

    /// `SerializableChunkData.copyOf`'s view of the chunk: every section from one below the
    /// level to one above, the heightmaps it has.
    pub fn to_data(&self) -> ChunkData {
        let first = self.min_section_y - 1;
        let last = self.min_section_y + self.sections.len() as i32;
        let sections = (first..=last)
            .map(|y| {
                let light = self.light.get(&y).cloned().unwrap_or_default();
                SectionData {
                    y,
                    section: self.section_index(y << 4).map(|i| self.sections[i].clone()),
                    block_light: light.block_light,
                    sky_light: light.sky_light,
                    block_light_state: light.block_light_state,
                    sky_light_state: light.sky_light_state,
                }
            })
            .collect();
        ChunkData {
            pos: self.pos,
            min_section_y: self.min_section_y,
            last_update: self.last_update,
            inhabited_time: self.inhabited_time,
            status: self.status,
            light_correct: self.light_correct,
            heightmaps: std::array::from_fn(|i| {
                let words = self.heightmaps[i].as_ref()?.words();
                Some(words.iter().map(|&w| w as i64).collect())
            }),
            sections,
            raw: self.raw.clone(),
        }
    }

    pub fn pos(&self) -> ChunkPos {
        self.pos
    }
    pub fn min_y(&self) -> i32 {
        self.min_section_y << 4
    }
    pub fn height(&self) -> i32 {
        self.sections.len() as i32 * 16
    }
    pub fn sections(&self) -> &[LevelChunkSection] {
        &self.sections
    }
    /// The bottom section's y (`getMinSectionY`).
    pub fn min_section_y(&self) -> i32 {
        self.min_section_y
    }
    /// A heightmap's raw words (`Heightmap.getRawData`), `None` without it.
    pub fn heightmap_words(&self, kind: Heightmap) -> Option<Vec<i64>> {
        let map = self.heightmaps[kind as usize].as_ref()?;
        Some(map.words().iter().map(|&w| w as i64).collect())
    }
    /// The saved light of section `y` (block or sky layer) as Moonrise's `loadLightHook` reads it:
    /// the Starlight state (0, null, when the tag is missing or the section is not listed) and the
    /// nibbles.
    pub fn saved_light(&self, y: i32, sky: bool) -> (i32, Option<&[u8; 2048]>) {
        let Some(light) = self.light.get(&y) else {
            return (0, None);
        };
        let (state, data) = if sky {
            (light.sky_light_state, &light.sky_light)
        } else {
            (light.block_light_state, &light.block_light)
        };
        (state.unwrap_or(0), data.as_deref())
    }
    fn heightmap_bits(&self) -> u8 {
        (self.height() as u32 + 1)
            .next_power_of_two()
            .trailing_zeros() as u8
    }
    fn section_index(&self, y: i32) -> Option<usize> {
        usize::try_from((y >> 4) - self.min_section_y)
            .ok()
            .filter(|&i| i < self.sections.len())
    }

    /// `getBlockState`: air outside the sections. `x` and `z` are taken within the chunk.
    pub fn block_state(&self, pos: BlockPos) -> BlockState {
        block_at(&self.sections, self.min_section_y, pos.x, pos.y, pos.z)
    }

    /// `setBlockState` without block updates: the section's counts, the four live heightmaps, and
    /// the chunk marked unsaved.
    /// `None` when nothing changed (air into an empty section, or the same state), as vanilla.
    pub fn set_block_state(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: &impl SectionFlags,
        heightmap_flags: &impl HeightmapFlags,
    ) -> Option<BlockState> {
        let slot = self.section_index(pos.y)?;
        let section = &mut self.sections[slot];
        if section.has_only_air() && flags.section_flags(state).has(StateFlags::AIR) {
            return None;
        }
        let (x, z) = ((pos.x & 15) as u32, (pos.z & 15) as u32);
        let old = section.set_block_state(x, (pos.y & 15) as u32, z, state, flags);
        if old == state {
            return None;
        }
        self.unsaved = true;
        let mask = heightmap_flags.heightmap_mask(state);
        let min_y = self.min_y();
        for kind in [
            Heightmap::MotionBlocking,
            Heightmap::MotionBlockingNoLeaves,
            Heightmap::OceanFloor,
            Heightmap::WorldSurface,
        ] {
            if let Some(map) = &mut self.heightmaps[kind as usize] {
                let opaque = |s| heightmap_flags.heightmap_mask(s) >> kind as u8 & 1 != 0;
                let i = index(pos.x, pos.z);
                let first_available = map.get(i) as i32 + min_y;
                if pos.y <= first_available - 2 {
                    continue;
                }
                if mask >> kind as u8 & 1 != 0 {
                    if pos.y >= first_available {
                        map.set(i, (pos.y + 1 - min_y) as u32);
                    }
                } else if first_available - 1 == pos.y {
                    let below = (min_y..pos.y).rev().find(|&y| {
                        opaque(block_at(
                            &self.sections,
                            self.min_section_y,
                            pos.x,
                            y,
                            pos.z,
                        ))
                    });
                    map.set(i, below.map_or(0, |y| (y + 1 - min_y) as u32));
                }
            }
        }
        Some(old)
    }

    /// `getHeight(type, x, z)`: the highest opaque block's y (`min_y - 1` when none), or `None`
    /// without that heightmap.
    pub fn height_at(&self, kind: Heightmap, x: i32, z: i32) -> Option<i32> {
        let map = self.heightmaps[kind as usize].as_ref()?;
        Some(map.get(index(x, z)) as i32 + self.min_y() - 1)
    }

    /// `getHighestSectionPosition`: the bottom of the highest section with blocks.
    pub fn highest_section_position(&self) -> i32 {
        let highest = self.sections.iter().rposition(|s| !s.has_only_air());
        highest.map_or(self.min_y(), |i| (self.min_section_y + i as i32) << 4)
    }

    /// `Heightmap.primeHeightmaps`: each column scanned down from the top of the highest
    /// non-empty section; the first state (not `minecraft:air`) opaque for a heightmap sets it.
    pub fn prime_heightmaps(&mut self, kinds: &[Heightmap], heightmap_flags: &impl HeightmapFlags) {
        if kinds.is_empty() {
            return;
        }
        let bits = self.heightmap_bits();
        for &kind in kinds {
            self.heightmaps[kind as usize].get_or_insert_with(|| BitStorage::new(bits, 256));
        }
        let wanted = kinds.iter().fold(0u8, |m, &k| m | 1 << k as u8);
        let (top, min_y) = (self.highest_section_position() + 16, self.min_y());
        for i in 0..256 {
            let (x, z) = ((i % 16) as i32, (i / 16) as i32);
            let mut remaining = wanted;
            for y in (min_y..top).rev() {
                let state = block_at(&self.sections, self.min_section_y, x, y, z);
                if state == BlockState::AIR {
                    continue;
                }
                let hit = heightmap_flags.heightmap_mask(state) & remaining;
                for kind in Heightmap::ALL
                    .into_iter()
                    .filter(|&k| hit >> k as u8 & 1 != 0)
                {
                    let map = self.heightmaps[kind as usize]
                        .as_mut()
                        .expect("created above");
                    map.set(i, (y + 1 - min_y) as u32);
                }
                remaining &= !hit;
                if remaining == 0 {
                    break;
                }
            }
        }
    }
}

fn block_at(
    sections: &[LevelChunkSection],
    min_section_y: i32,
    x: i32,
    y: i32,
    z: i32,
) -> BlockState {
    match usize::try_from((y >> 4) - min_section_y)
        .ok()
        .and_then(|i| sections.get(i))
    {
        Some(s) if !s.has_only_air() => {
            s.block_state((x & 15) as u32, (y & 15) as u32, (z & 15) as u32)
        }
        _ => BlockState::AIR,
    }
}
