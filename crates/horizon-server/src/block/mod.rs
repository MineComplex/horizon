//! Blocks and block states as data: `Block.BLOCK_STATE_REGISTRY` and each block's
//! `StateDefinition` as one flat, read-only table (ARCHITECTURE.md, "Data").
//!
//! A `BlockState` is the state's global id (the id the protocol, chunk palettes and saves use). A
//! state's properties are not stored: vanilla builds a block's states as the cartesian product of
//! its properties sorted by name (`ImmutableSortedMap`), first property slowest, each property's
//! values in `getPossibleValues` order. So a state's value index for a property is
//! `(state - first) / stride % count`, and `setValue` (Moonrise's `ZeroCollidingReferenceStateTable`
//! does the same) is a subtraction and an addition. Per state the table keeps only its block.
//!
//! The table comes from vanilla's data generator report (`report.rs`); it is built once at startup
//! and then only read, so the server shares it as one `Arc<BlockStates>` with the other registries
//! (a single allocation shared by the tick thread and the workers, never mutated).

mod report;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::ops::Range;

use thiserror::Error;

pub use report::default_report_path;

use crate::world::chunk::PaletteValue;

/// A block state's global id (`Block.BLOCK_STATE_REGISTRY.getId(state)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockState(pub u32);

impl BlockState {
    /// `Blocks.AIR.defaultBlockState()`: air is the first block registered, with one state.
    pub const AIR: BlockState = BlockState(0);
    pub fn id(self) -> u32 {
        self.0
    }
}

impl PaletteValue for BlockState {
    fn id(self) -> u32 {
        self.0
    }
    fn from_id(id: u32) -> Self {
        BlockState(id)
    }
}

/// A block's id in the block registry (`BuiltInRegistries.BLOCK.getId(block)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);

/// A distinct property (name and allowed values): `BlockStateProperties.HORIZONTAL_FACING` and
/// `FACING` share the name `facing` but are two properties, as in vanilla.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PropertyId(u16);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BlockDataError {
    #[error("cannot read {path}: {message}")]
    Io { path: String, message: String },
    #[error("malformed block report: {0}")]
    Json(String),
    #[error("duplicate block {0}")]
    DuplicateBlock(String),
    #[error("block {block}: invalid property {property}: {reason}")]
    Property {
        block: String,
        property: String,
        reason: &'static str,
    },
    #[error(
        "block {block}: state {id} does not match its properties' order (expected id {expected})"
    )]
    StateOrder {
        block: String,
        id: u32,
        expected: u32,
    },
    #[error("block {block}: {count} states is not the product of its property sizes ({expected})")]
    StateCount {
        block: String,
        count: usize,
        expected: u32,
    },
    #[error("block {0} has no default state")]
    NoDefault(String),
    #[error("too many blocks or properties for the table")]
    TooLarge,
}

struct Property {
    name: Box<str>,
    values: Box<[Box<str>]>,
}

/// One of a block's properties: which property, and the distance in ids between two of its values.
#[derive(Clone, Copy)]
struct Slot {
    property: PropertyId,
    stride: u32,
}

struct Block {
    name: Box<str>,
    first_state: u32,
    state_count: u32,
    default_state: u32,
    slots: Range<u32>,
    air: bool,
}

/// Every block and block state (see the module docs).
#[derive(Default)]
pub struct BlockStates {
    properties: Vec<Property>,
    blocks: Vec<Block>,
    /// Every block's slots, in the block's property order (by name).
    slots: Vec<Slot>,
    /// Per state, its block (the only per-state data: 2 bytes).
    state_block: Vec<u16>,
    by_name: HashMap<Box<str>, BlockId>,
}

/// The blocks `BlockBehaviour.Properties.air()` marks in `Blocks` (`isAir()`).
const AIR_BLOCKS: [&str; 3] = ["minecraft:air", "minecraft:void_air", "minecraft:cave_air"];

impl BlockStates {
    /// The number of block states (the global palette's size).
    pub fn state_count(&self) -> u32 {
        self.state_block.len() as u32
    }
    pub fn block_count(&self) -> u32 {
        self.blocks.len() as u32
    }
    /// Whether `state` is an id of this table.
    pub fn contains(&self, state: BlockState) -> bool {
        (state.0 as usize) < self.state_block.len()
    }
    fn entry(&self, block: BlockId) -> &Block {
        &self.blocks[block.0 as usize]
    }
    fn slots_of(&self, block: BlockId) -> &[Slot] {
        let r = &self.entry(block).slots;
        &self.slots[r.start as usize..r.end as usize]
    }

    /// `state.getBlock()`. Panics on an id outside the table.
    pub fn block(&self, state: BlockState) -> BlockId {
        BlockId(u32::from(self.state_block[state.0 as usize]))
    }
    /// The block's registry name (`minecraft:oak_stairs`).
    pub fn block_name(&self, block: BlockId) -> &str {
        &self.entry(block).name
    }
    /// The block with this full name (`minecraft:stone`; no default namespace).
    pub fn block_by_name(&self, name: &str) -> Option<BlockId> {
        self.by_name.get(name).copied()
    }
    /// `block.defaultBlockState()`.
    pub fn default_state(&self, block: BlockId) -> BlockState {
        BlockState(self.entry(block).default_state)
    }
    /// `getStateDefinition().getPossibleStates()`, as the contiguous id range they occupy.
    pub fn states(&self, block: BlockId) -> impl ExactSizeIterator<Item = BlockState> + use<> {
        let b = self.entry(block);
        (b.first_state..b.first_state + b.state_count).map(BlockState)
    }
    /// `isAir()`: the blocks built with `Properties.air()`.
    pub fn is_air(&self, state: BlockState) -> bool {
        self.entry(self.block(state)).air
    }

    /// The block's properties in vanilla's order (sorted by name).
    pub fn properties(&self, block: BlockId) -> impl ExactSizeIterator<Item = PropertyId> + '_ {
        self.slots_of(block).iter().map(|s| s.property)
    }
    /// The block's property with this name (`getStateDefinition().getProperty(name)`).
    pub fn property(&self, block: BlockId, name: &str) -> Option<PropertyId> {
        self.properties(block)
            .find(|&p| self.property_name(p) == name)
    }
    pub fn property_name(&self, property: PropertyId) -> &str {
        &self.properties[property.0 as usize].name
    }
    /// The property's values as their serialized names, in `getPossibleValues` order.
    pub fn property_values(&self, property: PropertyId) -> &[Box<str>] {
        &self.properties[property.0 as usize].values
    }

    fn slot(&self, state: BlockState, property: PropertyId) -> Option<(u32, Slot)> {
        let block = self.entry(self.block(state));
        let slot = *self
            .slots_of(self.block(state))
            .iter()
            .find(|s| s.property == property)?;
        Some((state.0 - block.first_state, slot))
    }
    fn count(&self, slot: Slot) -> u32 {
        self.property_values(slot.property).len() as u32
    }

    /// The index of the state's value for `property` in its values, or `None` when its block does
    /// not have it (`getNullableValue`).
    pub fn value_index(&self, state: BlockState, property: PropertyId) -> Option<usize> {
        let (offset, slot) = self.slot(state, property)?;
        Some((offset / slot.stride % self.count(slot)) as usize)
    }
    /// The state's value for the property named `name`, serialized (`"north"`, `"true"`, `"3"`).
    pub fn value(&self, state: BlockState, name: &str) -> Option<&str> {
        let property = self.property(self.block(state), name)?;
        let index = self.value_index(state, property)?;
        Some(&self.property_values(property)[index])
    }
    /// Every property of the state with its value, in vanilla's order.
    pub fn values(&self, state: BlockState) -> impl Iterator<Item = (&str, &str)> + '_ {
        let offset = state.0 - self.entry(self.block(state)).first_state;
        self.slots_of(self.block(state)).iter().map(move |&slot| {
            let p = &self.properties[slot.property.0 as usize];
            (
                &*p.name,
                &*p.values[(offset / slot.stride % self.count(slot)) as usize],
            )
        })
    }

    /// `setValue` by value index: `None` where vanilla throws (the block lacks the property, or the
    /// index is not one of its values).
    pub fn with_value_index(
        &self,
        state: BlockState,
        property: PropertyId,
        index: usize,
    ) -> Option<BlockState> {
        let (offset, slot) = self.slot(state, property)?;
        let count = self.count(slot);
        let index = u32::try_from(index).ok().filter(|&i| i < count)?;
        let current = offset / slot.stride % count;
        Some(BlockState(
            state.0 - current * slot.stride + index * slot.stride,
        ))
    }
    /// `setValue` with a property and value named as in commands and saves (`facing`, `east`).
    pub fn with_property(&self, state: BlockState, name: &str, value: &str) -> Option<BlockState> {
        let property = self.property(self.block(state), name)?;
        let index = self
            .property_values(property)
            .iter()
            .position(|v| &**v == value)?;
        self.with_value_index(state, property, index)
    }
    /// `trySetValue`: the state unchanged when its block does not have the property.
    pub fn try_with_value_index(
        &self,
        state: BlockState,
        property: PropertyId,
        index: usize,
    ) -> BlockState {
        match self.slot(state, property) {
            Some(_) => self
                .with_value_index(state, property, index)
                .unwrap_or(state),
            None => state,
        }
    }
    /// `cycle`: the next value, wrapping to the first (`findNextInCollection`).
    pub fn cycle(&self, state: BlockState, property: PropertyId) -> Option<BlockState> {
        let next = self.value_index(state, property)? + 1;
        let count = self.property_values(property).len();
        self.with_value_index(state, property, if next == count { 0 } else { next })
    }
    /// The block's state with these property values, the others taken from its default state (as
    /// the state codec reads a partial `Properties` compound). `None` on an unknown property or value.
    pub fn state_with<'a>(
        &self,
        block: BlockId,
        values: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Option<BlockState> {
        values
            .into_iter()
            .try_fold(self.default_state(block), |state, (name, value)| {
                self.with_property(state, name, value)
            })
    }
}

/// A property's name and values, which identify it.
type PropertyKey = (Box<str>, Box<[Box<str>]>);

/// Builds a [`BlockStates`] block by block, in registry order. The report loader uses it; tests
/// build small tables with it.
#[derive(Default)]
pub struct BlockStatesBuilder {
    table: BlockStates,
    property_ids: HashMap<PropertyKey, PropertyId>,
}

impl BlockStatesBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the next block with `properties` (name, values in `getPossibleValues` order; any
    /// order of properties, sorted by name as `StateDefinition` does). Its states take the next
    /// ids; its default is its first state until [`set_default`](Self::set_default).
    pub fn add_block(
        &mut self,
        name: &str,
        properties: &[(&str, &[&str])],
    ) -> Result<BlockId, BlockDataError> {
        let table = &mut self.table;
        if table.by_name.contains_key(name) {
            return Err(BlockDataError::DuplicateBlock(name.into()));
        }
        let id = u16::try_from(table.blocks.len()).map_err(|_| BlockDataError::TooLarge)?;
        let mut sorted: Vec<(&str, &[&str])> = properties.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let invalid = |property: &str, reason| BlockDataError::Property {
            block: name.into(),
            property: property.into(),
            reason,
        };
        let mut ids = Vec::with_capacity(sorted.len());
        for (i, &(property, values)) in sorted.iter().enumerate() {
            if i > 0 && sorted[i - 1].0 == property {
                return Err(invalid(property, "listed twice"));
            }
            if values.is_empty() {
                return Err(invalid(property, "no values"));
            }
            if values
                .iter()
                .enumerate()
                .any(|(j, v)| values[..j].contains(v))
            {
                return Err(invalid(property, "a value is listed twice"));
            }
            let key: PropertyKey = (property.into(), values.iter().map(|&v| v.into()).collect());
            let next = PropertyId(
                u16::try_from(table.properties.len()).map_err(|_| BlockDataError::TooLarge)?,
            );
            let pid = *self.property_ids.entry(key.clone()).or_insert_with(|| {
                table.properties.push(Property {
                    name: key.0,
                    values: key.1,
                });
                next
            });
            ids.push((pid, values.len() as u32));
        }
        // Strides: the last property varies fastest (Guava's `Lists.cartesianProduct`).
        let slots_start = table.slots.len() as u32;
        let mut stride = 1u32;
        let mut slots = Vec::with_capacity(ids.len());
        for &(property, count) in ids.iter().rev() {
            slots.push(Slot { property, stride });
            stride = stride.checked_mul(count).ok_or(BlockDataError::TooLarge)?;
        }
        slots.reverse();
        table.slots.extend(slots);
        let first_state = table.state_block.len() as u32;
        first_state
            .checked_add(stride)
            .ok_or(BlockDataError::TooLarge)?;
        table
            .state_block
            .extend(std::iter::repeat_n(id, stride as usize));
        table.blocks.push(Block {
            name: name.into(),
            first_state,
            state_count: stride,
            default_state: first_state,
            slots: slots_start..table.slots.len() as u32,
            air: AIR_BLOCKS.contains(&name),
        });
        table.by_name.insert(name.into(), BlockId(u32::from(id)));
        Ok(BlockId(u32::from(id)))
    }

    /// `registerDefaultState`: `state` must be one of the block's states.
    pub fn set_default(&mut self, block: BlockId, state: BlockState) -> Result<(), BlockDataError> {
        let entry = &mut self.table.blocks[block.0 as usize];
        if !(entry.first_state..entry.first_state + entry.state_count).contains(&state.0) {
            return Err(BlockDataError::NoDefault(entry.name.to_string()));
        }
        entry.default_state = state.0;
        Ok(())
    }

    /// The table built so far (read-only, e.g. to check states against a report while loading).
    pub fn table(&self) -> &BlockStates {
        &self.table
    }

    pub fn finish(self) -> BlockStates {
        self.table
    }
}
