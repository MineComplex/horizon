//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Property(pub std::marker::PhantomData<()>);
impl Clone for Property { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PropertyMap(pub std::marker::PhantomData<()>);
impl Clone for PropertyMap { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
