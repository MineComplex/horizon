//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Base64Encoder(pub std::marker::PhantomData<()>);
impl Clone for Base64Encoder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct BitSet(pub std::marker::PhantomData<()>);
impl Clone for BitSet { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Comparator<A0 = JObjectRef>: Send + Sync {}
pub struct Date(pub std::marker::PhantomData<()>);
impl Clone for Date { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Entry<A0 = JObjectRef, A1 = JObjectRef>: Send + Sync {}
pub struct Enumeration<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Enumeration<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait EventListener: Send + Sync {}
pub struct HexFormat(pub std::marker::PhantomData<()>);
impl Clone for HexFormat { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait JIterator<A0 = JObjectRef>: Send + Sync {}
pub struct Locale(pub std::marker::PhantomData<()>);
impl Clone for Locale { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PriorityQueue<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for PriorityQueue<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Properties(pub std::marker::PhantomData<()>);
impl Clone for Properties { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Random(pub std::marker::PhantomData<()>);
impl Clone for Random { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct V(pub std::marker::PhantomData<()>);
impl Clone for V { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
