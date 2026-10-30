//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Float2FloatFunction(pub std::marker::PhantomData<()>);
impl Clone for Float2FloatFunction { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FloatUnaryOperator(pub std::marker::PhantomData<()>);
impl Clone for FloatUnaryOperator { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
