//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ConcurrentChainedLong2LongHashTable(pub std::marker::PhantomData<()>);
impl Clone for ConcurrentChainedLong2LongHashTable { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ConcurrentChainedLong2ReferenceHashTable<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ConcurrentChainedLong2ReferenceHashTable<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
