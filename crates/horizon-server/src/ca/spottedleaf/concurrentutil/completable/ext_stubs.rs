//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct CallbackCompletable<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for CallbackCompletable<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Completable<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Completable<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
