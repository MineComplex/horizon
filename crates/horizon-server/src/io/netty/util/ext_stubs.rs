//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct AttributeKey<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for AttributeKey<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ByteProcessor(pub std::marker::PhantomData<()>);
impl Clone for ByteProcessor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ResourceLeakDetectorLevel(pub std::marker::PhantomData<()>);
impl Clone for ResourceLeakDetectorLevel { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
