//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct PlainTextComponentSerializer(pub std::marker::PhantomData<()>);
impl Clone for PlainTextComponentSerializer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PlainTextComponentSerializerBuilder(pub std::marker::PhantomData<()>);
impl Clone for PlainTextComponentSerializerBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
