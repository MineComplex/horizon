//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait FieldDiscoverer<A0 = JObjectRef>: Send + Sync {}
pub struct ObjectMapper(pub std::marker::PhantomData<()>);
impl Clone for ObjectMapper { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ObjectMapperFactory(pub std::marker::PhantomData<()>);
impl Clone for ObjectMapperFactory { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
