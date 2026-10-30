//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait Appendable: Send + Sync {}
pub trait AutoCloseable: Send + Sync {}
pub struct Class<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Class<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ClassLoader(pub std::marker::PhantomData<()>);
impl Clone for ClassLoader { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Cloneable: Send + Sync {}
pub trait Comparable<A0 = JObjectRef>: Send + Sync {}
pub struct Enum<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Enum<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct EnumEnumDesc(pub std::marker::PhantomData<()>);
impl Clone for EnumEnumDesc { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Package(pub std::marker::PhantomData<()>);
impl Clone for Package { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Runnable: Send + Sync {}
pub struct ScopedValue<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ScopedValue<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct StackTraceElement(pub std::marker::PhantomData<()>);
impl Clone for StackTraceElement { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct StackWalker(pub std::marker::PhantomData<()>);
impl Clone for StackWalker { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Thread(pub std::marker::PhantomData<()>);
impl Clone for Thread { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ThreadGroup(pub std::marker::PhantomData<()>);
impl Clone for ThreadGroup { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ThreadLocal<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ThreadLocal<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Throwable(pub std::marker::PhantomData<()>);
impl Clone for Throwable { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
