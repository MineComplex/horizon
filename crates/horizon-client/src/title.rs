//! The title screen (TitleScreen) and what it opens: Multiplayer and its screens, drawn as HUD
//! quads. This fork has no single player: the game only joins servers.
use crate::{
    hud::{HudAtlas, HudBatch, rgba},
    lang,
};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;
pub use horizon_game::world_options::{DIFFICULTIES, FlatLayer, FlatSettings, WorldOptions};

pub(crate) fn confirm_top(h: f32, lines: usize) -> f32 {
    ((h.ceil() - 61.0 - lines as f32 * 9.0) / 2.0).floor()
}

#[derive(Clone, Debug)]
pub enum Mode {
    Main,
    Error {
        message: String,
        return_to: Box<Mode>,
    },
    Multiplayer(crate::multiplayer::Screen),
}
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    Quit,
    Options,
    Accessibility,
    Language,
    Connect { address: String },
}
pub struct Title {
    pub mode: Mode,
    /// Direct Connection's address, synchronized with `options.txt`'s `lastServer`.
    pub last_server: String,
    /// The game folder's saves, beside which the server list lives.
    pub saves: PathBuf,
    pub started: std::time::Instant,
    /// The focused text field's cursor, selection and scroll.
    caret: RefCell<crate::ui::Caret>,
    message_lines: Cell<usize>,
    splash: Option<String>,
}
pub(crate) struct Button {
    x: f32,
    y: f32,
    w: f32,
    label: String,
    enabled: bool,
    id: u8,
    icon: Option<(&'static str, f32)>,
}
fn choose_splash(text: &str, index: usize, date: (u32, u32)) -> Option<String> {
    // SplashManager checks SpecialDates before the resource list (local date).
    let special = match date {
        (12, 24) => Some("Merry X-mas!"),
        (1, 1) => Some("Happy new year!"),
        (10, 31) => Some("OOoooOOOoooo! Spooky!"),
        _ => None,
    };
    if let Some(special) = special {
        return Some(special.to_owned());
    }
    let splashes: Vec<_> = text
        .lines()
        .map(str::trim)
        // SplashManager.prepare keeps blank lines: only the hidden splash is dropped.
        .filter(|s| horizon_core::random::java_string_hash(s.encode_utf16()) != 125780783)
        .collect();
    splashes
        .get(index.checked_rem(splashes.len())?)
        .map(|s| (*s).to_owned())
}

impl Title {
    pub fn new(saves: PathBuf, resources: &Path) -> Self {
        use chrono::Datelike;
        let today = chrono::Local::now();
        Self {
            mode: Mode::Main,
            last_server: String::new(),
            saves,
            started: std::time::Instant::now(),
            caret: RefCell::default(),
            message_lines: Cell::new(1),
            splash: choose_splash(
                &std::fs::read_to_string(resources.join("assets/minecraft/texts/splashes.txt"))
                    .unwrap_or_default(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as usize,
                (today.month(), today.day()),
            ),
        }
    }
    /// The pointer moved with the button held (`mouseDragged`).
    pub fn drag(&mut self, cursor: (f32, f32), size: (f32, f32)) {
        if let Mode::Multiplayer(screen) = &mut self.mode {
            screen.drag(cursor.1, size);
        }
    }
    /// The button was released (`onRelease`).
    pub fn release(&mut self) {
        if let Mode::Multiplayer(screen) = &mut self.mode {
            screen.release();
        }
    }
    /// Mouse wheel over the server list.
    pub fn scroll_worlds(&mut self, delta: f32, cursor: (f32, f32), size: (f32, f32)) {
        if let Mode::Multiplayer(screen) = &mut self.mode {
            screen.wheel(delta, cursor, size);
        }
    }
    pub fn failed(&mut self, error: impl std::fmt::Display) {
        let return_to = Box::new(self.mode.clone());
        self.mode = Mode::Error {
            message: error.to_string(),
            return_to,
        };
    }
    fn buttons(&self, w: f32, h: f32) -> Vec<Button> {
        let (w, h) = (w.ceil(), h.ceil());
        let b = |x: f32, y: f32, w: f32, label: &str, enabled: bool, id: u8| Button {
            x,
            y,
            w,
            label: label.into(),
            enabled,
            id,
            icon: None,
        };
        match &self.mode {
            Mode::Main => {
                // TitleScreen without Singleplayer and Realms (this fork only joins its servers).
                let cx = (w / 2.0).floor();
                let y = (h / 4.0).floor() + 48.0;
                let mut buttons = vec![
                    b(
                        cx - 100.0,
                        y,
                        200.0,
                        &lang::get_or("menu.multiplayer", "Multiplayer"),
                        true,
                        4,
                    ),
                    b(
                        cx - 100.0,
                        y + 72.0,
                        98.0,
                        &lang::get_or("menu.options", "Options..."),
                        true,
                        3,
                    ),
                    b(
                        cx + 2.0,
                        y + 72.0,
                        98.0,
                        &lang::get_or("menu.quit", "Quit Game"),
                        true,
                        2,
                    ),
                ];
                // CommonButtons' icon buttons.
                for (i, (key, plain, id, icon)) in [
                    ("options.language", "Language...", 6, "icon/language"),
                    (
                        "options.accessibility",
                        "Accessibility Settings...",
                        7,
                        "icon/accessibility",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut button = b(
                        cx - 22.0 + i as f32 * 24.0,
                        y + 48.0,
                        20.0,
                        &lang::get_or(key, plain),
                        true,
                        id,
                    );
                    button.icon = Some((icon, 15.0));
                    buttons.push(button);
                }
                buttons
            }
            Mode::Error { .. } => vec![b(
                (w / 2.0).floor() - 75.0,
                confirm_top(h, self.message_lines.get())
                    + 41.0
                    + self.message_lines.get() as f32 * 9.0,
                150.0,
                &lang::get_or("gui.back", "Back"),
                true,
                39,
            )],
            Mode::Multiplayer(screen) => screen
                .buttons(w, h)
                .into_iter()
                .map(|(x, y, bw, label, enabled, id)| b(x, y, bw, &label, enabled, id))
                .collect(),
        }
    }
    pub fn draw(
        &self,
        atlas: &HudAtlas,
        batch: &mut HudBatch,
        cursor: (f32, f32),
        hide_splash: bool,
    ) {
        let (w, h) = (batch.width.ceil(), batch.height.ceil());
        match &self.mode {
            Mode::Main => {
                // LogoRenderer: the logo's top 44 of 64 rows, then the edition
                // banner's top 14 of 16 rows overlapping its bottom by 7.
                batch.sprite_region(
                    atlas,
                    "gui/title/minecraft",
                    [(w / 2.0).floor() - 128.0, 30.0, 256.0, 44.0],
                    256.0,
                    64.0,
                );
                batch.sprite_region(
                    atlas,
                    crate::title_edition::SPRITE,
                    [(w / 2.0).floor() - 64.0, 30.0 + 44.0 - 7.0, 128.0, 14.0],
                    128.0,
                    16.0,
                );
                if !hide_splash && let Some(splash) = self.splash.as_deref() {
                    let tw = HudBatch::text_width(atlas, splash);
                    let start = batch.vertices.len();
                    batch.text(
                        atlas,
                        splash,
                        -(tw / 2.0).floor(),
                        -8.0,
                        rgba(255, 255, 0, 255),
                        true,
                    );
                    let scale = (1.8
                        - (self.started.elapsed().as_secs_f32() * std::f32::consts::TAU)
                            .sin()
                            .abs()
                            * 0.1)
                        * 100.0
                        / (tw + 32.0);
                    let (sin, cos) = (-20.0f32).to_radians().sin_cos();
                    for v in &mut batch.vertices[start..] {
                        let [x, y] = v.pos;
                        v.pos = [
                            w / 2.0 + 123.0 + scale * (x * cos - y * sin),
                            69.0 + scale * (x * sin + y * cos),
                        ];
                    }
                }
                let version = format!("Horizon {} (Rust)", horizon_core::VANILLA_VERSION);
                batch.text(atlas, &version, 2.0, h - 10.0, u32::MAX, true);
                // A nightly names its build in the top-left corner, the one spot no button reaches at any GUI
                // scale, so screenshots and reports say which build they come from.
                if let Some(nightly) = horizon_core::release::nightly_label() {
                    batch.text(atlas, &nightly, 2.0, 2.0, u32::MAX, true);
                }
                let notice = "Not affiliated with Mojang or Microsoft";
                let tw = HudBatch::text_width(atlas, notice);
                batch.text(atlas, notice, w - tw - 2.0, h - 10.0, u32::MAX, true);
            }
            Mode::Error { message, .. } => {
                crate::ui::menu_background(batch, atlas, 0.0, 0.0, w, h, false);
                let lines = wrapped(atlas, message, w - 50.0);
                self.message_lines.set(lines.len());
                let top = confirm_top(h, lines.len());
                centered(
                    atlas,
                    batch,
                    &lang::get_or("selectWorld.access_failure", "Failed to access world"),
                    top,
                    u32::MAX,
                );
                for (i, line) in lines.iter().enumerate() {
                    centered(atlas, batch, line, top + 17.0 + i as f32 * 9.0, u32::MAX);
                }
            }
            Mode::Multiplayer(screen) => screen.draw(
                atlas,
                batch,
                &mut self.caret.borrow_mut(),
                self.started,
                cursor,
            ),
        }
        for b in self.buttons(w, h) {
            crate::widgets::record(&b.label, [b.x, b.y, b.w, 20.0], b.enabled);
            let hover = b.enabled
                && cursor.0 >= b.x
                && cursor.0 < b.x + b.w
                && cursor.1 >= b.y
                && cursor.1 < b.y + 20.0;
            let sprite = if !b.enabled {
                "gui/sprites/widget/button_disabled"
            } else if hover {
                "gui/sprites/widget/button_highlighted"
            } else {
                "gui/sprites/widget/button"
            };
            // Nine-slice approximation: left and right halves of the 200-wide sprite.
            if let Some(r) = atlas.sprites.get(sprite).copied() {
                let half = b.w / 2.0;
                let mut left = r;
                left.u1 = r.u0 + (r.u1 - r.u0) * (half / r.w as f32);
                batch.rect_uv(b.x, b.y, half, 20.0, left, u32::MAX);
                let mut right = r;
                right.u0 = r.u1 - (r.u1 - r.u0) * (half / r.w as f32);
                batch.rect_uv(b.x + half, b.y, half, 20.0, right, u32::MAX);
            }
            let color = if b.enabled {
                u32::MAX
            } else {
                rgba(160, 160, 160, 255)
            };
            if let Some((icon, size)) = b.icon {
                // CenteredIcon divides the button and sprite sizes separately:
                // a 15px icon in a 20px button starts at 10 - 7 = 3, not 2.
                batch.sprite_scaled(
                    atlas,
                    &format!("gui/sprites/{icon}"),
                    b.x + (b.w / 2.0).floor() - (size / 2.0).floor(),
                    b.y + 10.0 - (size / 2.0).floor(),
                    size,
                    size,
                    u32::MAX,
                );
                continue;
            }
            let tw = HudBatch::text_width(atlas, &b.label);
            batch.text(
                atlas,
                &b.label,
                b.x + (b.w / 2.0).floor() - (tw.ceil() / 2.0).floor(),
                b.y + 6.0,
                color,
                true,
            );
        }
        if let Mode::Multiplayer(screen) = &self.mode {
            screen.draw_tooltip(atlas, batch, cursor);
        }
    }
    /// The action and whether to play `AbstractWidget.playButtonClickSound`.
    /// Navigation buttons can return `Action::None` while still sounding.
    pub fn click(&mut self, x: f32, y: f32, size: (f32, f32)) -> (Action, bool) {
        self.remember_direct_address();
        let (w, h) = (size.0.ceil(), size.1.ceil());
        let hit = self
            .buttons(w, h)
            .into_iter()
            .find(|b| b.enabled && x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + 20.0)
            .map(|b| b.id);
        let action = self.click_action(x, y, (w, h), hit);
        (action, hit.is_some())
    }
    fn click_action(&mut self, x: f32, y: f32, (w, h): (f32, f32), hit: Option<u8>) -> Action {
        match (&mut self.mode, hit) {
            (Mode::Main, Some(2)) => Action::Quit,
            (Mode::Main, Some(4)) => {
                self.mode = Mode::Multiplayer(crate::multiplayer::Screen::list(&self.saves));
                Action::None
            }
            (Mode::Multiplayer(_), hit) => match self.multiplayer_click(hit, x, y, (w, h)) {
                crate::multiplayer::Outcome::Back => {
                    self.mode = Mode::Main;
                    Action::None
                }
                crate::multiplayer::Outcome::Connect(address) => Action::Connect { address },
                crate::multiplayer::Outcome::None => Action::None,
            },
            (Mode::Main, Some(3)) => Action::Options,
            (Mode::Main, Some(7)) => Action::Accessibility,
            (Mode::Main, Some(6)) => Action::Language,
            (Mode::Error { return_to, .. }, Some(39)) => {
                self.mode = *return_to.clone();
                Action::None
            }
            _ => Action::None,
        }
    }
    /// The focused text field, its id and maximum length (copy and paste).
    pub fn focused_field(&mut self) -> Option<(crate::clipboard::FieldId, &mut String, usize)> {
        caret_field(&mut self.mode)
    }
    /// Whether the focused field edits at its cursor.
    pub fn has_caret_field(&mut self) -> bool {
        caret_field(&mut self.mode).is_some()
    }
    /// Edit the focused field through its `EditBox`; false when there is none.
    fn edit_field(&mut self, f: impl FnOnce(&mut crate::ui::EditBox)) -> bool {
        let Some((id, value, max)) = caret_field(&mut self.mode) else {
            return false;
        };
        self.caret.get_mut().edit(id, value, max, f);
        true
    }
    /// EditBox.charTyped / insertText at the cursor.
    pub fn type_text(&mut self, text: &str) {
        self.edit_field(|edit| edit.insert_text(text));
    }
    pub fn backspace(&mut self) {
        self.field_key(winit::keyboard::KeyCode::Backspace, false, false);
    }
    /// EditBox.keyPressed: moving and deleting in the focused field.
    pub fn field_key(&mut self, key: winit::keyboard::KeyCode, ctrl: bool, shift: bool) -> bool {
        let mut handled = false;
        self.edit_field(|edit| handled = edit.key(key, ctrl, shift));
        handled
    }
    /// Ctrl+A/C/V/X in the focused field.
    pub fn field_shortcut(
        &mut self,
        shortcut: crate::clipboard::EditShortcut,
        board: &mut dyn crate::clipboard::Clipboard,
    ) {
        self.edit_field(|edit| {
            edit.shortcut(shortcut, board);
        });
    }
    /// The focused field's box on screen.
    fn focused_rect(&self, size: (f32, f32)) -> Option<[f32; 4]> {
        match &self.mode {
            Mode::Multiplayer(screen) => screen.focused_box(size),
            _ => None,
        }
    }
    /// A left press after `click` focused a field: the cursor goes under the
    /// pointer (`EditBox.onClick`).
    pub fn press_field(
        &mut self,
        atlas: &HudAtlas,
        (x, y): (f32, f32),
        size: (f32, f32),
        shift: bool,
    ) {
        let (w, h) = (size.0.ceil(), size.1.ceil());
        let Some(rect @ [fx, fy, fw, fh]) = self.focused_rect((w, h)) else {
            return;
        };
        if !crate::ui::hovered((x, y), fx, fy, fw, fh) {
            return;
        }
        if let Some(field) = caret_field(&mut self.mode) {
            self.caret.get_mut().press(atlas, field, rect, x, shift);
        }
    }
    /// The pointer moved with the button held (`EditBox.onDrag`).
    pub fn drag_field(&mut self, atlas: &HudAtlas, x: f32) {
        if let Some(field) = caret_field(&mut self.mode) {
            self.caret.get_mut().drag(atlas, field, x);
        }
    }
    /// The button was released: drags stop selecting.
    pub fn release_field(&mut self) {
        self.caret.get_mut().release();
    }
    pub fn tab(&mut self) {
        if let Mode::Multiplayer(screen) = &mut self.mode {
            screen.tab();
        }
    }
    /// The server auto-reconnect retries now, once its countdown is over.
    pub fn due_retry(&mut self) -> Option<crate::reconnect::Retry> {
        match &mut self.mode {
            Mode::Multiplayer(screen) => screen.due_retry(),
            _ => None,
        }
    }
    pub(crate) fn remember_direct_address(&mut self) {
        if let Mode::Multiplayer(crate::multiplayer::Screen::Direct { address }) = &self.mode {
            self.last_server.clone_from(address);
        }
    }
    pub(crate) fn multiplayer_click(
        &mut self,
        hit: Option<u8>,
        x: f32,
        y: f32,
        size: (f32, f32),
    ) -> crate::multiplayer::Outcome {
        let Mode::Multiplayer(screen) = &mut self.mode else {
            return crate::multiplayer::Outcome::None;
        };
        let outcome = screen.click(hit, x, y, size, &self.saves);
        if hit == Some(41)
            && let crate::multiplayer::Screen::Direct { address } = screen
        {
            address.clone_from(&self.last_server);
        }
        outcome
    }
    pub fn escape(&mut self) {
        self.remember_direct_address();
        if let Mode::Multiplayer(screen) = &mut self.mode
            && !screen.escape(&self.saves)
        {
            return;
        }
        match &self.mode {
            Mode::Error { return_to, .. } => self.mode = *return_to.clone(),
            _ => self.mode = Mode::Main,
        }
    }
}
fn centered(atlas: &HudAtlas, batch: &mut HudBatch, text: &str, y: f32, color: u32) {
    batch.text(
        atlas,
        text,
        (batch.width.ceil() / 2.0).floor()
            - (HudBatch::text_width(atlas, text).ceil() / 2.0).floor(),
        y,
        color,
        true,
    );
}
fn wrapped(atlas: &HudAtlas, text: &str, width: f32) -> Vec<String> {
    atlas
        .font
        .split(&crate::text::Text::literal(text), width.max(1.0))
        .iter()
        .map(|t| t.plain())
        .collect()
}
/// The focused field of the world screens and the server screens: the list's
/// search, a world name or seed, or Add Server, Direct Connection and Join a
/// Friend.
fn caret_field(mode: &mut Mode) -> Option<(crate::clipboard::FieldId, &mut String, usize)> {
    match mode {
        Mode::Multiplayer(screen) => screen.focused_text(),
        _ => None,
    }
}

#[cfg(test)]
mod classic_tests {
    use super::*;

    #[test]
    fn classic_main_menu_icons_route_language_and_accessibility() {
        let mut title = Title::new(
            PathBuf::new(),
            Path::new("/nonexistent/horizon-test-resources"),
        );
        for (w, h) in [(960.0, 523.0), (480.0, 320.0), (427.0, 240.0)] {
            let buttons = title.buttons(w, h);
            let cx = (w / 2.0).floor();
            let y = (h / 4.0).floor() + 48.0;
            for (id, x, enabled, icon) in [
                (6, cx - 22.0, true, "icon/language"),
                (7, cx + 2.0, true, "icon/accessibility"),
            ] {
                let b = buttons.iter().find(|b| b.id == id).unwrap();
                assert_eq!(
                    (b.x, b.y, b.w, b.enabled, b.icon),
                    (x, y + 48.0, 20.0, enabled, Some((icon, 15.0)))
                );
                assert_eq!(
                    title.click(x + 10.0, y + 58.0, (w, h)),
                    match id {
                        6 => (Action::Language, true),
                        _ => (Action::Accessibility, true),
                    }
                );
            }
            for id in [2, 3] {
                assert_eq!(buttons.iter().find(|b| b.id == id).unwrap().y, y + 72.0);
            }
            // The four-pixel gaps around icons must not activate a button.
            assert_eq!(title.click(cx, y + 58.0, (w, h)), (Action::None, false));
            assert_eq!(
                title.click(cx + 10.0, y + 70.0, (w, h)),
                (Action::None, false)
            );
        }
    }

    #[test]
    fn splash_selection_filters_the_forbidden_hash_and_has_no_fallback() {
        let hidden = "This message will never appear on the splash screen, isn't that weird?";
        assert_eq!(
            horizon_core::random::java_string_hash(hidden.encode_utf16()),
            125780783
        );
        let text = format!("\n First! \r\n{hidden}\nSecond!\n");
        for index in 0..6 {
            assert_eq!(
                choose_splash(&text, index, (10, 15)).as_deref(),
                Some(["", "First!", "Second!"][index % 3])
            );
        }
        assert_eq!(choose_splash(" \r\n", 42, (10, 15)).as_deref(), Some(""));
        for text in ["", hidden] {
            assert_eq!(choose_splash(text, 42, (10, 15)), None);
        }
    }

    #[test]
    fn special_date_splashes_override_resources_even_when_the_list_is_empty() {
        for (date, expected) in [
            ((12, 24), "Merry X-mas!"),
            ((1, 1), "Happy new year!"),
            ((10, 31), "OOoooOOOoooo! Spooky!"),
        ] {
            for text in ["", "Ordinary splash!"] {
                assert_eq!(choose_splash(text, 0, date).as_deref(), Some(expected));
            }
        }
    }

    #[test]
    fn classic_logo_and_icon_geometry_matches_minecraft() {
        let root = Path::new("/nonexistent/horizon-test-resources");
        let mut atlas = HudAtlas::build(root).unwrap();
        let r = crate::hud::Rect {
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
            w: 1024,
            h: 256,
        };
        atlas.sprites.insert("gui/title/minecraft".into(), r);
        atlas.sprites.insert(
            "gui/title/edition".into(),
            crate::hud::Rect { w: 128, h: 16, ..r },
        );
        for (i, icon) in ["icon/language", "icon/accessibility"]
            .into_iter()
            .enumerate()
        {
            atlas.sprites.insert(
                format!("gui/sprites/{icon}"),
                crate::hud::Rect {
                    u0: 0.25 + i as f32 * 0.125,
                    w: 15,
                    h: 15,
                    ..r
                },
            );
        }
        let mut title = Title::new(PathBuf::new(), root);
        title.splash = Some("Visible splash!".into());
        let mut batch = HudBatch::new([1920, 1080]);
        title.draw(&atlas, &mut batch, (0.0, 0.0), false);
        let cx = (batch.width / 2.0).floor();
        assert_eq!(batch.vertices[0].pos, [cx - 128.0, 30.0]);
        assert_eq!(batch.vertices[2].pos, [cx + 128.0, 74.0]);
        assert_eq!(batch.vertices[2].uv, [1.0, 44.0 / 64.0]);
        assert_eq!(batch.vertices[6].pos, [cx - 64.0, 67.0]);
        assert_eq!(batch.vertices[8].pos, [cx + 64.0, 81.0]);
        assert_eq!(batch.vertices[8].uv, [1.0, 14.0 / 16.0]);
        for i in 0..2 {
            let quad = batch
                .vertices
                .as_chunks::<6>()
                .0
                .iter()
                .find(|quad| quad[0].uv == [0.25 + i as f32 * 0.125, 0.0])
                .unwrap();
            let x = cx - 22.0 + i as f32 * 24.0 + 3.0;
            let y = (batch.height / 4.0).floor() + 48.0 + 48.0 + 3.0;
            assert_eq!(quad[0].pos, [x, y]);
            assert_eq!(quad[2].pos, [x + 15.0, y + 15.0]);
            assert!(quad.iter().all(|v| v.color == u32::MAX));
        }
        assert!(
            batch.vertices[12..]
                .iter()
                .any(|v| v.color == rgba(255, 255, 0, 255))
        );
        let settings = crate::settings::Settings::parse("hideSplashTexts:true\n");
        for (hide_splash, splash) in [
            (
                settings.file.bool("hideSplashTexts"),
                Some("Hidden splash!".into()),
            ),
            (false, None),
        ] {
            title.splash = splash;
            let mut hidden = HudBatch::new([1920, 1080]);
            title.draw(&atlas, &mut hidden, (0.0, 0.0), hide_splash);
            assert!(
                !hidden
                    .vertices
                    .iter()
                    .any(|v| v.color == rgba(255, 255, 0, 255))
            );
        }
    }
}
