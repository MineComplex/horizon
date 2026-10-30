//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct MethodHandle(pub std::marker::PhantomData<()>);
impl Clone for MethodHandle { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MethodHandlesLookup(pub std::marker::PhantomData<()>);
impl Clone for MethodHandlesLookup { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct VarHandle(pub std::marker::PhantomData<()>);
impl Clone for VarHandle { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
