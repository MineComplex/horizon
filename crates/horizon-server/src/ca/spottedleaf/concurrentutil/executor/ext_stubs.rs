//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait Cancellable: Send + Sync {}
pub struct PrioritisedExecutor(pub std::marker::PhantomData<()>);
impl Clone for PrioritisedExecutor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PrioritisedExecutorPrioritisedTask(pub std::marker::PhantomData<()>);
impl Clone for PrioritisedExecutorPrioritisedTask { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
