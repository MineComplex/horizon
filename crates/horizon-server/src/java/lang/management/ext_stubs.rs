//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct MemoryUsage(pub std::marker::PhantomData<()>);
impl Clone for MemoryUsage { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ThreadInfo(pub std::marker::PhantomData<()>);
impl Clone for ThreadInfo { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
