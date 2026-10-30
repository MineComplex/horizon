//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct MutableBoolean(pub std::marker::PhantomData<()>);
impl Clone for MutableBoolean { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MutableDouble(pub std::marker::PhantomData<()>);
impl Clone for MutableDouble { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MutableFloat(pub std::marker::PhantomData<()>);
impl Clone for MutableFloat { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MutableInt(pub std::marker::PhantomData<()>);
impl Clone for MutableInt { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MutableObject<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for MutableObject<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
