//! `Level.setBlock` and what follows a change (26.3 with Paper's and Leaf's changes): the chunk's
//! part (`LevelChunk.setBlockState`: sections, heightmaps, `onPlace`), the clients told
//! (`sendBlockUpdated`, broadcast by the chunk holder), the neighbours told
//! (`updateNeighborsAt`) and their shapes updated (`updateNeighbourShapes`), all through one
//! `CollectingNeighborUpdater` per level ([`NeighborUpdater`]: updates queued while one runs are
//! run depth first, at most `max-chained-neighbor-updates` in a chain), plus `destroyBlock` and
//! `Block.updateOrDestroy`.
//!
//! The level is a [`BlockLevel`] (the server's `Level`, or a map in tests); what blocks do is a
//! `match` over the state's facts ([`BlockRules`]), with no object per block:
//! - `onPlace`, `neighborChanged`, `updateShape`: the liquids' (`world::fluid`); `updateShape` also
//!   schedules the water tick of a waterlogged state (every `SimpleWaterloggedBlock` does first)
//!   and turns vegetation (`VegetationBlock`: a `TagBelow` survival rule) that can no longer
//!   survive into air. Every other block keeps its state and does nothing: their behaviour is not
//!   ported yet (stairs and fences keep their shapes, redstone does not react, torches stay).
//! - Not ported: block entities, `affectNeighborsAfterRemoval` (no block in scope has one),
//!   comparators (`updateNeighbourForOutputSignal`), redstone wire's indirect shape updates, light,
//!   points of interest, game events, drops (no item entities: `dropResources` makes no loot
//!   draws), Paper's capture of block states (no plugins). A chunk that is not loaded is not
//!   loaded for a read (it reads as air) or a write (nothing happens): vanilla would load it
//!   synchronously, which never happens from the scheduled ticks (their chunks' neighbours are
//!   loaded).

use std::convert::Infallible;

use tracing::error;

use super::block_rules::{BlockRules, LIQUID_BLOCK, VEGETATION, WATERLOGGED};
use super::fluid::{self, FluidSettings};
use crate::block::survival::SurvivalWorld;
use crate::block::{BlockId, BlockState};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::LegacyRandom;

pub const UPDATE_NEIGHBORS: i32 = 1;
pub const UPDATE_CLIENTS: i32 = 2;
pub const UPDATE_KNOWN_SHAPE: i32 = 16;
pub const UPDATE_SUPPRESS_DROPS: i32 = 32;
pub const UPDATE_MOVE_BY_PISTON: i32 = 64;
pub const UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE: i32 = 128;
pub const UPDATE_SKIP_ON_PLACE: i32 = 512;
pub const UPDATE_ALL: i32 = UPDATE_NEIGHBORS | UPDATE_CLIENTS;
/// `Block.UPDATE_LIMIT`: how deep shape updates chain.
pub const UPDATE_LIMIT: i32 = 512;
/// `max-chained-neighbor-updates`' default.
pub const MAX_CHAINED_NEIGHBOR_UPDATES: i32 = 1_000_000;

/// `LevelEvent.PARTICLES_AND_SOUND_DESTROY_BLOCK`.
pub const EVENT_DESTROY_BLOCK: i32 = 2001;
/// `LevelEvent.LAVA_FIZZ`.
pub const EVENT_LAVA_FIZZ: i32 = 1501;

/// `NeighborUpdater.UPDATE_ORDER`.
const UPDATE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
];
/// `BlockBehaviour.UPDATE_SHAPE_ORDER`.
const UPDATE_SHAPE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::North,
    Direction::South,
    Direction::Down,
    Direction::Up,
];

pub fn relative(pos: BlockPos, direction: Direction) -> BlockPos {
    let [x, y, z] = direction.normal();
    pos.offset(x, y, z)
}

/// What the block-change path needs from a level.
pub trait BlockLevel {
    fn rules(&self) -> &BlockRules;
    /// The lowest block y, and the build height.
    fn min_y(&self) -> i32;
    fn height(&self) -> i32;
    /// The state at an in-height position of a loaded full chunk (`None`: not loaded).
    fn loaded_state(&self, pos: BlockPos) -> Option<BlockState>;
    /// The chunk's half of `LevelChunk.setBlockState` (sections, heightmaps, unsaved): the old
    /// state, or `None` when nothing changed or the chunk is not loaded.
    fn write_state(&mut self, pos: BlockPos, state: BlockState) -> Option<BlockState>;
    /// The chunk at `pos` is at least `FULL` (changes there are sent).
    fn sends_updates(&self, pos: BlockPos) -> bool;
    /// `ServerChunkCache.blockChanged`: queued for the players.
    fn block_changed(&mut self, pos: BlockPos);
    /// `levelEvent(null, event, pos, data)`.
    fn level_event(&mut self, event: i32, pos: BlockPos, data: i32);
    fn schedule_block_tick(&mut self, pos: BlockPos, block: BlockId, delay: i32);
    fn schedule_fluid_tick(&mut self, pos: BlockPos, fluid: u8, delay: i32);
    /// The level's random (`Level.random`).
    fn random(&mut self) -> &mut LegacyRandom;
    fn updater(&mut self) -> &mut NeighborUpdater;
    fn fluid_settings(&self) -> FluidSettings;
    /// The neighbour updater's debug listener (`forEachUpdatedPos`): tests compare it.
    fn trace_updated(&mut self, _pos: BlockPos) {}
    /// A `setBlock` that changed something, once it returns.
    fn trace_set(&mut self, _pos: BlockPos, _state: BlockState, _flags: i32) {}
}

fn in_height<L: BlockLevel + ?Sized>(level: &L, y: i32) -> bool {
    y >= level.min_y() && y < level.min_y() + level.height()
}

/// `isInValidBounds`.
fn in_valid_bounds<L: BlockLevel + ?Sized>(level: &L, pos: BlockPos) -> bool {
    in_height(level, pos.y)
        && (-30_000_000..30_000_000).contains(&pos.x)
        && (-30_000_000..30_000_000).contains(&pos.z)
}

/// `getBlockState`: void air outside the valid bounds, air in a chunk not loaded.
pub fn block_state<L: BlockLevel + ?Sized>(level: &L, pos: BlockPos) -> BlockState {
    if !in_valid_bounds(level, pos) {
        return level.rules().named.void_air;
    }
    level.loaded_state(pos).unwrap_or(BlockState::AIR)
}

/// `getBlockStateIfLoaded`: void air outside the build height, `None` in a chunk not loaded.
pub fn state_if_loaded<L: BlockLevel + ?Sized>(level: &L, pos: BlockPos) -> Option<BlockState> {
    if !in_height(level, pos.y) {
        return Some(level.rules().named.void_air);
    }
    level.loaded_state(pos)
}

fn is_air<L: BlockLevel + ?Sized>(level: &L, state: BlockState) -> bool {
    level.rules().data.is_air(state)
}

/// `setBlock(pos, state, flags, updateLimit)`: true when the block changed.
pub fn set_block<L: BlockLevel + ?Sized>(
    level: &mut L,
    pos: BlockPos,
    state: BlockState,
    flags: i32,
    limit: i32,
) -> bool {
    if !in_valid_bounds(level, pos) {
        return false;
    }
    let Some(old) = level.write_state(pos, state) else {
        return false;
    };
    // The rest of `LevelChunk.setBlockState`: no block in scope has block entities or
    // `affectNeighborsAfterRemoval`, so the block is still there for `onPlace`.
    if flags & UPDATE_SKIP_ON_PLACE == 0 {
        on_place(level, state, pos, old);
    }
    let new = block_state(level, pos);
    if new == state {
        if flags & UPDATE_CLIENTS != 0 && level.sends_updates(pos) {
            level.block_changed(pos);
        }
        if flags & UPDATE_NEIGHBORS != 0 {
            let block = level.rules().blocks.block(old);
            update_neighbors_at(level, pos, block);
        }
        if flags & UPDATE_KNOWN_SHAPE == 0 && limit > 0 {
            let neighbour_flags = flags & !(UPDATE_SUPPRESS_DROPS | UPDATE_NEIGHBORS);
            update_neighbour_shapes(level, state, pos, neighbour_flags, limit - 1);
        }
    }
    level.trace_set(pos, state, flags);
    true
}

/// `setBlockAndUpdate`.
pub fn set_block_and_update<L: BlockLevel + ?Sized>(
    level: &mut L,
    pos: BlockPos,
    state: BlockState,
) {
    set_block(level, pos, state, UPDATE_ALL, UPDATE_LIMIT);
}

/// `destroyBlock(pos, drop, null, limit)`: the destroy particles and sound, then the block
/// replaced by its fluid (drops are not ported).
pub fn destroy_block<L: BlockLevel + ?Sized>(level: &mut L, pos: BlockPos, limit: i32) -> bool {
    let state = block_state(level, pos);
    if is_air(level, state) {
        return false;
    }
    level.level_event(EVENT_DESTROY_BLOCK, pos, state.0 as i32);
    let rules = level.rules();
    let replacement = rules.legacy_block(rules.fluid(block_state(level, pos)));
    set_block(level, pos, replacement, UPDATE_ALL, limit)
}

/// `Block.updateOrDestroy`.
fn update_or_destroy<L: BlockLevel + ?Sized>(
    level: &mut L,
    state: BlockState,
    new: BlockState,
    pos: BlockPos,
    flags: i32,
    limit: i32,
) {
    if new == state {
        return;
    }
    if is_air(level, new) {
        destroy_block(level, pos, limit);
    } else {
        set_block(level, pos, new, flags & !UPDATE_SUPPRESS_DROPS, limit);
    }
}

/// `updateNeighborsAt(pos, block)`: each neighbour in `UPDATE_ORDER` gets `neighborChanged`.
pub fn update_neighbors_at<L: BlockLevel + ?Sized>(level: &mut L, pos: BlockPos, block: BlockId) {
    add_and_run(level, pos, Update::multi(pos, block, None));
}

/// `BlockState.updateNeighbourShapes`: each neighbour in `UPDATE_SHAPE_ORDER` updates its shape
/// against `state`.
fn update_neighbour_shapes<L: BlockLevel + ?Sized>(
    level: &mut L,
    state: BlockState,
    pos: BlockPos,
    flags: i32,
    limit: i32,
) {
    for direction in UPDATE_SHAPE_ORDER {
        let neighbour = relative(pos, direction);
        let update = Update::Shape {
            direction: direction.opposite(),
            neighbour_state: state,
            pos: neighbour,
            neighbour_pos: pos,
            flags,
            limit,
        };
        add_and_run(level, neighbour, update);
    }
}

/// One queued update of `CollectingNeighborUpdater` (its four record types).
#[derive(Clone, Copy, Debug)]
enum Update {
    Shape {
        direction: Direction,
        neighbour_state: BlockState,
        pos: BlockPos,
        neighbour_pos: BlockPos,
        flags: i32,
        limit: i32,
    },
    /// `MultiNeighborUpdate`: the neighbours of `source` in `UPDATE_ORDER` but `skip`, `next`
    /// being the index of the next one.
    Multi {
        source: BlockPos,
        block: BlockId,
        skip: Option<Direction>,
        next: u8,
    },
}

impl Update {
    fn multi(source: BlockPos, block: BlockId, skip: Option<Direction>) -> Self {
        let next = u8::from(Some(UPDATE_ORDER[0]) == skip);
        Self::Multi {
            source,
            block,
            skip,
            next,
        }
    }
}

/// What one `runNext` does.
enum Action {
    Shape {
        direction: Direction,
        neighbour_state: BlockState,
        pos: BlockPos,
        neighbour_pos: BlockPos,
        flags: i32,
        limit: i32,
    },
    Neighbour {
        pos: BlockPos,
        block: BlockId,
    },
}

/// `CollectingNeighborUpdater`'s state: the stack of running updates, those added by the one
/// running, and how many were added in this chain.
#[derive(Debug)]
pub struct NeighborUpdater {
    stack: Vec<Update>,
    added: Vec<Update>,
    count: i32,
    max: i32,
}

impl NeighborUpdater {
    /// `max_chained` is `max-chained-neighbor-updates` (negative: no limit).
    pub fn new(max_chained: i32) -> Self {
        Self {
            stack: Vec::new(),
            added: Vec::new(),
            count: 0,
            max: max_chained,
        }
    }

    pub fn set_max_chained(&mut self, max_chained: i32) {
        self.max = max_chained;
    }

    /// `runNext` of the update at `index`: what to run, and whether it has more.
    fn next(&mut self, index: usize) -> (Action, bool) {
        match &mut self.stack[index] {
            &mut Update::Shape {
                direction,
                neighbour_state,
                pos,
                neighbour_pos,
                flags,
                limit,
            } => (
                Action::Shape {
                    direction,
                    neighbour_state,
                    pos,
                    neighbour_pos,
                    flags,
                    limit,
                },
                false,
            ),
            Update::Multi {
                source,
                block,
                skip,
                next,
            } => {
                let direction = UPDATE_ORDER[*next as usize];
                *next += 1;
                if (*next as usize) < UPDATE_ORDER.len()
                    && Some(UPDATE_ORDER[*next as usize]) == *skip
                {
                    *next += 1;
                }
                let action = Action::Neighbour {
                    pos: relative(*source, direction),
                    block: *block,
                };
                (action, (*next as usize) < UPDATE_ORDER.len())
            }
        }
    }
}

/// `addAndRun`.
fn add_and_run<L: BlockLevel + ?Sized>(level: &mut L, pos: BlockPos, update: Update) {
    let u = level.updater();
    let running = u.count > 0;
    let too_many = u.max >= 0 && u.count >= u.max;
    u.count += 1;
    if !too_many {
        if running {
            u.added.push(update);
        } else {
            u.stack.push(update);
        }
    } else if u.count - 1 == u.max {
        error!(
            "Too many chained neighbor updates. Skipping the rest. First skipped position: {}, {}, {}",
            pos.x, pos.y, pos.z
        );
    }
    if !running {
        run_updates(level);
    }
}

/// `runUpdates`: the added updates go on the stack (the first added on top), the top one runs
/// until it adds more or is done.
fn run_updates<L: BlockLevel + ?Sized>(level: &mut L) {
    loop {
        let u = level.updater();
        if u.stack.is_empty() && u.added.is_empty() {
            break;
        }
        while let Some(update) = u.added.pop() {
            u.stack.push(update);
        }
        let top = u.stack.len() - 1;
        let traced = u.stack[top];
        trace_update(level, traced);
        loop {
            let (action, more) = level.updater().next(top);
            run_action(level, action);
            let u = level.updater();
            if !more {
                u.stack.truncate(top);
                break;
            }
            if !u.added.is_empty() {
                break;
            }
        }
    }
    let u = level.updater();
    u.stack.clear();
    u.added.clear();
    u.count = 0;
}

/// `forEachUpdatedPos` to the debug listener.
fn trace_update<L: BlockLevel + ?Sized>(level: &mut L, update: Update) {
    match update {
        Update::Shape { pos, .. } => level.trace_updated(pos),
        Update::Multi { source, skip, .. } => {
            for direction in UPDATE_ORDER {
                if Some(direction) != skip {
                    level.trace_updated(relative(source, direction));
                }
            }
        }
    }
}

fn run_action<L: BlockLevel + ?Sized>(level: &mut L, action: Action) {
    match action {
        Action::Shape {
            direction,
            neighbour_state,
            pos,
            neighbour_pos,
            flags,
            limit,
        } => {
            // `executeShapeUpdate`.
            let current = block_state(level, pos);
            let wire = level.rules().named.redstone_wire;
            if flags & UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE == 0
                || Some(level.rules().blocks.block(current)) != wire
            {
                let new = update_shape(
                    level,
                    current,
                    pos,
                    direction,
                    neighbour_pos,
                    neighbour_state,
                );
                update_or_destroy(level, current, new, pos, flags, limit);
            }
        }
        Action::Neighbour { pos, block } => {
            // `executeUpdate`: no `BlockPhysicsEvent` (no plugins).
            let state = block_state(level, pos);
            neighbor_changed(level, state, pos, block);
        }
    }
}

/// `BlockState.onPlace`.
fn on_place<L: BlockLevel + ?Sized>(
    level: &mut L,
    state: BlockState,
    pos: BlockPos,
    _old: BlockState,
) {
    if level.rules().has(state, LIQUID_BLOCK) {
        fluid::liquid_on_place(level, state, pos);
    }
}

/// `BlockState.handleNeighborChanged`.
fn neighbor_changed<L: BlockLevel + ?Sized>(
    level: &mut L,
    state: BlockState,
    pos: BlockPos,
    _block: BlockId,
) {
    if level.rules().has(state, LIQUID_BLOCK) {
        fluid::liquid_neighbor_changed(level, state, pos);
    }
}

/// `BlockState.updateShape(level, ticks, pos, direction, neighbourPos, neighbourState, random)`.
fn update_shape<L: BlockLevel + ?Sized>(
    level: &mut L,
    state: BlockState,
    pos: BlockPos,
    direction: Direction,
    _neighbour_pos: BlockPos,
    neighbour_state: BlockState,
) -> BlockState {
    let info = level.rules().info(state);
    if info & LIQUID_BLOCK != 0 {
        return fluid::liquid_update_shape(level, state, pos, direction, neighbour_state);
    }
    if info & WATERLOGGED != 0 {
        let water = level.rules().water;
        let delay = fluid::tick_delay(level, water);
        level.schedule_fluid_tick(pos, water, delay);
    }
    if info & VEGETATION != 0 && !survives(level, state, pos) {
        return BlockState::AIR;
    }
    state
}

/// `canSurvive` through the survival rules (a rule that cannot answer keeps the block).
fn survives<L: BlockLevel + ?Sized>(level: &L, state: BlockState, pos: BlockPos) -> bool {
    struct View<'a, L: ?Sized>(&'a L);
    impl<L: BlockLevel + ?Sized> SurvivalWorld for View<'_, L> {
        type Error = Infallible;
        fn block_state(&self, x: i32, y: i32, z: i32) -> Result<BlockState, Infallible> {
            Ok(block_state(self.0, BlockPos::new(x, y, z)))
        }
        /// Light is not ported; the vegetation rules do not read it.
        fn raw_brightness(&self, _: i32, _: i32, _: i32) -> Result<i32, Infallible> {
            Ok(0)
        }
    }
    let rules = level.rules();
    let Some(survival) = &rules.survival else {
        return true;
    };
    survival
        .can_survive(
            &rules.blocks,
            &rules.data,
            &View(level),
            state,
            [pos.x, pos.y, pos.z],
        )
        .unwrap_or(true)
}
