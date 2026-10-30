//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct XXHash32(pub std::marker::PhantomData<()>);
impl Clone for XXHash32 { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
