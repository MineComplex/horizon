//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct AmbiguityConsumer<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for AmbiguityConsumer<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Command<A0 = JObjectRef>: Send + Sync {}
pub struct ImmutableStringReader(pub std::marker::PhantomData<()>);
impl Clone for ImmutableStringReader { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Message: Send + Sync {}
pub struct ParseResults<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ParseResults<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait RedirectModifier<A0 = JObjectRef>: Send + Sync {}
pub struct ResultConsumer<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ResultConsumer<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SingleRedirectModifier<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for SingleRedirectModifier<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct StringReader(pub std::marker::PhantomData<()>);
impl Clone for StringReader { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
