//! Port of `net.kyori.adventure.text.ComponentLike`.
use crate::prelude::*;
use crate::net::kyori::adventure::text::Component;

// ---- interface net.kyori.adventure.text.ComponentLike
pub trait ComponentLike: Send + Sync {
    fn as_component(&self) -> Component;
}

/// `ComponentLike.asComponents(List)`.
pub fn as_components(likes: &[JRef<dyn ComponentLike>]) -> Vec<Component> {
    likes.iter().map(|l| l.as_component()).collect()
}
/// `ComponentLike.asComponents(List, Predicate)`.
pub fn as_components_2(likes: &[JRef<dyn ComponentLike>], filter: impl Fn(&Component) -> bool) -> Vec<Component> {
    likes.iter().map(|l| l.as_component()).filter(|c| filter(c)).collect()
}
/// `ComponentLike.unbox(ComponentLike)`.
pub fn unbox(like: Option<&dyn ComponentLike>) -> Option<Component> {
    like.map(|l| l.as_component())
}
