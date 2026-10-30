//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ImmutableListBuilder<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ImmutableListBuilder<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ImmutableMapBuilder<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for ImmutableMapBuilder<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ImmutableSetBuilder<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ImmutableSetBuilder<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Interner<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Interner<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Range<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Range<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
