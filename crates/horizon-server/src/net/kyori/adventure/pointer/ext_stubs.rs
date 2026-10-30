//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Pointers(pub std::marker::PhantomData<()>);
impl Clone for Pointers { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PointersBuilder(pub std::marker::PhantomData<()>);
impl Clone for PointersBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
