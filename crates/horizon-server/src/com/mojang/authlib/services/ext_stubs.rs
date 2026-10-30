//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ProfileResult(pub std::marker::PhantomData<()>);
impl Clone for ProfileResult { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ServicesKeySet(pub std::marker::PhantomData<()>);
impl Clone for ServicesKeySet { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ServicesKeyType(pub std::marker::PhantomData<()>);
impl Clone for ServicesKeyType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
