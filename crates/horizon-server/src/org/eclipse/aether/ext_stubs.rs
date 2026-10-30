//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct DefaultRepositorySystemSession(pub std::marker::PhantomData<()>);
impl Clone for DefaultRepositorySystemSession { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct RepositorySystem(pub std::marker::PhantomData<()>);
impl Clone for RepositorySystem { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
