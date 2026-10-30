//! Port of `net.kyori.adventure.util.Ticks`.
use std::time::Duration;

pub const TICKS_PER_SECOND: i32 = 20;
pub const SINGLE_TICK_DURATION_MS: i64 = 1000 / TICKS_PER_SECOND as i64;

pub struct Ticks;
impl Ticks {
    pub fn duration(ticks: i64) -> Duration {
        Duration::from_millis((ticks * SINGLE_TICK_DURATION_MS).max(0) as u64)
    }
}
pub fn duration(ticks: i64) -> Duration {
    Ticks::duration(ticks)
}
