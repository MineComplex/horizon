//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct SentryLevel(pub std::marker::PhantomData<()>);
impl Clone for SentryLevel { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
