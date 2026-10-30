//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Int2BooleanFunction(pub std::marker::PhantomData<()>);
impl Clone for Int2BooleanFunction { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Int2IntFunction(pub std::marker::PhantomData<()>);
impl Clone for Int2IntFunction { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Int2ObjectMapEntry<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Int2ObjectMapEntry<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
