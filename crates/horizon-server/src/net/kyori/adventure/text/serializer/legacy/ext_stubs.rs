//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct LegacyComponentSerializer(pub std::marker::PhantomData<()>);
impl Clone for LegacyComponentSerializer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LegacyComponentSerializerBuilder(pub std::marker::PhantomData<()>);
impl Clone for LegacyComponentSerializerBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
