//! What the level's block-change path and its fluids read about blocks, resolved once from the
//! state tables and the data packs' tags (read-only, shared by every level like the tables).
//!
//! Per state, a byte of facts ([`BlockRules::info`] bits) answers the class questions vanilla asks
//! with `instanceof` (`LiquidBlock`, `LiquidBlockContainer.canPlaceLiquid`) and the tags
//! (`washed_away_by_fluids`); the water and lava blocks' states by `level` give
//! `FluidState.createLegacyBlock`. Fluid ids are the registry's (`block_states.bin` lists them).

use std::sync::Arc;

use crate::block::flags::{BlockStateData, FLAG_LIQUID_CONTAINER, StateFluid};
use crate::block::survival::{BlockSurvival, IdSet};
use crate::block::{BlockId, BlockState, BlockStates};
use crate::world::chunk::SectionFlags;

/// A water or lava `LiquidBlock` (`state.getBlock() instanceof LiquidBlock`).
pub const LIQUID_BLOCK: u8 = 1;
/// `canHoldAnyFluid`: a `LiquidBlockContainer`, or in `#washed_away_by_fluids`.
pub const HOLDS_ANY_FLUID: u8 = 1 << 1;
/// A `LiquidBlockContainer` (its `placeLiquid`, not a replacement, takes the fluid).
pub const CONTAINER: u8 = 1 << 2;
/// `canPlaceLiquid(null, .., minecraft:water)`: a `SimpleWaterloggedBlock` other than the barrier
/// (no creative player) and the double slab; every other fluid is refused.
pub const TAKES_WATER: u8 = 1 << 3;
/// The state has `waterlogged=true` (its `updateShape` schedules a water tick).
pub const WATERLOGGED: u8 = 1 << 4;
/// `VegetationBlock.updateShape`: air where it can no longer survive (a `TagBelow` survival rule).
pub const VEGETATION: u8 = 1 << 5;

/// Water or lava, the two `FlowingFluid`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FluidKind {
    Water,
    Lava,
}

/// The blocks the fluid and update code names.
#[derive(Clone, Copy, Debug)]
pub struct NamedBlocks {
    pub water: Option<BlockId>,
    pub lava: Option<BlockId>,
    pub redstone_wire: Option<BlockId>,
    pub soul_soil: Option<BlockId>,
    pub blue_ice: Option<BlockId>,
    pub void_air: BlockState,
    pub stone: Option<BlockState>,
    pub cobblestone: Option<BlockState>,
    pub obsidian: Option<BlockState>,
    pub basalt: Option<BlockState>,
}

/// Resolved block and fluid facts (see the module docs).
pub struct BlockRules {
    pub blocks: Arc<BlockStates>,
    pub data: Arc<BlockStateData>,
    info: Vec<u8>,
    pub named: NamedBlocks,
    /// `water[level]`, `lava[level]`: the `LiquidBlock` states by their `level` (0..=15).
    water_states: Option<[BlockState; 16]>,
    lava_states: Option<[BlockState; 16]>,
    /// Registry ids: `flowing_water`, `water`, `flowing_lava`, `lava` (`u8::MAX` when absent).
    pub flowing_water: u8,
    pub water: u8,
    pub flowing_lava: u8,
    pub lava: u8,
    /// Fluid tag masks (bit = fluid id): `#water`, `#lava`, `#bubble_column_can_occupy`.
    water_tag: u8,
    lava_tag: u8,
    bubble_tag: u8,
    /// `#enables_bubble_column_drag_down` and `#enables_bubble_column_push_up`.
    bubble_blocks: IdSet,
    /// The blocks' `canSurvive` rules (vegetation's `updateShape`); `None` when the tables lack
    /// what they need (synthetic test tables).
    pub survival: Option<BlockSurvival>,
}

fn tag_ids<'a>(tags: &'a [(String, Vec<i32>)], name: &str) -> &'a [i32] {
    tags.iter()
        .find(|(t, _)| t == name)
        .map_or(&[], |(_, ids)| ids.as_slice())
}

fn fluid_mask(tags: &[(String, Vec<i32>)], name: &str) -> u8 {
    tag_ids(tags, name)
        .iter()
        .filter(|&&i| (0..8).contains(&i))
        .fold(0, |m, &i| m | 1 << i)
}

impl BlockRules {
    /// From the tables and the tags (tag name to registry ids, as `BlockSurvival::new` takes
    /// them; fluids by `block_states.bin`'s fluid ids).
    pub fn new(
        blocks: Arc<BlockStates>,
        data: Arc<BlockStateData>,
        block_tags: &[(String, Vec<i32>)],
        fluid_tags: &[(String, Vec<i32>)],
    ) -> Self {
        let id = |name: &str| blocks.block_by_name(name);
        let state = |name: &str| id(name).map(|b| blocks.default_state(b));
        let named = NamedBlocks {
            water: id("minecraft:water"),
            lava: id("minecraft:lava"),
            redstone_wire: id("minecraft:redstone_wire"),
            soul_soil: id("minecraft:soul_soil"),
            blue_ice: id("minecraft:blue_ice"),
            void_air: state("minecraft:void_air").unwrap_or(BlockState::AIR),
            stone: state("minecraft:stone"),
            cobblestone: state("minecraft:cobblestone"),
            obsidian: state("minecraft:obsidian"),
            basalt: state("minecraft:basalt"),
        };
        let levels = |block: Option<BlockId>| {
            let block = block?;
            let base = blocks.default_state(block);
            let mut out = [base; 16];
            for (level, slot) in out.iter_mut().enumerate() {
                *slot = blocks.with_property(base, "level", &level.to_string())?;
            }
            Some(out)
        };
        let fluid = |name: &str| data.fluid_by_name(name).unwrap_or(u8::MAX);
        let washed: IdSet = IdSet::new(
            tag_ids(block_tags, "minecraft:washed_away_by_fluids")
                .iter()
                .map(|&i| i as u32)
                .collect(),
        );
        let barrier = id("minecraft:barrier");
        let survival = BlockSurvival::new(&blocks, &data, block_tags, fluid_tags).ok();
        let rule_names = data.survival_rule_names();
        let mut info = Vec::with_capacity(blocks.state_count() as usize);
        for s in 0..blocks.state_count() {
            let s = BlockState(s);
            let block = blocks.block(s);
            let mut bits = 0;
            if Some(block) == named.water || Some(block) == named.lava {
                bits |= LIQUID_BLOCK;
            }
            let container = data.has(s, FLAG_LIQUID_CONTAINER);
            if container {
                bits |= CONTAINER;
            }
            if container || washed.contains(block.0) {
                bits |= HOLDS_ANY_FLUID;
            }
            let waterlogged = blocks.value(s, "waterlogged");
            if container
                && waterlogged.is_some()
                && Some(block) != barrier
                && blocks.value(s, "type") != Some("double")
            {
                bits |= TAKES_WATER;
            }
            if waterlogged == Some("true") {
                bits |= WATERLOGGED;
            }
            if data
                .survival_rule(s)
                .is_some_and(|r| rule_names[r].starts_with("TagBelow"))
            {
                bits |= VEGETATION;
            }
            info.push(bits);
        }
        let mut bubble: Vec<u32> = ["drag_down", "push_up"]
            .iter()
            .flat_map(|t| tag_ids(block_tags, &format!("minecraft:enables_bubble_column_{t}")))
            .map(|&i| i as u32)
            .collect();
        bubble.sort_unstable();
        bubble.dedup();
        Self {
            water_states: levels(named.water),
            lava_states: levels(named.lava),
            named,
            info,
            flowing_water: fluid("minecraft:flowing_water"),
            water: fluid("minecraft:water"),
            flowing_lava: fluid("minecraft:flowing_lava"),
            lava: fluid("minecraft:lava"),
            water_tag: fluid_mask(fluid_tags, "minecraft:water"),
            lava_tag: fluid_mask(fluid_tags, "minecraft:lava"),
            bubble_tag: fluid_mask(fluid_tags, "minecraft:bubble_column_can_occupy"),
            bubble_blocks: IdSet::new(bubble),
            survival,
            blocks,
            data,
        }
    }

    /// The state's [`LIQUID_BLOCK`].. bits.
    #[inline]
    pub fn info(&self, state: BlockState) -> u8 {
        self.info.get(state.0 as usize).copied().unwrap_or(0)
    }

    pub fn has(&self, state: BlockState, bit: u8) -> bool {
        self.info(state) & bit != 0
    }

    /// `getFluidState()`.
    #[inline]
    pub fn fluid(&self, state: BlockState) -> StateFluid {
        self.data.fluid(state)
    }

    /// Which `FlowingFluid` a fluid id is (`None`: empty).
    pub fn kind(&self, fluid: u8) -> Option<FluidKind> {
        if fluid == self.water || fluid == self.flowing_water {
            Some(FluidKind::Water)
        } else if fluid == self.lava || fluid == self.flowing_lava {
            Some(FluidKind::Lava)
        } else {
            None
        }
    }

    pub fn is_source(&self, fluid: u8) -> bool {
        fluid == self.water || fluid == self.lava
    }

    /// `fluid.is(FluidTags.WATER)`.
    pub fn in_water_tag(&self, fluid: u8) -> bool {
        fluid < 8 && self.water_tag >> fluid & 1 != 0
    }

    /// `fluid.is(FluidTags.LAVA)`.
    pub fn in_lava_tag(&self, fluid: u8) -> bool {
        fluid < 8 && self.lava_tag >> fluid & 1 != 0
    }

    /// `fluidState.is(FluidTags.BUBBLE_COLUMN_CAN_OCCUPY)`.
    pub fn bubble_column_can_occupy(&self, fluid: u8) -> bool {
        fluid < 8 && self.bubble_tag >> fluid & 1 != 0
    }

    /// In `#enables_bubble_column_drag_down` or `#enables_bubble_column_push_up`.
    pub fn enables_bubble_column(&self, state: BlockState) -> bool {
        self.bubble_blocks.contains(self.blocks.block(state).0)
    }

    /// The source and flowing ids of a kind.
    pub fn ids(&self, kind: FluidKind) -> (u8, u8) {
        match kind {
            FluidKind::Water => (self.water, self.flowing_water),
            FluidKind::Lava => (self.lava, self.flowing_lava),
        }
    }

    /// `createLegacyBlock`: the `LiquidBlock` state of `level` (air without the block).
    pub fn liquid_state(&self, kind: FluidKind, level: u8) -> BlockState {
        let states = match kind {
            FluidKind::Water => &self.water_states,
            FluidKind::Lava => &self.lava_states,
        };
        states.map_or(BlockState::AIR, |s| s[level as usize & 15])
    }

    /// `FluidState.createLegacyBlock` (air for the empty fluid): `level` 0 for a source, else
    /// `8 - min(amount, 8)`, plus 8 when falling.
    pub fn legacy_block(&self, fluid: StateFluid) -> BlockState {
        let Some(kind) = self.kind(fluid.fluid) else {
            return BlockState::AIR;
        };
        let level = if self.is_source(fluid.fluid) {
            0
        } else {
            8 - fluid.amount.min(8) + if fluid.falling { 8 } else { 0 }
        };
        self.liquid_state(kind, level)
    }
}
