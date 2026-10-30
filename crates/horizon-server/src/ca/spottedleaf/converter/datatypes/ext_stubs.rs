//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct DataHook<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for DataHook<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataType<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for DataType<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataWalker<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for DataWalker<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
