//! Port of `net.kyori.adventure.text.format.TextDecoration` (with `State` and `TextDecorationAndState`).
use crate::prelude::*;
use crate::net::kyori::adventure::text::format::{StyleBuilder, StyleBuilderApplicable};
use crate::net::kyori::adventure::util::{Index, TriState};

// ---- enum net.kyori.adventure.text.format.TextDecoration
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum TextDecoration {
    OBFUSCATED,
    BOLD,
    STRIKETHROUGH,
    UNDERLINED,
    ITALIC,
}
impl TextDecoration {
    pub const VALUES: [TextDecoration; 5] = [
        TextDecoration::OBFUSCATED,
        TextDecoration::BOLD,
        TextDecoration::STRIKETHROUGH,
        TextDecoration::UNDERLINED,
        TextDecoration::ITALIC,
    ];
    pub fn ordinal(self) -> i32 {
        self as i32
    }
    pub fn name(self) -> &'static str {
        match self {
            TextDecoration::OBFUSCATED => "OBFUSCATED",
            TextDecoration::BOLD => "BOLD",
            TextDecoration::STRIKETHROUGH => "STRIKETHROUGH",
            TextDecoration::UNDERLINED => "UNDERLINED",
            TextDecoration::ITALIC => "ITALIC",
        }
    }
    pub fn value_of(name: &str) -> Option<TextDecoration> {
        Self::VALUES.into_iter().find(|v| v.name() == name)
    }
    /// The decoration's lower-case name (`toString()`, `NAMES` key, the JSON field).
    pub fn to_string(self) -> &'static str {
        match self {
            TextDecoration::OBFUSCATED => "obfuscated",
            TextDecoration::BOLD => "bold",
            TextDecoration::STRIKETHROUGH => "strikethrough",
            TextDecoration::UNDERLINED => "underlined",
            TextDecoration::ITALIC => "italic",
        }
    }
    /// `as(boolean)`.
    pub fn as_(self, state: bool) -> TextDecorationAndState {
        self.with_state(TextDecorationState::by_boolean(state))
    }
    /// `as(State)`.
    pub fn as_1(self, state: TextDecorationState) -> TextDecorationAndState {
        self.with_state(state)
    }
    /// `withState(State)`; `withState(boolean)` and `withState(TriState)` go through `Into`.
    pub fn with_state(self, state: impl Into<TextDecorationState>) -> TextDecorationAndState {
        TextDecorationAndState { decoration: self, state: state.into() }
    }
}
impl StyleBuilderApplicable for TextDecoration {
    fn style_apply(&self, style: &StyleBuilder) {
        style.decorate(*self);
    }
}
impl std::fmt::Display for TextDecoration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_string())
    }
}

/// `TextDecoration.NAMES`.
pub static NAMES: LazyLock<Index<String, TextDecoration>> =
    LazyLock::new(|| Index::create(TextDecoration::VALUES.to_vec(), |d: &TextDecoration| d.to_string().to_string()));

// ---- enum net.kyori.adventure.text.format.TextDecoration.State
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum TextDecorationState {
    NOT_SET,
    FALSE,
    TRUE,
}
impl TextDecorationState {
    pub const VALUES: [TextDecorationState; 3] = [TextDecorationState::NOT_SET, TextDecorationState::FALSE, TextDecorationState::TRUE];
    pub fn ordinal(self) -> i32 {
        self as i32
    }
    pub fn name(self) -> &'static str {
        match self {
            TextDecorationState::NOT_SET => "NOT_SET",
            TextDecorationState::FALSE => "FALSE",
            TextDecorationState::TRUE => "TRUE",
        }
    }
    pub fn value_of(name: &str) -> Option<TextDecorationState> {
        Self::VALUES.into_iter().find(|v| v.name() == name)
    }
    pub fn to_string(self) -> &'static str {
        match self {
            TextDecorationState::NOT_SET => "not_set",
            TextDecorationState::FALSE => "false",
            TextDecorationState::TRUE => "true",
        }
    }
    /// `State.byBoolean(boolean)`.
    pub fn by_boolean(flag: bool) -> TextDecorationState {
        if flag { TextDecorationState::TRUE } else { TextDecorationState::FALSE }
    }
    /// `State.byBoolean(Boolean)`.
    pub fn by_boolean_1(flag: Option<bool>) -> TextDecorationState {
        match flag {
            None => TextDecorationState::NOT_SET,
            Some(f) => TextDecorationState::by_boolean(f),
        }
    }
    /// `State.byTriState(TriState)`.
    pub fn by_tri_state(flag: TriState) -> TextDecorationState {
        match flag {
            TriState::TRUE => TextDecorationState::TRUE,
            TriState::FALSE => TextDecorationState::FALSE,
            TriState::NOT_SET => TextDecorationState::NOT_SET,
        }
    }
}
impl From<bool> for TextDecorationState {
    fn from(b: bool) -> Self {
        TextDecorationState::by_boolean(b)
    }
}
impl From<TriState> for TextDecorationState {
    fn from(t: TriState) -> Self {
        TextDecorationState::by_tri_state(t)
    }
}

// ---- interface net.kyori.adventure.text.format.TextDecorationAndState
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TextDecorationAndState {
    pub decoration: TextDecoration,
    pub state: TextDecorationState,
}
impl TextDecorationAndState {
    pub fn decoration(&self) -> TextDecoration {
        self.decoration
    }
    pub fn state(&self) -> TextDecorationState {
        self.state
    }
}
impl StyleBuilderApplicable for TextDecorationAndState {
    fn style_apply(&self, style: &StyleBuilder) {
        style.decoration(self.decoration, self.state);
    }
}
