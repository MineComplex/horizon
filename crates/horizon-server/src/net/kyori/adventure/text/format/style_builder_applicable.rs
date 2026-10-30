//! Port of `net.kyori.adventure.text.format.StyleBuilderApplicable`.
use crate::net::kyori::adventure::text::format::StyleBuilder;

// ---- interface net.kyori.adventure.text.format.StyleBuilderApplicable
pub trait StyleBuilderApplicable: Send + Sync {
    fn style_apply(&self, style: &StyleBuilder);
    /// `componentBuilderApply(ComponentBuilder)`: `component.style(this::styleApply)`.
    fn component_builder_apply(&self, component: &crate::net::kyori::adventure::text::ComponentBuilder) {
        component.style_consumer(|b| self.style_apply(b));
    }
}
