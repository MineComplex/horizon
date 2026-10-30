//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Long2BooleanFunction(pub std::marker::PhantomData<()>);
impl Clone for Long2BooleanFunction { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Long2IntFunction(pub std::marker::PhantomData<()>);
impl Clone for Long2IntFunction { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Long2ObjectFunction<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Long2ObjectFunction<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LongComparator(pub std::marker::PhantomData<()>);
impl Clone for LongComparator { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LongConsumer(pub std::marker::PhantomData<()>);
impl Clone for LongConsumer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LongSpliterator(pub std::marker::PhantomData<()>);
impl Clone for LongSpliterator { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
