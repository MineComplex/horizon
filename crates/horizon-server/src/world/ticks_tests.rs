//! `world::ticks` without an oracle: Moonrise's dirty tracking (not in the vanilla jar), proto
//! chunks' ticks, and the small rules the oracle's scripts do not isolate.

use super::ticks::{
    LevelChunkTicks, LevelTicks, ProtoChunkTicks, SavedTick, ScheduledTick, ScheduledTicks,
    TickArea, TickPriority,
};
use crate::core::position::{BlockPos, ChunkPos};

fn tick(ty: u8, x: i32, trigger: i64, priority: TickPriority, sub: i64) -> ScheduledTick<u8> {
    ScheduledTick {
        ty,
        pos: BlockPos::new(x, 64, 0),
        trigger_tick: trigger,
        priority,
        sub_tick_order: sub,
    }
}

#[test]
fn a_chunk_drains_by_time_then_priority_then_sub_tick_order_and_drops_repeats() {
    let mut c = LevelChunkTicks::default();
    assert!(c.schedule(tick(1, 1, 5, TickPriority::Normal, 0)).is_some());
    assert!(c.schedule(tick(1, 2, 5, TickPriority::Normal, 1)).is_none());
    assert!(c.schedule(tick(1, 3, 5, TickPriority::High, 2)).is_some());
    assert!(
        c.schedule(tick(1, 4, 3, TickPriority::VeryLow, 3))
            .is_some()
    );
    // The same thing at the same position, whatever its time: dropped.
    assert!(c.schedule(tick(1, 1, 1, TickPriority::Normal, 4)).is_none());
    assert_eq!(c.count(), 4);
    assert!(c.has_scheduled_tick(BlockPos::new(1, 64, 0), 1));
    let order: Vec<i32> = std::iter::from_fn(|| c.poll()).map(|t| t.pos.x).collect();
    assert_eq!(order, [4, 3, 1, 2]);
    assert!(!c.has_scheduled_tick(BlockPos::new(1, 64, 0), 1));
}

#[test]
fn saved_ticks_wait_for_unpack_and_come_back_relative() {
    let saved = |x, delay| SavedTick {
        ty: 7u8,
        pos: BlockPos::new(x, 64, 0),
        delay,
        priority: TickPriority::Normal,
    };
    let mut c = LevelChunkTicks::from_pending(vec![saved(1, 3), saved(2, 0), saved(3, 1)]);
    assert_eq!((c.count(), c.peek()), (3, None));
    assert!(c.has_scheduled_tick(BlockPos::new(2, 64, 0), 7));
    // Pending ticks pack as they are, whatever the time.
    assert_eq!(c.saved(1000), vec![saved(1, 3), saved(2, 0), saved(3, 1)]);
    let first = c.unpack(100).unwrap();
    assert_eq!(
        (first.pos.x, first.trigger_tick, first.sub_tick_order),
        (2, 100, -2)
    );
    assert_eq!(c.unpack(200), None);
    // Queued in the order read (sub-tick orders -3..-1), packed against the new time.
    assert_eq!(c.saved(102), vec![saved(1, 1), saved(2, -2), saved(3, -1)]);
    assert_eq!(c.count(), 3);
}

#[test]
fn moonrise_dirty_tracking() {
    let mut c = LevelChunkTicks::default();
    assert!(!c.is_dirty(5));
    c.schedule(tick(1, 1, 9, TickPriority::Normal, 0));
    assert!(c.is_dirty(5));
    c.clear_dirty();
    // Queued ticks were never packed: dirty whatever the time.
    assert!(c.is_dirty(5));
    c.pack(5);
    assert!(!c.is_dirty(5));
    // Delays packed at 5 are wrong at 6.
    assert!(c.is_dirty(6));
    c.poll();
    c.clear_dirty();
    assert!(!c.is_dirty(6), "nothing queued, nothing to be wrong");
    c.schedule(tick(1, 1, 9, TickPriority::Normal, 1));
    c.clear_dirty();
    c.remove_if(|_| true);
    assert!(c.is_dirty(6) && c.count() == 0);
}

#[test]
fn proto_chunk_ticks_drop_repeats_and_save_without_a_delay() {
    let mut p = ProtoChunkTicks::default();
    let pos = BlockPos::new(1, 2, 3);
    p.schedule(pos, 4u8, TickPriority::Normal);
    p.schedule(pos, 4, TickPriority::High);
    p.schedule(pos, 5, TickPriority::High);
    assert_eq!(p.count(), 2);
    assert!(p.has_scheduled_tick(pos, 5));
    assert_eq!(p.ticks()[0].delay, 0);
    let loaded = ProtoChunkTicks::load(vec![p.ticks()[0], p.ticks()[0], p.ticks()[1]]);
    assert_eq!(loaded.ticks(), p.ticks());
}

#[test]
fn level_ticks_collect_in_priority_order_up_to_the_limit() {
    let mut level = LevelTicks::default();
    let (a, b) = (ChunkPos::new(0, 0), ChunkPos::new(1, 0));
    level.add_container(a, LevelChunkTicks::default());
    level.add_container(b, LevelChunkTicks::default());
    level.schedule(tick(1, 1, 10, TickPriority::Normal, 0));
    level.schedule(tick(1, 17, 10, TickPriority::High, 1));
    level.schedule(tick(1, 2, 10, TickPriority::Normal, 2));
    level.schedule(tick(1, 18, 11, TickPriority::ExtremelyHigh, 3));
    assert!(!level.will_tick_this_tick(BlockPos::new(1, 64, 0), 1));
    let mut ran = Vec::new();
    level.tick(
        10,
        100,
        |_| true,
        |l, pos, _| {
            ran.push(pos.x);
            // What is collected and not yet run is "about to tick".
            assert_eq!(
                l.will_tick_this_tick(BlockPos::new(2, 64, 0), 1),
                pos.x != 2
            );
        },
    );
    assert_eq!(ran, [17, 1, 2]);
    assert_eq!(level.count(), 1);
    // A chunk that does not tick keeps its ticks; the limit leaves the rest for later.
    level.schedule(tick(1, 3, 11, TickPriority::Normal, 4));
    level.schedule(tick(1, 4, 11, TickPriority::Normal, 5));
    let mut ran = Vec::new();
    level.tick(11, 2, |key| key == b.pack(), |_, pos, _| ran.push(pos.x));
    assert_eq!(ran, [18]);
    level.tick(11, 1, |_| true, |_, pos, _| ran.push(pos.x));
    level.tick(12, 10, |_| true, |_, pos, _| ran.push(pos.x));
    assert_eq!(ran, [18, 3, 4]);
    assert_eq!(level.count(), 0);
}

#[test]
fn clearing_and_copying_an_area() {
    let mut level = LevelTicks::default();
    for x in 0..2 {
        level.add_container(ChunkPos::new(x, 0), LevelChunkTicks::default());
    }
    level.schedule(tick(1, 1, 10, TickPriority::Normal, 0));
    level.schedule(tick(1, 20, 12, TickPriority::Low, 1));
    level.schedule(tick(1, 30, 12, TickPriority::Low, 2));
    let area = TickArea {
        min: BlockPos::new(0, 0, 0),
        max: BlockPos::new(20, 100, 0),
    };
    level.copy_area(&area, (5, 0, 0));
    assert_eq!(level.count(), 5);
    assert!(level.has_scheduled_tick(BlockPos::new(25, 64, 0), 1));
    level.clear_area(&area);
    assert_eq!(level.count(), 2);
    assert!(!level.has_scheduled_tick(BlockPos::new(1, 64, 0), 1));
    assert!(level.has_scheduled_tick(BlockPos::new(30, 64, 0), 1));
    // The copies follow the source's ticks in the order.
    let mut ran = Vec::new();
    level.tick(12, 10, |_| true, |_, pos, _| ran.push(pos.x));
    assert_eq!(ran, [30, 25]);
}

#[test]
fn a_tick_in_an_unloaded_chunk_is_dropped_and_a_removed_container_forgotten() {
    let mut level = LevelTicks::default();
    level.schedule(tick(1, 1, 10, TickPriority::Normal, 0));
    assert_eq!(level.count(), 0);
    level.add_container(ChunkPos::new(0, 0), LevelChunkTicks::default());
    level.schedule(tick(1, 1, 10, TickPriority::Normal, 0));
    assert!(level.remove_container(ChunkPos::new(0, 0)).is_some());
    let mut ran = 0;
    level.tick(10, 10, |_| true, |_, _, _| ran += 1);
    assert_eq!((ran, level.count()), (0, 0));
}

#[test]
fn priorities_clamp_and_ticks_get_sub_tick_orders_from_the_level() {
    assert_eq!(TickPriority::by_value(-9), TickPriority::ExtremelyHigh);
    assert_eq!(TickPriority::by_value(9), TickPriority::ExtremelyLow);
    assert_eq!(TickPriority::by_value(0).value(), 0);
    let mut ticks = ScheduledTicks::<u8, u8>::default();
    let a = ticks.create_block_tick(10, BlockPos::new(0, 0, 0), 1, 5, TickPriority::Normal);
    let b = ticks.create_fluid_tick(10, BlockPos::new(0, 0, 0), 2, 0, TickPriority::Low);
    assert_eq!((a.trigger_tick, a.sub_tick_order), (15, 0));
    assert_eq!((b.trigger_tick, b.sub_tick_order), (10, 1));
}
