//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct CentralProcessor(pub std::marker::PhantomData<()>);
impl Clone for CentralProcessor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct GlobalMemory(pub std::marker::PhantomData<()>);
impl Clone for GlobalMemory { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct GraphicsCard(pub std::marker::PhantomData<()>);
impl Clone for GraphicsCard { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PhysicalMemory(pub std::marker::PhantomData<()>);
impl Clone for PhysicalMemory { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct VirtualMemory(pub std::marker::PhantomData<()>);
impl Clone for VirtualMemory { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
