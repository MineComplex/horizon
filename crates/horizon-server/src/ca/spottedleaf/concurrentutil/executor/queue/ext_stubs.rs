//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct AreaDependentQueue(pub std::marker::PhantomData<()>);
impl Clone for AreaDependentQueue { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PrioritisedTaskQueue(pub std::marker::PhantomData<()>);
impl Clone for PrioritisedTaskQueue { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
