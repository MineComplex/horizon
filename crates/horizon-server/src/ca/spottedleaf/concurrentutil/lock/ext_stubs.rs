//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ReentrantAreaLock(pub std::marker::PhantomData<()>);
impl Clone for ReentrantAreaLock { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
