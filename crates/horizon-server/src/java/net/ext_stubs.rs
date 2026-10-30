//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Proxy(pub std::marker::PhantomData<()>);
impl Clone for Proxy { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct URLClassLoader(pub std::marker::PhantomData<()>);
impl Clone for URLClassLoader { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
