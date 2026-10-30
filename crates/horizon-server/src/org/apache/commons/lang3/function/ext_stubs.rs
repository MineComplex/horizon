//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait TriConsumer<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef>: Send + Sync {}
pub struct TriFunction<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef, A3 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2, fn() -> A3)>);
impl<A0, A1, A2, A3> Clone for TriFunction<A0, A1, A2, A3> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
