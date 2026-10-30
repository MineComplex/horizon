//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait ArgumentType<A0 = JObjectRef>: Send + Sync {}
pub struct DoubleArgumentType(pub std::marker::PhantomData<()>);
impl Clone for DoubleArgumentType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FloatArgumentType(pub std::marker::PhantomData<()>);
impl Clone for FloatArgumentType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct IntegerArgumentType(pub std::marker::PhantomData<()>);
impl Clone for IntegerArgumentType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LongArgumentType(pub std::marker::PhantomData<()>);
impl Clone for LongArgumentType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct StringArgumentType(pub std::marker::PhantomData<()>);
impl Clone for StringArgumentType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct StringArgumentTypeStringType(pub std::marker::PhantomData<()>);
impl Clone for StringArgumentTypeStringType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
