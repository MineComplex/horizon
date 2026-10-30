//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ByteBuf(pub std::marker::PhantomData<()>);
impl Clone for ByteBuf { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ByteBufAllocator(pub std::marker::PhantomData<()>);
impl Clone for ByteBufAllocator { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
