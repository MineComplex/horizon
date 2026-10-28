//! The pause menu (PauseScreen) and death screen (DeathScreen) layouts.
use super::{Game, Screen};
use horizon_client::{
    Result,
    confirm_screen::Confirm,
    hud::{HudBatch, rgba},
    text::{self, Text},
    ui,
};

impl Screen {
    /// Screen.extractBackground for non-isInGameUi screens, and
    /// PauseScreen.extractBackground only when showPauseMenu is true.
    pub(super) fn blurs_background(self, pause_menu: bool) -> bool {
        match self {
            Self::Paused => pause_menu,
            Self::Options
            | Self::Stats
            | Self::Advancements
            | Self::ServerLinks
            | Self::ConfirmLink
            | Self::DialogWarning => true,
            Self::Playing
            | Self::GameModeSwitcher
            | Self::Chat
            | Self::Creative
            | Self::Inventory
            | Self::Dead
            | Self::CommandBlock => false,
            // WinScreen.extractBackground: the poem fills the screen with the End portal, the
            // credits alone blur. Game::menu_blur knows which one rolls.
            Self::Win => false,
        }
    }
}

/// PauseScreen buttons: (x, y, width, label key, action id, active).
/// `links`: the `minecraft:server_links` dialog's button, which the
/// `pause_screen_additions` tag (empty in vanilla) falls back to when the
/// server sent links — a full-width row that pushes the last two rows down.
/// https://mcsrc.dev/2/26.3/net/minecraft/client/gui/screens/PauseScreen
fn pause_buttons(
    w: f32,
    h: f32,
    remote: bool,
    links: bool,
) -> Vec<(f32, f32, f32, &'static str, u8, bool)> {
    // GridLayout of two 98px columns (padding 4), aligned at (0.5, 0.25).
    let (grid_w, grid_h) = (212.0, 166.0 + if links { 24.0 } else { 0.0 });
    let x0 = ((w - grid_w) * 0.5).floor();
    let y0 = ((h - grid_h) * 0.25).floor();
    let down = if links { 24.0 } else { 0.0 };
    let mut v = vec![
        (x0 + 4.0, y0 + 50.0, 204.0, "menu.returnToGame", 0, true),
        (x0 + 4.0, y0 + 74.0, 98.0, "gui.advancements", 4, true),
        (x0 + 110.0, y0 + 74.0, 98.0, "gui.stats", 3, true),
    ];
    if links {
        // CommonDialogData.computeExternalTitle of Dialogs.SERVER_LINKS.
        v.push((x0 + 4.0, y0 + 122.0, 204.0, "menu.server_links", 11, true));
    }
    v.extend([
        // No world options, Open to LAN or reporting: the game only joins servers, and their
        // accounts are the launcher's.
        (x0 + 4.0, y0 + 122.0 + down, 204.0, "menu.options", 1, true),
        (
            x0 + 4.0,
            y0 + 146.0 + down,
            204.0,
            if remote {
                "menu.disconnect"
            } else {
                "menu.returnToMenu"
            },
            2,
            true,
        ),
    ]);
    v
}

/// DeathScreen.init: a hardcore player can only spectate.
/// `DeathScreen.TitleConfirmScreen`: its title and the yes and no labels.
fn death_confirm_texts() -> (Text, [Text; 2]) {
    let t = |key: &str| Text::translate(key, &[]);
    (
        t("deathScreen.quit.confirm"),
        [t("deathScreen.titleScreen"), t("deathScreen.respawn")],
    )
}
fn death_buttons(w: f32, h: f32, hardcore: bool) -> [(f32, f32, &'static str, u8); 2] {
    let respawn = if hardcore {
        "deathScreen.spectate"
    } else {
        "deathScreen.respawn"
    };
    [
        (w / 2.0 - 100.0, h / 4.0 + 72.0, respawn, 0),
        (
            w / 2.0 - 100.0,
            h / 4.0 + 96.0,
            "deathScreen.titleScreen",
            1,
        ),
    ]
}
impl Game<'_> {
    pub(super) fn draw_pause_menu(&mut self, batch: &mut HudBatch) {
        let atlas = &self.front.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        ui::menu_background(batch, atlas, 0.0, 0.0, w, h, true);
        let title = Text::translate("menu.game", &[]);
        let tw = atlas.font.width_text(&title);
        batch.text_styled(
            atlas,
            &title,
            ((w - tw) / 2.0).floor(),
            40.0,
            u32::MAX,
            true,
        );
        let links = !self.server_links().is_empty();
        let mut tooltip = None;
        for (x, y, bw, key, id, active) in pause_buttons(w, h, self.external_server(), links) {
            let hover = active && ui::hovered(self.front.cursor, x, y, bw, 20.0);
            ui::button(
                batch,
                atlas,
                x,
                y,
                bw,
                20.0,
                &Text::translate(key, &[]),
                active,
                hover,
            );
            // PauseScreen's customAdditions button: CUSTOM_OPTIONS_TOOLTIP.
            if id == 11 && hover {
                tooltip = Some(Text::translate("menu.custom_options.tooltip", &[]));
            }
        }
        // SpriteIconButton.withTootip: the label as a tooltip.
        if let Some(tip) = tooltip {
            let lines: Vec<Text> = atlas
                .font
                .split_lines(&tip, batch.width)
                .into_iter()
                .map(|(line, _)| line)
                .collect();
            ui::tooltip(batch, atlas, &lines, self.front.cursor);
        }
    }
    pub(super) fn pause_click(&mut self) -> Result<()> {
        let (w, h) = {
            let size = self
                .front
                .renderer
                .as_ref()
                .map_or([1, 1], |r| [r.config.width, r.config.height]);
            let b = HudBatch::new(size);
            (b.width, b.height)
        };
        let links = !self.server_links().is_empty();
        let hit = pause_buttons(w, h, self.external_server(), links)
            .into_iter()
            .find(|b| b.5 && ui::hovered(self.front.cursor, b.0, b.1, b.2, 20.0))
            .map(|b| b.4);
        match hit {
            Some(0) => {
                self.screen = Screen::Playing;
                self.capture(true);
            }
            Some(1) => self.open_options(false),
            Some(3) => self.open_stats(),
            Some(4) => self.open_advancements(true),
            // DialogConnectionAccess.openDialog(Dialogs.SERVER_LINKS).
            Some(11) => {
                self.links_ui.scroll = 0.0;
                self.screen = Screen::ServerLinks;
            }
            Some(2) => self.close_world()?,
            _ => {}
        }
        if hit.is_some() {
            self.front.audio.play_button_click();
        }
        Ok(())
    }
    /// DeathScreen: gradient, the title at double size, the cause and score.
    pub(super) fn draw_death_screen(&mut self, batch: &mut HudBatch) {
        let atlas = &self.front.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        // fillGradient(0x60500000 -> 0xA0803030).
        let rows = h.ceil() as i32;
        for y in 0..rows {
            let t = y as f32 / h.max(1.0);
            let lerp = |a: f32, b: f32| (a + (b - a) * t) as u8;
            batch.fill(
                atlas,
                0.0,
                y as f32,
                w,
                1.0,
                rgba(
                    lerp(80.0, 128.0),
                    lerp(0.0, 48.0),
                    lerp(0.0, 48.0),
                    lerp(96.0, 160.0),
                ),
            );
        }
        // DeathScreen.handleExitToTitleScreen: the TitleConfirmScreen takes the
        // screen over, on the same background.
        if let Some(opened) = self.ui.death_confirm {
            let (title, labels) = death_confirm_texts();
            let size = self.gui_size();
            Confirm::new(&atlas.font, size, &title, &Text::literal("")).draw_active(
                batch,
                atlas,
                self.front.cursor,
                [&labels[0], &labels[1]],
                opened.elapsed().as_millis() >= 1000,
            );
            return;
        }
        let title = if self.level.hardcore {
            "deathScreen.title.hardcore"
        } else {
            "deathScreen.title"
        };
        let title = Text::translate(title, &[]);
        let tw = atlas.font.width_text(&title);
        ui::text_scaled(
            batch,
            atlas,
            &title,
            (w / 2.0 - tw).floor(),
            60.0,
            2.0,
            u32::MAX,
        );
        if let Some(cause) = &self.ui.death_text {
            let cw = atlas.font.width_text(cause);
            batch.text_styled(atlas, cause, ((w - cw) / 2.0).floor(), 85.0, u32::MAX, true);
        }
        let score = Text::translate(
            "deathScreen.score.value",
            &[Text::colored(
                self.experience.total.to_string(),
                text::YELLOW,
            )],
        );
        let sw = atlas.font.width_text(&score);
        batch.text_styled(
            atlas,
            &score,
            ((w - sw) / 2.0).floor(),
            100.0,
            u32::MAX,
            true,
        );
        let active = self
            .ui
            .died_at
            .is_none_or(|t| t.elapsed().as_millis() >= 1000);
        for (x, y, key, _) in death_buttons(w, h, self.level.hardcore) {
            let hover = active && ui::hovered(self.front.cursor, x, y, 200.0, 20.0);
            ui::button(
                batch,
                atlas,
                x,
                y,
                200.0,
                20.0,
                &Text::translate(key, &[]),
                active,
                hover,
            );
        }
    }
    /// LocalPlayer.respawn: the player comes back (the death screen's
    /// button, or at once with `immediate_respawn`).
    pub(super) fn respawn_player(&mut self) -> Result<()> {
        // ClientCommand PERFORM_RESPAWN: the server respawns the player
        // (this window is always a server's client).
        self.ui.death_text = None;
        self.front.toggles.respawn();
        self.remote_respawn();
        Ok(())
    }
    pub(super) fn death_click(&mut self) -> Result<()> {
        if self
            .ui
            .died_at
            .is_some_and(|t| t.elapsed().as_millis() < 1000)
        {
            return Ok(());
        }
        if let Some(opened) = self.ui.death_confirm {
            // ConfirmScreen.setDelay: both buttons wait 20 ticks.
            if opened.elapsed().as_millis() < 1000 {
                return Ok(());
            }
            let (title, _) = death_confirm_texts();
            let confirm = Confirm::new(
                &self.front.gui.atlas.font,
                self.gui_size(),
                &title,
                &Text::literal(""),
            );
            if let Some(yes) = confirm.answer_at(self.front.cursor) {
                self.death_confirm_answer(yes)?;
            }
            return Ok(());
        }
        let size = self
            .front
            .renderer
            .as_ref()
            .map_or([1, 1], |r| [r.config.width, r.config.height]);
        let b = HudBatch::new(size);
        let hit = death_buttons(b.width, b.height, self.level.hardcore)
            .into_iter()
            .find(|d| ui::hovered(self.front.cursor, d.0, d.1, 200.0, 20.0))
            .map(|d| d.3);
        // handleExitToTitleScreen: outside hardcore, Title Screen asks first.
        if hit == Some(1) && !self.level.hardcore {
            self.ui.death_confirm = Some(std::time::Instant::now());
            self.front.audio.play_button_click();
            return Ok(());
        }
        self.death_action(hit)
    }
    /// The confirm screen's answer: yes leaves the world, no respawns
    /// (`DeathScreen.handleExitToTitleScreen`'s callback).
    pub(super) fn death_confirm_answer(&mut self, yes: bool) -> Result<()> {
        self.ui.death_confirm = None;
        if yes {
            self.front.audio.play_button_click();
            self.close_world()
        } else {
            self.death_action(Some(0))
        }
    }
    /// A death screen button: 0 respawns (or spectates in hardcore), 1 leaves.
    fn death_action(&mut self, hit: Option<u8>) -> Result<()> {
        match hit {
            Some(0) => self.respawn_player()?,
            Some(1) => self.close_world()?,
            _ => {}
        }
        if hit.is_some() {
            self.front.audio.play_button_click();
        }
        Ok(())
    }
}
impl crate::game::Sim<'_> {
    /// The window plays on a server (it has no world of its own).
    pub(super) fn external_server(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{Screen, pause_buttons};

    #[test]
    fn menu_blur_matches_vanilla_screen_backgrounds() {
        for screen in [Screen::Options, Screen::Stats, Screen::Advancements] {
            assert!(screen.blurs_background(false));
            assert!(screen.blurs_background(true));
        }
        assert!(Screen::Paused.blurs_background(true));
        assert!(!Screen::Paused.blurs_background(false));
        for screen in [
            Screen::Playing,
            Screen::GameModeSwitcher,
            Screen::Chat,
            Screen::Creative,
            Screen::Inventory,
            Screen::Dead,
            Screen::CommandBlock,
            Screen::Win,
        ] {
            assert!(!screen.blurs_background(true));
            assert!(!screen.blurs_background(false));
        }
    }

    #[test]
    fn pause_menu_uses_disconnect_for_servers_and_has_no_world_options() {
        for (remote, label) in [(false, "menu.returnToMenu"), (true, "menu.disconnect")] {
            let buttons = pause_buttons(427.0, 240.0, remote, false);
            let quit = buttons.iter().find(|b| b.4 == 2).unwrap();
            assert_eq!(quit.3, label);
            assert!(quit.5);
            assert!(!buttons.iter().any(|b| b.4 == 9));
            let options = buttons.iter().find(|b| b.4 == 1).unwrap();
            assert_eq!(options.2, 204.0);
        }
    }

    #[test]
    fn server_links_adds_a_full_width_row_above_options() {
        let with = pause_buttons(427.0, 240.0, true, true);
        let without = pause_buttons(427.0, 240.0, true, false);
        let links = with.iter().find(|b| b.4 == 11).unwrap();
        assert_eq!(links.2, 204.0);
        assert_eq!(links.3, "menu.server_links");
        assert!(links.5);
        // Options and Disconnect move down 24.
        for b in &with {
            if b.4 == 1 || b.4 == 2 {
                let same = without.iter().find(|o| o.4 == b.4).unwrap();
                assert_eq!(b.1, same.1 + 24.0 - 6.0, "{}", b.3);
            }
        }
    }
}
