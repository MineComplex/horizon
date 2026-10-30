//! Port of `net.kyori.adventure.text.KeybindComponent` (and its `Builder`).
//!
//! Every component is one value type, `Component` (its kind is `ComponentKind::Keybind`); `KeybindComponent` is an
//! alias, `c.is_keybind()` replaces `instanceof KeybindComponent`, and its
//! accessors are methods of `Component`.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type KeybindComponent = Component;
pub type KeybindComponentBuilder = ComponentBuilder;
