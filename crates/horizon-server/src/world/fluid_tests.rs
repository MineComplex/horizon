//! The block-change path and the fluids over a map-backed level ([`MapLevel`], also the oracle
//! test's level): the neighbour updater's order and chain limit over the synthetic tables, and
//! hand-checked water and lava over the real 26.3 tables when `HORIZON_SERVER_BLOCKS_JSON`,
//! `HORIZON_SERVER_BLOCK_STATES` and `HORIZON_VANILLA_JAR` are set (those pass without checking
//! otherwise).

use std::collections::HashMap;
use std::sync::Arc;

use super::block_rules::BlockRules;
use super::block_update::{
    BlockLevel, MAX_CHAINED_NEIGHBOR_UPDATES, NeighborUpdater, UPDATE_ALL, UPDATE_LIMIT, set_block,
};
use super::fluid::{self, FluidSettings};
use super::ticks::{ScheduledTicks, TickPriority};
use crate::block::{BlockId, BlockState, BlockStateData, BlockStates, BlockStatesBuilder};
use crate::core::position::{BlockPos, ChunkPos};
use crate::util::random::LegacyRandom;
use crate::world::ticks::LevelChunkTicks;

/// A level over a map of positions, every chunk from -3 to 3 loaded and ticking.
pub(super) struct MapLevel {
    pub rules: Arc<BlockRules>,
    pub cells: HashMap<BlockPos, BlockState>,
    pub min_y: i32,
    pub height: i32,
    pub ticks: ScheduledTicks<BlockId, u8>,
    pub time: i64,
    pub random: LegacyRandom,
    pub updater: NeighborUpdater,
    pub settings: FluidSettings,
    /// The trace lines (`set`, `upd`, `chg`, `ev`), as the oracle writes them.
    pub log: Vec<String>,
}

impl MapLevel {
    pub fn new(
        rules: Arc<BlockRules>,
        (min_y, height): (i32, i32),
        seed: i64,
        fast_lava: bool,
    ) -> Self {
        let mut ticks = ScheduledTicks::default();
        for x in -3..=3 {
            for z in -3..=3 {
                let pos = ChunkPos::new(x, z);
                ticks.block.add_container(pos, LevelChunkTicks::default());
                ticks.fluid.add_container(pos, LevelChunkTicks::default());
            }
        }
        Self {
            rules,
            cells: HashMap::new(),
            min_y,
            height,
            ticks,
            time: 1000,
            random: LegacyRandom::new(seed),
            updater: NeighborUpdater::new(MAX_CHAINED_NEIGHBOR_UPDATES),
            settings: FluidSettings::new(fast_lava),
            log: Vec::new(),
        }
    }

    /// One game tick: the time, the block ticks (not run: put back), the fluid ticks.
    pub fn tick(&mut self) {
        self.time += 1;
        let time = self.time;
        self.ticks.block.collect_ticks(time, 65536, |_| true);
        let mut deferred = Vec::new();
        while let Some(t) = self.ticks.block.pop_due_tick() {
            deferred.push(t);
        }
        self.ticks.block.cleanup_after_tick();
        for t in deferred {
            self.ticks.block.schedule(t);
        }
        self.ticks.fluid.collect_ticks(time, 65536, |_| true);
        while let Some((pos, id)) = self.ticks.fluid.pop_due() {
            fluid::tick_fluid(self, pos, id);
        }
        self.ticks.fluid.cleanup_after_tick();
    }

    pub fn at(&self, pos: BlockPos) -> BlockState {
        self.cells.get(&pos).copied().unwrap_or(BlockState::AIR)
    }
}

fn line(pos: BlockPos) -> String {
    format!("{} {} {}", pos.x, pos.y, pos.z)
}

impl BlockLevel for MapLevel {
    fn rules(&self) -> &BlockRules {
        &self.rules
    }
    fn min_y(&self) -> i32 {
        self.min_y
    }
    fn height(&self) -> i32 {
        self.height
    }
    fn loaded_state(&self, pos: BlockPos) -> Option<BlockState> {
        Some(self.at(pos))
    }
    fn write_state(&mut self, pos: BlockPos, state: BlockState) -> Option<BlockState> {
        let old = self.at(pos);
        if old == state {
            return None;
        }
        if state == BlockState::AIR {
            self.cells.remove(&pos);
        } else {
            self.cells.insert(pos, state);
        }
        Some(old)
    }
    fn sends_updates(&self, _: BlockPos) -> bool {
        true
    }
    fn block_changed(&mut self, pos: BlockPos) {
        self.log.push(format!("chg {}", line(pos)));
    }
    fn level_event(&mut self, event: i32, pos: BlockPos, data: i32) {
        self.log.push(format!("ev {event} {} {data}", line(pos)));
    }
    fn schedule_block_tick(&mut self, pos: BlockPos, block: BlockId, delay: i32) {
        let tick = self
            .ticks
            .create_block_tick(self.time, pos, block, delay, TickPriority::Normal);
        self.ticks.block.schedule(tick);
    }
    fn schedule_fluid_tick(&mut self, pos: BlockPos, fluid: u8, delay: i32) {
        let tick = self
            .ticks
            .create_fluid_tick(self.time, pos, fluid, delay, TickPriority::Normal);
        self.ticks.fluid.schedule(tick);
    }
    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.random
    }
    fn updater(&mut self) -> &mut NeighborUpdater {
        &mut self.updater
    }
    fn fluid_settings(&self) -> FluidSettings {
        self.settings
    }
    fn trace_updated(&mut self, pos: BlockPos) {
        self.log.push(format!("upd {}", line(pos)));
    }
    fn trace_set(&mut self, pos: BlockPos, state: BlockState, flags: i32) {
        self.log
            .push(format!("set {} {} {flags}", line(pos), state.0));
    }
}

/// The synthetic tables (air, stone, a slab, a water without levels): no block behaves.
fn synthetic_rules() -> Arc<BlockRules> {
    let mut b = BlockStatesBuilder::new();
    for name in ["air", "stone", "oak_slab", "water"] {
        b.add_block(&format!("minecraft:{name}"), &[]).unwrap();
    }
    let data = BlockStateData::from_bytes(&crate::block::flags_tests::synthetic(4), 4).unwrap();
    Arc::new(BlockRules::new(
        Arc::new(b.finish()),
        Arc::new(data),
        &[],
        &[],
    ))
}

#[test]
fn neighbour_and_shape_updates_in_vanilla_order() {
    let mut level = MapLevel::new(synthetic_rules(), (-64, 384), 0, false);
    let stone = BlockState(1);
    assert!(set_block(
        &mut level,
        BlockPos::new(0, 0, 0),
        stone,
        UPDATE_ALL,
        UPDATE_LIMIT
    ));
    let mut expected = vec!["chg 0 0 0".to_owned()];
    // `updateNeighborsAt`: one multi update, traced once (its six positions).
    for p in ["-1 0 0", "1 0 0", "0 -1 0", "0 1 0", "0 0 -1", "0 0 1"] {
        expected.push(format!("upd {p}"));
    }
    // `updateNeighbourShapes`: one shape update per neighbour, in `UPDATE_SHAPE_ORDER`.
    for p in ["-1 0 0", "1 0 0", "0 0 -1", "0 0 1", "0 -1 0", "0 1 0"] {
        expected.push(format!("upd {p}"));
    }
    expected.push("set 0 0 0 1 3".to_owned());
    assert_eq!(level.log, expected);
    // The same state again changes nothing; out of the world nothing either.
    level.log.clear();
    assert!(!set_block(
        &mut level,
        BlockPos::new(0, 0, 0),
        stone,
        UPDATE_ALL,
        UPDATE_LIMIT
    ));
    assert!(!set_block(
        &mut level,
        BlockPos::new(0, 320, 0),
        stone,
        UPDATE_ALL,
        UPDATE_LIMIT
    ));
    assert!(!set_block(
        &mut level,
        BlockPos::new(30_000_000, 0, 0),
        stone,
        UPDATE_ALL,
        UPDATE_LIMIT
    ));
    assert!(level.log.is_empty());
    // Flags 16 (known shape) and 2 only: no neighbour or shape updates.
    assert!(set_block(
        &mut level,
        BlockPos::new(5, 0, 0),
        stone,
        2 | 16,
        UPDATE_LIMIT
    ));
    assert_eq!(level.log, ["chg 5 0 0", "set 5 0 0 1 18"]);
}

#[test]
fn chained_updates_stop_at_the_limit() {
    // Each top-level update is its own chain; a limit of 0 skips them all, 1 lets each chain's
    // first update run.
    let mut level = MapLevel::new(synthetic_rules(), (-64, 384), 0, false);
    level.updater.set_max_chained(0);
    set_block(
        &mut level,
        BlockPos::new(0, 0, 0),
        BlockState(1),
        UPDATE_ALL,
        UPDATE_LIMIT,
    );
    assert_eq!(level.log, ["chg 0 0 0", "set 0 0 0 1 3"]);
    level.log.clear();
    level.updater.set_max_chained(1);
    set_block(
        &mut level,
        BlockPos::new(9, 0, 0),
        BlockState(1),
        UPDATE_ALL,
        UPDATE_LIMIT,
    );
    assert_eq!(
        level.log.iter().filter(|l| l.starts_with("upd")).count(),
        12
    );
}

/// The real tables, when the environment names them.
pub(super) fn real_rules() -> Option<Arc<BlockRules>> {
    use crate::registry::{DataPack, load_tags, registry_dir};
    use crate::server::vanilla_jar::VanillaJar;
    let (Some(json), Some(bin), Some(jar)) = (
        std::env::var_os("HORIZON_SERVER_BLOCKS_JSON"),
        std::env::var_os("HORIZON_SERVER_BLOCK_STATES"),
        std::env::var_os("HORIZON_VANILLA_JAR"),
    ) else {
        return None;
    };
    let blocks = BlockStates::load(json.as_ref()).unwrap();
    let data = BlockStateData::load(bin.as_ref(), &blocks).unwrap();
    let mut jar = VanillaJar::open(jar.as_ref()).expect("the 26.3 jar");
    let pack = DataPack::vanilla(&mut jar).expect("the vanilla pack");
    let packs = std::slice::from_ref(&pack);
    let block_tags = load_tags(packs, &registry_dir("minecraft:block"), |id| {
        blocks.block_by_name(id).map(|b| b.0 as i32)
    });
    let fluid_tags = load_tags(packs, &registry_dir("minecraft:fluid"), |id| {
        crate::worldgen::placement::FLUIDS
            .iter()
            .position(|f| *f == id)
            .map(|i| i as i32)
    });
    Some(Arc::new(BlockRules::new(
        Arc::new(blocks),
        Arc::new(data),
        &block_tags,
        &fluid_tags,
    )))
}

fn named(rules: &BlockRules, text: &str) -> BlockState {
    let (name, props) = text.split_once('[').unwrap_or((text, ""));
    let block = rules
        .blocks
        .block_by_name(&format!("minecraft:{name}"))
        .unwrap();
    let values = props
        .trim_end_matches(']')
        .split(',')
        .filter(|p| !p.is_empty())
        .map(|p| p.split_once('=').unwrap());
    rules.blocks.state_with(block, values).unwrap()
}

#[test]
fn water_spreads_seven_blocks_and_lava_hardens() {
    let Some(rules) = real_rules() else {
        return;
    };
    let mut level = MapLevel::new(rules.clone(), (-64, 384), 1, false);
    let stone = named(&rules, "stone");
    for x in -9..=9 {
        for z in -9..=9 {
            level.cells.insert(BlockPos::new(x, 0, z), stone);
        }
    }
    let water = named(&rules, "water");
    set_block(
        &mut level,
        BlockPos::new(0, 1, 0),
        water,
        UPDATE_ALL,
        UPDATE_LIMIT,
    );
    for _ in 0..60 {
        level.tick();
    }
    let level_at = |l: &MapLevel, x: i32| rules.fluid(l.at(BlockPos::new(x, 1, 0))).amount;
    let amounts: Vec<u8> = (0..=8).map(|x| level_at(&level, x)).collect();
    assert_eq!(amounts, [8, 7, 6, 5, 4, 3, 2, 1, 0]);
    // A lava source placed next to the water source becomes obsidian at once (`onPlace`).
    let lava = named(&rules, "lava");
    set_block(
        &mut level,
        BlockPos::new(0, 1, 1),
        lava,
        UPDATE_ALL,
        UPDATE_LIMIT,
    );
    assert_eq!(level.at(BlockPos::new(0, 1, 1)), named(&rules, "obsidian"));
    assert!(level.log.iter().any(|l| l == "ev 1501 0 1 1 0"));
}
