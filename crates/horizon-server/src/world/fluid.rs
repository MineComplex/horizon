//! Water and lava (`FlowingFluid`, `WaterFluid`, `LavaFluid`) and the `LiquidBlock`s holding them,
//! with Paper's, Purpur's and Leaf's changes: what a fluid tick does (the new level from the
//! neighbours, falling, sources made of two neighbouring sources, spreading down then to the
//! sides towards the nearest hole), lava meeting water (stone, cobblestone, obsidian; basalt over
//! soul soil next to blue ice), liquid containers taking water (`waterlogged`), and the liquid
//! blocks' `onPlace` / `neighborChanged` / `updateShape`, which schedule the fluid ticks.
//!
//! Fluids are `StateFluid`s of the state table (registry id, amount, falling); a fluid state's
//! identity in Java is equality of the three here. Paper reads neighbours with
//! `getBlockStateIfLoaded` (skipped when not loaded); Leaf's slope search is a breadth-first search
//! (the same nearest-hole distance as vanilla's recursion); Paper's `water-over-lava-flow-speed`
//! applies to water placed next to lava; Purpur's per-world `blocks.*` settings and the game rules
//! are [`FluidSettings`] (their defaults: neither is read yet).
//!
//! Not ported: drops of blocks water washes away (no item entities), `BlockFromToEvent` and
//! `FluidLevelChangeEvent` (no plugins), `getFlow` (entities), random ticks (lava setting fire),
//! bubble columns (the water block's tick is scheduled, its behaviour is not ported).

use std::collections::{HashMap, HashSet, VecDeque};

use super::block_rules::{
    BlockRules, CONTAINER, FluidKind, HOLDS_ANY_FLUID, LIQUID_BLOCK, TAKES_WATER,
};
use super::block_update::{
    BlockLevel, EVENT_LAVA_FIZZ, UPDATE_ALL, UPDATE_LIMIT, block_state, relative, set_block,
    set_block_and_update, state_if_loaded,
};
use crate::block::BlockState;
use crate::block::flags::{FLAG_LEGACY_SOLID, FLAG_OCCLUDES_FULL_BLOCK, StateFluid};
use crate::core::geometry::{Axis, Direction, ShapeOp, VoxelShape};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;

/// `Direction.Plane.HORIZONTAL`.
const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];
/// `LiquidBlock.POSSIBLE_FLOW_DIRECTIONS`.
const POSSIBLE_FLOW_DIRECTIONS: [Direction; 5] = [
    Direction::Down,
    Direction::South,
    Direction::North,
    Direction::East,
    Direction::West,
];
/// `LavaFluid.MIN_LEVEL_CUTOFF`.
const LAVA_MIN_LEVEL_CUTOFF: f32 = 0.444_444_45;

/// The level's fluid settings: the dimension's `gameplay/fast_lava`, the game rules
/// `water_source_conversion` / `lava_source_conversion`, Paper's
/// `environment.water-over-lava-flow-speed`, Purpur's `blocks.water.infinite-required-sources`,
/// `blocks.lava.{infinite-required-sources,speed.nether,speed.not-nether}` and `tick-fluids`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidSettings {
    pub fast_lava: bool,
    pub water_source_conversion: bool,
    pub lava_source_conversion: bool,
    pub water_over_lava_flow_speed: i32,
    pub water_required_sources: i32,
    pub lava_required_sources: i32,
    pub lava_speed_fast: i32,
    pub lava_speed_slow: i32,
    pub tick_fluids: bool,
}

impl FluidSettings {
    /// The defaults, in a dimension with or without fast lava.
    pub fn new(fast_lava: bool) -> Self {
        Self {
            fast_lava,
            water_source_conversion: true,
            lava_source_conversion: false,
            water_over_lava_flow_speed: 5,
            water_required_sources: 2,
            lava_required_sources: 2,
            lava_speed_fast: 10,
            lava_speed_slow: 30,
            tick_fluids: true,
        }
    }
}

fn empty() -> StateFluid {
    StateFluid {
        fluid: 0,
        amount: 0,
        falling: false,
    }
}

fn is_empty(rules: &BlockRules, f: StateFluid) -> bool {
    rules.kind(f.fluid).is_none()
}

fn same_kind(rules: &BlockRules, f: StateFluid, kind: FluidKind) -> bool {
    rules.kind(f.fluid) == Some(kind)
}

fn is_source_of(rules: &BlockRules, f: StateFluid, kind: FluidKind) -> bool {
    same_kind(rules, f, kind) && rules.is_source(f.fluid)
}

/// `getSource(false)`.
fn source(rules: &BlockRules, kind: FluidKind) -> StateFluid {
    StateFluid {
        fluid: rules.ids(kind).0,
        amount: 8,
        falling: false,
    }
}

/// `getFlowing(amount, falling)`.
fn flowing(rules: &BlockRules, kind: FluidKind, amount: i32, falling: bool) -> StateFluid {
    StateFluid {
        fluid: rules.ids(kind).1,
        amount: amount as u8,
        falling,
    }
}

/// `Fluid.getTickDelay` of a fluid id.
pub fn tick_delay<L: BlockLevel + ?Sized>(level: &L, fluid: u8) -> i32 {
    match level.rules().kind(fluid) {
        Some(kind) => kind_tick_delay(level, kind),
        None => 0,
    }
}

fn kind_tick_delay<L: BlockLevel + ?Sized>(level: &L, kind: FluidKind) -> i32 {
    let s = level.fluid_settings();
    match kind {
        FluidKind::Water => 5,
        FluidKind::Lava if s.fast_lava => s.lava_speed_fast,
        FluidKind::Lava => s.lava_speed_slow,
    }
}

fn drop_off<L: BlockLevel + ?Sized>(level: &L, kind: FluidKind) -> i32 {
    match kind {
        FluidKind::Lava if !level.fluid_settings().fast_lava => 2,
        _ => 1,
    }
}

fn slope_find_distance<L: BlockLevel + ?Sized>(level: &L, kind: FluidKind) -> i32 {
    match kind {
        FluidKind::Lava if !level.fluid_settings().fast_lava => 2,
        _ => 4,
    }
}

fn required_sources<L: BlockLevel + ?Sized>(level: &L, kind: FluidKind) -> i32 {
    let s = level.fluid_settings();
    match kind {
        FluidKind::Water => s.water_required_sources,
        FluidKind::Lava => s.lava_required_sources,
    }
}

fn can_convert_to_source<L: BlockLevel + ?Sized>(level: &L, kind: FluidKind) -> bool {
    let s = level.fluid_settings();
    match kind {
        FluidKind::Water => s.water_source_conversion,
        FluidKind::Lava => s.lava_source_conversion,
    }
}

/// `getFluidState`.
fn fluid_at<L: BlockLevel + ?Sized>(level: &L, pos: BlockPos) -> StateFluid {
    level.rules().fluid(block_state(level, pos))
}

/// `FluidState.getHeight(level, pos)`: full under the same fluid, else `amount / 9`.
fn height<L: BlockLevel + ?Sized>(level: &L, f: StateFluid, pos: BlockPos) -> f32 {
    let rules = level.rules();
    let above = fluid_at(level, pos.offset(0, 1, 0));
    if rules.kind(f.fluid).is_some() && rules.kind(f.fluid) == rules.kind(above.fluid) {
        1.0
    } else {
        f32::from(f.amount) / 9.0
    }
}

/// The collision shape for `CollisionContext.empty()` (the dynamic ones, scaffolding and powder
/// snow, have none there).
fn collision(rules: &BlockRules, state: BlockState) -> VoxelShape {
    rules
        .data
        .collision_shape(state)
        .cloned()
        .unwrap_or_else(VoxelShape::empty)
}

/// `Shapes.mergedFaceOccludes`: the two faces between the blocks together cover the whole face.
fn merged_face_occludes(shape: &VoxelShape, occluder: &VoxelShape, direction: Direction) -> bool {
    if shape.is_block() || occluder.is_block() {
        return true;
    }
    let (axis, positive_face, negative_face) = match direction {
        Direction::Down | Direction::Up => (Axis::Y, Direction::Up, Direction::Down),
        Direction::North | Direction::South => (Axis::Z, Direction::South, Direction::North),
        Direction::West | Direction::East => (Axis::X, Direction::East, Direction::West),
    };
    let positive = matches!(
        direction,
        Direction::Up | Direction::South | Direction::East
    );
    let (first, second) = if positive {
        (shape, occluder)
    } else {
        (occluder, shape)
    };
    let face = |s: &VoxelShape, touches: bool, side: Direction| {
        if touches {
            s.face_shape(side).unwrap_or_else(|_| VoxelShape::empty())
        } else {
            VoxelShape::empty()
        }
    };
    let first = face(first, (first.max(axis) - 1.0).abs() <= 1e-7, positive_face);
    let second = face(second, second.min(axis).abs() <= 1e-7, negative_face);
    first
        .join_unoptimized(&second, ShapeOp::OR)
        .and_then(|joined| VoxelShape::block().join_is_not_empty(&joined, ShapeOp::ONLY_FIRST))
        .is_ok_and(|uncovered| !uncovered)
}

/// `canPassThroughWall` (Paper's version, with Moonrise's shortcuts).
fn can_pass_through_wall(
    rules: &BlockRules,
    direction: Direction,
    from: BlockState,
    to: BlockState,
) -> bool {
    let (a, b) = (collision(rules, from), collision(rules, to));
    if a.is_empty() && b.is_empty() {
        return true;
    }
    if rules.data.has(from, FLAG_OCCLUDES_FULL_BLOCK)
        || rules.data.has(to, FLAG_OCCLUDES_FULL_BLOCK)
    {
        return false;
    }
    !merged_face_occludes(&a, &b, direction)
}

/// `canHoldSpecificFluid`: a container takes only what `canPlaceLiquid` allows (water sources).
fn can_hold_specific(rules: &BlockRules, state: BlockState, fluid: u8) -> bool {
    let info = rules.info(state);
    info & CONTAINER == 0 || (info & TAKES_WATER != 0 && fluid == rules.water)
}

/// `canHoldFluid`.
fn can_hold_fluid(rules: &BlockRules, state: BlockState, fluid: u8) -> bool {
    rules.has(state, HOLDS_ANY_FLUID) && can_hold_specific(rules, state, fluid)
}

/// `canMaybePassThrough`.
fn can_maybe_pass_through(
    rules: &BlockRules,
    kind: FluidKind,
    from: BlockState,
    direction: Direction,
    to: BlockState,
    to_fluid: StateFluid,
) -> bool {
    !is_source_of(rules, to_fluid, kind)
        && rules.has(to, HOLDS_ANY_FLUID)
        && can_pass_through_wall(rules, direction, from, to)
}

/// `canPassThrough` (for this kind's flowing fluid).
fn can_pass_through(
    rules: &BlockRules,
    kind: FluidKind,
    from: BlockState,
    direction: Direction,
    to: BlockState,
    to_fluid: StateFluid,
) -> bool {
    can_maybe_pass_through(rules, kind, from, direction, to, to_fluid)
        && can_hold_specific(rules, to, rules.ids(kind).1)
}

/// `isWaterHole`.
fn is_water_hole(rules: &BlockRules, kind: FluidKind, top: BlockState, bottom: BlockState) -> bool {
    can_pass_through_wall(rules, Direction::Down, top, bottom)
        && (same_kind(rules, rules.fluid(bottom), kind)
            || can_hold_fluid(rules, bottom, rules.ids(kind).1))
}

/// `FluidState.canBeReplacedWith(level, pos, fluid, direction)` of the fluid at `pos`.
fn can_be_replaced_with<L: BlockLevel + ?Sized>(
    level: &L,
    current: StateFluid,
    pos: BlockPos,
    fluid: u8,
    direction: Direction,
) -> bool {
    let rules = level.rules();
    match rules.kind(current.fluid) {
        None => true,
        Some(FluidKind::Water) => direction == Direction::Down && !rules.in_water_tag(fluid),
        Some(FluidKind::Lava) => {
            height(level, current, pos) >= LAVA_MIN_LEVEL_CUTOFF && rules.in_water_tag(fluid)
        }
    }
}

/// `getNewLiquid`: what the fluid at `pos` becomes from its neighbours.
fn new_liquid<L: BlockLevel + ?Sized>(
    level: &L,
    kind: FluidKind,
    pos: BlockPos,
    state: BlockState,
) -> StateFluid {
    let rules = level.rules();
    let mut highest = 0;
    let mut sources = 0;
    for direction in HORIZONTAL {
        let neighbour = relative(pos, direction);
        let Some(n) = state_if_loaded(level, neighbour) else {
            continue;
        };
        let f = rules.fluid(n);
        if same_kind(rules, f, kind) && can_pass_through_wall(rules, direction, state, n) {
            if rules.is_source(f.fluid) {
                sources += 1;
            }
            highest = highest.max(i32::from(f.amount));
        }
    }
    if sources >= required_sources(level, kind) && can_convert_to_source(level, kind) {
        let below = block_state(level, pos.offset(0, -1, 0));
        if rules.data.has(below, FLAG_LEGACY_SOLID) || is_source_of(rules, rules.fluid(below), kind)
        {
            return source(rules, kind);
        }
    }
    let above = block_state(level, pos.offset(0, 1, 0));
    let above_fluid = rules.fluid(above);
    if same_kind(rules, above_fluid, kind)
        && can_pass_through_wall(rules, Direction::Up, state, above)
    {
        return flowing(rules, kind, 8, true);
    }
    let amount = highest - drop_off(level, kind);
    if amount <= 0 {
        empty()
    } else {
        flowing(rules, kind, amount, false)
    }
}

/// `FlowingFluid.SpreadContext`: the states and holes of one `getSpread`, by horizontal offset.
struct SpreadContext {
    origin: BlockPos,
    states: HashMap<i16, BlockState>,
    holes: HashMap<i16, bool>,
}

impl SpreadContext {
    fn new(origin: BlockPos) -> Self {
        Self {
            origin,
            states: HashMap::new(),
            holes: HashMap::new(),
        }
    }

    fn key(&self, pos: BlockPos) -> i16 {
        let x = (pos.x - self.origin.x + 128) & 0xff;
        let z = (pos.z - self.origin.z + 128) & 0xff;
        (x << 8 | z) as i16
    }

    fn state<L: BlockLevel + ?Sized>(
        &mut self,
        level: &L,
        pos: BlockPos,
        load: bool,
    ) -> Option<BlockState> {
        let key = self.key(pos);
        if let Some(&s) = self.states.get(&key) {
            return Some(s);
        }
        let s = if load {
            Some(block_state(level, pos))
        } else {
            state_if_loaded(level, pos)
        }?;
        self.states.insert(key, s);
        Some(s)
    }

    fn is_hole<L: BlockLevel + ?Sized>(
        &mut self,
        level: &L,
        kind: FluidKind,
        pos: BlockPos,
    ) -> bool {
        let key = self.key(pos);
        if let Some(&hole) = self.holes.get(&key) {
            return hole;
        }
        let state = self.state(level, pos, true).unwrap_or(BlockState::AIR);
        let below = block_state(level, pos.offset(0, -1, 0));
        let hole = is_water_hole(level.rules(), kind, state, below);
        self.holes.insert(key, hole);
        hole
    }
}

/// `getSlopeDistance`, as Leaf's breadth-first search: the depth of the nearest hole within
/// the slope distance, 1000 when none.
fn slope_distance<L: BlockLevel + ?Sized>(
    level: &L,
    kind: FluidKind,
    pos: BlockPos,
    pass: i32,
    from: Direction,
    state: BlockState,
    context: &mut SpreadContext,
) -> i32 {
    let rules = level.rules();
    let mut visited: HashSet<(i32, i32, u8)> = HashSet::new();
    let mut queue: VecDeque<(BlockPos, i32, Direction, BlockState)> = VecDeque::new();
    let mut expand = |context: &mut SpreadContext,
                      queue: &mut VecDeque<_>,
                      at: BlockPos,
                      depth: i32,
                      excluded: Direction,
                      at_state: BlockState| {
        for direction in HORIZONTAL {
            if direction == excluded {
                continue;
            }
            let next = relative(at, direction);
            let Some(next_state) = context.state(level, next, false) else {
                continue;
            };
            if !can_pass_through(
                rules,
                kind,
                at_state,
                direction,
                next_state,
                rules.fluid(next_state),
            ) {
                continue;
            }
            if visited.insert((next.x, next.z, direction.opposite() as u8)) {
                queue.push_back((next, depth, direction.opposite(), next_state));
            }
        }
    };
    expand(context, &mut queue, pos, pass, from, state);
    let find = slope_find_distance(level, kind);
    while let Some((at, depth, excluded, at_state)) = queue.pop_front() {
        if context.is_hole(level, kind, at) {
            return depth;
        }
        if depth >= find {
            continue;
        }
        expand(context, &mut queue, at, depth + 1, excluded, at_state);
    }
    1000
}

/// `getSpread`: the sides to flow to (nearest holes first), with what goes there; indexed by
/// direction (`Maps.newEnumMap`'s order).
fn spread_targets<L: BlockLevel + ?Sized>(
    level: &L,
    kind: FluidKind,
    pos: BlockPos,
    state: BlockState,
) -> [Option<StateFluid>; 6] {
    let rules = level.rules();
    let mut lowest = 1000;
    let mut result = [None; 6];
    let mut context: Option<SpreadContext> = None;
    for direction in HORIZONTAL {
        let test = relative(pos, direction);
        let Some(test_state) = state_if_loaded(level, test) else {
            continue;
        };
        let test_fluid = rules.fluid(test_state);
        if !can_maybe_pass_through(rules, kind, state, direction, test_state, test_fluid) {
            continue;
        }
        let new = new_liquid(level, kind, test, test_state);
        if !can_hold_specific(rules, test_state, new.fluid) {
            continue;
        }
        let context = context.get_or_insert_with(|| SpreadContext::new(pos));
        let distance = if context.is_hole(level, kind, test) {
            0
        } else {
            slope_distance(
                level,
                kind,
                test,
                1,
                direction.opposite(),
                test_state,
                context,
            )
        };
        if distance < lowest {
            result = [None; 6];
        }
        if distance <= lowest {
            if can_be_replaced_with(level, test_fluid, test, new.fluid, direction) {
                result[direction as usize] = Some(new);
            }
            lowest = distance;
        }
    }
    result
}

fn fizz<L: BlockLevel + ?Sized>(level: &mut L, pos: BlockPos) {
    level.level_event(EVENT_LAVA_FIZZ, pos, 0);
}

/// `spreadTo` (`LavaFluid`'s first: lava flowing down into water makes stone).
fn spread_to<L: BlockLevel + ?Sized>(
    level: &mut L,
    kind: FluidKind,
    pos: BlockPos,
    state: BlockState,
    direction: Direction,
    target: StateFluid,
) {
    let rules = level.rules();
    if kind == FluidKind::Lava && direction == Direction::Down {
        let here = fluid_at(level, pos);
        if rules.in_water_tag(here.fluid) {
            if rules.has(state, LIQUID_BLOCK)
                && let Some(stone) = rules.named.stone
            {
                set_block(level, pos, stone, UPDATE_ALL, UPDATE_LIMIT);
            }
            fizz(level, pos);
            return;
        }
    }
    if rules.has(state, CONTAINER) {
        place_liquid(level, pos, state, target);
        return;
    }
    let new = rules.legacy_block(target);
    // `beforeDestroyingBlock`: water drops the block's loot (not ported), lava fizzes.
    if !rules.data.is_air(state) && kind == FluidKind::Lava {
        fizz(level, pos);
    }
    set_block_and_update(level, pos, new);
}

/// `LiquidBlockContainer.placeLiquid`: a `SimpleWaterloggedBlock` not waterlogged takes a water
/// source (a lit candle is put out: flags 11), and ticks it.
fn place_liquid<L: BlockLevel + ?Sized>(
    level: &mut L,
    pos: BlockPos,
    state: BlockState,
    target: StateFluid,
) {
    let rules = level.rules();
    if target.fluid != rules.water {
        return;
    }
    let Some((wet, flags)) = rules.data.waterlogged(state) else {
        return;
    };
    let water = rules.water;
    set_block(level, pos, wet, flags, UPDATE_LIMIT);
    let delay = tick_delay(level, water);
    level.schedule_fluid_tick(pos, water, delay);
}

/// `sourceNeighborCount`.
fn source_neighbour_count<L: BlockLevel + ?Sized>(
    level: &L,
    kind: FluidKind,
    pos: BlockPos,
) -> i32 {
    let rules = level.rules();
    HORIZONTAL
        .iter()
        .filter(|&&d| is_source_of(rules, fluid_at(level, relative(pos, d)), kind))
        .count() as i32
}

/// `spreadToSides`.
fn spread_to_sides<L: BlockLevel + ?Sized>(
    level: &mut L,
    kind: FluidKind,
    pos: BlockPos,
    fluid: StateFluid,
    state: BlockState,
) {
    let mut neighbour = i32::from(fluid.amount) - drop_off(level, kind);
    if fluid.falling {
        neighbour = 7;
    }
    if neighbour <= 0 {
        return;
    }
    let targets = spread_targets(level, kind, pos, state);
    for direction in Direction::ALL {
        let Some(new) = targets[direction as usize] else {
            continue;
        };
        let to = relative(pos, direction);
        let Some(to_state) = state_if_loaded(level, to) else {
            continue;
        };
        spread_to(level, kind, to, to_state, direction, new);
    }
}

/// `spread`: down when it can, then (from a source, or where there is no hole below) to the sides.
fn spread<L: BlockLevel + ?Sized>(
    level: &mut L,
    kind: FluidKind,
    pos: BlockPos,
    state: BlockState,
    fluid: StateFluid,
) {
    let rules = level.rules();
    if is_empty(rules, fluid) {
        return;
    }
    let below_pos = pos.offset(0, -1, 0);
    let below = block_state(level, below_pos);
    let below_fluid = rules.fluid(below);
    if can_maybe_pass_through(rules, kind, state, Direction::Down, below, below_fluid) {
        let new = new_liquid(level, kind, below_pos, below);
        if can_be_replaced_with(level, below_fluid, below_pos, new.fluid, Direction::Down)
            && can_hold_specific(level.rules(), below, new.fluid)
        {
            spread_to(level, kind, below_pos, below, Direction::Down, new);
            if source_neighbour_count(level, kind, pos) >= 3 {
                spread_to_sides(level, kind, pos, fluid, state);
            }
            return;
        }
    }
    let rules = level.rules();
    if rules.is_source(fluid.fluid) || !is_water_hole(rules, kind, state, below) {
        spread_to_sides(level, kind, pos, fluid, state);
    }
}

/// `getSpreadDelay` (lava: four times slower where it rises, three times in four).
fn spread_delay<L: BlockLevel + ?Sized>(
    level: &mut L,
    kind: FluidKind,
    pos: BlockPos,
    old: StateFluid,
    new: StateFluid,
) -> i32 {
    let delay = kind_tick_delay(level, kind);
    if kind == FluidKind::Water {
        return delay;
    }
    let rules = level.rules();
    if !is_empty(rules, old)
        && !is_empty(rules, new)
        && !old.falling
        && !new.falling
        && height(level, new, pos) > height(level, old, pos)
        && level.random().next_int_bound(4) != Ok(0)
    {
        delay * 4
    } else {
        delay
    }
}

/// `ServerLevel.tickFluid`: the fluid at `pos` ticks when it is still the one scheduled.
pub fn tick_fluid<L: BlockLevel + ?Sized>(level: &mut L, pos: BlockPos, fluid: u8) {
    let state = block_state(level, pos);
    let current = level.rules().fluid(state);
    if current.fluid != fluid {
        return;
    }
    if let Some(kind) = level.rules().kind(fluid) {
        flowing_tick(level, kind, pos, state, current);
    }
}

/// `FlowingFluid.tick`.
fn flowing_tick<L: BlockLevel + ?Sized>(
    level: &mut L,
    kind: FluidKind,
    pos: BlockPos,
    mut state: BlockState,
    mut fluid: StateFluid,
) {
    let rules = level.rules();
    if !rules.is_source(fluid.fluid) {
        let new = new_liquid(level, kind, pos, block_state(level, pos));
        let delay = spread_delay(level, kind, pos, fluid, new);
        let rules = level.rules();
        if is_empty(rules, new) {
            fluid = new;
            state = BlockState::AIR;
            set_block_and_update(level, pos, state);
        } else if new != fluid {
            fluid = new;
            state = rules.legacy_block(new);
            set_block_and_update(level, pos, state);
            level.schedule_fluid_tick(pos, new.fluid, delay);
        }
    }
    spread(level, kind, pos, state, fluid);
}

/// `LiquidBlock.getFlowSpeed` (Paper): water next to lava flows at
/// `water-over-lava-flow-speed`.
fn flow_speed<L: BlockLevel + ?Sized>(level: &L, kind: FluidKind, pos: BlockPos) -> i32 {
    if kind == FluidKind::Water {
        let rules = level.rules();
        let lava = |d: [i32; 3]| {
            state_if_loaded(level, pos.offset(d[0], d[1], d[2]))
                .is_some_and(|s| rules.in_lava_tag(rules.fluid(s).fluid))
        };
        if lava([0, 0, -1]) || lava([0, 0, 1]) || lava([-1, 0, 0]) || lava([1, 0, 0]) {
            return level.fluid_settings().water_over_lava_flow_speed;
        }
    }
    kind_tick_delay(level, kind)
}

/// `LiquidBlock.shouldSpreadLiquid`: lava next to water hardens (obsidian from a source, else
/// cobblestone), lava over soul soil next to blue ice becomes basalt; false when it did.
fn should_spread_liquid<L: BlockLevel + ?Sized>(
    level: &mut L,
    kind: FluidKind,
    pos: BlockPos,
) -> bool {
    if kind != FluidKind::Lava {
        return true;
    }
    let rules = level.rules();
    let named = rules.named;
    let blocks = rules.blocks.clone();
    let over_soul_soil =
        Some(blocks.block(block_state(level, pos.offset(0, -1, 0)))) == named.soul_soil;
    for direction in POSSIBLE_FLOW_DIRECTIONS {
        let neighbour = relative(pos, direction.opposite());
        let rules = level.rules();
        if rules.in_water_tag(fluid_at(level, neighbour).fluid) {
            let into = if rules.is_source(fluid_at(level, pos).fluid) {
                named.obsidian
            } else {
                named.cobblestone
            };
            if let Some(into) = into {
                set_block_and_update(level, pos, into);
            }
            fizz(level, pos);
            return false;
        }
        if over_soul_soil && Some(blocks.block(block_state(level, neighbour))) == named.blue_ice {
            if let Some(basalt) = named.basalt {
                set_block_and_update(level, pos, basalt);
            }
            fizz(level, pos);
            return false;
        }
    }
    true
}

/// `LiquidBlock.shouldBubbleColumnOccupy`: a full source of a fluid bubble columns can occupy.
fn bubble_column_occupies(rules: &BlockRules, state: BlockState) -> bool {
    let f = rules.fluid(state);
    rules.bubble_column_can_occupy(f.fluid) && rules.is_source(f.fluid) && f.amount == 8
}

/// `tryScheduleBubbleBlockColumn`: the liquid block's own tick in 20 ticks.
fn try_schedule_bubble_column<L: BlockLevel + ?Sized>(
    level: &mut L,
    state: BlockState,
    pos: BlockPos,
    below: BlockState,
) {
    if level.rules().enables_bubble_column(below) {
        let block = level.rules().blocks.block(state);
        level.schedule_block_tick(pos, block, 20);
    }
}

/// `LiquidBlock.onPlace` and `neighborChanged` (the same).
fn liquid_changed<L: BlockLevel + ?Sized>(level: &mut L, state: BlockState, pos: BlockPos) {
    let fluid = level.rules().fluid(state).fluid;
    let Some(kind) = level.rules().kind(fluid) else {
        return;
    };
    if level.fluid_settings().tick_fluids && should_spread_liquid(level, kind, pos) {
        let delay = flow_speed(level, kind, pos);
        level.schedule_fluid_tick(pos, fluid, delay);
    }
    if bubble_column_occupies(level.rules(), state) {
        let below = block_state(level, pos.offset(0, -1, 0));
        try_schedule_bubble_column(level, state, pos, below);
    }
}

pub fn liquid_on_place<L: BlockLevel + ?Sized>(level: &mut L, state: BlockState, pos: BlockPos) {
    liquid_changed(level, state, pos);
}

pub fn liquid_neighbor_changed<L: BlockLevel + ?Sized>(
    level: &mut L,
    state: BlockState,
    pos: BlockPos,
) {
    liquid_changed(level, state, pos);
}

/// `LiquidBlock.updateShape`: a source, or a source next to it, ticks the fluid; the state stays.
pub fn liquid_update_shape<L: BlockLevel + ?Sized>(
    level: &mut L,
    state: BlockState,
    pos: BlockPos,
    direction: Direction,
    neighbour: BlockState,
) -> BlockState {
    let rules = level.rules();
    let fluid = rules.fluid(state).fluid;
    let neighbour_source = rules.is_source(rules.fluid(neighbour).fluid);
    if (level.fluid_settings().tick_fluids && rules.is_source(fluid)) || neighbour_source {
        let delay = tick_delay(level, fluid);
        level.schedule_fluid_tick(pos, fluid, delay);
    }
    if direction == Direction::Down && bubble_column_occupies(level.rules(), state) {
        try_schedule_bubble_column(level, state, pos, neighbour);
    }
    state
}
