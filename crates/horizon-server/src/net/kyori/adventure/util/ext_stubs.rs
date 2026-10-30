//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Codec<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef, A3 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2, fn() -> A3)>);
impl<A0, A1, A2, A3> Clone for Codec<A0, A1, A2, A3> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct CodecDecoder<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2)>);
impl<A0, A1, A2> Clone for CodecDecoder<A0, A1, A2> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct CodecEncoder<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2)>);
impl<A0, A1, A2> Clone for CodecEncoder<A0, A1, A2> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait ComponentMessageThrowable: Send + Sync {}
pub struct Index<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for Index<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TriState(pub std::marker::PhantomData<()>);
impl Clone for TriState { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
