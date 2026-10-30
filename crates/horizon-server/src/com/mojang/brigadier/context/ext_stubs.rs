//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct CommandContext<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for CommandContext<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct CommandContextBuilder<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for CommandContextBuilder<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ContextChain<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ContextChain<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ParsedArgument<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for ParsedArgument<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct StringRange(pub std::marker::PhantomData<()>);
impl Clone for StringRange { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
