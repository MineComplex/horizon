//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct GsonComponentSerializer(pub std::marker::PhantomData<()>);
impl Clone for GsonComponentSerializer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct GsonComponentSerializerBuilder(pub std::marker::PhantomData<()>);
impl Clone for GsonComponentSerializerBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
