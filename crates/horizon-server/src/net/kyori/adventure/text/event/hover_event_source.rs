//! Port of `net.kyori.adventure.text.event.HoverEventSource`: something that can be shown on hover
//! (`Component`, `HoverEvent`, Bukkit's `Entity` and `ItemStack`).
use crate::net::kyori::adventure::text::event::{HoverEvent, HoverEventValueType};

// ---- interface net.kyori.adventure.text.event.HoverEventSource<V>
pub trait HoverEventSource<V: HoverEventValueType = crate::net::kyori::adventure::text::event::HoverEventValue>: Send + Sync {
    /// `asHoverEvent()`: `asHoverEvent(UnaryOperator.identity())`.
    fn as_hover_event(&self) -> HoverEvent<V> {
        self.as_hover_event_1(&|v| v)
    }
    /// `asHoverEvent(UnaryOperator<V>)`.
    fn as_hover_event_1(&self, op: &dyn Fn(V) -> V) -> HoverEvent<V>;
}

/// `HoverEventSource.unbox(HoverEventSource)`: the event, type-erased as `Style` stores it.
pub fn unbox<V: HoverEventValueType>(source: Option<&dyn HoverEventSource<V>>) -> Option<HoverEvent> {
    source.map(|s| s.as_hover_event().erase())
}
