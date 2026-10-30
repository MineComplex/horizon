//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct FloatGoal(pub std::marker::PhantomData<()>);
impl Clone for FloatGoal { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
