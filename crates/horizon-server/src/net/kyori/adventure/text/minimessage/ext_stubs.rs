//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct MiniMessage(pub std::marker::PhantomData<()>);
impl Clone for MiniMessage { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MiniMessageBuilder(pub std::marker::PhantomData<()>);
impl Clone for MiniMessageBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
