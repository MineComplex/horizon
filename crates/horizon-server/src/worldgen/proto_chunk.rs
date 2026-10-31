//! `ProtoChunk`: a chunk while it is generated, on the server's own sections
//! ([`LevelChunkSection`]) and heightmap storage. One job owns the chunk it writes; the chunks
//! around it come as [`GenerationRegion`] neighbours, writable inside the step's write radius and
//! read-only (shared snapshots) outside, the shape of 26.3's `WorldGenRegion` without a level.
//!
//! Kept so far: blocks (with the generation heightmaps of the status), biomes, fluid
//! post-processing marks, pending block entity and entity NBT. Structure starts and references,
//! the carving mask, scheduled ticks and light come with the steps that write them.

use std::collections::BTreeMap;

use crate::block::BlockState;
use crate::core::position::{BlockPos, ChunkPos};
use crate::nbt::Compound;
use crate::world::chunk::serial::Heightmap;
use crate::world::chunk::{
    BitStorage, ChunkStatus, ContainerFactory, LevelChunkSection, SectionFlags, StateFlags,
};
use crate::world::level_chunk::{HeightmapFlags, heightmaps_after};

/// A chunk being generated (see the module docs).
#[derive(Clone, Debug)]
pub struct ProtoChunk {
    pos: ChunkPos,
    min_section_y: i32,
    status: ChunkStatus,
    sections: Vec<LevelChunkSection>,
    /// First free heights above the minimum, by `Heightmap as usize`.
    heightmaps: [Option<BitStorage>; 6],
    /// Per section: packed local positions (`x | y << 4 | z << 8`), in order, duplicates kept.
    postprocessing: Vec<Vec<u16>>,
    /// `minecraft:void_air`, what reads outside the height give.
    void_air: BlockState,
    pub block_entities: BTreeMap<BlockPos, Compound>,
    pub entities: Vec<Compound>,
}

fn column(x: i32, z: i32) -> usize {
    (x & 15) as usize + (z & 15) as usize * 16
}

impl ProtoChunk {
    /// An empty chunk (status `empty`) of `section_count` all-air sections.
    pub fn new(
        pos: ChunkPos,
        min_section_y: i32,
        section_count: usize,
        factory: &ContainerFactory,
        void_air: BlockState,
    ) -> Self {
        Self {
            pos,
            min_section_y,
            status: ChunkStatus::Empty,
            sections: (0..section_count)
                .map(|_| LevelChunkSection::empty(factory))
                .collect(),
            heightmaps: Default::default(),
            postprocessing: vec![Vec::new(); section_count],
            void_air,
            block_entities: BTreeMap::new(),
            entities: Vec::new(),
        }
    }

    pub fn pos(&self) -> ChunkPos {
        self.pos
    }
    pub fn status(&self) -> ChunkStatus {
        self.status
    }
    /// Changing the status primes or clears no heightmap.
    pub fn set_status(&mut self, status: ChunkStatus) {
        self.status = status;
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
    pub fn sections_mut(&mut self) -> &mut [LevelChunkSection] {
        &mut self.sections
    }
    pub fn postprocessing(&self) -> &[Vec<u16>] {
        &self.postprocessing
    }

    fn section_index(&self, y: i32) -> Option<usize> {
        usize::try_from((y >> 4) - self.min_section_y)
            .ok()
            .filter(|&i| i < self.sections.len())
    }

    /// `getBlockState`: `void_air` outside the height, air in an all-air section.
    pub fn block_state(&self, x: i32, y: i32, z: i32) -> BlockState {
        match self.section_index(y) {
            None => self.void_air,
            Some(i) if self.sections[i].has_only_air() => BlockState::AIR,
            Some(i) => {
                self.sections[i].block_state((x & 15) as u32, (y & 15) as u32, (z & 15) as u32)
            }
        }
    }

    /// `setBlockState` of a proto chunk: the section write, then the heightmaps of the status
    /// (`heightmapsAfter`): existing ones updated, missing ones primed after. Returns the old
    /// state (`void_air` outside the height; the new state for air into an all-air section).
    pub fn set_block_state(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        state: BlockState,
        flags: &impl SectionFlags,
        heightmap_flags: &impl HeightmapFlags,
    ) -> BlockState {
        let Some(index) = self.section_index(y) else {
            return self.void_air;
        };
        if self.sections[index].has_only_air() && state == BlockState::AIR {
            return state;
        }
        let old = self.sections[index].set_block_state(
            (x & 15) as u32,
            (y & 15) as u32,
            (z & 15) as u32,
            state,
            flags,
        );
        let mut missing = Vec::new();
        for &kind in heightmaps_after(self.status) {
            if self.heightmaps[kind as usize].is_some() {
                self.update_heightmap(kind, x, y, z, state, heightmap_flags);
            } else {
                missing.push(kind);
            }
        }
        if !missing.is_empty() {
            self.prime_heightmaps(&missing, heightmap_flags);
        }
        old
    }

    /// A section write without heightmaps (the noise fill updates its heightmaps itself).
    pub fn set_block_raw(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        state: BlockState,
        flags: &impl SectionFlags,
    ) -> Option<BlockState> {
        let index = self.section_index(y)?;
        Some(self.sections[index].set_block_state(
            (x & 15) as u32,
            (y & 15) as u32,
            (z & 15) as u32,
            state,
            flags,
        ))
    }

    /// `getHeight(type, x, z)`: the highest opaque block (`min_y - 1` when none); `None` without
    /// that heightmap.
    pub fn height_at(&self, kind: Heightmap, x: i32, z: i32) -> Option<i32> {
        let map = self.heightmaps[kind as usize].as_ref()?;
        Some(map.get(column(x, z)) as i32 + self.min_y() - 1)
    }

    /// The heightmap's raw words (for saving).
    pub fn heightmap(&self, kind: Heightmap) -> Option<&BitStorage> {
        self.heightmaps[kind as usize].as_ref()
    }

    fn heightmap_bits(&self) -> u8 {
        (self.height() as u32 + 1)
            .next_power_of_two()
            .trailing_zeros() as u8
    }

    /// Creates the heightmap (all at the bottom) if missing.
    pub fn ensure_heightmap(&mut self, kind: Heightmap) {
        let bits = self.heightmap_bits();
        self.heightmaps[kind as usize].get_or_insert_with(|| BitStorage::new(bits, 256));
    }

    /// `Heightmap.update`: whether the height changed.
    pub fn update_heightmap(
        &mut self,
        kind: Heightmap,
        x: i32,
        y: i32,
        z: i32,
        state: BlockState,
        heightmap_flags: &impl HeightmapFlags,
    ) -> bool {
        self.ensure_heightmap(kind);
        let min_y = self.min_y();
        let opaque = |s: BlockState| heightmap_flags.heightmap_mask(s) >> kind as u8 & 1 != 0;
        let i = column(x, z);
        let first_available = self.heightmaps[kind as usize]
            .as_ref()
            .expect("ensured")
            .get(i) as i32
            + min_y;
        if y <= first_available - 2 {
            return false;
        }
        let height = if opaque(state) {
            if y < first_available {
                return false;
            }
            y + 1 - min_y
        } else if first_available - 1 == y {
            (min_y..y)
                .rev()
                .find(|&below| opaque(self.block_state(x, below, z)))
                .map_or(0, |below| below + 1 - min_y)
        } else {
            return false;
        };
        self.heightmaps[kind as usize]
            .as_mut()
            .expect("ensured")
            .set(i, height as u32);
        true
    }

    /// `Heightmap.primeHeightmaps`: each column scanned down from the top of the highest
    /// non-empty section; the first state (not `minecraft:air`) opaque for a heightmap sets it.
    pub fn prime_heightmaps(&mut self, kinds: &[Heightmap], heightmap_flags: &impl HeightmapFlags) {
        for &kind in kinds {
            self.ensure_heightmap(kind);
        }
        let wanted = kinds.iter().fold(0u8, |m, &k| m | 1 << k as u8);
        let highest = self.sections.iter().rposition(|s| !s.has_only_air());
        let top = highest.map_or(self.min_y(), |i| (self.min_section_y + i as i32) << 4) + 16;
        let min_y = self.min_y();
        for i in 0..256 {
            let (x, z) = ((i % 16) as i32, (i / 16) as i32);
            let mut remaining = wanted;
            for y in (min_y..top).rev() {
                let state = self.block_state(x, y, z);
                if state == BlockState::AIR {
                    continue;
                }
                let hit = heightmap_flags.heightmap_mask(state) & remaining;
                for kind in Heightmap::ALL {
                    if hit >> kind as u8 & 1 != 0 {
                        self.heightmaps[kind as usize]
                            .as_mut()
                            .expect("created above")
                            .set(i, (y + 1 - min_y) as u32);
                    }
                }
                remaining &= !hit;
                if remaining == 0 {
                    break;
                }
            }
        }
    }

    /// `markPosForPostprocessing` (fluids the noise fill placed, blocks that need an update).
    pub fn mark_postprocessing(&mut self, x: i32, y: i32, z: i32) {
        if let Some(index) = self.section_index(y) {
            self.postprocessing[index].push(((x & 15) | ((y & 15) << 4) | ((z & 15) << 8)) as u16);
        }
    }

    /// The biome (registry id) at quart coordinates of the chunk, clamped to its height as
    /// `getNoiseBiome` does.
    pub fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u32 {
        let min_quart = self.min_section_y << 2;
        let max_quart = min_quart + (self.sections.len() as i32) * 4 - 1;
        let quart_y = quart_y.clamp(min_quart, max_quart);
        let section = ((quart_y >> 2) - self.min_section_y) as usize;
        self.sections[section].noise_biome(
            (quart_x & 3) as u32,
            (quart_y & 3) as u32,
            (quart_z & 3) as u32,
        )
    }

    /// `ChunkAccess.fillBiomesFromNoise`: every section, bottom to top, refilled by Paper's
    /// section fill (`LevelChunkSection::fill_biomes_from_noise`) from `biome` at absolute quart
    /// coordinates.
    pub fn fill_biomes_from_noise(&mut self, mut biome: impl FnMut([i32; 3]) -> u32) {
        let (quart_x, quart_z) = (self.pos.x.wrapping_shl(2), self.pos.z.wrapping_shl(2));
        for (i, section) in self.sections.iter_mut().enumerate() {
            let quart_y = (self.min_section_y + i as i32) << 2;
            section.fill_biomes_from_noise(|x, y, z| {
                biome([quart_x + x as i32, quart_y + y as i32, quart_z + z as i32])
            });
        }
    }

    /// Whether a state is air by the section flags (generation's `isAir`).
    pub fn is_air(state: BlockState, flags: &impl SectionFlags) -> bool {
        flags.section_flags(state).has(StateFlags::AIR)
    }
}

/// A chunk of a [`GenerationRegion`].
#[derive(Debug)]
pub enum RegionChunk<'a> {
    /// Inside the step's write radius: owned by this job for the step.
    Writable(&'a mut ProtoChunk),
    /// Outside it: a snapshot other jobs may read at the same time.
    ReadOnly(&'a ProtoChunk),
}

/// The chunks a generation step sees around its centre: a square of `radius`, row by row
/// (z, then x).
#[derive(Debug)]
pub struct GenerationRegion<'a> {
    center: ChunkPos,
    radius: i32,
    chunks: Vec<RegionChunk<'a>>,
}

/// A region access outside its chunks, or a write outside its write radius: vanilla logs it
/// (`Detected setBlock in a far chunk`) and refuses the write; reads give void air.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("chunk {0:?} is outside the generation region or not writable")]
pub struct OutsideRegion(pub ChunkPos);

impl<'a> GenerationRegion<'a> {
    /// `chunks` are the `(2 * radius + 1)²` chunks around `center`, row by row.
    pub fn new(center: ChunkPos, radius: i32, chunks: Vec<RegionChunk<'a>>) -> Self {
        let side = (2 * radius + 1) as usize;
        assert_eq!(chunks.len(), side * side, "a square of chunks");
        for (i, chunk) in chunks.iter().enumerate() {
            let pos = match chunk {
                RegionChunk::Writable(c) => c.pos(),
                RegionChunk::ReadOnly(c) => c.pos(),
            };
            let expected = ChunkPos::new(
                center.x - radius + (i % side) as i32,
                center.z - radius + (i / side) as i32,
            );
            assert_eq!(pos, expected, "chunks row by row around the centre");
        }
        Self {
            center,
            radius,
            chunks,
        }
    }

    pub fn center(&self) -> ChunkPos {
        self.center
    }

    fn slot(&self, pos: ChunkPos) -> Option<usize> {
        let dx = pos.x - (self.center.x - self.radius);
        let dz = pos.z - (self.center.z - self.radius);
        let side = 2 * self.radius + 1;
        ((0..side).contains(&dx) && (0..side).contains(&dz)).then(|| (dz * side + dx) as usize)
    }

    pub fn chunk(&self, pos: ChunkPos) -> Option<&ProtoChunk> {
        self.slot(pos).map(|i| match &self.chunks[i] {
            RegionChunk::Writable(chunk) => &**chunk,
            RegionChunk::ReadOnly(chunk) => *chunk,
        })
    }

    pub fn chunk_mut(&mut self, pos: ChunkPos) -> Result<&mut ProtoChunk, OutsideRegion> {
        match self.slot(pos).map(|i| &mut self.chunks[i]) {
            Some(RegionChunk::Writable(chunk)) => Ok(chunk),
            _ => Err(OutsideRegion(pos)),
        }
    }

    pub fn block_state(&self, pos: BlockPos) -> Result<BlockState, OutsideRegion> {
        let chunk_pos = ChunkPos::new(pos.x >> 4, pos.z >> 4);
        self.chunk(chunk_pos)
            .map(|chunk| chunk.block_state(pos.x, pos.y, pos.z))
            .ok_or(OutsideRegion(chunk_pos))
    }

    /// `WorldGenRegion.getNoiseBiome`: the stored biome of the chunk holding the quart.
    pub fn noise_biome(&self, quart: [i32; 3]) -> Result<u32, OutsideRegion> {
        let chunk_pos = ChunkPos::new(quart[0] >> 2, quart[2] >> 2);
        self.chunk(chunk_pos)
            .map(|chunk| chunk.noise_biome(quart[0], quart[1], quart[2]))
            .ok_or(OutsideRegion(chunk_pos))
    }

    pub fn set_block_state(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: &impl SectionFlags,
        heightmap_flags: &impl HeightmapFlags,
    ) -> Result<BlockState, OutsideRegion> {
        let chunk = self.chunk_mut(ChunkPos::new(pos.x >> 4, pos.z >> 4))?;
        Ok(chunk.set_block_state(pos.x, pos.y, pos.z, state, flags, heightmap_flags))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// States 0 (air), 1 (stone: solid everywhere), 2 (water: fluid, motion blocking only),
    /// 3 (void air).
    struct Flags;
    impl SectionFlags for Flags {
        fn section_flags(&self, state: BlockState) -> StateFlags {
            match state.0 {
                0 | 3 => StateFlags(StateFlags::AIR),
                2 => StateFlags(StateFlags::HAS_FLUID),
                _ => StateFlags(0),
            }
        }
    }
    impl HeightmapFlags for Flags {
        fn heightmap_mask(&self, state: BlockState) -> u8 {
            match state.0 {
                0 | 3 => 0,
                2 => {
                    1 << Heightmap::WorldSurfaceWg as u8
                        | 1 << Heightmap::WorldSurface as u8
                        | 1 << Heightmap::MotionBlocking as u8
                }
                _ => 0b11_1111,
            }
        }
    }

    fn chunk(x: i32, z: i32) -> ProtoChunk {
        ProtoChunk::new(
            ChunkPos::new(x, z),
            -4,
            24,
            &ContainerFactory::new(4, 2, 0),
            BlockState(3),
        )
    }

    #[test]
    fn writes_update_the_status_heightmaps() {
        let mut c = chunk(0, 0);
        let (stone, water) = (BlockState(1), BlockState(2));
        assert_eq!(c.block_state(0, -65, 0), BlockState(3));
        assert_eq!(
            c.set_block_state(0, -65, 0, stone, &Flags, &Flags),
            BlockState(3)
        );
        assert_eq!(
            c.set_block_state(1, 10, 1, BlockState::AIR, &Flags, &Flags),
            BlockState::AIR
        );
        c.set_block_state(1, 10, 1, stone, &Flags, &Flags);
        c.set_block_state(1, 11, 1, water, &Flags, &Flags);
        // Before `terrain`, the world generation pair is kept.
        assert_eq!(c.height_at(Heightmap::OceanFloorWg, 1, 1), Some(10));
        assert_eq!(c.height_at(Heightmap::WorldSurfaceWg, 1, 1), Some(11));
        assert_eq!(c.height_at(Heightmap::MotionBlocking, 1, 1), None);
        c.set_block_state(1, 11, 1, BlockState::AIR, &Flags, &Flags);
        assert_eq!(c.height_at(Heightmap::WorldSurfaceWg, 1, 1), Some(10));
        c.set_status(ChunkStatus::Features);
        c.set_block_state(2, 0, 2, water, &Flags, &Flags);
        // The final four, primed by scanning the column.
        assert_eq!(c.height_at(Heightmap::MotionBlocking, 1, 1), Some(10));
        assert_eq!(c.height_at(Heightmap::MotionBlocking, 2, 2), Some(0));
        assert_eq!(c.height_at(Heightmap::OceanFloor, 2, 2), Some(-65));
        c.mark_postprocessing(2, 0, 2);
        c.mark_postprocessing(2, 0, 2);
        assert_eq!(c.postprocessing()[4], vec![2 | 2 << 8, 2 | 2 << 8]);
    }

    #[test]
    fn regions_write_inside_the_radius_and_read_neighbours() {
        let mut center = chunk(5, 5);
        let others: Vec<ProtoChunk> = (0..9)
            .filter(|&i| i != 4)
            .map(|i| chunk(4 + i % 3, 4 + i / 3))
            .collect();
        let mut chunks: Vec<RegionChunk<'_>> = others.iter().map(RegionChunk::ReadOnly).collect();
        chunks.insert(4, RegionChunk::Writable(&mut center));
        let mut region = GenerationRegion::new(ChunkPos::new(5, 5), 1, chunks);
        let inside = BlockPos::new(5 * 16 + 3, 7, 5 * 16 + 9);
        region
            .set_block_state(inside, BlockState(1), &Flags, &Flags)
            .unwrap();
        assert_eq!(region.block_state(inside), Ok(BlockState(1)));
        let neighbour = BlockPos::new(4 * 16, 7, 6 * 16);
        assert_eq!(region.block_state(neighbour), Ok(BlockState::AIR));
        assert_eq!(
            region.set_block_state(neighbour, BlockState(1), &Flags, &Flags),
            Err(OutsideRegion(ChunkPos::new(4, 6)))
        );
        assert!(region.block_state(BlockPos::new(0, 0, 0)).is_err());
    }
}
