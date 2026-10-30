//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct HashStrategy<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for HashStrategy<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Pair<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for Pair<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PriorityQueue<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for PriorityQueue<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Stack<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Stack<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
