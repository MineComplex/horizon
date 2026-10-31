//! Each block state's cached data: what vanilla's `BlockStateBase` constructor and `initCache()`
//! store per state (flags, light, push reaction, fluid, destroy speed, the collision and occlusion
//! shapes of its `Cache`), plus the flags Paper's Moonrise and Leaf derive from them.
//!
//! The values come from the server owner's own jar: `LeafBlockStatesExporter.java` (under
//! `mods/horizon/src/test/java/dev/horizonlab/leafserver/`), run by `tools/leaf_server_reports.py`,
//! writes `<server>/cache/vanilla-26.3/block_states.bin`
//! (ARCHITECTURE.md, "Data"). The server reads it once at startup next to `blocks.json` and keeps
//! it as parallel flat arrays indexed by state id; shapes are de-duplicated into one list (529
//! for 35,723 states in 26.3) and states hold an index into it.
//!
//! # `block_states.bin`, version 2 (little-endian)
//!
//! Version 2 added the survival rules: a name list after the instruments and, in each record,
//! a rule index (the old padding). Version 1 files are rejected (rerun `tools/leaf_server_reports.py`).
//!
//! ```text
//! "HZBS"  u32 version  u32 state_count
//! names: fluids (BuiltInRegistries.FLUID order), PushReaction values, NoteBlockInstrument values,
//!        survival rules (`survival.rs`: `Fire`, `TagBelow:<tag>`...);
//!        each list is u16 count, then per name u8 length + UTF-8
//! u32 shape_count, then each shape:
//!   u8 kind: 0 Shapes.empty(), 1 Shapes.block(), 2 a grid:
//!   u8 cube (CubeVoxelShape); per axis X, Y, Z: u8 cube axis (CubePointRange), u16 n, then for a
//!   cube axis nothing (n parts, coordinates i/n) else n f64 coordinates (n - 1 cells);
//!   occupancy bits, cell (x, y, z) at bit x*sy*sz + y*sz + z, LSB first, ceil(volume/8) bytes
//! state_count records of 28 bytes:
//!   u32 flags (FLAG_* below)  u32 face sturdy bits (bit direction*3 + SupportType)
//!   u8 light emission  u8 light dampening  u8 push reaction  u8 map colour id  u8 instrument
//!   u8 fluid: type id (bits 0-2) | amount << 3 (bits 3-6) | falling (bit 7)
//!   u16 survival rule: 0 none, else 1 + the index in the rule names
//!   f32 destroy speed  u32 collision shape (u32::MAX: dynamic shape, not cached)  u32 occlusion shape
//! ```
//!
//! Equal grids share one shape, so two states whose vanilla shapes are distinct but equal objects
//! get one (identity-equal) `VoxelShape`; only `Shapes.empty()` and `Shapes.block()` identities
//! matter to vanilla's join shortcuts, and those are kept.

use std::path::{Path, PathBuf};

use super::{BlockDataError, BlockState, BlockStates};
use crate::core::geometry::{Direction, VoxelShape};

pub const VERSION: u32 = 2;
const RECORD: usize = 28;
const NO_SHAPE: u32 = u32::MAX;

pub const FLAG_AIR: u32 = 1;
pub const FLAG_IGNITED_BY_LAVA: u32 = 1 << 1;
/// `liquid()` (deprecated in vanilla, still read by fluids and placement).
pub const FLAG_LIQUID: u32 = 1 << 2;
/// `isSolid()`: the deprecated `legacySolid`.
pub const FLAG_LEGACY_SOLID: u32 = 1 << 3;
pub const FLAG_REQUIRES_CORRECT_TOOL: u32 = 1 << 4;
pub const FLAG_CAN_OCCLUDE: u32 = 1 << 5;
pub const FLAG_SPAWN_TERRAIN_PARTICLES: u32 = 1 << 6;
/// `canBeReplaced()`: `Properties.replaceable()`.
pub const FLAG_REPLACEABLE: u32 = 1 << 7;
pub const FLAG_RANDOMLY_TICKING: u32 = 1 << 8;
pub const FLAG_SOLID_RENDER: u32 = 1 << 9;
pub const FLAG_PROPAGATES_SKYLIGHT_DOWN: u32 = 1 << 10;
pub const FLAG_USE_SHAPE_FOR_LIGHT_OCCLUSION: u32 = 1 << 11;
/// `Cache.isCollisionShapeFullBlock`.
pub const FLAG_COLLISION_FULL_BLOCK: u32 = 1 << 12;
/// `Cache.largeCollisionShape`: the shape leaves the unit cube.
pub const FLAG_LARGE_COLLISION_SHAPE: u32 = 1 << 13;
/// `Block.hasDynamicShape()`: no `Cache`, shapes are asked of the block each time.
pub const FLAG_DYNAMIC_SHAPE: u32 = 1 << 14;
pub const FLAG_HAS_OFFSET: u32 = 1 << 15;
pub const FLAG_SIGNAL_SOURCE: u32 = 1 << 16;
pub const FLAG_ANALOG_OUTPUT: u32 = 1 << 17;
/// Leaf's `optimize-collision-shape`: the cached collision shape holds for every context (not
/// lava, scaffolding or powder snow); Moonrise's `constantCollisionShape`.
pub const FLAG_CONSTANT_COLLISION: u32 = 1 << 18;
/// Moonrise's `isSpecialCollidingBlock`: a large collision shape, or a moving piston.
pub const FLAG_SPECIAL_COLLIDING: u32 = 1 << 19;
/// Moonrise's `occludesFullBlock`: the collision shape covers the whole unit cube.
pub const FLAG_OCCLUDES_FULL_BLOCK: u32 = 1 << 20;
/// `FireBlock.getIgniteOdds(state) > 0`: burnable and not waterlogged (`canSurvive` of fire).
pub const FLAG_FLAMMABLE: u32 = 1 << 21;
/// The block is a `FenceGateBlock` (`PathBlock.canSurvive`).
pub const FLAG_FENCE_GATE: u32 = 1 << 22;
/// The block is a `DoublePlantBlock` (the simple block feature places both halves).
pub const FLAG_DOUBLE_PLANT: u32 = 1 << 23;
/// The block is a `MossyCarpetBlock` (the simple block feature places it with its own rules).
pub const FLAG_MOSSY_CARPET: u32 = 1 << 24;

/// `PushReaction` (26.3's names), in vanilla's order (checked against the file's names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushReaction {
    PushPull,
    Push,
    Popped,
    Immoveable,
    IgnoreEntity,
}
const PUSH_REACTIONS: [(&str, PushReaction); 5] = [
    ("PUSH_PULL", PushReaction::PushPull),
    ("PUSH", PushReaction::Push),
    ("POPPED", PushReaction::Popped),
    ("IMMOVEABLE", PushReaction::Immoveable),
    ("IGNORE_ENTITY", PushReaction::IgnoreEntity),
];

/// `SupportType`, in vanilla's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportType {
    Full,
    Center,
    Rigid,
}

/// A state's `getFluidState()`: its fluid's registry id (0 is `minecraft:empty`), `getAmount()`
/// and the `falling` property of flowing fluids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateFluid {
    pub fluid: u8,
    pub amount: u8,
    pub falling: bool,
}

/// Every state's cached data (see the module docs). Read-only after loading; shared like
/// [`BlockStates`].
#[derive(Default)]
pub struct BlockStateData {
    flags: Vec<u32>,
    sturdy: Vec<u32>,
    light_emission: Vec<u8>,
    light_dampening: Vec<u8>,
    push_reaction: Vec<PushReaction>,
    map_color: Vec<u8>,
    instrument: Vec<u8>,
    fluid: Vec<u8>,
    destroy_speed: Vec<f32>,
    collision: Vec<u32>,
    occlusion: Vec<u32>,
    shapes: Vec<VoxelShape>,
    fluid_names: Vec<Box<str>>,
    instrument_names: Vec<Box<str>>,
    survival_names: Vec<Box<str>>,
    survival: Vec<u16>,
    /// Bit `i` set: fluid `i` is `minecraft:empty` (`FluidState.isEmpty`).
    empty_fluids: u8,
    /// Bit `i` set: fluid `i` ticks randomly (`Fluid.isRandomlyTicking`: only `LavaFluid` does).
    random_ticking_fluids: u8,
}

/// Where the prepare tool writes the file, relative to the server directory.
pub fn default_state_data_path(server_dir: &Path) -> PathBuf {
    server_dir.join("cache/vanilla-26.3/block_states.bin")
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BlockDataError> {
        if self.bytes.len() < n {
            return Err(format_error("unexpected end of file"));
        }
        let (head, tail) = self.bytes.split_at(n);
        self.bytes = tail;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8, BlockDataError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, BlockDataError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, BlockDataError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, BlockDataError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn names(&mut self) -> Result<Vec<Box<str>>, BlockDataError> {
        (0..self.u16()?)
            .map(|_| {
                let len = self.u8()? as usize;
                std::str::from_utf8(self.take(len)?)
                    .map(Into::into)
                    .map_err(|_| format_error("a name is not UTF-8"))
            })
            .collect()
    }

    fn shape(&mut self) -> Result<VoxelShape, BlockDataError> {
        match self.u8()? {
            0 => return Ok(VoxelShape::empty()),
            1 => return Ok(VoxelShape::block()),
            2 => {}
            _ => return Err(format_error("unknown shape kind")),
        }
        let cube = self.u8()? != 0;
        let mut coords: [Vec<f64>; 3] = Default::default();
        let mut cube_axes = [false; 3];
        for axis in 0..3 {
            cube_axes[axis] = self.u8()? != 0;
            let n = self.u16()? as usize;
            coords[axis] = if cube_axes[axis] {
                (0..=n).map(|i| i as f64 / n as f64).collect()
            } else {
                (0..n).map(|_| self.f64()).collect::<Result<_, _>>()?
            };
        }
        let sizes = coords.clone().map(|c| c.len().saturating_sub(1));
        let volume = sizes[0] * sizes[1] * sizes[2];
        let bits = self.take(volume.div_ceil(8))?;
        let occupied: Vec<bool> = (0..volume)
            .map(|i| bits[i / 8] >> (i % 8) & 1 != 0)
            .collect();
        let shape = if cube && cube_axes == [true; 3] {
            VoxelShape::from_cube(sizes, &occupied)
        } else {
            VoxelShape::from_mixed_grid(coords, &occupied, cube_axes)
        };
        shape.map_err(|e| format_error(&e.to_string()))
    }
}

fn format_error(message: &str) -> BlockDataError {
    BlockDataError::StateData(message.into())
}

impl BlockStateData {
    /// Reads `block_states.bin` (see [`default_state_data_path`]) for the states of `states`.
    pub fn load(path: &Path, states: &BlockStates) -> Result<Self, BlockDataError> {
        let bytes = std::fs::read(path).map_err(|e| BlockDataError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::from_bytes(&bytes, states.state_count())
    }

    /// Decodes the file; its state count must be `state_count` (`blocks.json`'s).
    pub fn from_bytes(bytes: &[u8], state_count: u32) -> Result<Self, BlockDataError> {
        let mut r = Reader { bytes };
        if r.take(4)? != b"HZBS" {
            return Err(format_error("not a block_states.bin file"));
        }
        let version = r.u32()?;
        if version != VERSION {
            return Err(format_error(&format!(
                "version {version}, this server reads version {VERSION}: rerun tools/leaf_server_reports.py"
            )));
        }
        let count = r.u32()?;
        if count != state_count {
            return Err(BlockDataError::StateDataCount {
                file: count,
                report: state_count,
            });
        }
        let fluid_names = r.names()?;
        let push_names = r.names()?;
        if push_names.len() != PUSH_REACTIONS.len()
            || push_names
                .iter()
                .zip(PUSH_REACTIONS)
                .any(|(a, (b, _))| **a != *b)
        {
            return Err(format_error("unexpected PushReaction values"));
        }
        let instrument_names = r.names()?;
        let survival_names = r.names()?;
        let shapes = (0..r.u32()?)
            .map(|_| r.shape())
            .collect::<Result<Vec<_>, _>>()?;
        let n = count as usize;
        let records = r.take(n * RECORD)?;
        if !r.bytes.is_empty() {
            return Err(format_error("trailing bytes"));
        }
        let fluid_mask = |pred: fn(&str) -> bool| {
            fluid_names
                .iter()
                .enumerate()
                .filter(|(i, name)| *i < 8 && pred(name))
                .fold(0u8, |mask, (i, _)| mask | 1 << i)
        };
        let empty_fluids = fluid_mask(|name| name == "minecraft:empty");
        let random_ticking_fluids =
            fluid_mask(|name| name == "minecraft:lava" || name == "minecraft:flowing_lava");
        let mut data = BlockStateData {
            shapes,
            fluid_names,
            instrument_names,
            survival_names,
            empty_fluids,
            random_ticking_fluids,
            ..Default::default()
        };
        for rec in records.as_chunks::<RECORD>().0 {
            let word = |i: usize| u32::from_le_bytes(rec[i..i + 4].try_into().unwrap());
            let shape = |i: usize| {
                let index = word(i);
                if index == NO_SHAPE || (index as usize) < data.shapes.len() {
                    Ok(index)
                } else {
                    Err(format_error("a shape index is out of range"))
                }
            };
            let collision = shape(20)?;
            let occlusion = shape(24)?;
            if occlusion == NO_SHAPE
                || (collision == NO_SHAPE) != (word(0) & FLAG_DYNAMIC_SHAPE != 0)
            {
                return Err(format_error("a state lacks a shape"));
            }
            let push = PUSH_REACTIONS
                .get(rec[10] as usize)
                .ok_or_else(|| format_error("unknown push reaction"))?
                .1;
            if rec[12] as usize >= data.instrument_names.len().max(1)
                || (rec[13] & 7) as usize >= data.fluid_names.len().max(1)
            {
                return Err(format_error("an instrument or fluid is out of range"));
            }
            data.flags.push(word(0));
            data.sturdy.push(word(4));
            data.light_emission.push(rec[8]);
            data.light_dampening.push(rec[9]);
            data.push_reaction.push(push);
            data.map_color.push(rec[11]);
            data.instrument.push(rec[12]);
            data.fluid.push(rec[13]);
            let rule = u16::from_le_bytes([rec[14], rec[15]]);
            if rule as usize > data.survival_names.len() {
                return Err(format_error("a survival rule is out of range"));
            }
            data.survival.push(rule);
            data.destroy_speed
                .push(f32::from_le_bytes(rec[16..20].try_into().unwrap()));
            data.collision.push(collision);
            data.occlusion.push(occlusion);
        }
        Ok(data)
    }

    fn at<T: Copy>(column: &[T], state: BlockState) -> T {
        column[state.0 as usize]
    }

    /// Every `FLAG_*` bit of the state.
    pub fn flags(&self, state: BlockState) -> u32 {
        Self::at(&self.flags, state)
    }
    /// Whether the state has every bit of `flag`.
    pub fn has(&self, state: BlockState, flag: u32) -> bool {
        self.flags(state) & flag == flag
    }
    pub fn is_air(&self, state: BlockState) -> bool {
        self.has(state, FLAG_AIR)
    }
    pub fn is_randomly_ticking(&self, state: BlockState) -> bool {
        self.has(state, FLAG_RANDOMLY_TICKING)
    }
    pub fn is_solid_render(&self, state: BlockState) -> bool {
        self.has(state, FLAG_SOLID_RENDER)
    }
    pub fn propagates_skylight_down(&self, state: BlockState) -> bool {
        self.has(state, FLAG_PROPAGATES_SKYLIGHT_DOWN)
    }
    pub fn is_replaceable(&self, state: BlockState) -> bool {
        self.has(state, FLAG_REPLACEABLE)
    }
    pub fn requires_correct_tool_for_drops(&self, state: BlockState) -> bool {
        self.has(state, FLAG_REQUIRES_CORRECT_TOOL)
    }
    /// Moonrise/Starlight's `isConditionallyFullOpaque`: `canOcclude && useShapeForLightOcclusion`.
    pub fn is_conditionally_full_opaque(&self, state: BlockState) -> bool {
        self.has(state, FLAG_CAN_OCCLUDE | FLAG_USE_SHAPE_FOR_LIGHT_OCCLUSION)
    }
    /// `getLightEmission()`, 0..=15.
    pub fn light_emission(&self, state: BlockState) -> u8 {
        Self::at(&self.light_emission, state)
    }
    /// `getLightDampening()` (the light block / opacity), 0..=15.
    pub fn light_dampening(&self, state: BlockState) -> u8 {
        Self::at(&self.light_dampening, state)
    }
    pub fn push_reaction(&self, state: BlockState) -> PushReaction {
        Self::at(&self.push_reaction, state)
    }
    /// `getMapColor(..).id`.
    pub fn map_color(&self, state: BlockState) -> u8 {
        Self::at(&self.map_color, state)
    }
    /// `instrument()`, by its serialized name (`harp`, `basedrum`...).
    pub fn instrument(&self, state: BlockState) -> &str {
        &self.instrument_names[Self::at(&self.instrument, state) as usize]
    }
    /// `getDestroySpeed`: the block's hardness, -1 for unbreakable blocks.
    pub fn destroy_speed(&self, state: BlockState) -> f32 {
        Self::at(&self.destroy_speed, state)
    }
    pub fn fluid(&self, state: BlockState) -> StateFluid {
        let byte = Self::at(&self.fluid, state);
        StateFluid {
            fluid: byte & 7,
            amount: byte >> 3 & 15,
            falling: byte & 0x80 != 0,
        }
    }
    /// The survival rule names the file lists (`survival.rs` resolves them).
    pub fn survival_rule_names(&self) -> &[Box<str>] {
        &self.survival_names
    }
    /// The state's rule: an index into [`Self::survival_rule_names`].
    pub fn survival_rule(&self, state: BlockState) -> Option<usize> {
        (Self::at(&self.survival, state) as usize).checked_sub(1)
    }
    /// The fluid's registry name (`minecraft:water`, `minecraft:flowing_lava`, `minecraft:empty`).
    pub fn fluid_name(&self, fluid: u8) -> &str {
        &self.fluid_names[fluid as usize]
    }
    /// `isFaceSturdy` over an empty level (the `Cache`'s table); false for dynamic shapes.
    pub fn is_face_sturdy(&self, state: BlockState, face: Direction, support: SupportType) -> bool {
        Self::at(&self.sturdy, state) >> (face as u32 * 3 + support as u32) & 1 != 0
    }
    /// The `Cache`'s collision shape (`CollisionContext.empty()`), or `None` for blocks with a
    /// dynamic shape, which have no cache.
    pub fn collision_shape(&self, state: BlockState) -> Option<&VoxelShape> {
        let index = Self::at(&self.collision, state);
        (index != NO_SHAPE).then(|| &self.shapes[index as usize])
    }
    /// `getOcclusionShape()` (empty when the block cannot occlude).
    pub fn occlusion_shape(&self, state: BlockState) -> &VoxelShape {
        &self.shapes[Self::at(&self.occlusion, state) as usize]
    }
    /// The de-duplicated shapes.
    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }
}

/// A summary: the arrays hold an entry per block state.
impl std::fmt::Debug for BlockStateData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockStateData")
            .field("shapes", &self.shape_count())
            .finish_non_exhaustive()
    }
}

/// What chunk sections count (`LevelChunkSection.recalcBlockCounts`, Moonrise's special-colliding
/// count): air, random ticking, a fluid, a randomly ticking fluid (lava), special colliding.
impl crate::world::chunk::SectionFlags for BlockStateData {
    fn fluid_name(&self, fluid: u8) -> &str {
        BlockStateData::fluid_name(self, fluid)
    }
    fn fluid_by_name(&self, name: &str) -> Option<u8> {
        self.fluid_names
            .iter()
            .position(|n| &**n == name)
            .map(|i| i as u8)
    }
    #[inline]
    fn section_flags(&self, state: BlockState) -> crate::world::chunk::StateFlags {
        use crate::world::chunk::StateFlags;
        let flags = self.flags(state);
        let fluid = 1u8 << (Self::at(&self.fluid, state) & 7);
        let mut out = 0;
        if flags & FLAG_AIR != 0 {
            out |= StateFlags::AIR;
        }
        if flags & FLAG_RANDOMLY_TICKING != 0 {
            out |= StateFlags::RANDOMLY_TICKING;
        }
        if self.empty_fluids & fluid == 0 {
            out |= StateFlags::HAS_FLUID;
        }
        if self.random_ticking_fluids & fluid != 0 {
            out |= StateFlags::FLUID_RANDOMLY_TICKING;
        }
        if flags & FLAG_SPECIAL_COLLIDING != 0 {
            out |= StateFlags::SPECIAL_COLLIDING;
        }
        StateFlags(out)
    }
}
