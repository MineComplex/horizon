//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct CommandExceptionType(pub std::marker::PhantomData<()>);
impl Clone for CommandExceptionType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Dynamic2CommandExceptionType(pub std::marker::PhantomData<()>);
impl Clone for Dynamic2CommandExceptionType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Dynamic3CommandExceptionType(pub std::marker::PhantomData<()>);
impl Clone for Dynamic3CommandExceptionType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Dynamic4CommandExceptionType(pub std::marker::PhantomData<()>);
impl Clone for Dynamic4CommandExceptionType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DynamicCommandExceptionType(pub std::marker::PhantomData<()>);
impl Clone for DynamicCommandExceptionType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SimpleCommandExceptionType(pub std::marker::PhantomData<()>);
impl Clone for SimpleCommandExceptionType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
