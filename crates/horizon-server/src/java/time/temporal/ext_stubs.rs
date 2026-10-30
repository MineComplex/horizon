//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Temporal(pub std::marker::PhantomData<()>);
impl Clone for Temporal { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TemporalAccessor(pub std::marker::PhantomData<()>);
impl Clone for TemporalAccessor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
