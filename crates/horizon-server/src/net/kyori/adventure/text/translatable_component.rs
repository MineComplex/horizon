//! Port of `net.kyori.adventure.text.TranslatableComponent` (and its `Builder`).
//!
//! Every component is one value type, `Component` (its kind is `ComponentKind::Translatable`); `TranslatableComponent` is an
//! alias, `c.is_translatable()` replaces `instanceof TranslatableComponent`, and its
//! accessors are methods of `Component`.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type TranslatableComponent = Component;
pub type TranslatableComponentBuilder = ComponentBuilder;
