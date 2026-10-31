//! `LevelChunkSection` as Leaf runs it: 16^3 block states and 4^3 biomes, with vanilla's counts
//! and Moonrise's block counting (Paper's `0003-Moonrise-optimisation-patches`): the count of
//! "special colliding" blocks and the list of randomly ticking positions, in the order Moonrise
//! builds and edits it (random ticks pick positions from it by index).
//!
//! The per-state facts the counts need (air, random ticks, fluid) come from the block flag table
//! through [`SectionFlags`], so this module does not depend on how that table is built.

use std::io::Cursor;

use super::paletted::{ContainerError, PalettedContainer, Strategy};
use crate::azalea::buf::AzBuf;
use crate::block::BlockState;

/// What a section counts about a state, in one byte (`BlockState.isAir`, `isRandomlyTicking`,
/// `getFluidState().isEmpty()`/`isRandomlyTicking()`, Moonrise's `CollisionUtil.isSpecialCollidingBlock`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateFlags(pub u8);

impl StateFlags {
    pub const AIR: u8 = 1;
    pub const RANDOMLY_TICKING: u8 = 2;
    pub const HAS_FLUID: u8 = 4;
    pub const FLUID_RANDOMLY_TICKING: u8 = 8;
    pub const SPECIAL_COLLIDING: u8 = 16;
    #[inline]
    pub fn has(self, flag: u8) -> bool {
        self.0 & flag != 0
    }
}

/// The block flag table, as far as sections need it.
pub trait SectionFlags {
    fn section_flags(&self, state: BlockState) -> StateFlags;

    /// The fluid registry's name of a fluid id (`fluid_ticks` save it). A table without fluids
    /// (the tests') has only `minecraft:empty`.
    fn fluid_name(&self, _fluid: u8) -> &str {
        "minecraft:empty"
    }

    /// The fluid id of a registry name.
    fn fluid_by_name(&self, _name: &str) -> Option<u8> {
        None
    }
}

/// `PalettedContainerFactory`: the two strategies and the values new sections start with
/// (`Blocks.AIR.defaultBlockState()`, `Biomes.PLAINS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerFactory {
    pub block_states: Strategy,
    pub biomes: Strategy,
    pub default_biome: u32,
}

impl ContainerFactory {
    /// `block_states`: `BlockStates::state_count()`; `biomes`: the biome registry's size.
    pub fn new(block_state_count: u32, biome_count: u32, default_biome: u32) -> Self {
        Self {
            block_states: Strategy::block_states(block_state_count),
            biomes: Strategy::biomes(biome_count),
            default_biome,
        }
    }
    pub fn create_for_block_states(&self) -> PalettedContainer<BlockState> {
        PalettedContainer::new(self.block_states, BlockState::AIR)
    }
    pub fn create_for_biomes(&self) -> PalettedContainer<u32> {
        PalettedContainer::new(self.biomes, self.default_biome)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelChunkSection {
    states: PalettedContainer<BlockState>,
    biomes: PalettedContainer<u32>,
    non_empty_blocks: u16,
    fluids: u16,
    ticking_blocks: u16,
    ticking_fluids: u16,
    special_colliding: u16,
    /// Moonrise's `ShortList` of ticking positions (`x | z << 4 | y << 8`). Moonrise pairs it with a
    /// hash map from position to index; at most 4096 entries, a scan of the `u16`s finds an entry
    /// as fast without a map per section. Removal moves the last entry into the hole, as Moonrise does.
    ticking: Vec<u16>,
}

impl LevelChunkSection {
    /// `new LevelChunkSection(PalettedContainerFactory)`: all air, default biome, zero counts.
    pub fn empty(factory: &ContainerFactory) -> Self {
        Self::from_parts(
            factory.create_for_block_states(),
            factory.create_for_biomes(),
        )
    }
    fn from_parts(states: PalettedContainer<BlockState>, biomes: PalettedContainer<u32>) -> Self {
        Self {
            states,
            biomes,
            non_empty_blocks: 0,
            fluids: 0,
            ticking_blocks: 0,
            ticking_fluids: 0,
            special_colliding: 0,
            ticking: Vec::new(),
        }
    }
    /// `new LevelChunkSection(states, biomes)`: the counts are recalculated.
    pub fn new(
        states: PalettedContainer<BlockState>,
        biomes: PalettedContainer<u32>,
        flags: &impl SectionFlags,
    ) -> Self {
        let mut section = Self::from_parts(states, biomes);
        section.recalc_block_counts(flags);
        section
    }

    pub fn states(&self) -> &PalettedContainer<BlockState> {
        &self.states
    }
    pub fn biomes(&self) -> &PalettedContainer<u32> {
        &self.biomes
    }
    pub fn block_state(&self, x: u32, y: u32, z: u32) -> BlockState {
        self.states.get_xyz(x, y, z)
    }
    pub fn noise_biome(&self, x: u32, y: u32, z: u32) -> u32 {
        self.biomes.get_xyz(x, y, z)
    }
    /// CraftBukkit's `setNoiseBiome`.
    pub fn set_noise_biome(&mut self, x: u32, y: u32, z: u32, biome: u32) {
        self.biomes
            .set(self.biomes.strategy().index(x, y, z), biome);
    }

    /// `setBlockState`: the previous state, with vanilla's counts and Moonrise's callback.
    pub fn set_block_state(
        &mut self,
        x: u32,
        y: u32,
        z: u32,
        state: BlockState,
        flags: &impl SectionFlags,
    ) -> BlockState {
        let previous = self.states.get_and_set_xyz(x, y, z, state);
        let old = flags.section_flags(previous);
        let new = flags.section_flags(state);
        // Counts stay within 0..=4096; Java's short arithmetic never wraps here.
        if !old.has(StateFlags::AIR) {
            self.non_empty_blocks -= 1;
            self.ticking_blocks -= u16::from(old.has(StateFlags::RANDOMLY_TICKING));
            self.fluids -= u16::from(old.has(StateFlags::HAS_FLUID));
            self.ticking_fluids -= u16::from(old.has(StateFlags::FLUID_RANDOMLY_TICKING));
        }
        if !new.has(StateFlags::AIR) {
            self.non_empty_blocks += 1;
            self.ticking_blocks += u16::from(new.has(StateFlags::RANDOMLY_TICKING));
            self.fluids += u16::from(new.has(StateFlags::HAS_FLUID));
            self.ticking_fluids += u16::from(new.has(StateFlags::FLUID_RANDOMLY_TICKING));
        }
        // Moonrise's `updateBlockCallback`.
        if previous != state {
            let (was, is) = (
                old.has(StateFlags::SPECIAL_COLLIDING),
                new.has(StateFlags::SPECIAL_COLLIDING),
            );
            if was != is {
                if was {
                    self.special_colliding -= 1;
                } else {
                    self.special_colliding += 1;
                }
            }
            let (was, is) = (
                old.has(StateFlags::RANDOMLY_TICKING),
                new.has(StateFlags::RANDOMLY_TICKING),
            );
            if was != is {
                let position = (x | z << 4 | y << 8) as u16;
                let found = self.ticking.iter().position(|&p| p == position);
                match (was, found) {
                    (true, Some(i)) => {
                        self.ticking.swap_remove(i);
                    }
                    (false, None) => self.ticking.push(position),
                    _ => {}
                }
            }
        }
        previous
    }

    /// Moonrise's `recalcBlockCounts`: counts per palette entry in first-occurrence order, and the
    /// ticking positions grouped the same way.
    pub fn recalc_block_counts(&mut self, flags: &impl SectionFlags) {
        let mut counts = [0u16; 5];
        self.ticking.clear();
        if !self
            .states
            .maybe_has(|s| !flags.section_flags(s).has(StateFlags::AIR))
        {
            (
                self.non_empty_blocks,
                self.fluids,
                self.ticking_blocks,
                self.ticking_fluids,
                self.special_colliding,
            ) = (0, 0, 0, 0, 0);
            return;
        }
        self.states.count(|state, count| {
            let f = flags.section_flags(state);
            if f.has(StateFlags::AIR) {
                return;
            }
            let count = count as u16;
            counts[0] += count;
            for (i, flag) in [
                StateFlags::HAS_FLUID,
                StateFlags::RANDOMLY_TICKING,
                StateFlags::FLUID_RANDOMLY_TICKING,
                StateFlags::SPECIAL_COLLIDING,
            ]
            .into_iter()
            .enumerate()
            {
                if f.has(flag) {
                    counts[i + 1] += count;
                }
            }
        });
        [
            self.non_empty_blocks,
            self.fluids,
            self.ticking_blocks,
            self.ticking_fluids,
            self.special_colliding,
        ] = counts;
        if self.ticking_blocks > 0 {
            self.ticking.reserve_exact(usize::from(self.ticking_blocks));
            self.states.positions_where(
                |s| {
                    let f = flags.section_flags(s);
                    !f.has(StateFlags::AIR) && f.has(StateFlags::RANDOMLY_TICKING)
                },
                &mut self.ticking,
            );
        }
    }

    /// `hasOnlyAir`.
    pub fn has_only_air(&self) -> bool {
        self.non_empty_blocks == 0
    }
    pub fn non_empty_block_count(&self) -> u16 {
        self.non_empty_blocks
    }
    pub fn fluid_count(&self) -> u16 {
        self.fluids
    }
    pub fn has_fluid(&self) -> bool {
        self.fluids > 0
    }
    pub fn is_randomly_ticking(&self) -> bool {
        self.is_randomly_ticking_blocks() || self.is_randomly_ticking_fluids()
    }
    pub fn is_randomly_ticking_blocks(&self) -> bool {
        self.ticking_blocks > 0
    }
    pub fn is_randomly_ticking_fluids(&self) -> bool {
        self.ticking_fluids > 0
    }
    /// Moonrise's `hasSpecialCollidingBlocks`.
    pub fn has_special_colliding_blocks(&self) -> bool {
        self.special_colliding != 0
    }
    /// Moonrise's ticking block list (`getTickingBlockList`), in its order.
    pub fn ticking_blocks(&self) -> &[u16] {
        &self.ticking
    }
    /// `maybeHas` on the block states.
    pub fn maybe_has(&self, predicate: impl FnMut(BlockState) -> bool) -> bool {
        self.states.maybe_has(predicate)
    }

    /// Moonrise's `fillBiomesFromNoise`: a container seeded with the biome at (0, 0, 0), then
    /// every other position once in y, z, x order.
    pub fn fill_biomes_from_noise(&mut self, mut biome: impl FnMut(u32, u32, u32) -> u32) {
        let mut biomes = PalettedContainer::new(self.biomes.strategy(), biome(0, 0, 0));
        for i in 1..64u32 {
            biomes.set(i as usize, biome(i & 3, i >> 4, (i >> 2) & 3));
        }
        self.biomes = biomes;
    }

    // ---- network form ----

    /// `getSerializedSize`.
    pub fn serialized_size(&self) -> usize {
        4 + self.states.serialized_size() + self.biomes.serialized_size()
    }
    /// `write`: the non-empty block and fluid counts as shorts, then both containers.
    pub fn write(&self, out: &mut Vec<u8>) {
        out.reserve(self.serialized_size());
        out.extend_from_slice(&self.non_empty_blocks.to_be_bytes());
        out.extend_from_slice(&self.fluids.to_be_bytes());
        self.states.write(out);
        self.biomes.write(out);
    }
    /// `read` (the client's side): only the two counts the wire carries are set; Moonrise's
    /// client-only special colliding handling is not ported.
    pub fn read(&mut self, buf: &mut Cursor<&[u8]>) -> Result<(), ContainerError> {
        self.non_empty_blocks = i16::azalea_read(buf)? as u16;
        self.fluids = i16::azalea_read(buf)? as u16;
        self.states.read(buf)?;
        let mut biomes = self.biomes.recreate();
        biomes.read(buf)?;
        self.biomes = biomes;
        Ok(())
    }
}
