//! Server text displays: metadata, interpolation, billboard transforms and world text.
use crate::hc::{text::Text, text_display::TextDisplayData};

pub(super) struct RemoteTextDisplay {
    pub data: TextDisplayData,
    from: TextDisplayData,
    shown: TextDisplayData,
    position: glam::DVec3,
    previous: glam::DVec3,
    target: glam::DVec3,
    rotation: [f32; 2],
    old_rotation: [f32; 2],
    target_rotation: [f32; 2],
    steps: i32,
    age: i64,
    start: i64,
    duration: i32,
    dirty: bool,
    start_dirty: bool,
    duration_dirty: bool,
    initialized: bool,
    /// `Display.lastProgress`: the progress the last frame drew, where a new
    /// interpolation starts from.
    last_progress: f32,
    /// `TextDisplay.clientDisplayCache`: the split lines, until the render
    /// state changes.
    lines: Option<Lines>,
}

/// `TextDisplay.CachedInfo`: each line and its width, and the widest.
struct Lines {
    lines: Vec<(Text, f32)>,
    width: f32,
}


impl RemoteTextDisplay {
}

impl crate::game::Sim<'_> {
}
