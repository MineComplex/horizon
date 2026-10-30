//! Port of `net.kyori.adventure.text.ObjectComponent` (and its `Builder`).
//!
//! Every component is one value type, `Component` (its kind is `ComponentKind::Object`); `ObjectComponent` is an
//! alias, `c.is_object()` replaces `instanceof ObjectComponent`, and its
//! accessors are methods of `Component`.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type ObjectComponent = Component;
pub type ObjectComponentBuilder = ComponentBuilder;
