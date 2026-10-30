//! Port of `net.kyori.adventure.text.ScoreComponent` (and its `Builder`).
//!
//! Every component is one value type, `Component` (its kind is `ComponentKind::Score`); `ScoreComponent` is an
//! alias, `c.is_score()` replaces `instanceof ScoreComponent`, and its
//! accessors are methods of `Component`.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type ScoreComponent = Component;
pub type ScoreComponentBuilder = ComponentBuilder;
