//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait App<A0 = JObjectRef, A1 = JObjectRef>: Send + Sync {}
pub trait Applicative<A0 = JObjectRef, A1 = JObjectRef>: Send + Sync {}
pub struct ConstMu<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ConstMu<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct IdFMu(pub std::marker::PhantomData<()>);
impl Clone for IdFMu { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct OptionalBoxMu(pub std::marker::PhantomData<()>);
impl Clone for OptionalBoxMu { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
