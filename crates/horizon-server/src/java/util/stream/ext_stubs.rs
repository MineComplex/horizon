//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Collector<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2)>);
impl<A0, A1, A2> Clone for Collector<A0, A1, A2> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Stream<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Stream<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
