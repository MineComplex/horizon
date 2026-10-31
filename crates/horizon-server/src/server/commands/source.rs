//! `CommandSourceStack` as plain data, and the messages a command sends to it (`sendSuccess`,
//! `sendFailure`, `broadcastToAdmins`).

use uuid::Uuid;

use crate::azalea::chat::FormattedText;
use crate::azalea::chat::style::{ChatFormatting, Style};
use crate::azalea::chat::text_component::TextComponent;
use crate::azalea::chat::translatable_component::{PrimitiveOrComponent, TranslatableComponent};
use crate::command::exec::{ExecSource, ResultCallback};
use crate::server::server_properties::PermissionLevel;

/// Who runs a command: the console (`MinecraftServer` is its `CommandSource`) or a player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Console,
    Player(Uuid),
}

/// A command source: who, with what permission level, where (the position, rotation and level that
/// coordinates and `getLevel()` read) and what to tell. Cloned freely, as forks do.
#[derive(Clone, Debug)]
pub struct CommandSource {
    pub origin: Origin,
    /// `getTextName`: "Server" for the console.
    pub name: String,
    pub permissions: PermissionLevel,
    pub position: [f64; 3],
    /// `[x_rot, y_rot]`.
    pub rotation: [f32; 2],
    /// Index of the level in `World::levels`.
    pub level: usize,
    /// `silent`: no feedback at all.
    pub silent: bool,
    pub callback: ResultCallback,
}

impl CommandSource {
    /// `MinecraftServer.createCommandSourceStack`: the server, owner permissions, at the respawn
    /// position of the respawn level, "Server".
    pub fn console(level: usize, position: [f64; 3]) -> Self {
        Self {
            origin: Origin::Console,
            name: "Server".into(),
            permissions: PermissionLevel::Owners,
            position,
            rotation: [0.0, 0.0],
            level,
            silent: false,
            callback: ResultCallback::Empty,
        }
    }

    /// `Commands.hasPermission(LEVEL_x)`.
    pub fn has_level(&self, level: u8) -> bool {
        self.permissions.id() >= i32::from(level)
    }

    /// `withSuppressedOutput`.
    pub fn suppressed(mut self) -> Self {
        self.silent = true;
        self
    }
}

impl ExecSource for CommandSource {
    fn callback(&self) -> &ResultCallback {
        &self.callback
    }

    fn with_callback(mut self, callback: ResultCallback) -> Self {
        self.callback = callback;
        self
    }
}

/// `Component.literal`.
pub fn literal(text: impl Into<String>) -> FormattedText {
    TextComponent::new(text).into()
}

/// `Component.translatable` / `translatableEscape`.
pub fn translatable(key: &str, args: Vec<PrimitiveOrComponent>) -> FormattedText {
    TranslatableComponent::new(key.to_owned(), args).into()
}

/// `withStyle(formatting...)`.
pub fn with_formatting(mut text: FormattedText, formatting: &[ChatFormatting]) -> FormattedText {
    let mut style = Style::new();
    for f in formatting {
        style.apply_formatting(f);
    }
    text.get_base_mut().style.apply(&style);
    text
}

/// `Component.empty().append(message).withStyle(RED)`: how `sendFailure` styles a failure.
pub fn failure_style(message: FormattedText) -> FormattedText {
    let mut out: FormattedText = TextComponent::new("").into();
    out.append(message);
    with_formatting(out, &[ChatFormatting::Red])
}

pub fn string_arg(text: impl Into<String>) -> PrimitiveOrComponent {
    PrimitiveOrComponent::String(text.into())
}

pub fn text_arg(text: FormattedText) -> PrimitiveOrComponent {
    PrimitiveOrComponent::FormattedText(text)
}
