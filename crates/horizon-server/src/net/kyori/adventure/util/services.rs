//! Port of `net.kyori.adventure.util.Services`: Adventure finds its platform's implementations with
//! Java's `ServiceLoader`. Paper registers them in `META-INF/services` (paper-server); this port wires
//! the same providers in directly, in the provider modules (`provider()` functions next to each
//! `Provider` trait: `legacy_component_serializer::provider()`...).
use crate::prelude::*;

/// `Services.service(Class)`: the one provider of a type, if any. Here the caller passes the
/// provider's constructor, which stands for its `META-INF/services` line.
pub fn service<P: ?Sized>(make: impl FnOnce() -> Option<JRef<P>>) -> Option<JRef<P>> {
    make()
}
