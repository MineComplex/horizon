//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Schedule(pub std::marker::PhantomData<()>);
impl Clone for Schedule { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TickData(pub std::marker::PhantomData<()>);
impl Clone for TickData { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TickDataMSPTData(pub std::marker::PhantomData<()>);
impl Clone for TickDataMSPTData { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TickTime(pub std::marker::PhantomData<()>);
impl Clone for TickTime { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
