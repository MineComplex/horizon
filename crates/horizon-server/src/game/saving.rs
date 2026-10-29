//! The save screen freezes simulation, but keeps pumping window events while
//! snapshots are prepared in small batches and the region workers drain.
use std::{
    collections::VecDeque,
    time::Instant,
};

pub(super) struct ClosingSave {
    pub quit: bool,
    close: bool,
    reply: Option<std::sync::mpsc::Sender<String>>,
    started: Instant,
    stage: Stage,
}
enum Stage {
    Generation,
    Chunks(VecDeque<(i32, i32)>),
    Entities(VecDeque<(i32, i32)>),
    Writes,
}

// Bound the work in both sparse and block-entity-heavy worlds. A single chunk
// is indivisible; check the time budget after each snapshot, not each batch.

impl crate::game::Sim<'_> {
}
