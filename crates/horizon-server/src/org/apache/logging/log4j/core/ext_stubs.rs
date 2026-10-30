//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Appender(pub std::marker::PhantomData<()>);
impl Clone for Appender { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Filter(pub std::marker::PhantomData<()>);
impl Clone for Filter { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LogEvent(pub std::marker::PhantomData<()>);
impl Clone for LogEvent { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
