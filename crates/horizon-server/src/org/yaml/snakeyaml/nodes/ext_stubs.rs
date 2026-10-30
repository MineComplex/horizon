//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct MappingNode(pub std::marker::PhantomData<()>);
impl Clone for MappingNode { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Node(pub std::marker::PhantomData<()>);
impl Clone for Node { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
