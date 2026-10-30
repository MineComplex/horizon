//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Reference<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Reference<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SoftReference<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for SoftReference<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct WeakReference<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for WeakReference<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
