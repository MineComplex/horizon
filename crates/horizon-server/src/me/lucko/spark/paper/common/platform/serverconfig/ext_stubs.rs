//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait ConfigParser: Send + Sync {}
pub struct ExcludedConfigFilter(pub std::marker::PhantomData<()>);
impl Clone for ExcludedConfigFilter { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ServerConfigProvider(pub std::marker::PhantomData<()>);
impl Clone for ServerConfigProvider { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
