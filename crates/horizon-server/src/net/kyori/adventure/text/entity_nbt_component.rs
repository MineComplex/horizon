//! Port of `net.kyori.adventure.text.EntityNBTComponent` (and its `Builder`).
//!
//! Every component is one value type, `Component` (its kind is `ComponentKind::EntityNbt`); `EntityNBTComponent` is an
//! alias, `c.is_entity_nbt()` replaces `instanceof EntityNBTComponent`, and its
//! accessors are methods of `Component`.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type EntityNBTComponent = Component;
pub type EntityNBTComponentBuilder = ComponentBuilder;
