//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Long2ObjectArraySortedMap<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Long2ObjectArraySortedMap<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
