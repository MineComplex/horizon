//! The loom screen (`LoomScreen`): the scrolling pattern list, the banner
//! preview and the six-layer error. The menu itself (`LoomMenu`: slots,
//! selectable patterns, the pattern buttons, quick moves, the result) runs on
//! the verified engine, `crate::hc::menu`, through `crate::game::Sim::menu_session`.
use super::stations::Kind;
use crate::hc::{banner::{self, BannerLayer}, items::ItemStack, screen_layout::{self, LoomView, ScreenDraw}};
use std::sync::Arc;

/// Per-menu loom state: `selectablePatterns`, the selected index and the
/// screen's scroller.
#[derive(Default)]
pub(super) struct LoomState {
    pub(super) patterns: Vec<Arc<str>>,
    pub(super) selected: Option<usize>,
    /// `displayPatterns`: the pattern buttons are shown.
    pub(super) display: bool,
    pub(super) has_max: bool,
    pub(super) scroll: f32,
    pub(super) start_row: usize,
    pub(super) scrolling: bool,
    /// The inputs the screen last saw (`bannerStack`, `dyeStack`, `patternStack`).
    last_inputs: [Option<ItemStack>; 3],
}
impl LoomState {
    fn total_rows(&self) -> usize {
        self.patterns.len().div_ceil(4)
    }
    fn offscreen_rows(&self) -> i32 {
        self.total_rows() as i32 - 4
    }
    /// `startRow` for a scroller position.
    fn row_for(&self, offs: f32) -> usize {
        ((offs * self.offscreen_rows() as f32 + 0.5) as i32).max(0) as usize
    }
}

const BUTTON: f32 = 14.0;

fn inside(p: (f32, f32), x: f32, y: f32, w: f32, h: f32) -> bool {
    p.0 >= x && p.1 >= y && p.0 < x + w && p.1 < y + h
}
impl crate::game::Sim<'_> {
    /// `LoomScreen.containerChanged`: the error flag, whether the pattern
    /// buttons show (only when an input changed), and the scroller reset.
    pub(super) fn loom_screen_changed(&mut self) {
        let Some(s) = self.station.as_mut().filter(|s| s.kind == Kind::Loom) else {
            return;
        };
        let inputs = [
            s.inputs[0].clone(),
            s.inputs[1].clone(),
            s.inputs[2].clone(),
        ];
        let l = &mut s.loom;
        l.has_max = inputs[0].as_ref().is_some_and(banner::has_max_layers);
        if inputs != l.last_inputs {
            l.display =
                inputs[0].is_some() && inputs[1].is_some() && !l.has_max && !l.patterns.is_empty();
        }
        if l.start_row >= l.total_rows() {
            l.start_row = 0;
            l.scroll = 0.0;
        }
        l.last_inputs = inputs;
    }
    /// A sound the loom or stonecutter menu played: the result's once per
    /// tick (`lastSoundTime`).
    pub(super) fn station_sound(&mut self, sound: &str) {
        let ticks = self.ticks;
        let Some(s) = self.station.as_mut() else {
            return;
        };
        if s.last_sound == ticks {
            return;
        }
        s.last_sound = ticks;
        let pos = s.pos;
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        self.sounds.play(sound, Some(at), 1.0, 1.0);
    }
    /// The pattern buttons a controller can focus (GUI origin relative).
    pub(super) fn loom_targets(&self) -> Vec<[f32; 4]> {
        let Some(loom) = self.station.as_ref().map(|s| &s.loom).filter(|l| l.display) else {
            return Vec::new();
        };
        (0..16)
            .filter(|i| (i / 4 + loom.start_row) * 4 + i % 4 < loom.patterns.len())
            .map(|i| {
                let (row, column) = (i / 4, i % 4);
                [
                    60.0 + column as f32 * BUTTON,
                    13.0 + row as f32 * BUTTON,
                    BUTTON,
                    BUTTON,
                ]
            })
            .collect()
    }
    pub(super) fn loom_release(&mut self) {
        if let Some(s) = self.station.as_mut() {
            s.loom.scrolling = false;
        }
    }
    /// `LoomScreen.mouseScrolled`.
    pub(super) fn loom_scroll(&mut self, d: f32) {
        let Some(l) = self.station.as_mut().map(|s| &mut s.loom) else {
            return;
        };
        let offscreen = l.offscreen_rows();
        if l.display && offscreen > 0 {
            l.scroll = (l.scroll - d / offscreen as f32).clamp(0.0, 1.0);
            l.start_row = l.row_for(l.scroll);
        }
    }
}
