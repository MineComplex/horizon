//! `WinScreen` in the game: the credits from Credits & Attribution (and
//! `horizon-ctl credits`). A server's `WIN_GAME` shows nothing here: the
//! client respawns at once (game/remote.rs).
use super::{Game, Screen};
use horizon_client::{
    Result,
    hud::{EndPortalFill, HudBatch},
    win_screen::{Key, WinScreen},
};
use std::time::Instant;
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{KeyCode, PhysicalKey},
};

/// What closing the screen does (`WinScreen.onFinished`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Finish {
    /// Back to Credits & Attribution.
    Options,
    /// Back to the game (`horizon-ctl credits`).
    Closed,
}
pub(super) struct Win {
    screen: WinScreen,
    finish: Finish,
    /// The last drawn frame, for the scroll's game-time delta.
    frame: Instant,
}
impl Game<'_> {
    pub(super) fn open_win_screen(&mut self, poem: bool, finish: Finish) {
        let screen = WinScreen::new(
            poem,
            &self.options.resources,
            &self.options.player_name(),
            &self.front.gui.atlas.font,
        );
        self.win = Some(Win {
            screen,
            finish,
            frame: Instant::now(),
        });
        self.keys.clear();
        self.screen = Screen::Win;
        self.capture(false);
    }
    pub(super) fn draw_win_screen(&mut self, batch: &mut HudBatch) {
        let Some(win) = self.sim.win.as_mut() else {
            return;
        };
        let now = Instant::now();
        // DeltaTracker.getGameTimeDeltaTicks: the frame's length in ticks.
        let delta_ticks = now.duration_since(win.frame).as_secs_f32()
            / self.sim.clock.tick_duration().as_secs_f32();
        win.frame = now;
        let sprite = |n: &str| {
            self.front
                .gui
                .atlas
                .sprites
                .get(n)
                .map_or([0.0; 4], |r| [r.u0, r.v0, r.u1 - r.u0, r.v1 - r.v0])
        };
        let fill = EndPortalFill {
            sprites: [
                sprite("environment/end_sky"),
                sprite("entity/end_portal/end_portal"),
            ],
            game_time: (self.sim.world.game_time % 24000) as f32 / 24000.0,
        };
        win.screen
            .draw(batch, &self.front.gui.atlas, delta_ticks, fill);
    }
    /// `WinScreen.keyPressed` / `keyReleased`; Escape closes it (`onClose`).
    pub(super) fn win_key(&mut self, event: &KeyEvent) -> Result<()> {
        let PhysicalKey::Code(code) = event.physical_key else {
            return Ok(());
        };
        let pressed = event.state == ElementState::Pressed;
        if code == KeyCode::Escape && pressed {
            return self.close_win_screen();
        }
        let Some(key) = Key::from_code(code) else {
            return Ok(());
        };
        if let Some(win) = self.win.as_mut() {
            win.screen.key(key, pressed);
        }
        Ok(())
    }
    /// `WinScreen.tick`: done once everything has rolled past the top.
    pub(super) fn tick_win_screen(&mut self) -> Result<()> {
        let height = self.gui_size().1;
        if self.win.as_ref().is_some_and(|w| w.screen.finished(height)) {
            self.close_win_screen()?;
        }
        Ok(())
    }
    /// `WinScreen.respawn` (`onFinished`) and `removed`: the credits music stops.
    pub(super) fn close_win_screen(&mut self) -> Result<()> {
        let Some(win) = self.win.take() else {
            return Ok(());
        };
        let credits = horizon_client::music::Music::credits();
        self.front
            .music
            .stop_playing_music(&credits, None, &mut self.front.audio);
        match win.finish {
            Finish::Options => self.screen = Screen::Options,
            Finish::Closed if self.front.title.is_some() => self.screen = Screen::Playing,
            Finish::Closed => {
                self.screen = Screen::Playing;
                self.capture(true);
            }
        }
        Ok(())
    }
}
impl crate::game::Sim<'_> {
    /// Stop the scroll `scroll` pixels in (`horizon-ctl credits ... SCROLL`).
    pub(super) fn hold_win_screen(&mut self, scroll: f32) {
        if let Some(win) = self.win.as_mut() {
            win.screen.hold(scroll);
        }
    }
    /// `WinScreen.isPauseScreen`: only the credits without the poem pause.
    pub(super) fn win_screen_pauses(&self) -> bool {
        self.win.as_ref().is_some_and(|w| !w.screen.poem)
    }
}
