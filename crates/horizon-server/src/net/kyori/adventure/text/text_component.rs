//! Port of `net.kyori.adventure.text.TextComponent` (and its `Builder`).
//!
//! Every component is one value type, `Component` (its kind is `ComponentKind::Text`); `TextComponent` is an
//! alias, `c.is_text()` replaces `instanceof TextComponent`, and its
//! accessors are methods of `Component`.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type TextComponent = Component;
pub type TextComponentBuilder = ComponentBuilder;
