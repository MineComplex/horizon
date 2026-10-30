//! The component contents azalea did not have (`KeybindContents`, `ScoreContents`,
//! `SelectorContents`, `NbtContents`). Score, selector and NBT contents are resolved against the
//! world before they are sent (`ComponentUtils.updateForEntity`); until then they show no text.

use crate::azalea::chat::{FormattedText, base_component::BaseComponent};

/// A key binding's name (`key.jump`), shown by the client as the key it is bound to.
#[derive(Clone, Debug, PartialEq)]
pub struct KeybindComponent {
    pub base: BaseComponent,
    pub keybind: String,
}

/// A scoreboard score: `name` is a player name, a selector or `*`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoreComponent {
    pub base: BaseComponent,
    pub name: String,
    pub objective: String,
}

/// The names of the entities an entity selector finds.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectorComponent {
    pub base: BaseComponent,
    pub selector: String,
    pub separator: Option<Box<FormattedText>>,
}

/// Values read with an NBT path from an entity, a block entity or command storage.
#[derive(Clone, Debug, PartialEq)]
pub struct NbtComponent {
    pub base: BaseComponent,
    pub path: String,
    /// Read the values as components (exclusive with `plain`).
    pub interpret: bool,
    pub plain: bool,
    pub separator: Option<Box<FormattedText>>,
    pub source: DataSource,
}

/// Where an NBT component reads from (`DataSources`), as the command text vanilla compiles.
#[derive(Clone, Debug, PartialEq)]
pub enum DataSource {
    /// An entity selector.
    Entity(String),
    /// Block coordinates (`~ ~1 ~`).
    Block(String),
    /// A storage identifier.
    Storage(String),
}

impl DataSource {
    /// The source's field name, which is also its `source` type name.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Entity(_) => "entity",
            Self::Block(_) => "block",
            Self::Storage(_) => "storage",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Self::Entity(v) | Self::Block(v) | Self::Storage(v) => v,
        }
    }
}
