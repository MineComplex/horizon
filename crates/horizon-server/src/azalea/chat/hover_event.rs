use uuid::Uuid;

use crate::{azalea::chat::FormattedText, nbt::Tag};

/// What hovering over a component shows (`HoverEvent`, 26.3 shapes).
#[derive(Clone, Debug, PartialEq)]
pub enum HoverEvent {
    ShowText {
        value: Box<FormattedText>,
    },
    /// `ItemStackTemplate`: the item id, a count from 1 to 99, and the component patch kept as its
    /// NBT form until item components are ported.
    ShowItem {
        id: String,
        count: i32,
        components: Option<Tag>,
    },
    /// `EntityTooltipInfo`: the entity type's id.
    ShowEntity {
        id: String,
        uuid: Uuid,
        name: Option<Box<FormattedText>>,
    },
}

impl HoverEvent {
    pub fn action(&self) -> &'static str {
        match self {
            Self::ShowText { .. } => "show_text",
            Self::ShowItem { .. } => "show_item",
            Self::ShowEntity { .. } => "show_entity",
        }
    }
}
