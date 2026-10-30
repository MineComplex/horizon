//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct BalancedPrioritisedThreadPool(pub std::marker::PhantomData<()>);
impl Clone for BalancedPrioritisedThreadPool { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct BalancedPrioritisedThreadPoolOrderedStreamGroup(pub std::marker::PhantomData<()>);
impl Clone for BalancedPrioritisedThreadPoolOrderedStreamGroup { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
