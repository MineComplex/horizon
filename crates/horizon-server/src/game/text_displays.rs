//! Server text displays: metadata, interpolation, billboard transforms and world text.
use crate::hc::{text::Text, text_display::TextDisplayData};
use horizon_protocol::entity_data::Value;

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

/// `Display.RENDER_STATE_IDS` and `TextDisplay.TEXT_RENDER_STATE_IDS`: the
/// data a render state is built from (not the view range, size or glow).
fn renders(id: u8) -> bool {
    matches!(id, 11..=16 | 18 | 19 | 23..=27)
}

impl RemoteTextDisplay {
    pub fn new(position: [f64; 3], yaw: f32) -> Self {
        Self {
            data: Default::default(),
            from: Default::default(),
            shown: Default::default(),
            position: position.into(),
            previous: position.into(),
            target: position.into(),
            rotation: [yaw, 0.0],
            old_rotation: [yaw, 0.0],
            target_rotation: [yaw, 0.0],
            steps: 0,
            age: 0,
            start: 0,
            duration: 0,
            dirty: true,
            start_dirty: false,
            duration_dirty: false,
            initialized: false,
            last_progress: 0.0,
            lines: None,
        }
    }
    fn progress(&self, partial: f32) -> f32 {
        if self.duration <= 0 {
            1.0
        } else {
            (((self.age - self.start) as f32 + partial) / self.duration as f32).clamp(0.0, 1.0)
        }
    }
    pub fn metadata(&mut self, values: &[(u8, i32, Value)]) {
        self.start_dirty |= values.iter().any(|v| v.0 == 8);
        self.duration_dirty |= values.iter().any(|v| v.0 == 9);
        self.dirty |= values.iter().any(|v| renders(v.0));
        self.data.apply(values);
    }
    fn tick(&mut self) {
        self.age += 1;
        self.previous = self.position;
        self.old_rotation = self.rotation;
        if self.steps > 0 {
            self.position += (self.target - self.position) / f64::from(self.steps);
            for i in 0..2 {
                self.rotation[i] +=
                    angle(self.target_rotation[i] - self.rotation[i]) / self.steps as f32;
            }
            self.steps -= 1;
        }
        if self.start_dirty {
            self.start = self.age + i64::from(self.data.interpolation_delay);
        }
        if self.duration_dirty {
            self.duration = self.data.interpolation_duration;
        }
        if self.dirty {
            self.from = if self.initialized && self.duration > 0 {
                self.shown.blend(&self.from, self.last_progress)
            } else {
                self.data.clone()
            };
            self.shown = self.data.clone();
            self.initialized = true;
            self.lines = None;
        }
        self.dirty = false;
        self.start_dirty = false;
        self.duration_dirty = false;
    }
    fn move_to(&mut self, position: [f64; 3], rotation: Option<(f32, f32)>) {
        self.target = position.into();
        if let Some((yaw, pitch)) = rotation {
            self.target_rotation = [yaw, pitch];
        }
        self.steps = self.data.teleport_duration.clamp(0, 59);
        if self.steps == 0 {
            self.position = self.target;
            self.previous = self.target;
            self.rotation = self.target_rotation;
            self.old_rotation = self.rotation;
        }
    }
}
fn angle(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

impl crate::game::Sim<'_> {
    pub(super) fn move_remote_text_display(
        &mut self,
        id: i32,
        position: [f64; 3],
        rotation: Option<(f32, f32)>,
    ) {
        if let Some(d) = self.text_displays.get_mut(&id) {
            d.move_to(position, rotation);
        }
    }
    pub(super) fn tick_text_displays(&mut self) {
        for display in self.text_displays.values_mut() {
            display.tick();
        }
    }
}
