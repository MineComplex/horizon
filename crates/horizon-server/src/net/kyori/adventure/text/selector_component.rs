//! Port of `net.kyori.adventure.text.SelectorComponent` (and its `Builder`).
//!
//! Every component is one value type, `Component` (its kind is `ComponentKind::Selector`); `SelectorComponent` is an
//! alias, `c.is_selector()` replaces `instanceof SelectorComponent`, and its
//! accessors are methods of `Component`.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type SelectorComponent = Component;
pub type SelectorComponentBuilder = ComponentBuilder;
