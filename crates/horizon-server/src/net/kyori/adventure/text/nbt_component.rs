//! Port of `net.kyori.adventure.text.NBTComponent` and `NBTComponentBuilder`: the block, entity and
//! storage NBT components, all `Component` (see text_component.rs). The type parameters are Java's
//! self types and stay unused.
use crate::net::kyori::adventure::text::{Component, ComponentBuilder};

pub type NBTComponent<C = Component> = NbtComponentOf<C>;
pub type NBTComponentBuilder<C = Component, B = ComponentBuilder> = NbtComponentBuilderOf<C, B>;

/// `NBTComponent<C>` (the parameter is Java's self type): derefs to the component.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct NbtComponentOf<C = Component>(pub Component, pub std::marker::PhantomData<fn() -> C>);
impl<C> std::ops::Deref for NbtComponentOf<C> {
    type Target = Component;
    fn deref(&self) -> &Component {
        &self.0
    }
}
impl<C> From<Component> for NbtComponentOf<C> {
    fn from(c: Component) -> Self {
        NbtComponentOf(c, std::marker::PhantomData)
    }
}
impl<C> From<NbtComponentOf<C>> for Component {
    fn from(c: NbtComponentOf<C>) -> Self {
        c.0
    }
}
/// `NBTComponentBuilder<C, B>`: derefs to the builder.
#[derive(Clone)]
pub struct NbtComponentBuilderOf<C = Component, B = ComponentBuilder>(pub ComponentBuilder, pub std::marker::PhantomData<fn() -> (C, B)>);
impl<C, B> std::ops::Deref for NbtComponentBuilderOf<C, B> {
    type Target = ComponentBuilder;
    fn deref(&self) -> &ComponentBuilder {
        &self.0
    }
}
impl<C, B> From<ComponentBuilder> for NbtComponentBuilderOf<C, B> {
    fn from(b: ComponentBuilder) -> Self {
        NbtComponentBuilderOf(b, std::marker::PhantomData)
    }
}
