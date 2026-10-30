//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Graph<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Graph<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MutableGraph<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for MutableGraph<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
