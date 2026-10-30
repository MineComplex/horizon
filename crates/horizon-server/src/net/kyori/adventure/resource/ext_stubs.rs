//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ResourcePackCallback(pub std::marker::PhantomData<()>);
impl Clone for ResourcePackCallback { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ResourcePackRequest(pub std::marker::PhantomData<()>);
impl Clone for ResourcePackRequest { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
