//! The Rust side of tools/test_dispenser_parity.py: with
//! `HORIZON_DISPENSE_FIXTURE=CASES,OUTPUT` the game loads its resources, runs
//! every case (a dispenser or dropper with items, blocks and target entities
//! around it) through `dispense_from` on a fresh headless world and prints what
//! changed in the format of `VanillaDispenserOracle`.
use std::cell::RefCell;

thread_local! {
    /// Level and game events recorded while a fixture case runs.
    static LOG: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Record a line if a fixture case is running.
pub(super) fn record(line: impl FnOnce() -> String) {
    LOG.with(|log| {
        if let Some(lines) = log.borrow_mut().as_mut() {
            lines.push(line());
        }
    });
}
/// `Level.levelEvent` lines for the level events the behaviour library reported.
pub(super) fn record_events(events: &[horizon_world::level::LevelEvent]) {
    for event in events {
        if let horizon_world::level::LevelEvent::LevelEvent { id, pos, data } = event {
            record(|| format!("L {id} {},{},{} {data}", pos.x, pos.y, pos.z));
        }
    }
}
