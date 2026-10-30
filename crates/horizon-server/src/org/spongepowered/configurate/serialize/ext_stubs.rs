//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ScalarSerializer<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ScalarSerializer<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SerializationException(pub std::marker::PhantomData<()>);
impl Clone for SerializationException { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait TypeSerializer<A0 = JObjectRef>: Send + Sync {}
pub trait TypeSerializerAnnotated<A0 = JObjectRef>: Send + Sync {}
