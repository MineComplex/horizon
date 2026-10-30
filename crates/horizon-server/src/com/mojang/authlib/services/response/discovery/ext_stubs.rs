//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct DiscoveryResponse(pub std::marker::PhantomData<()>);
impl Clone for DiscoveryResponse { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Endpoint(pub std::marker::PhantomData<()>);
impl Clone for Endpoint { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Service(pub std::marker::PhantomData<()>);
impl Clone for Service { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
