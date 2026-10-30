//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct JsonWriter(pub std::marker::PhantomData<()>);
impl Clone for JsonWriter { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
