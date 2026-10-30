//! Port of `net.kyori.adventure.text.StorageNBTComponent` (and its `Builder`).
//!
//! Every component is one value type, `Component` (its kind is `ComponentKind::StorageNbt`); `StorageNBTComponent` is an
//! alias, `c.is_storage_nbt()` replaces `instanceof StorageNBTComponent`, and its
//! accessors are methods of `Component`.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type StorageNBTComponent = Component;
pub type StorageNBTComponentBuilder = ComponentBuilder;
