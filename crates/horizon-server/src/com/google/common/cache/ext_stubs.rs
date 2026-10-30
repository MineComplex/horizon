//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Cache<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for Cache<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LoadingCache<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for LoadingCache<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
