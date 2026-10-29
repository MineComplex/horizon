//! Command blocks: `CommandBlock`'s placement (`setPlacedBy`), its scheduled
//! tick (`execute`, `executeChain`), `BaseCommandBlock.performCommand`
//! through the command system with the block's output, and
//! `CommandBlockEditScreen` with what `handleSetCommandBlock` does with it.
use super::{Game, Screen};
use horizon_client::{
    command_block::{self, CommandBlock, Mode},
    containers::BlockEntity,
    hud::{HudBatch, rgba},
    text::Text,
    ui::{self, EditBox},
    world::World,
};
use horizon_protocol::packets::play::{self, Serverbound as S};
use horizon_world::{
    chunk::BlockState,
    registry::{BlockRegistry, BlockStateId},
};
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{KeyCode, PhysicalKey},
};

type Pos = (i32, i32, i32);

/// The open `CommandBlockEditScreen`.
pub(super) struct CommandBlockScreen {
    pos: Pos,
    command: EditBox,
    previous: EditBox,
    track_output: bool,
    mode: Mode,
    conditional: bool,
    auto: bool,
}

/// What `ServerboundSetCommandBlockPacket` carries.
struct CommandBlockEdit {
    pos: Pos,
    command: String,
    track_output: bool,
    mode: Mode,
    conditional: bool,
    auto: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Button {
    Output,
    Mode,
    Conditional,
    Auto,
    Done,
    Cancel,
}

/// `EditBox.textColorUneditable`.
const UNEDITABLE: u32 = 0xFF70_7070;

impl CommandBlockScreen {
    /// `AbstractCommandBlockEditScreen.init` and `CommandBlockEditScreen`'s
    /// buttons, in GUI pixels of a `w`×`h` screen.
    fn buttons(w: f32, h: f32) -> [(Button, f32, f32, f32); 6] {
        let mid = (w / 2.0).floor();
        let low = (h / 4.0).floor() + 132.0;
        [
            (Button::Output, mid + 130.0, 135.0, 20.0),
            (Button::Mode, mid - 154.0, 165.0, 100.0),
            (Button::Conditional, mid - 50.0, 165.0, 100.0),
            (Button::Auto, mid + 54.0, 165.0, 100.0),
            (Button::Done, mid - 154.0, low, 150.0),
            (Button::Cancel, mid + 4.0, low, 150.0),
        ]
    }
    fn label(&self, b: Button) -> Text {
        let tr = |key: &str| Text::translate(key, &[]);
        match b {
            Button::Output => Text::literal(if self.track_output { "O" } else { "X" }),
            Button::Mode => tr(self.mode.label()),
            Button::Conditional => tr(if self.conditional {
                "advMode.mode.conditional"
            } else {
                "advMode.mode.unconditional"
            }),
            Button::Auto => tr(if self.auto {
                "advMode.mode.autoexec.bat"
            } else {
                "advMode.mode.redstoneTriggered"
            }),
            Button::Done => tr("gui.done"),
            Button::Cancel => tr("gui.cancel"),
        }
    }
    /// `updatePreviousOutput`.
    fn show_previous(&mut self, block: &CommandBlock) {
        let shown = if self.track_output {
            block.last_output_string()
        } else {
            "-".to_owned()
        };
        self.previous.set_value(&shown);
    }
}

fn state_of(registry: &BlockRegistry, state: BlockStateId) -> Option<&BlockState> {
    registry.state(state)
}
fn facing_step(state: &BlockState) -> [i32; 3] {
    match state.properties.get("facing").map(String::as_str) {
        Some("down") => [0, -1, 0],
        Some("up") => [0, 1, 0],
        Some("south") => [0, 0, 1],
        Some("west") => [-1, 0, 0],
        Some("east") => [1, 0, 0],
        _ => [0, 0, -1],
    }
}
fn conditional(state: &BlockState) -> bool {
    state
        .properties
        .get("conditional")
        .is_some_and(|c| c == "true")
}
fn offset(pos: Pos, step: [i32; 3]) -> Pos {
    (pos.0 + step[0], pos.1 + step[1], pos.2 + step[2])
}
fn command_block_mut(world: &mut World, pos: Pos) -> Option<&mut CommandBlock> {
    match world.block_entities.get_mut(&pos)? {
        BlockEntity::CommandBlock(c) => Some(c),
        _ => None,
    }
}

/// `CommandBlockEntity.markConditionMet`: a conditional block needs the
/// command block behind it to have succeeded.
pub(super) fn mark_condition_met(world: &mut World, registry: &BlockRegistry, pos: Pos) -> bool {
    let Some(state) = state_of(registry, world.block(pos.0, pos.1, pos.2)) else {
        return false;
    };
    let mut met = true;
    if conditional(state) {
        let back = facing_step(state).map(|v| -v);
        let behind = offset(pos, back);
        let behind_block = state_of(registry, world.block(behind.0, behind.1, behind.2))
            .is_some_and(|s| Mode::of_block(&s.name).is_some());
        met = behind_block
            && matches!(world.block_entities.get(&behind),
                Some(BlockEntity::CommandBlock(c)) if c.success_count > 0);
    }
    if let Some(c) = command_block_mut(world, pos) {
        c.condition_met = met;
    }
    met
}

/// `CommandBlockEntity.setPowered` from `CommandBlock.setPoweredAndUpdate`
/// (the level's neighbour hook): the previous flag and `isAutomatic`; a
/// rising signal that will tick the block checks its condition.
pub(super) fn set_powered(
    world: &mut World,
    registry: &BlockRegistry,
    pos: Pos,
    powered: bool,
) -> Option<(bool, bool)> {
    let mode = state_of(registry, world.block(pos.0, pos.1, pos.2))
        .and_then(|s| Mode::of_block(&s.name))?;
    let c = command_block_mut(world, pos)?;
    let (was, auto) = (c.powered, c.auto);
    c.powered = powered;
    if command_block::power_schedules(mode, was, powered, auto) {
        mark_condition_met(world, registry, pos);
    }
    Some((was, auto))
}

/// `getAnalogOutputSignal`: the success count.
pub(super) fn analog_signal(world: &World, pos: Pos) -> Option<i32> {
    match world.block_entities.get(&pos)? {
        BlockEntity::CommandBlock(c) => Some(c.success_count),
        _ => None,
    }
}

impl Game<'_> {
    pub(super) fn draw_command_block(&mut self, batch: &mut HudBatch) {
        let Some(mut s) = self.command_block_screen.take() else {
            return;
        };
        // AbstractCommandBlockEditScreen.tick: the block is gone.
        if self.command_block(s.pos).is_none() {
            self.close_command_block();
            return;
        }
        let atlas = &self.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        let mid = (w / 2.0).floor();
        ui::menu_background(batch, atlas, 0.0, 0.0, w, h, true);
        let title = Text::translate("advMode.setCommand", &[]);
        let tw = atlas.font.width_text(&title);
        batch.text_styled(
            atlas,
            &title,
            (mid - tw / 2.0).floor(),
            20.0,
            u32::MAX,
            true,
        );
        let grey = rgba(160, 160, 160, 255);
        let label = Text::translate("advMode.command", &[]);
        batch.text_styled(atlas, &label, mid - 149.0, 40.0, grey, true);
        s.command.x = mid - 150.0;
        s.command.draw(batch, atlas, None);
        if !s.previous.value.is_empty() {
            let label = Text::translate("advMode.previousOutput", &[]);
            batch.text_styled(atlas, &label, mid - 149.0, 125.0, grey, true);
            s.previous.x = mid - 150.0;
            s.previous.draw(batch, atlas, None);
        }
        for (b, x, y, bw) in CommandBlockScreen::buttons(w, h) {
            let hover = ui::hovered(self.cursor, x, y, bw, 20.0);
            ui::button(batch, atlas, x, y, bw, 20.0, &s.label(b), true, hover);
        }
        self.command_block_screen = Some(s);
    }
    pub(super) fn command_block_click(&mut self) {
        let Some(mut s) = self.command_block_screen.take() else {
            return;
        };
        let size = self
            .renderer
            .as_ref()
            .map_or([1, 1], |r| [r.config.width, r.config.height]);
        let b = HudBatch::new(size);
        let hit = CommandBlockScreen::buttons(b.width, b.height)
            .into_iter()
            .find(|(_, x, y, bw)| ui::hovered(self.cursor, *x, *y, *bw, 20.0))
            .map(|b| b.0);
        if hit.is_some() {
            self.audio.play_button_click();
        }
        match hit {
            Some(Button::Output) => {
                s.track_output = !s.track_output;
                if let Some(block) = self.command_block(s.pos) {
                    s.show_previous(block);
                }
            }
            Some(Button::Mode) => s.mode = s.mode.next(),
            Some(Button::Conditional) => s.conditional = !s.conditional,
            Some(Button::Auto) => s.auto = !s.auto,
            Some(Button::Done) => {
                self.set_command_block(&s);
                self.close_command_block();
                return;
            }
            Some(Button::Cancel) => {
                self.close_command_block();
                return;
            }
            None => {}
        }
        self.command_block_screen = Some(s);
    }
    /// `AbstractCommandBlockEditScreen.keyPressed`: Enter is Done, Escape
    /// cancels, the rest edits the command.
    pub(super) fn command_block_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let PhysicalKey::Code(key) = event.physical_key else {
            return;
        };
        match key {
            KeyCode::Escape => return self.close_command_block(),
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some(s) = self.command_block_screen.take() {
                    self.set_command_block(&s);
                }
                return self.close_command_block();
            }
            _ => {}
        }
        let ctrl = self.keys.contains(&KeyCode::ControlLeft);
        let shift = self.keys.contains(&KeyCode::ShiftLeft);
        let shortcut = self.edit_shortcut(event);
        let Some(s) = self.command_block_screen.as_mut() else {
            return;
        };
        if let Some(shortcut) = shortcut {
            s.command.shortcut(shortcut, &mut self.clipboard);
        } else if !s.command.key(key, ctrl, shift) && !ctrl {
            self.command_block_type(event.text.as_deref().unwrap_or(""));
        }
    }
}
impl crate::game::Game<'_> {
    /// `Player.canUseGameMasterBlocks`: creative with game master commands.
    pub(super) fn can_use_game_master_blocks(&self) -> bool {
        self.creative && self.world_data.registries.permission >= 2
    }
    /// `CommandBlock.useWithoutItem` on a server's client, which opens the
    /// screen itself (`Player.openCommandBlock`) while the server only
    /// answers the click: the block at `pos`, when it is a command block the
    /// player may edit.
    pub(super) fn open_command_block_if_editable(&mut self, pos: Pos) {
        let is_command_block = self
            .block_state_at(pos)
            .is_some_and(|state| Mode::of_block(&state.name).is_some());
        if is_command_block && self.can_use_game_master_blocks() {
            self.open_command_block(pos);
        }
    }
    fn command_block(&self, pos: Pos) -> Option<&CommandBlock> {
        match self.world.block_entities.get(&pos)? {
            BlockEntity::CommandBlock(c) => Some(c),
            _ => None,
        }
    }
    fn block_state_at(&self, pos: Pos) -> Option<&'static BlockState> {
        let registry = self.pack.assets.registry;
        state_of(registry, self.world.block(pos.0, pos.1, pos.2))
    }
    /// `LocalPlayer.openCommandBlock`.
    pub(super) fn open_command_block(&mut self, pos: Pos) {
        let Some(block) = self.command_block(pos).cloned() else {
            return;
        };
        let Some(state) = self.block_state_at(pos) else {
            return;
        };
        let mut command = EditBox::new(0.0, 50.0, 300.0, 20.0);
        command.max_length = 32500;
        command.set_value(&block.command);
        command.focused = true;
        let mut previous = EditBox::new(0.0, 135.0, 276.0, 20.0);
        previous.max_length = 32500;
        previous.text_color = UNEDITABLE;
        previous.focused = false;
        let mut screen = CommandBlockScreen {
            pos,
            command,
            previous,
            track_output: block.track_output,
            mode: Mode::of_block(&state.name).unwrap_or(Mode::Redstone),
            conditional: conditional(state),
            auto: block.auto,
        };
        screen.show_previous(&block);
        self.command_block_screen = Some(screen);
        self.screen = Screen::CommandBlock;
        self.capture(false);
    }
    pub(super) fn close_command_block(&mut self) {
        self.command_block_screen = None;
        self.screen = Screen::Playing;
        self.capture(true);
    }
    /// Characters typed into the command box (`StringUtil.isAllowedChatCharacter`).
    pub(super) fn command_block_type(&mut self, typed: &str) {
        let allowed: String = typed
            .chars()
            .filter(|&c| c != '\u{a7}' && c >= ' ' && c != '\u{7f}')
            .collect();
        if let Some(s) = self.command_block_screen.as_mut()
            && !allowed.is_empty()
        {
            s.command.insert_text(&allowed);
        }
    }
    /// The edit screen's Done: a server's client sends the packet, the
    /// window's own world applies it (`handleSetCommandBlock`).
    fn set_command_block(&mut self, s: &CommandBlockScreen) {
        if !self.can_use_game_master_blocks() {
            self.system_message(Text::translate("advMode.notAllowed", &[]));
            return;
        }
        let edit = CommandBlockEdit {
            pos: s.pos,
            command: s.command.value.clone(),
            track_output: s.track_output,
            mode: s.mode,
            conditional: s.conditional,
            auto: s.auto,
        };
        {
            self.send(S::SetCommandBlock(play::SetCommandBlock {
                pos: [edit.pos.0, edit.pos.1, edit.pos.2],
                command: edit.command,
                mode: edit.mode.ordinal(),
                flags: i8::from(edit.track_output)
                    | (i8::from(edit.conditional) << 1)
                    | (i8::from(edit.auto) << 2),
            }));
        }
    }
}
