//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ByteBuffer(pub std::marker::PhantomData<()>);
impl Clone for ByteBuffer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ByteOrder(pub std::marker::PhantomData<()>);
impl Clone for ByteOrder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct IntBuffer(pub std::marker::PhantomData<()>);
impl Clone for IntBuffer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
