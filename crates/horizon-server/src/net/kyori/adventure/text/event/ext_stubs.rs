//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ClickCallback<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ClickCallback<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ClickCallbackOptions(pub std::marker::PhantomData<()>);
impl Clone for ClickCallbackOptions { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ClickEvent(pub std::marker::PhantomData<()>);
impl Clone for ClickEvent { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataComponentValue(pub std::marker::PhantomData<()>);
impl Clone for DataComponentValue { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataComponentValueConverterRegistryConversion<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for DataComponentValueConverterRegistryConversion<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct HoverEvent<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for HoverEvent<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct HoverEventShowEntity(pub std::marker::PhantomData<()>);
impl Clone for HoverEventShowEntity { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct HoverEventShowItem(pub std::marker::PhantomData<()>);
impl Clone for HoverEventShowItem { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait HoverEventSource<A0 = JObjectRef>: Send + Sync {}
